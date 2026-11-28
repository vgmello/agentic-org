//! Browser HTTP routes for C3 agent sessions, grants, approval decisions, stall recovery,
//! live turn controls (interrupt and steer, `TASK-103`), and browser-driven provider CLI
//! enrollment (`TASK-42` / C3, `ADR-0030`, `ADR-0032`).

use hyper::{http::request::Parts, Method};
use org_proto::sessions::{
    MemberSessionApprovalDecideParams, MemberSessionClearStallParams, MemberSessionInterruptParams,
    MemberSessionListParams, MemberSessionStartParams, MemberSessionSteerParams,
    MemberSessionStopParams, MemberSessionTranscriptParams, ProviderCredentialRevokeParams,
    ProviderEnrollmentCancelParams, ProviderEnrollmentStartParams, ProviderEnrollmentSubmitParams,
};
use org_proto::web::organizations;
use serde::Deserialize;

use super::{bad_input, invalid, Call, CallerAuth, Route, SessionApi};
use crate::service::{ApiReply, PreparedCall};

mod calls;

pub(super) use calls::{execute, prepare};

pub enum SessionRoute {
    Start(CallerAuth, String, String),
    Stop(CallerAuth, String, String),
    Interrupt(CallerAuth, String, String),
    Steer(CallerAuth, String, String),
    Inspect(CallerAuth, String, String),
    ListSessions(CallerAuth, String, MemberSessionListParams),
    Transcript(CallerAuth, String, MemberSessionTranscriptParams),
    Grants(CallerAuth, String, String),
    Approval(CallerAuth, String, String),
    ClearStall(CallerAuth, String, String),
    EnrollStart(CallerAuth, String, String),
    EnrollStatus(CallerAuth, String, String, String),
    EnrollSubmit(CallerAuth, String, String, String),
    EnrollCancel(CallerAuth, String, String, String),
    CredentialStatus(CallerAuth, String, String),
    CredentialRevoke(CallerAuth, String, String),
}

