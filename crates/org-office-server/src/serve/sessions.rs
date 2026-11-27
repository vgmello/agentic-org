//! Sessions and HR dispatch: hiring, hiring policy, agent lifecycle, and session control.

use org_proto::{ApiError, ErrorCode, Request, Response};

use super::auth::Binding;
use super::org::{notify_agent_stop_on_lifecycle, project_read, revoke_bound_member};
use super::rpc::{
    attachments_rpc, copilot_rpc, costs_rpc, docs_rpc, federation_rpc, git_rpc, processes_rpc,
    providers_rpc, resume_rpc, search_rpc, tasks_rpc, tess_rpc,
};
use super::shared::Shared;

/// Dispatches `project.list` and `project.inspect` when `request` names one of them.
fn is_role_method(request: &Request) -> bool {
    use org_proto::hiring::{RoleCreate, RoleInspect, RoleList, RoleUpdate};
    request.is::<RoleList>()
        || request.is::<RoleInspect>()
        || request.is::<RoleCreate>()
        || request.is::<RoleUpdate>()
}

fn is_hr_hire_method(request: &Request) -> bool {
    use org_proto::hiring::{
        HrHire, HrHireConfirm, HrHireDraft, HrHireInspect, HrHireList, HrHireWithdraw,
    };
    request.is::<HrHireList>()
        || request.is::<HrHireInspect>()
        || request.is::<HrHireDraft>()
        || request.is::<HrHireConfirm>()
        || request.is::<HrHire>()
        || request.is::<HrHireWithdraw>()
}

fn is_agent_lifecycle_method(request: &Request) -> bool {
    use org_proto::hiring::{
        AgentExecutionGet, AgentExecutionSet, AgentMove, AgentOffboard, AgentSetStatus, AgentUpdate,
    };
    request.is::<AgentUpdate>()
        || request.is::<AgentMove>()
        || request.is::<AgentSetStatus>()
        || request.is::<AgentOffboard>()
        || request.is::<AgentExecutionGet>()
        || request.is::<AgentExecutionSet>()
}

fn is_hiring_policy_method(request: &Request) -> bool {
    use org_proto::hiring::{
        MemberSetMayHire, ModelProfileList, ProviderConnectionCreate, TeamLimitRequestCreate,
        TeamLimitRequestDecide, TeamLimitRequestList, TeamSetMaxMembers,
    };
    request.is::<MemberSetMayHire>()
        || request.is::<TeamSetMaxMembers>()
        || request.is::<TeamLimitRequestCreate>()
        || request.is::<TeamLimitRequestDecide>()
        || request.is::<TeamLimitRequestList>()
        || request.is::<ProviderConnectionCreate>()
        || request.is::<ModelProfileList>()
}

fn is_hiring_method(request: &Request) -> bool {
    is_role_method(request)
        || is_hr_hire_method(request)
        || is_agent_lifecycle_method(request)
        || is_hiring_policy_method(request)
}

/// A person's live turn controls (`TASK-103`) are session controls too; unlike a stop, they
/// keep the warm process, so they never revoke the agent's binding.
fn is_session_control_method(request: &Request) -> bool {
    use org_proto::sessions::{
        MemberSessionApprovalDecide, MemberSessionClearStall, MemberSessionGrants,
        MemberSessionInspect, MemberSessionInterrupt, MemberSessionList, MemberSessionStart,
        MemberSessionSteer, MemberSessionStop, MemberSessionTranscript,
    };
    request.is::<MemberSessionStart>()
        || request.is::<MemberSessionStop>()
        || request.is::<MemberSessionInterrupt>()
        || request.is::<MemberSessionSteer>()
        || request.is::<MemberSessionInspect>()
        || request.is::<MemberSessionList>()
        || request.is::<MemberSessionTranscript>()
        || request.is::<MemberSessionGrants>()
        || request.is::<MemberSessionApprovalDecide>()
        || request.is::<MemberSessionClearStall>()
}

fn is_provider_enrollment_method(request: &Request) -> bool {
    use org_proto::sessions::{
        ProviderCredentialRevoke, ProviderCredentialStatus, ProviderEnrollmentCancel,
        ProviderEnrollmentStart, ProviderEnrollmentStatus, ProviderEnrollmentSubmit,
    };
    request.is::<ProviderEnrollmentStart>()
        || request.is::<ProviderEnrollmentStatus>()
        || request.is::<ProviderEnrollmentSubmit>()
        || request.is::<ProviderEnrollmentCancel>()
        || request.is::<ProviderCredentialStatus>()
        || request.is::<ProviderCredentialRevoke>()
}

