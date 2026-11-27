//! Browser HTTP routes for BYOA external-agent authorization and lease management (`TASK-249`):
//!
//! - `GET  /api/v1/organizations/{org_id}/external-auth/pending` (optional `?member_id=<uuid>`)
//! - `GET  /api/v1/organizations/{org_id}/members/{member_id}/external-sessions` (optional `?limit=<1..=200>`)
//! - `POST /api/v1/organizations/{org_id}/external-auth/approve`
//! - `POST /api/v1/organizations/{org_id}/members/{member_id}/external-sessions/renew`
//! - `POST /api/v1/organizations/{org_id}/members/{member_id}/external-sessions/revoke`
//!
//! Every route requires a human browser session (`401` when unauthenticated; all `POST` routes
//! enforce `Origin`, `Sec-Fetch-Site`, and `application/json` at ingress -> `403`/`415`).
//! Approval codes stay in zeroizing memory, are redacted from `Debug`, and are never echoed.

use std::fmt;

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::external_auth::{
    MemberExternalPendingParams, MemberExternalRenewParams, MemberExternalRevokeParams,
    MemberExternalSessionsParams, MAX_LEASE_TTL_SECS, MIN_LEASE_TTL_SECS,
};
use org_proto::web::WebErrorCode;
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use zeroize::Zeroizing;

use super::{invalid, not_found, Call, CallerAuth, Route, SessionApi};
use crate::service::{ApiReply, PreparedCall};

mod calls;
#[cfg(test)]
mod tests;

pub(super) use calls::execute;

/// Crockford base32 alphabet shared with `org-office-sessions` (`codes.rs`): no `I`, `L`, `O`, `U`.
pub(super) const CROCKFORD_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
pub(super) const CODE_PART_LEN: usize = 4;
const MAX_RAW_CODE_BYTES: usize = 32;
const MAX_CODE_LITERAL_BYTES: usize = 64;
const MAX_HISTORY_LIMIT: u32 = 200;

/// Validated, normalized Crockford approval code (`LLLL-SSSS`). Wiped on drop; never formatted.
#[derive(Clone)]
pub struct ApprovalCode(Zeroizing<String>);

impl ApprovalCode {
    pub(super) fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub(super) fn clone_zeroizing(&self) -> Zeroizing<String> {
        self.0.clone()
    }
}

impl fmt::Debug for ApprovalCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApprovalCode(<redacted>)")
    }
}

pub enum ExternalAuthRoute {
    Pending(CallerAuth, String, MemberExternalPendingParams),
    MemberSessions(CallerAuth, String, MemberExternalSessionsParams),
    Approve(CallerAuth, String),
    Renew(CallerAuth, String, String),
    Revoke(CallerAuth, String, String),
}

