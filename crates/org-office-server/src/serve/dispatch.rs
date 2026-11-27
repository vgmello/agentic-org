//! Request dispatch: `hello`, recorded answers, and routing by method family.

use org_office_sessions::PeerIdentity;
use org_proto::directory::{StatusSet, Who};
use org_proto::msg::MsgBind;
use org_proto::{ApiError, ErrorCode, Hello, OrgSnapshotCall, Request, Response};

use super::auth::Binding;
use super::binding::bind_connection;
use super::external::{answer_operator, external_may_call, external_refusal};
use super::messaging::answer_message;
use super::org::{answer_org_command, org_snapshot, status_set, who};
use super::sessions::project_or_hiring;
use super::shared::{live_owner, Shared};
use super::wire::{bind_error, internal};

/// X-02 §3, B1-07.1: this office socket supports only v1 until instance-aware
/// launch is available. Every call therefore stays at v1, including legacy-first
/// calls and repeated hellos; the wider SUPPORTED_API is not active here yet.
pub(super) fn hello_response(request: &Request, shared: &Shared) -> Response {
    match request.params::<Hello>() {
        Ok(params) if params.api_range().contains(org_proto::ApiRange::V1.min) => {
            Response::ok::<Hello>(request.id, &shared.hello)
                .unwrap_or_else(|_| internal(request.id))
        }
        Ok(_) => Response::error(
            request.id,
            ApiError::new(
                ErrorCode::UnsupportedVersion,
                "the office supports only API version 1",
            ),
        ),
        Err(_) => Response::error(
            request.id,
            ApiError::new(ErrorCode::InvalidRequest, "invalid hello parameters"),
        ),
    }
}

pub(super) fn answer_and_record(
    request: &Request,
    shared: &Shared,
    binding: &mut Binding,
    peer: &PeerIdentity,
) -> Response {
    if binding.external_session().is_some() && !external_may_call(&request.method) {
        return external_refusal(request.id);
    }
    let response = answer_with_peer(request, shared, binding, peer);
    if matches!(response.outcome, org_proto::Outcome::Result(_))
        && !request.is::<MsgBind>()
        && !binding.via_grant()
    {
        if let (Some(member), Some(recorder)) =
            (binding.member(), &shared.ports.record_agent_command)
        {
            recorder(member, &request.method);
        }
    }
    response
}

pub(super) fn answer(request: &Request, shared: &Shared, binding: &mut Binding) -> Response {
    answer_with_peer(request, shared, binding, &PeerIdentity::none())
}

fn answer_with_peer(
    request: &Request,
    shared: &Shared,
    binding: &mut Binding,
    peer: &PeerIdentity,
) -> Response {
    // The Chief of Staff's `caller ∩ tess` gate, before any method runs, and on what the
    // method answers (`TASK-80.1`).
    let delegation = match super::delegation::gate(request, shared, binding) {
        Ok(delegation) => delegation,
        Err(refused) => return refused,
    };
    let response = answer_ungated(request, shared, binding, peer, delegation.as_ref());
    match delegation {
        Some(delegation) => {
            super::delegation::narrow(request, shared, binding, &delegation, response)
        }
        None => response,
    }
}

fn answer_ungated(
    request: &Request,
    shared: &Shared,
    binding: &mut Binding,
    peer: &PeerIdentity,
    delegation: Option<&super::delegation::Delegation>,
) -> Response {
    if let Some(response) = answer_operator(request, shared, binding) {
        return response;
    }
    if let Some(response) = answer_inbox_or_operation(request, shared, binding, delegation) {
        return response;
    }
    if let Some(response) = answer_directory_or_session(request, shared, binding, peer, delegation)
    {
        return response;
    }
    if let Some(response) = answer_org_command(request, shared, binding) {
        return response;
    }
    Response::error(
        request.id,
        ApiError::new(
            ErrorCode::UnknownMethod,
            format!("this office does not know `{}`", request.method),
        ),
    )
}

fn answer_inbox_or_operation(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
    delegation: Option<&super::delegation::Delegation>,
) -> Option<Response> {
    if request.is::<org_proto::inbox::InboxList>() {
        let inbox = shared.inbox.as_ref()?;
        let delegated_owner = super::delegation::delegated_owner_read_actor(request, delegation);
        if !matches!(live_owner(shared, binding), Ok(true)) && delegated_owner.is_none() {
            return Some(bind_error(
                request.id,
                ErrorCode::NotAuthorized,
                "inbox is for the live owner",
            ));
        }
        if let Some(mb) = &shared.ports.mailbox {
            if let Some(owner_id) = delegated_owner.or_else(|| binding.member()) {
                let _ = mb.release_due_snoozes_for(owner_id);
            }
        }
        return Some(match request.params::<org_proto::inbox::InboxList>() {
            Ok(params) => match inbox.list(params) {
                Ok(result) => Response::ok::<org_proto::inbox::InboxList>(request.id, &result)
                    .unwrap_or_else(|_| internal(request.id)),
                Err(error) => Response::error(request.id, error),
            },
            Err(_) => bind_error(
                request.id,
                ErrorCode::InvalidRequest,
                "invalid inbox list parameters",
            ),
        });
    }
    if request.is::<org_proto::OperationGet>() {
        return Some(answer_operation_get(request, shared, binding));
    }
    None
}

fn answer_operation_get(request: &Request, shared: &Shared, binding: &Binding) -> Response {
    let params = match request.params::<org_proto::OperationGet>() {
        Ok(params) if org_proto::is_uuid_v7(&params.id) => params,
        _ => {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "invalid operation lookup parameters",
                ),
            )
        }
    };
    if params.kind != org_proto::OperationKind::Org {
        return Response::error(
            request.id,
            ApiError::new(
                ErrorCode::Unavailable,
                "this operation kind is not available yet",
            ),
        );
    }
    if !binding.via_grant() {
        return Response::error(
            request.id,
            ApiError::new(ErrorCode::NotFound, "operation not found"),
        );
    }
    let Some(member) = binding.member() else {
        return internal(request.id);
    };
    match (shared.ports.operation_get)(member, &params.id) {
        Ok(Ok(outcome)) => Response::ok::<org_proto::OperationGet>(request.id, &outcome)
            .unwrap_or_else(|_| internal(request.id)),
        Ok(Err(refusal)) => Response::error(request.id, refusal),
        Err(_) => internal(request.id),
    }
}

fn answer_directory_or_session(
    request: &Request,
    shared: &Shared,
    binding: &mut Binding,
    peer: &PeerIdentity,
    delegation: Option<&super::delegation::Delegation>,
) -> Option<Response> {
    if request.is::<OrgSnapshotCall>() {
        return Some(org_snapshot(request, shared, binding));
    }
    if let Some(response) = project_or_hiring(request, shared, binding, delegation, answer) {
        return Some(response);
    }
    if request.is::<Who>() {
        return Some(who(request, shared, binding));
    }
    if request.is::<StatusSet>() {
        return Some(status_set(request, shared, binding));
    }
    if request.is::<MsgBind>() {
        return Some(bind_connection(request, shared, binding, peer));
    }
    answer_message(request, shared, binding)
}

#[cfg(test)]
impl Shared {
    /// Test helper to invoke `serve::answer` directly with an explicit binding.
    pub(crate) fn answer_for_test(
        &self,
        request: &Request,
        member: &str,
        via_grant: bool,
    ) -> Response {
        let mut binding = Binding::Member {
            id: member.to_owned(),
            via_grant,
        };
        answer(request, self, &mut binding)
    }
}