fn is_session_method(request: &Request) -> bool {
    is_session_control_method(request) || is_provider_enrollment_method(request)
}

pub(super) fn project_or_hiring(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
    delegation: Option<&super::delegation::Delegation>,
    dispatch_fn: fn(&Request, &Shared, &mut Binding) -> Response,
) -> Option<Response> {
    project_read(request, shared, binding)
        .or_else(|| hiring_rpc(request, shared, binding, delegation))
        .or_else(|| sessions_rpc(request, shared, binding, delegation))
        .or_else(|| tasks_rpc(request, shared, binding))
        .or_else(|| attachments_rpc(request, shared, binding, delegation))
        .or_else(|| git_rpc(request, shared, binding))
        .or_else(|| processes_rpc(request, shared, binding))
        .or_else(|| docs_rpc(request, shared, binding))
        .or_else(|| search_rpc(request, shared, binding))
        .or_else(|| copilot_rpc(request, shared, binding))
        .or_else(|| costs_rpc(request, shared, binding, delegation))
        .or_else(|| providers_rpc(request, shared, binding, delegation))
        .or_else(|| federation_rpc(request, shared, binding))
        .or_else(|| tess_rpc(request, shared, binding, dispatch_fn))
        .or_else(|| resume_rpc(request, shared, binding))
}

fn sessions_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
    delegation: Option<&super::delegation::Delegation>,
) -> Option<Response> {
    if !is_session_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    let actor =
        super::delegation::delegated_owner_read_actor(request, delegation).unwrap_or(member);
    let resolved = resolve_session_target_handle(request, shared);
    let effective = resolved.as_ref().unwrap_or(request);
    if let Some(sessions) = &shared.ports.sessions {
        if let Some(response) = sessions(Some(actor), effective) {
            if matches!(response.outcome, org_proto::Outcome::Result(_))
                && effective.is::<org_proto::sessions::MemberSessionStop>()
            {
                if let Ok(params) = effective.params::<org_proto::sessions::MemberSessionStop>() {
                    revoke_bound_member(shared, &params.employee_id);
                }
            }
            return Some(response);
        }
    }
    Some(Response::error(
        request.id,
        ApiError::new(
            ErrorCode::UnknownMethod,
            format!("this office does not know `{}`", request.method),
        ),
    ))
}

/// Resolves an `@handle` or handle `employee_id` on session read RPCs (`member.session_list`,
/// `member.session_inspect`, `member.session_grants`) to its canonical employee UUIDv7.
fn resolve_session_target_handle(request: &Request, shared: &Shared) -> Option<Request> {
    if !matches!(
        request.method.as_str(),
        "member.session_list" | "member.session_inspect" | "member.session_grants"
    ) {
        return None;
    }
    let raw = request.params.get("employee_id")?.as_str()?;
    let trimmed = org_office::directory::trim_address(raw);
    let reference = trimmed.strip_prefix('@').unwrap_or(trimmed);
    if org_proto::is_uuid_v7(reference) {
        return None;
    }
    let (member, _) = shared.ports.directory.resolve_member(reference).ok()??;
    let mut rewritten = request.clone();
    if let Some(obj) = rewritten.params.as_object_mut() {
        obj.insert("employee_id".into(), serde_json::Value::String(member.id));
    }
    Some(rewritten)
}

fn hiring_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
    delegation: Option<&super::delegation::Delegation>,
) -> Option<Response> {
    if !is_hiring_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    let actor =
        super::delegation::delegated_owner_read_actor(request, delegation).unwrap_or(member);
    if let Some(hiring) = &shared.ports.hiring {
        if let Some(response) = hiring(Some(actor), request) {
            if matches!(response.outcome, org_proto::Outcome::Result(_)) {
                notify_agent_stop_on_lifecycle(request, shared);
            }
            return Some(response);
        }
    }
    Some(Response::error(
        request.id,
        ApiError::new(
            ErrorCode::UnknownMethod,
            format!("this office does not know `{}`", request.method),
        ),
    ))
}
