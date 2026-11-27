//! Executing BYOA external-auth RPCs against the HQ office and mapping refusals cleanly.
//!
//! Security-critical mutations (`approve`, `renew`, `revoke`) revalidate the browser session
//! fresh against the host up front and dispatch at most once (`retry_on_transport = false`, no
//! `NotAuthorized` retry), so a wrong approval code never spends two attempts and a revoked
//! session fails with `401` before reaching the office.

use hyper::header::{HeaderValue, RETRY_AFTER};
use hyper::StatusCode;
use org_proto::external_auth::{
    ExternalHistoryEntry, ExternalSessionView, MemberExternalApprove, MemberExternalApproveParams,
    MemberExternalPending, MemberExternalPendingParams, MemberExternalPendingResult,
    MemberExternalRenew, MemberExternalRevoke, MemberExternalSessions,
    MemberExternalSessionsParams, PendingExternalAuthView,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde::Serialize;
use serde_json::Value;

use super::super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::super::{failure, result, unavailable, CallerAuth, SessionApi};
use super::{ApprovalCode, ExternalAuthCall, CODE_PART_LEN, CROCKFORD_ALPHABET};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::ApiReply;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MemberExternalSessionsWebResult {
    employee_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active: Option<ExternalSessionView>,
    pending: Vec<PendingExternalAuthView>,
    history: Vec<ExternalHistoryEntry>,
}

/// Ensures `code_prefix` is strictly `XXXX-••••` (lookup half only), never leaking a secret half.
pub(super) fn mask_code_prefix(raw: &str) -> String {
    let lookup = raw.split('-').next().unwrap_or_default().trim();
    let valid = lookup.len() == CODE_PART_LEN
        && lookup
            .bytes()
            .all(|b| CROCKFORD_ALPHABET.contains(&b.to_ascii_uppercase()));
    if valid {
        format!("{}-••••", lookup.to_ascii_uppercase())
    } else {
        "••••-••••".to_owned()
    }
}

fn sanitize_pending_view(mut view: PendingExternalAuthView) -> PendingExternalAuthView {
    view.code_prefix = mask_code_prefix(&view.code_prefix);
    view
}

pub(in crate::api) async fn execute(api: &SessionApi, call: ExternalAuthCall) -> ApiReply {
    match call {
        ExternalAuthCall::Pending(caller, org, params) => {
            execute_pending(api, caller, &org, &params).await
        }
        ExternalAuthCall::MemberSessions(caller, org, params) => {
            execute_member_sessions(api, caller, &org, &params).await
        }
        ExternalAuthCall::Approve {
            caller,
            org_id,
            command_id,
            code,
            ttl_secs,
            takeover,
        } => execute_approve(api, caller, &org_id, command_id, &code, ttl_secs, takeover).await,
        ExternalAuthCall::Renew(caller, org, params) => {
            execute_mutation::<MemberExternalRenew>(api, caller, &org, &params, None).await
        }
        ExternalAuthCall::Revoke(caller, org, params) => {
            execute_mutation::<MemberExternalRevoke>(api, caller, &org, &params, None).await
        }
    }
}

async fn run_read_with_retry<C: org_proto::Call>(
    api: &SessionApi,
    caller: CallerAuth,
    org_id: &str,
    params: &C::Params,
) -> Result<(String, C::Result), ApiReply> {
    let mut office_id = resolve_hq_office(api, caller, org_id)
        .await
        .map_err(map_host_failure)?;
    let auth = api.auth(&caller);
    let mut reply = api
        .offices
        .call_typed::<C>(&auth, &office_id, params, true)
        .await;
    if matches!(
        &reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    ) {
        office_id = revalidate_and_resolve_hq(api, caller, org_id)
            .await
            .map_err(map_host_failure)?;
        reply = api
            .offices
            .call_typed::<C>(&auth, &office_id, params, true)
            .await;
    }
    reply
        .map(|res| (office_id, res))
        .map_err(|err| map_office_failure(err, None))
}

async fn execute_pending(
    api: &SessionApi,
    caller: CallerAuth,
    org_id: &str,
    params: &MemberExternalPendingParams,
) -> ApiReply {
    match run_read_with_retry::<MemberExternalPending>(api, caller, org_id, params).await {
        Ok((_, res)) => {
            let sanitized = MemberExternalPendingResult {
                pending: res.pending.into_iter().map(sanitize_pending_view).collect(),
            };
            result(sanitized)
        }
        Err(reply) => reply,
    }
}

async fn execute_member_sessions(
    api: &SessionApi,
    caller: CallerAuth,
    org_id: &str,
    params: &MemberExternalSessionsParams,
) -> ApiReply {
    let (office_id, sessions) =
        match run_read_with_retry::<MemberExternalSessions>(api, caller, org_id, params).await {
            Ok(ok) => ok,
            Err(reply) => return reply,
        };
    let auth = api.auth(&caller);
    let pending_params = MemberExternalPendingParams {
        employee_id: Some(sessions.employee_id.clone()),
    };
    let pending = match api
        .offices
        .call_typed::<MemberExternalPending>(&auth, &office_id, &pending_params, true)
        .await
    {
        Ok(res) => res.pending.into_iter().map(sanitize_pending_view).collect(),
        Err(err) => return map_office_failure(err, None),
    };
    result(MemberExternalSessionsWebResult {
        employee_id: sessions.employee_id,
        active: sessions.active,
        pending,
        history: sessions.history,
    })
}

async fn execute_approve(
    api: &SessionApi,
    caller: CallerAuth,
    org_id: &str,
    command_id: Option<String>,
    code: &ApprovalCode,
    ttl_secs: Option<u64>,
    takeover: bool,
) -> ApiReply {
    let params = MemberExternalApproveParams {
        command_id,
        code: code.as_str().to_owned(),
        ttl_secs,
        takeover,
    };
    execute_mutation::<MemberExternalApprove>(api, caller, org_id, &params, Some(code)).await
}

async fn execute_mutation<C: org_proto::Call>(
    api: &SessionApi,
    caller: CallerAuth,
    org_id: &str,
    params: &C::Params,
    redact_code: Option<&ApprovalCode>,
) -> ApiReply {
    let office_id = match revalidate_and_resolve_hq(api, caller, org_id).await {
        Ok(id) => id,
        Err(err) => return map_host_failure(err),
    };
    let auth = api.auth(&caller);
    match api
        .offices
        .call_typed::<C>(&auth, &office_id, params, false)
        .await
    {
        Ok(res) => result(res),
        Err(err) => map_office_failure(err, redact_code),
    }
}

fn map_host_failure(error: Failure) -> ApiReply {
    match error {
        Failure::Refused(_) => unavailable(),
        other => failure(other),
    }
}

fn message_leaks_code(message: &str, code: &ApprovalCode) -> bool {
    let full = code.as_str();
    let Some((prefix, secret)) = full.split_once('-') else {
        return true;
    };
    let upper = message.to_ascii_uppercase();
    if upper.contains(full) {
        return true;
    }
    let prefix_bytes = prefix.as_bytes();
    let secret_bytes = secret.as_bytes();
    let suffix_match = upper
        .as_bytes()
        .windows(1 + secret_bytes.len())
        .any(|w| w[0] == b'-' && &w[1..] == secret_bytes);
    if suffix_match {
        return true;
    }
    upper
        .as_bytes()
        .windows(prefix_bytes.len() + secret_bytes.len())
        .any(|w| {
            &w[..prefix_bytes.len()] == prefix_bytes && &w[prefix_bytes.len()..] == secret_bytes
        })
}

fn generic_message(code: &WebErrorCode) -> &'static str {
    match code {
        WebErrorCode::InvalidRequest | WebErrorCode::InvalidName => "Invalid request.",
        WebErrorCode::NotAuthorized | WebErrorCode::OwnerProtected => "Not authorized.",
        WebErrorCode::NotFound => "Not found.",
        WebErrorCode::Conflict | WebErrorCode::AlreadyExists | WebErrorCode::OperationReused => {
            "Request conflicts with current session state."
        }
        WebErrorCode::Expired => "Approval code has expired.",
        WebErrorCode::TooLarge => "Request is too large.",
        WebErrorCode::RateLimited => "Too many attempts. Try again later.",
        _ => "Service unavailable.",
    }
}

