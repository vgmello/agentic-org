//! Office service ports: federation, providers, costs, processes, documentation, search, Git,
//! tasks and attachments.

use org_proto::{ApiError, ErrorCode, Request, Response};

use super::auth::Binding;
use super::delegation::{delegated_owner_read_actor, Delegation};
use super::shared::Shared;
use super::HiringFn;

fn dispatch_actor_port(request: &Request, actor: &str, port: Option<&HiringFn>) -> Response {
    if let Some(handler) = port {
        if let Some(response) = handler(Some(actor), request) {
            return response;
        }
    }
    Response::error(
        request.id,
        ApiError::new(
            ErrorCode::UnknownMethod,
            format!("this office does not know `{}`", request.method),
        ),
    )
}

fn dispatch_bound_port(
    request: &Request,
    binding: &Binding,
    delegation: Option<&Delegation>,
    port: Option<&HiringFn>,
) -> Response {
    let Some(member) = binding.member() else {
        return Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        );
    };
    let actor = delegated_owner_read_actor(request, delegation).unwrap_or(member);
    dispatch_actor_port(request, actor, port)
}

pub(super) fn federation_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    crate::federation_calls::is_federation_method(request)
        .then(|| dispatch_bound_port(request, binding, None, shared.ports.federation.as_ref()))
}

pub(super) fn providers_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
    delegation: Option<&Delegation>,
) -> Option<Response> {
    crate::provider_calls::is_provider_pool_method(request).then(|| {
        dispatch_bound_port(
            request,
            binding,
            delegation,
            shared.ports.providers.as_ref(),
        )
    })
}

pub(super) fn costs_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
    delegation: Option<&Delegation>,
) -> Option<Response> {
    crate::cost_calls::is_cost_or_budget_method(request)
        .then(|| dispatch_bound_port(request, binding, delegation, shared.ports.costs.as_ref()))
}

fn is_tess_method(request: &Request) -> bool {
    use org_proto::tess::{
        SuggestionCreate, SuggestionDismiss, SuggestionInspect, SuggestionList, SuggestionWithdraw,
        UiPresent,
    };
    request.is::<SuggestionCreate>()
        || request.is::<SuggestionList>()
        || request.is::<SuggestionInspect>()
        || request.is::<SuggestionWithdraw>()
        || request.is::<SuggestionDismiss>()
        || request.is::<UiPresent>()
}

pub(super) fn tess_rpc(request: &Request, shared: &Shared, binding: &Binding) -> Option<Response> {
    is_tess_method(request)
        .then(|| dispatch_bound_port(request, binding, None, shared.ports.tess.as_ref()))
}

pub(super) fn processes_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    crate::process_calls::is_process_method(request)
        .then(|| dispatch_bound_port(request, binding, None, shared.ports.processes.as_ref()))
}

pub(super) fn docs_rpc(request: &Request, shared: &Shared, binding: &Binding) -> Option<Response> {
    crate::doc_calls::is_doc_method(request)
        .then(|| dispatch_bound_port(request, binding, None, shared.ports.docs.as_ref()))
}

/// `search.query` (`TASK-91`): a connection bound to nobody is refused `NotAuthorized`, as a
/// member who may not search is.
pub(super) fn search_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    if !crate::search_calls::is_search_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(crate::search_calls::not_authorized(request.id));
    };
    Some(dispatch_actor_port(
        request,
        member,
        shared.ports.search.as_ref(),
    ))
}

pub(super) fn git_rpc(request: &Request, shared: &Shared, binding: &Binding) -> Option<Response> {
    if !crate::git_calls::is_git_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if (request.is::<org_proto::git::RepoRegister>() || request.is::<org_proto::git::RepoAttach>())
        && !binding.via_grant()
    {
        return Some(Response::error(
            request.id,
            ApiError::new(
                ErrorCode::NotAuthorized,
                "repository registration and project attachment require a verified owner grant",
            ),
        ));
    }
    Some(dispatch_actor_port(
        request,
        member,
        shared.ports.git.as_ref(),
    ))
}

pub(super) fn tasks_rpc(request: &Request, shared: &Shared, binding: &Binding) -> Option<Response> {
    if !crate::task_calls::is_task_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if (request.is::<org_proto::tasks::OrganizationDefaultDodSet>()
        || request.is::<org_proto::tasks::ProjectDefaultDodSet>())
        && !binding.via_grant()
    {
        return Some(Response::error(
            request.id,
            ApiError::new(
                ErrorCode::NotAuthorized,
                "setting default definitions of done requires a verified person grant",
            ),
        ));
    }
    Some(dispatch_actor_port(
        request,
        member,
        shared.ports.tasks.as_ref(),
    ))
}

pub(super) fn attachments_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    crate::attachment_calls::is_attachment_method(request)
        .then(|| dispatch_bound_port(request, binding, None, shared.ports.attachments.as_ref()))
}
