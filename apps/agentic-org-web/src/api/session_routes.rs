//! Browser HTTP routes for C3 agent sessions, grants, approval decisions, stall recovery,
//! and browser-driven provider CLI enrollment (`TASK-42` / C3, `ADR-0030`, `ADR-0032`).

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::sessions::{
    MemberSessionApprovalDecide, MemberSessionApprovalDecideParams, MemberSessionClearStall,
    MemberSessionClearStallParams, MemberSessionGrants, MemberSessionGrantsParams,
    MemberSessionInspect, MemberSessionInspectParams, MemberSessionStart, MemberSessionStartParams,
    MemberSessionStop, MemberSessionStopParams, ProviderCredentialRevoke,
    ProviderCredentialRevokeParams, ProviderCredentialStatus, ProviderCredentialStatusParams,
    ProviderEnrollmentCancel, ProviderEnrollmentCancelParams, ProviderEnrollmentStart,
    ProviderEnrollmentStartParams, ProviderEnrollmentStatus, ProviderEnrollmentStatusParams,
    ProviderEnrollmentSubmit, ProviderEnrollmentSubmitParams,
};
use org_proto::web::{organizations, ReadResponse, WebError, WebErrorCode};
use serde::Deserialize;
use serde_json::Value;

use super::org_routes::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{
    bad_input, failure, invalid, not_found, result, unavailable, Call, CallerAuth, Route,
    SessionApi,
};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub enum SessionRoute {
    Start(CallerAuth, String, String),
    Stop(CallerAuth, String, String),
    Inspect(CallerAuth, String, String),
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
    Inspect(CallerAuth, String, String),
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

/// Attempts to route C3 paths under `/api/v1/organizations/{org}/...`.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    _get: bool,
    post: bool,
    query: bool,
) -> Option<Result<Route, ApiReply>> {
    let is_get = matches!(request.method, Method::GET | Method::HEAD) && !query;
    match tail {
        ["agents", id, action] => route_agent_session(api, request, org, id, action, is_get, post),
        ["provider-connections", id, rest @ ..] => {
            route_provider_conn(api, request, org, id, rest, is_get, post)
        }
        _ => None,
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
        "approvals" => Some(auth_uuid(api, request, org, id, SessionRoute::Approval)),
        "clear-stall" => Some(auth_uuid(api, request, org, id, SessionRoute::ClearStall)),
        a if is_known_agent_action(a) => Some(Err(invalid())),
        _ => None,
    }
}

