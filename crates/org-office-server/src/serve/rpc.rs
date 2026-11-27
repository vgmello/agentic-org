//! Office service ports: federation, providers, costs, processes, documentation, Git, tasks and
//! attachments.

use org_proto::{ApiError, ErrorCode, Request, Response};

use super::auth::Binding;
use super::shared::Shared;

pub(super) fn federation_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    if !crate::federation_calls::is_federation_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if let Some(federation) = &shared.ports.federation {
        if let Some(response) = federation(Some(member), request) {
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

pub(super) fn providers_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    if !crate::provider_calls::is_provider_pool_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if let Some(providers) = &shared.ports.providers {
        if let Some(response) = providers(Some(member), request) {
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

pub(super) fn costs_rpc(request: &Request, shared: &Shared, binding: &Binding) -> Option<Response> {
    if !crate::cost_calls::is_cost_or_budget_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if let Some(costs) = &shared.ports.costs {
        if let Some(response) = costs(Some(member), request) {
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

pub(super) fn processes_rpc(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    if !crate::process_calls::is_process_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if let Some(processes) = &shared.ports.processes {
        if let Some(response) = processes(Some(member), request) {
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

pub(super) fn docs_rpc(request: &Request, shared: &Shared, binding: &Binding) -> Option<Response> {
    if !crate::doc_calls::is_doc_method(request) {
        return None;
    }
    let Some(member) = binding.member() else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if let Some(docs) = &shared.ports.docs {
        if let Some(response) = docs(Some(member), request) {
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
    if let Some(git) = &shared.ports.git {
        if let Some(response) = git(Some(member), request) {
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
    if let Some(tasks) = &shared.ports.tasks {
        if let Some(response) = tasks(Some(member), request) {
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
    if let Some(attachments) = &shared.ports.attachments {
        if let Some(response) = attachments(Some(member), request) {
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