pub enum ExternalAuthCall {
    Pending(CallerAuth, String, MemberExternalPendingParams),
    MemberSessions(CallerAuth, String, MemberExternalSessionsParams),
    Approve {
        caller: CallerAuth,
        org_id: String,
        command_id: Option<String>,
        code: ApprovalCode,
        ttl_secs: Option<u64>,
        takeover: bool,
    },
    Renew(CallerAuth, String, MemberExternalRenewParams),
    Revoke(CallerAuth, String, MemberExternalRevokeParams),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApproveBody {
    #[serde(default)]
    command_id: Option<String>,
    #[serde(deserialize_with = "deserialize_approval_code")]
    code: ApprovalCode,
    #[serde(default)]
    ttl_seconds: Option<u64>,
    #[serde(default)]
    takeover: bool,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenewBody {
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    ttl_seconds: Option<u64>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeBody {
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

fn valid_member_id(id: &str) -> Result<(), ApiReply> {
    org_proto::is_uuid_v7(id).then_some(()).ok_or_else(invalid)
}

fn valid_optional_command_id(id: Option<&str>) -> Result<(), ApiReply> {
    match id {
        Some(value) if !org_proto::is_uuid_v7(value) => Err(invalid()),
        _ => Ok(()),
    }
}

fn valid_optional_session_id(id: Option<&str>) -> Result<(), ApiReply> {
    let Some(value) = id else {
        return Ok(());
    };
    let ok = !value.is_empty()
        && value.len() <= 128
        && !value.contains("..")
        && value != "."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    ok.then_some(()).ok_or_else(invalid)
}

fn valid_optional_ttl_seconds(ttl: Option<u64>) -> Result<(), ApiReply> {
    match ttl {
        Some(secs) if !(MIN_LEASE_TTL_SECS..=MAX_LEASE_TTL_SECS).contains(&secs) => {
            Err(ApiReply::error(
                StatusCode::BAD_REQUEST,
                WebErrorCode::InvalidRequest,
                "ttl_seconds must be between 300 and 604800 seconds.",
            ))
        }
        _ => Ok(()),
    }
}

/// Decodes and normalizes an approval code directly from the zeroizing JSON body buffer without
/// allocating an intermediate unzeroed string.
fn deserialize_approval_code<'de, D: Deserializer<'de>>(d: D) -> Result<ApprovalCode, D::Error> {
    let raw = <&'de RawValue>::deserialize(d)?;
    let literal = raw.get();
    if literal.len() < 2 || literal.len() > MAX_CODE_LITERAL_BYTES {
        return Err(serde::de::Error::custom("invalid approval code"));
    }
    let inner = literal
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| serde::de::Error::custom("invalid approval code"))?;
    normalize_code_literal(inner).ok_or_else(|| serde::de::Error::custom("invalid approval code"))
}

fn crockford_digit(byte: u8) -> Option<u8> {
    let mapped = match byte.to_ascii_uppercase() {
        b'O' => b'0',
        b'I' | b'L' => b'1',
        other => other,
    };
    CROCKFORD_ALPHABET.contains(&mapped).then_some(mapped)
}

pub(super) fn normalize_code_literal(inner: &str) -> Option<ApprovalCode> {
    let bytes = inner.as_bytes();
    let mut raw_chars = 0usize;
    let mut digits = Zeroizing::new([0u8; CODE_PART_LEN * 2]);
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        raw_chars += 1;
        if raw_chars > MAX_RAW_CODE_BYTES {
            return None;
        }
        let b = bytes[i];
        i += 1;
        if b == b'\\' {
            if bytes.get(i) != Some(&b't') {
                return None;
            }
            i += 1;
            continue;
        }
        if matches!(b, b'-' | b' ' | b'\t') {
            continue;
        }
        let digit = crockford_digit(b)?;
        if count >= digits.len() {
            return None;
        }
        digits[count] = digit;
        count += 1;
    }
    if count != CODE_PART_LEN * 2 {
        return None;
    }
    let mut formatted = Zeroizing::new(String::with_capacity(CODE_PART_LEN * 2 + 1));
    for (idx, &ch) in digits.iter().enumerate() {
        if idx == CODE_PART_LEN {
            formatted.push('-');
        }
        formatted.push(char::from(ch));
    }
    Some(ApprovalCode(formatted))
}

fn parse_pending_query(query: Option<&str>) -> Result<MemberExternalPendingParams, ApiReply> {
    let Some(raw) = query.filter(|q| !q.is_empty()) else {
        return Ok(MemberExternalPendingParams::default());
    };
    let mut member_id = None;
    for pair in raw.split('&') {
        let (key, value) = pair.split_once('=').ok_or_else(invalid)?;
        match key {
            "member_id" if member_id.is_none() => {
                valid_member_id(value)?;
                member_id = Some(value.to_owned());
            }
            _ => return Err(invalid()),
        }
    }
    Ok(MemberExternalPendingParams {
        employee_id: member_id,
    })
}

fn parse_sessions_query(query: Option<&str>) -> Result<Option<u32>, ApiReply> {
    let Some(raw) = query.filter(|q| !q.is_empty()) else {
        return Ok(None);
    };
    let mut limit = None;
    for pair in raw.split('&') {
        let (key, value) = pair.split_once('=').ok_or_else(invalid)?;
        match key {
            "limit" if limit.is_none() => {
                let parsed: u32 = value.parse().map_err(|_| invalid())?;
                if !(1..=MAX_HISTORY_LIMIT).contains(&parsed) {
                    return Err(invalid());
                }
                limit = Some(parsed);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(limit)
}

/// Attempts to route BYOA external-auth endpoints under `/api/v1/organizations/{org}/...`.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    _get: bool,
    post: bool,
    _query: bool,
) -> Option<Result<Route, ApiReply>> {
    let is_get_method = matches!(request.method, Method::GET | Method::HEAD);
    let routed = match tail {
        ["external-auth", rest @ ..] => {
            route_external_auth(api, request, org, rest, is_get_method, post)
        }
        ["members", member_id, "external-sessions", rest @ ..] => {
            route_member_external_sessions(api, request, org, member_id, rest, is_get_method, post)
        }
        _ => return None,
    };
    Some(routed.map(Route::ExternalAuth))
}

fn route_external_auth(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    rest: &[&str],
    is_get_method: bool,
    post: bool,
) -> Result<ExternalAuthRoute, ApiReply> {
    match rest {
        ["pending"] if is_get_method => {
            let caller = api.caller_auth(request)?;
            valid_org_id(org)?;
            let params = parse_pending_query(request.uri.query())?;
            Ok(ExternalAuthRoute::Pending(caller, org.to_owned(), params))
        }
        ["approve"] if post => {
            let caller = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(ExternalAuthRoute::Approve(caller, org.to_owned()))
        }
        ["pending"] | ["approve"] => Err(invalid()),
        _ => Err(not_found()),
    }
}

fn route_member_external_sessions(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    member_id: &str,
    rest: &[&str],
    is_get_method: bool,
    post: bool,
) -> Result<ExternalAuthRoute, ApiReply> {
    match rest {
        [] if is_get_method => {
            let caller = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_member_id(member_id)?;
            let limit = parse_sessions_query(request.uri.query())?;
            Ok(ExternalAuthRoute::MemberSessions(
                caller,
                org.to_owned(),
                MemberExternalSessionsParams {
                    employee_id: member_id.to_owned(),
                    limit,
                },
            ))
        }
        ["renew"] if post => {
            let caller = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_member_id(member_id)?;
            Ok(ExternalAuthRoute::Renew(
                caller,
                org.to_owned(),
                member_id.to_owned(),
            ))
        }
        ["revoke"] if post => {
            let caller = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_member_id(member_id)?;
            Ok(ExternalAuthRoute::Revoke(
                caller,
                org.to_owned(),
                member_id.to_owned(),
            ))
        }
        [] | ["renew" | "revoke"] => Err(invalid()),
        _ => Err(not_found()),
    }
}

pub(super) fn prepare(
    route: ExternalAuthRoute,
    bytes: &[u8],
) -> Result<PreparedCall<Call>, ApiReply> {
    let call = prepare_call(route, bytes)?;
    Ok(PreparedCall::read(Call::ExternalAuth(call), bytes.len()))
}

fn prepare_call(route: ExternalAuthRoute, bytes: &[u8]) -> Result<ExternalAuthCall, ApiReply> {
    let empty = || bytes.is_empty().then_some(()).ok_or_else(invalid);
    match route {
        ExternalAuthRoute::Pending(caller, org, params) => {
            empty()?;
            Ok(ExternalAuthCall::Pending(caller, org, params))
        }
        ExternalAuthRoute::MemberSessions(caller, org, params) => {
            empty()?;
            Ok(ExternalAuthCall::MemberSessions(caller, org, params))
        }
        ExternalAuthRoute::Approve(caller, org_id) => {
            let body: ApproveBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            valid_optional_command_id(body.command_id.as_deref())?;
            valid_optional_ttl_seconds(body.ttl_seconds)?;
            Ok(ExternalAuthCall::Approve {
                caller,
                org_id,
                command_id: body.command_id,
                code: body.code,
                ttl_secs: body.ttl_seconds,
                takeover: body.takeover,
            })
        }
        ExternalAuthRoute::Renew(caller, org_id, employee_id) => {
            let body: RenewBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            valid_optional_command_id(body.command_id.as_deref())?;
            valid_optional_session_id(body.session_id.as_deref())?;
            valid_optional_ttl_seconds(body.ttl_seconds)?;
            Ok(ExternalAuthCall::Renew(
                caller,
                org_id,
                MemberExternalRenewParams {
                    command_id: body.command_id,
                    employee_id,
                    session_id: body.session_id,
                    ttl_secs: body.ttl_seconds,
                },
            ))
        }
        ExternalAuthRoute::Revoke(caller, org_id, employee_id) => {
            let body: RevokeBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            valid_optional_command_id(body.command_id.as_deref())?;
            valid_optional_session_id(body.session_id.as_deref())?;
            Ok(ExternalAuthCall::Revoke(
                caller,
                org_id,
                MemberExternalRevokeParams {
                    command_id: body.command_id,
                    employee_id,
                    session_id: body.session_id,
                },
            ))
        }
    }
}