fn is_known_agent_action(action: &str) -> bool {
    matches!(
        action,
        "start" | "stop" | "session" | "grants" | "approvals" | "clear-stall"
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
    let id = organizations::host_operation_id(command_id(&body)).map_err(bad_input)?;
    Ok(PreparedCall::mutation(
        Call::Sessions(wrap(body)),
        id,
        bytes.len(),
    ))
}

fn read_call(call: SessionCall, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    if !bytes.is_empty() {
        return Err(invalid());
    }
    Ok(PreparedCall::read(Call::Sessions(call), 0))
}

pub(super) fn prepare(route: SessionRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    match route {
        SessionRoute::Inspect(c, o, id) => read_call(SessionCall::Inspect(c, o, id), bytes),
        SessionRoute::Grants(c, o, id) => read_call(SessionCall::Grants(c, o, id), bytes),
        SessionRoute::EnrollStatus(c, o, id, login) => {
            read_call(SessionCall::EnrollStatus(c, o, id, login), bytes)
        }
        SessionRoute::CredentialStatus(c, o, id) => {
            read_call(SessionCall::CredentialStatus(c, o, id), bytes)
        }
        SessionRoute::Start(..)
        | SessionRoute::Stop(..)
        | SessionRoute::Approval(..)
        | SessionRoute::ClearStall(..) => prepare_agent_mut(route, bytes),
        _ => prepare_provider_mut(route, bytes),
    }
}

fn prepare_agent_mut(route: SessionRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    match route {
        SessionRoute::Start(c, o, id) => decode_mut(
            bytes,
            |b: &StartBody| &b.command_id,
            |b| {
                SessionCall::Start(
                    c,
                    o,
                    MemberSessionStartParams {
                        command_id: b.command_id,
                        employee_id: id,
                        instruction: b.instruction,
                        workspace: b.workspace,
                    },
                )
            },
        ),
        SessionRoute::Stop(c, o, id) => decode_mut(
            bytes,
            |b: &StopBody| &b.command_id,
            |b| {
                SessionCall::Stop(
                    c,
                    o,
                    MemberSessionStopParams {
                        command_id: b.command_id,
                        employee_id: id,
                        reason: b.reason,
                    },
                )
            },
        ),
        SessionRoute::Approval(c, o, id) => decode_mut(
            bytes,
            |b: &ApprovalBody| &b.command_id,
            |b| {
                SessionCall::Approval(
                    c,
                    o,
                    MemberSessionApprovalDecideParams {
                        command_id: b.command_id,
                        employee_id: id,
                        approval_id: b.approval_id,
                        decision: b.decision,
                        reason: b.reason,
                    },
                )
            },
        ),
        SessionRoute::ClearStall(c, o, id) => decode_mut(
            bytes,
            |b: &CommandOnlyBody| &b.command_id,
            |b| {
                SessionCall::ClearStall(
                    c,
                    o,
                    MemberSessionClearStallParams {
                        command_id: b.command_id,
                        employee_id: id,
                    },
                )
            },
        ),
        _ => unreachable!(),
    }
}

fn prepare_provider_mut(route: SessionRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    match route {
        SessionRoute::EnrollStart(c, o, id) => decode_mut(
            bytes,
            |b: &EnrollStartBody| &b.command_id,
            |b| {
                SessionCall::EnrollStart(
                    c,
                    o,
                    ProviderEnrollmentStartParams {
                        command_id: b.command_id,
                        connection_id: id,
                        provider: b.provider,
                    },
                )
            },
        ),
        SessionRoute::EnrollSubmit(c, o, id, login_id) => decode_mut(
            bytes,
            |b: &EnrollSubmitBody| &b.command_id,
            |b| {
                SessionCall::EnrollSubmit(
                    c,
                    o,
                    ProviderEnrollmentSubmitParams {
                        command_id: b.command_id,
                        connection_id: id,
                        login_id,
                        code: b.code,
                    },
                )
            },
        ),
        SessionRoute::EnrollCancel(c, o, id, login_id) => decode_mut(
            bytes,
            |b: &CommandOnlyBody| &b.command_id,
            |b| {
                SessionCall::EnrollCancel(
                    c,
                    o,
                    ProviderEnrollmentCancelParams {
                        command_id: b.command_id,
                        connection_id: id,
                        login_id,
                    },
                )
            },
        ),
        SessionRoute::CredentialRevoke(c, o, id) => decode_mut(
            bytes,
            |b: &CommandOnlyBody| &b.command_id,
            |b| {
                SessionCall::CredentialRevoke(
                    c,
                    o,
                    ProviderCredentialRevokeParams {
                        command_id: b.command_id,
                        connection_id: id,
                    },
                )
            },
        ),
        _ => unreachable!(),
    }
}

async fn run_office<C: org_proto::Call>(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    params: &C::Params,
    is_read: bool,
) -> ApiReply {
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(err) => return map_host_failure(err),
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .call_typed::<C>(&auth, &office_id, params, is_read)
        .await;
    if is_not_authorized(&reply) {
        office_id = match revalidate_and_resolve_hq(api, pair, org_id).await {
            Ok(id) => id,
            Err(err) => return map_host_failure(err),
        };
        reply = api
            .offices
            .call_typed::<C>(&auth, &office_id, params, is_read)
            .await;
    }
    match reply {
        Ok(res) => result(res),
        Err(err) => map_office_failure(err),
    }
}

fn map_host_failure(error: Failure) -> ApiReply {
    match error {
        Failure::Refused(_) => unavailable(),
        other => failure(other),
    }
}

fn is_not_authorized<T>(reply: &Result<T, OfficeFailure>) -> bool {
    matches!(
        reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    )
}

fn map_office_failure(error: OfficeFailure) -> ApiReply {
    match error {
        OfficeFailure::BeforeDispatch(e) => failure(e),
        OfficeFailure::Transport => unavailable(),
        OfficeFailure::Refused(err) => {
            let web = WebError::from(err);
            let status = match web.code {
                WebErrorCode::InvalidRequest | WebErrorCode::InvalidName | WebErrorCode::Cycle => {
                    StatusCode::BAD_REQUEST
                }
                WebErrorCode::NotAuthorized | WebErrorCode::OwnerProtected => StatusCode::FORBIDDEN,
                WebErrorCode::NotFound => return not_found(),
                WebErrorCode::Conflict
                | WebErrorCode::AlreadyExists
                | WebErrorCode::NotEmpty
                | WebErrorCode::OperationReused => StatusCode::CONFLICT,
                WebErrorCode::Expired => StatusCode::GONE,
                _ => return unavailable(),
            };
            let payload = ReadResponse::<Value>::Error(web);
            ApiReply::json(
                status,
                serde_json::to_value(payload).expect("gateway error serializes"),
            )
        }
    }
}

pub(super) async fn execute(api: &SessionApi, call: SessionCall) -> ApiReply {
    match call {
        SessionCall::Start(..)
        | SessionCall::Stop(..)
        | SessionCall::Inspect(..)
        | SessionCall::Grants(..)
        | SessionCall::Approval(..)
        | SessionCall::ClearStall(..) => execute_agent(api, call).await,
        _ => execute_provider(api, call).await,
    }
}

async fn execute_agent(api: &SessionApi, call: SessionCall) -> ApiReply {
    match call {
        SessionCall::Start(c, o, p) => {
            run_office::<MemberSessionStart>(api, c, &o, &p, false).await
        }
        SessionCall::Stop(c, o, p) => run_office::<MemberSessionStop>(api, c, &o, &p, false).await,
        SessionCall::Inspect(c, o, employee_id) => {
            let params = MemberSessionInspectParams {
                employee_id: Some(employee_id),
            };
            run_office::<MemberSessionInspect>(api, c, &o, &params, true).await
        }
        SessionCall::Grants(c, o, employee_id) => {
            let params = MemberSessionGrantsParams {
                employee_id: Some(employee_id),
            };
            run_office::<MemberSessionGrants>(api, c, &o, &params, true).await
        }
        SessionCall::Approval(c, o, p) => {
            run_office::<MemberSessionApprovalDecide>(api, c, &o, &p, false).await
        }
        SessionCall::ClearStall(c, o, p) => {
            run_office::<MemberSessionClearStall>(api, c, &o, &p, false).await
        }
        _ => unreachable!(),
    }
}

async fn execute_provider(api: &SessionApi, call: SessionCall) -> ApiReply {
    match call {
        SessionCall::EnrollStart(c, o, p) => {
            run_office::<ProviderEnrollmentStart>(api, c, &o, &p, false).await
        }
        SessionCall::EnrollStatus(c, o, connection_id, login_id) => {
            let params = ProviderEnrollmentStatusParams {
                connection_id,
                login_id,
            };
            run_office::<ProviderEnrollmentStatus>(api, c, &o, &params, true).await
        }
        SessionCall::EnrollSubmit(c, o, p) => {
            run_office::<ProviderEnrollmentSubmit>(api, c, &o, &p, false).await
        }
        SessionCall::EnrollCancel(c, o, p) => {
            run_office::<ProviderEnrollmentCancel>(api, c, &o, &p, false).await
        }
        SessionCall::CredentialStatus(c, o, connection_id) => {
            let params = ProviderCredentialStatusParams { connection_id };
            run_office::<ProviderCredentialStatus>(api, c, &o, &params, true).await
        }
        SessionCall::CredentialRevoke(c, o, p) => {
            run_office::<ProviderCredentialRevoke>(api, c, &o, &p, false).await
        }
        _ => unreachable!(),
    }
}