fn map_office_failure(error: OfficeFailure, redact_code: Option<&ApprovalCode>) -> ApiReply {
    let err = match error {
        OfficeFailure::BeforeDispatch(e) => return failure(e),
        OfficeFailure::Transport => return unavailable(),
        OfficeFailure::Refused(err) => err,
    };
    let retry_after = err.retry_after_ms;
    let mut web = WebError::from(err);
    let status = match web.code {
        WebErrorCode::InvalidRequest | WebErrorCode::InvalidName => StatusCode::BAD_REQUEST,
        WebErrorCode::NotAuthorized | WebErrorCode::OwnerProtected => StatusCode::FORBIDDEN,
        WebErrorCode::NotFound => StatusCode::NOT_FOUND,
        WebErrorCode::Conflict | WebErrorCode::AlreadyExists | WebErrorCode::OperationReused => {
            StatusCode::CONFLICT
        }
        WebErrorCode::Expired => StatusCode::GONE,
        WebErrorCode::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        WebErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        _ => return unavailable(),
    };
    if redact_code.is_some_and(|code| message_leaks_code(&web.message, code)) {
        web.message = generic_message(&web.code).to_owned();
    }
    let reply = ApiReply::json(
        status,
        serde_json::to_value(ReadResponse::<Value>::Error(web)).expect("gateway error serializes"),
    );
    match retry_after {
        Some(ms) if status == StatusCode::TOO_MANY_REQUESTS => {
            reply.with_header(RETRY_AFTER, HeaderValue::from(ms.div_ceil(1000).max(1)))
        }
        _ => reply,
    }
}
