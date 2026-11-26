//! Preparing and executing agent-session and provider-connection calls against the HQ office.

use hyper::StatusCode;
use org_proto::sessions::{
    MemberSessionApprovalDecide, MemberSessionApprovalDecideParams, MemberSessionClearStall,
    MemberSessionClearStallParams, MemberSessionGrants, MemberSessionGrantsParams,
    MemberSessionInspect, MemberSessionInspectParams, MemberSessionList, MemberSessionStart,
    MemberSessionStartParams, MemberSessionStop, MemberSessionStopParams, MemberSessionTranscript,
    ProviderCredentialRevoke, ProviderCredentialRevokeParams, ProviderCredentialStatus,
    ProviderCredentialStatusParams, ProviderEnrollmentCancel, ProviderEnrollmentCancelParams,
    ProviderEnrollmentStart, ProviderEnrollmentStartParams, ProviderEnrollmentStatus,
    ProviderEnrollmentStatusParams, ProviderEnrollmentSubmit, ProviderEnrollmentSubmitParams,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde_json::Value;

use super::super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::super::{failure, not_found, result, unavailable, Call, CallerAuth, SessionApi};
use super::{
    decode_mut, read_call, ApprovalBody, CommandOnlyBody, EnrollStartBody, EnrollSubmitBody,
    SessionCall, SessionRoute, StartBody, StopBody,
};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub(in crate::api) fn prepare(
    route: SessionRoute,
    bytes: &[u8],
) -> Result<PreparedCall<Call>, ApiReply> {
    match route {
        SessionRoute::Inspect(c, o, id) => read_call(SessionCall::Inspect(c, o, id), bytes),
        SessionRoute::ListSessions(c, o, p) => read_call(SessionCall::ListSessions(c, o, p), bytes),
        SessionRoute::Transcript(c, o, p) => read_call(SessionCall::Transcript(c, o, p), bytes),
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

pub(super) fn prepare_agent_mut(
    route: SessionRoute,
    bytes: &[u8],
) -> Result<PreparedCall<Call>, ApiReply> {
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

pub(super) fn prepare_provider_mut(
    route: SessionRoute,
    bytes: &[u8],
) -> Result<PreparedCall<Call>, ApiReply> {
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

pub(super) async fn run_office<C: org_proto::Call>(
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

pub(super) fn map_host_failure(error: Failure) -> ApiReply {
    match error {
        Failure::Refused(_) => unavailable(),
        other => failure(other),
    }
}

pub(super) fn is_not_authorized<T>(reply: &Result<T, OfficeFailure>) -> bool {
    matches!(
        reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    )
}

pub(super) fn map_office_failure(error: OfficeFailure) -> ApiReply {
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

pub(in crate::api) async fn execute(api: &SessionApi, call: SessionCall) -> ApiReply {
    match call {
        SessionCall::Start(..)
        | SessionCall::Stop(..)
        | SessionCall::Inspect(..)
        | SessionCall::ListSessions(..)
        | SessionCall::Transcript(..)
        | SessionCall::Grants(..)
        | SessionCall::Approval(..)
        | SessionCall::ClearStall(..) => execute_agent(api, call).await,
        _ => execute_provider(api, call).await,
    }
}

pub(super) async fn execute_agent(api: &SessionApi, call: SessionCall) -> ApiReply {
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
        SessionCall::ListSessions(c, o, p) => {
            run_office::<MemberSessionList>(api, c, &o, &p, true).await
        }
        SessionCall::Transcript(c, o, p) => {
            run_office::<MemberSessionTranscript>(api, c, &o, &p, true).await
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

pub(super) async fn execute_provider(api: &SessionApi, call: SessionCall) -> ApiReply {
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