pub enum SessionCall {
    Start(CallerAuth, String, MemberSessionStartParams),
    Stop(CallerAuth, String, MemberSessionStopParams),
    Interrupt(CallerAuth, String, MemberSessionInterruptParams),
    Steer(CallerAuth, String, MemberSessionSteerParams),
    Inspect(CallerAuth, String, String),
    ListSessions(CallerAuth, String, MemberSessionListParams),
    Transcript(CallerAuth, String, MemberSessionTranscriptParams),
    Grants(CallerAuth, String, String),
    Approval(CallerAuth, String, MemberSessionApprovalDecideParams),
    ClearStall(CallerAuth, String, MemberSessionClearStallParams),
    EnrollStart(CallerAuth, String, ProviderEnrollmentStartParams),
    EnrollStatus(CallerAuth, String, String, String),
    EnrollSubmit(CallerAuth, String, ProviderEnrollmentSubmitParams),
    EnrollCancel(CallerAuth, String, ProviderEnrollmentCancelParams),
    CredentialStatus(CallerAuth, String, String),
    CredentialRevoke(CallerAuth, String, ProviderCredentialRevokeParams),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartBody {
    command_id: String,
    #[serde(default)]
    instruction: Option<String>,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StopBody {
    command_id: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SteerBody {
    command_id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalBody {
    command_id: String,
    approval_id: String,
    decision: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandOnlyBody {
    command_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnrollStartBody {
    command_id: String,
    #[serde(default)]
    provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnrollSubmitBody {
    command_id: String,
    code: String,
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

fn valid_id(id: &str) -> Result<(), ApiReply> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then_some(()).ok_or_else(invalid)
}

/// Attempts to route C3/C4 paths under `/api/v1/organizations/{org}/...`.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    _get: bool,
    post: bool,
    query: bool,
) -> Option<Result<Route, ApiReply>> {
    let is_get_method = matches!(request.method, Method::GET | Method::HEAD);
    let is_get = is_get_method && !query;
    match tail {
        ["agents", id, "sessions"] => {
            route_agent_sessions_list(api, request, org, id, is_get_method)
        }
        ["agents", id, "transcript"] => {
            route_agent_transcript(api, request, org, id, None, is_get_method)
        }
        ["agents", id, "sessions", session_id, "transcript"] => {
            route_agent_transcript(api, request, org, id, Some(session_id), is_get_method)
        }
        ["agents", id, action] => route_agent_session(api, request, org, id, action, is_get, post),
        ["provider-connections", id, rest @ ..] => {
            route_provider_conn(api, request, org, id, rest, is_get, post)
        }
        _ => None,
    }
}

fn route_agent_sessions_list(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    is_get_method: bool,
) -> Option<Result<Route, ApiReply>> {
    if !is_get_method {
        return Some(Err(invalid()));
    }
    Some(build_sessions_list_route(api, request, org, id))
}

fn build_sessions_list_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
) -> Result<Route, ApiReply> {
    let caller = api.caller_auth(request)?;
    valid_org_id(org)?;
    if !org_office_launch::is_storage_id(id) {
        return Err(invalid());
    }
    let limit = parse_list_query(request.uri.query())?;
    Ok(Route::Sessions(SessionRoute::ListSessions(
        caller,
        org.to_owned(),
        MemberSessionListParams {
            employee_id: Some(id.to_owned()),
            limit,
        },
    )))
}

fn route_agent_transcript(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    session_id: Option<&str>,
    is_get_method: bool,
) -> Option<Result<Route, ApiReply>> {
    if !is_get_method {
        return Some(Err(invalid()));
    }
    Some(build_transcript_route(api, request, org, id, session_id))
}

fn build_transcript_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    session_id: Option<&str>,
) -> Result<Route, ApiReply> {
    let caller = api.caller_auth(request)?;
    valid_org_id(org)?;
    if !org_office_launch::is_storage_id(id) {
        return Err(invalid());
    }
    if let Some(sid) = session_id {
        valid_session_id(sid)?;
    }
    let mut params = parse_transcript_query(request.uri.query())?;
    params.employee_id = Some(id.to_owned());
    params.session_id = session_id.map(ToOwned::to_owned);
    Ok(Route::Sessions(SessionRoute::Transcript(
        caller,
        org.to_owned(),
        params,
    )))
}

fn valid_session_id(id: &str) -> Result<(), ApiReply> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && !id.contains("..")
        && id != "."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    ok.then_some(()).ok_or_else(invalid)
}

fn parse_list_query(query: Option<&str>) -> Result<Option<u32>, ApiReply> {
    let Some(raw) = query.filter(|q| !q.is_empty()) else {
        return Ok(None);
    };
    let mut limit = None;
    for pair in raw.split('&') {
        let (k, v) = pair.split_once('=').ok_or_else(invalid)?;
        match k {
            "limit" if limit.is_none() => {
                let parsed: u32 = v.parse().map_err(|_| invalid())?;
                if parsed == 0 {
                    return Err(invalid());
                }
                limit = Some(parsed);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(limit)
}

fn parse_transcript_query(query: Option<&str>) -> Result<MemberSessionTranscriptParams, ApiReply> {
    let mut params = MemberSessionTranscriptParams::default();
    let Some(raw) = query.filter(|q| !q.is_empty()) else {
        return Ok(params);
    };
    for pair in raw.split('&') {
        let (k, v) = pair.split_once('=').ok_or_else(invalid)?;
        match k {
            "include_tool_calls" if params.include_tool_calls.is_none() => {
                params.include_tool_calls = Some(parse_bool_param(v)?);
            }
            "include_file_changes" if params.include_file_changes.is_none() => {
                params.include_file_changes = Some(parse_bool_param(v)?);
            }
            "include_subagents" if params.include_subagents.is_none() => {
                params.include_subagents = Some(parse_bool_param(v)?);
            }
            "cursor" if params.cursor.is_none() => {
                params.cursor = Some(v.parse().map_err(|_| invalid())?);
            }
            "limit" if params.limit.is_none() => {
                let parsed: u32 = v.parse().map_err(|_| invalid())?;
                if parsed == 0 {
                    return Err(invalid());
                }
                params.limit = Some(parsed);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(params)
}

fn parse_bool_param(raw: &str) -> Result<bool, ApiReply> {
    match raw {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(invalid()),
    }
}

fn route_agent_session(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    action: &str,
    is_get: bool,
    post: bool,
) -> Option<Result<Route, ApiReply>> {
    if is_get {
        return route_agent_get(api, request, org, id, action);
    }
    if post {
        return route_agent_post(api, request, org, id, action);
    }
    is_known_agent_action(action).then(|| Err(invalid()))
}

fn route_agent_get(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    action: &str,
) -> Option<Result<Route, ApiReply>> {
    match action {
        "session" => Some(auth_uuid(api, request, org, id, SessionRoute::Inspect)),
        "grants" => Some(auth_uuid(api, request, org, id, SessionRoute::Grants)),
        a if is_known_agent_action(a) => Some(Err(invalid())),
        _ => None,
    }
}

fn route_agent_post(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    action: &str,
) -> Option<Result<Route, ApiReply>> {
    match action {
        "start" => Some(auth_uuid(api, request, org, id, SessionRoute::Start)),
        "stop" => Some(auth_uuid(api, request, org, id, SessionRoute::Stop)),
        "interrupt" => Some(auth_uuid(api, request, org, id, SessionRoute::Interrupt)),
        "steer" => Some(auth_uuid(api, request, org, id, SessionRoute::Steer)),
        "approvals" => Some(auth_uuid(api, request, org, id, SessionRoute::Approval)),
        "clear-stall" => Some(auth_uuid(api, request, org, id, SessionRoute::ClearStall)),
        a if is_known_agent_action(a) => Some(Err(invalid())),
        _ => None,
    }
}

fn is_known_agent_action(action: &str) -> bool {
    matches!(
        action,
        "start"
            | "stop"
            | "interrupt"
            | "steer"
            | "session"
            | "grants"
            | "approvals"
            | "clear-stall"
    )
}

fn route_provider_conn(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    rest: &[&str],
    is_get: bool,
    post: bool,
) -> Option<Result<Route, ApiReply>> {
    if is_get {
        return route_provider_get(api, request, org, id, rest);
    }
    if post {
        return route_provider_post(api, request, org, id, rest);
    }
    is_known_provider_tail(rest).then(|| Err(invalid()))
}

fn route_provider_get(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    rest: &[&str],
) -> Option<Result<Route, ApiReply>> {
    match rest {
        ["enroll", "start"] => Some(Err(invalid())),
        ["enroll", login_id] => Some(auth_pair(
            api,
            request,
            org,
            id,
            login_id,
            SessionRoute::EnrollStatus,
        )),
        ["credential"] => Some(auth_uuid(
            api,
            request,
            org,
            id,
            SessionRoute::CredentialStatus,
        )),
        tail if is_known_provider_tail(tail) => Some(Err(invalid())),
        _ => None,
    }
}

fn route_provider_post(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    rest: &[&str],
) -> Option<Result<Route, ApiReply>> {
    match rest {
        ["enroll", "start"] => Some(auth_uuid(api, request, org, id, SessionRoute::EnrollStart)),
        ["enroll", login_id, "submit"] => Some(auth_pair(
            api,
            request,
            org,
            id,
            login_id,
            SessionRoute::EnrollSubmit,
        )),
        ["enroll", login_id, "cancel"] => Some(auth_pair(
            api,
            request,
            org,
            id,
            login_id,
            SessionRoute::EnrollCancel,
        )),
        ["credential", "revoke"] => Some(auth_uuid(
            api,
            request,
            org,
            id,
            SessionRoute::CredentialRevoke,
        )),
        tail if is_known_provider_tail(tail) => Some(Err(invalid())),
        _ => None,
    }
}

fn is_known_provider_tail(rest: &[&str]) -> bool {
    matches!(
        rest,
        ["enroll", _]
            | ["enroll", _, "submit" | "cancel"]
            | ["credential"]
            | ["credential", "revoke"]
    )
}

fn auth_uuid(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    build: fn(CallerAuth, String, String) -> SessionRoute,
) -> Result<Route, ApiReply> {
    let caller = api.caller_auth(request)?;
    valid_org_id(org)?;
    if !org_office_launch::is_storage_id(id) {
        return Err(invalid());
    }
    Ok(Route::Sessions(build(
        caller,
        org.to_owned(),
        id.to_owned(),
    )))
}

fn auth_pair(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    login_id: &str,
    build: fn(CallerAuth, String, String, String) -> SessionRoute,
) -> Result<Route, ApiReply> {
    let caller = api.caller_auth(request)?;
    valid_org_id(org)?;
    if !org_office_launch::is_storage_id(id) {
        return Err(invalid());
    }
    valid_id(login_id)?;
    Ok(Route::Sessions(build(
        caller,
        org.to_owned(),
        id.to_owned(),
        login_id.to_owned(),
    )))
}

fn decode_mut<B: serde::de::DeserializeOwned>(
    bytes: &[u8],
    command_id: impl FnOnce(&B) -> &str,
    wrap: impl FnOnce(B) -> SessionCall,
) -> Result<PreparedCall<Call>, ApiReply> {
    let body: B = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    prepare_mut(body, bytes.len(), command_id, wrap)
}

/// [`decode_mut`] for a body its route already decoded (and checked), `len` bytes long.
fn prepare_mut<B>(
    body: B,
    len: usize,
    command_id: impl FnOnce(&B) -> &str,
    wrap: impl FnOnce(B) -> SessionCall,
) -> Result<PreparedCall<Call>, ApiReply> {
    let id = organizations::host_operation_id(command_id(&body)).map_err(bad_input)?;
    Ok(PreparedCall::mutation(Call::Sessions(wrap(body)), id, len))
}

fn read_call(call: SessionCall, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    if !bytes.is_empty() {
        return Err(invalid());
    }
    Ok(PreparedCall::read(Call::Sessions(call), 0))
}

#[cfg(test)]
mod tests;
