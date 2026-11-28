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

/// Matches every Chief of Staff (`suggestion.*`, `ui.present`, `tess.*`) RPC using explicit
/// `request.is::<Call>()` checks so the host operation inventory AST scan recognizes each served
/// method directly inside `org-office-server`.
fn is_tess_method(request: &Request) -> bool {
    use org_proto::tess::{
        SuggestionApply, SuggestionCreate, SuggestionDismiss, SuggestionInspect, SuggestionList,
        SuggestionWithdraw, TessAsk, TessSessionReset, TessTurnCancel, TessTurnEvents, UiPresent,
    };
    request.is::<SuggestionCreate>()
        || request.is::<SuggestionList>()
        || request.is::<SuggestionInspect>()
        || request.is::<SuggestionWithdraw>()
        || request.is::<SuggestionDismiss>()
        || request.is::<SuggestionApply>()
        || request.is::<UiPresent>()
        || request.is::<TessAsk>()
        || request.is::<TessTurnEvents>()
        || request.is::<TessTurnCancel>()
        || request.is::<TessSessionReset>()
}

pub(super) fn tess_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
    dispatch_fn: fn(&Request, &Shared, &mut Binding) -> Response,
) -> Option<Response> {
    if !is_tess_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if org_tess::requires_human_grant(request) && !binding.via_grant() {
        return Some(Response::error(
            request.id,
            ApiError::new(
                ErrorCode::NotAuthorized,
                "this Chief of Staff action requires a verified human HostChannel grant",
            ),
        ));
    }
    let Some(handler) = shared.ports.tess.as_ref() else {
        return Some(Response::error(
            request.id,
            ApiError::new(
                ErrorCode::UnknownMethod,
                format!("this office does not know `{}`", request.method),
            ),
        ));
    };
    let apply_exec = |caller_id: &str, sub_req: &Request| {
        let mut human_binding = Binding::Member {
            id: caller_id.to_owned(),
            via_grant: true,
        };
        dispatch_fn(sub_req, shared, &mut human_binding)
    };
    let exec_ref: Option<&org_tess::ApplyExecFn<'_>> = request
        .is::<org_proto::tess::SuggestionApply>()
        .then_some(&apply_exec as &org_tess::ApplyExecFn<'_>);
    Some(
        handler(
            Some(member),
            binding.via_grant(),
            &shared.stopping,
            exec_ref,
            request,
        )
        .unwrap_or_else(|| {
            Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::UnknownMethod,
                    format!("this office does not know `{}`", request.method),
                ),
            )
        }),
    )
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

/// `copilot.features` and `copilot.draft` (`TASK-60` AC #3): strictly requires a verified human
/// grant on a bound connection (`binding.via_grant()`).
pub(super) fn copilot_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    if !crate::copilot_calls::is_copilot_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(crate::copilot_calls::not_authorized(request.id));
    };
    if !binding.via_grant() {
        return Some(crate::copilot_calls::not_authorized(request.id));
    }
    Some(dispatch_actor_port(
        request,
        member,
        shared.ports.copilot.as_ref(),
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
    if (request.is::<org_proto::git::RepoRegister>()
        || request.is::<org_proto::git::RepoAttach>()
        || request.is::<org_proto::git::RepoCredentialSet>()
        || request.is::<org_proto::git::RepoCredentialStatus>()
        || request.is::<org_proto::git::RepoCredentialRevoke>())
        && !binding.via_grant()
    {
        return Some(Response::error(
            request.id,
            ApiError::new(
                ErrorCode::NotAuthorized,
                "repository registration, attachment, and credential management require a verified owner grant",
            ),
        ));
    }
    Some(dispatch_actor_port(
        request,
        member,
        shared.ports.git.as_ref(),
    ))
}

pub(super) fn reviews_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    crate::review_calls::is_review_method(request)
        .then(|| dispatch_bound_port(request, binding, None, shared.ports.reviews.as_ref()))
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
    if let Some(handler) = shared.ports.tasks.as_ref() {
        if let Some(response) = handler(Some(member), binding.via_grant(), request) {
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

pub(super) fn attachments_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
    delegation: Option<&Delegation>,
) -> Option<Response> {
    if !crate::attachment_calls::is_attachment_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if let Some(handler) = shared.ports.attachments.as_ref() {
        if let Some(response) = handler(Some(member), delegation, request) {
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

pub(super) fn resume_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    if !crate::resume_calls::is_resume_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    let session_mode = match binding {
        Binding::External { .. } => Some(org_proto::external_auth::SessionMode::External),
        Binding::Member {
            via_grant: false, ..
        } => Some(org_proto::external_auth::SessionMode::Managed),
        Binding::Member {
            via_grant: true, ..
        }
        | Binding::Anonymous => None,
    };
    if let Some(handler) = shared.ports.resume.as_ref() {
        if let Some(response) = handler(Some(member), session_mode, request) {
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
