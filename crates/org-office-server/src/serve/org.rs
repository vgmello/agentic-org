//! Organization commands, the snapshot, projects, `who`, `status.set`, and agent stop on lifecycle.

use std::sync::atomic::Ordering;

use org_office::rules::OrgCommand;
use org_proto::directory::{StatusSet, Who};
use org_proto::org::TeamCreate;
use org_proto::projects::{ProjectInspect, ProjectList};
use org_proto::{AgentStatus, ApiError, ErrorCode, OrgSnapshotCall, Request, Response};

use super::auth::Binding;
use super::shared::Shared;
use super::wire::{bind_error, internal};
use super::SnapshotFailure;

pub(super) fn answer_org_command(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    match OrgCommand::parse_request(request) {
        Ok(None) => None,
        Ok(Some(_)) | Err(_) if !binding.via_grant() => Some(Response::error(
            request.id,
            ApiError::new(
                ErrorCode::NotAuthorized,
                "organization commands are for the app",
            ),
        )),
        Err(refusal) => Some(Response::error(request.id, refusal)),
        Ok(Some(parsed)) => {
            let Some(member) = binding.member() else {
                return Some(internal(request.id));
            };
            let dry_run = parsed.dry_run;
            let stop_target = (!dry_run)
                .then(|| org_command_stop_target(&parsed.command))
                .flatten();
            Some(
                match (shared.ports.submit)(member, parsed.command, dry_run) {
                    Ok(Ok(accepted)) => {
                        if let Some((emp_id, reason)) = stop_target {
                            stop_agent_and_revoke(shared, &emp_id, reason);
                        }
                        Response::ok::<TeamCreate>(request.id, &accepted)
                            .unwrap_or_else(|_| internal(request.id))
                    }
                    Ok(Err(refusal)) => Response::error(request.id, refusal),
                    Err(_) => internal(request.id),
                },
            )
        }
    }
}

/// `org.snapshot`: the whole organization for a bound connection.
pub(super) fn org_snapshot(request: &Request, shared: &Shared, binding: &Binding) -> Response {
    if !binding.is_bound() {
        return Response::error(
            request.id,
            ApiError::new(
                ErrorCode::NotAuthorized,
                "bind before reading the organization",
            ),
        );
    }
    match (shared.ports.snapshot)() {
        Ok(snapshot) => Response::ok::<OrgSnapshotCall>(request.id, &snapshot)
            .unwrap_or_else(|_| internal(request.id)),
        Err(SnapshotFailure::Unavailable) => Response::error(
            request.id,
            ApiError::new(
                ErrorCode::Unavailable,
                "organization snapshot is not available yet",
            ),
        ),
        Err(SnapshotFailure::Storage) => internal(request.id),
    }
}

pub(super) fn project_read(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    if request.is::<ProjectList>() {
        Some(project_list(request, shared, binding))
    } else if request.is::<ProjectInspect>() {
        Some(project_inspect(request, shared, binding))
    } else {
        None
    }
}

/// `project.list`: lists projects in the organization for a bound connection.
fn project_list(request: &Request, shared: &Shared, binding: &Binding) -> Response {
    if !binding.is_bound() {
        return Response::error(
            request.id,
            ApiError::new(ErrorCode::NotAuthorized, "bind before reading projects"),
        );
    }
    let params = match request.params::<ProjectList>() {
        Ok(params) => params,
        Err(_) => {
            return Response::error(
                request.id,
                ApiError::new(ErrorCode::InvalidRequest, "invalid project.list params"),
            );
        }
    };
    match (shared.ports.projects_list)(&params) {
        Ok(Ok(listed)) => Response::ok::<ProjectList>(request.id, &listed)
            .unwrap_or_else(|_| internal(request.id)),
        Ok(Err(refusal)) => Response::error(request.id, refusal),
        Err(_) => internal(request.id),
    }
}

/// `project.inspect`: inspects one project by ID, `P-<number>`, or slug for a bound connection.
fn project_inspect(request: &Request, shared: &Shared, binding: &Binding) -> Response {
    if !binding.is_bound() {
        return Response::error(
            request.id,
            ApiError::new(ErrorCode::NotAuthorized, "bind before reading projects"),
        );
    }
    let params = match request.params::<ProjectInspect>() {
        Ok(params) => params,
        Err(_) => {
            return Response::error(
                request.id,
                ApiError::new(ErrorCode::InvalidRequest, "invalid project.inspect params"),
            );
        }
    };
    match (shared.ports.project_inspect)(&params) {
        Ok(Ok(project)) => Response::ok::<ProjectInspect>(request.id, &project)
            .unwrap_or_else(|_| internal(request.id)),
        Ok(Err(refusal)) => Response::error(request.id, refusal),
        Err(_) => internal(request.id),
    }
}

/// `who`: the directory, for a bound connection (`not_bound` otherwise, as that call's contract
/// says). The binding is checked before the params are read.
pub(super) fn who(request: &Request, shared: &Shared, binding: &Binding) -> Response {
    if !binding.is_bound() {
        return Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "bind before asking who"),
        );
    }
    let Ok(params) = request.params::<Who>() else {
        return bind_error(
            request.id,
            ErrorCode::InvalidRequest,
            "the params of `who` are not valid",
        );
    };
    match (shared.ports.who)(&params) {
        Ok(Ok(result)) => {
            Response::ok::<Who>(request.id, &result).unwrap_or_else(|_| internal(request.id))
        }
        Ok(Err(refusal)) => Response::error(request.id, refusal),
        Err(_) => internal(request.id),
    }
}

/// `status.set`: an agent-bound connection's own status line. The member is the binding's, never
/// the call's. Unbound is `not_bound`; a person or the owner (acting through a grant) is
/// `not_authorized`. The binding is checked before the params are read.
pub(super) fn status_set(request: &Request, shared: &Shared, binding: &Binding) -> Response {
    let member = match binding {
        Binding::Member {
            id,
            via_grant: false,
        }
        | Binding::External { id, .. } => id,
        Binding::Member { .. } => {
            return bind_error(
                request.id,
                ErrorCode::NotAuthorized,
                "only an agent sets its status",
            )
        }
        Binding::Anonymous => {
            return bind_error(
                request.id,
                ErrorCode::NotBound,
                "bind as an agent to set a status",
            )
        }
    };
    let Ok(params) = request.params::<StatusSet>() else {
        return bind_error(
            request.id,
            ErrorCode::InvalidRequest,
            "the params of `status.set` are not valid",
        );
    };
    match (shared.ports.status)(member, &params) {
        Ok(Ok(result)) => {
            Response::ok::<StatusSet>(request.id, &result).unwrap_or_else(|_| internal(request.id))
        }
        Ok(Err(refusal)) => Response::error(request.id, refusal),
        Err(_) => internal(request.id),
    }
}

fn org_command_stop_target(command: &OrgCommand) -> Option<(String, &'static str)> {
    match command {
        OrgCommand::MemberSetAgentStatus(p) if p.status == AgentStatus::Paused => {
            Some((p.member_id.clone(), "agent paused"))
        }
        OrgCommand::MemberOffboard(p) => Some((p.member_id.clone(), "agent offboarded")),
        _ => None,
    }
}

pub(super) fn revoke_bound_member(shared: &Shared, member_id: &str) {
    revoke_bound_where(shared, member_id, |_| true);
}

/// Closes the connections bound as `member_id` whose external lease session (`None` for a
/// managed or grant binding) `close` picks: their held calls end now and their next call is
/// refused. A connection bound with a lease the caller spares (a fresh approval's) keeps going.
pub(super) fn revoke_bound_where(
    shared: &Shared,
    member_id: &str,
    close: impl Fn(Option<&str>) -> bool,
) {
    let bound = shared.bound.lock().unwrap_or_else(|p| p.into_inner());
    for (_, control) in bound.values().filter(|(m, _)| m == member_id) {
        if !close(control.external_session().as_deref()) {
            continue;
        }
        control.revoked.store(true, Ordering::SeqCst);
        control.generation.fetch_add(1, Ordering::SeqCst);
        control.held.cancel_all();
    }
}

fn stop_agent_and_revoke(shared: &Shared, employee_id: &str, reason: &str) {
    if let Some(hook) = &shared.ports.on_agent_stop {
        hook(employee_id, reason);
    }
    revoke_bound_member(shared, employee_id);
}

pub(super) fn notify_agent_stop_on_lifecycle(request: &Request, shared: &Shared) {
    if request.is::<org_proto::hiring::AgentSetStatus>() {
        if let Ok(params) = request.params::<org_proto::hiring::AgentSetStatus>() {
            if params.paused {
                stop_agent_and_revoke(shared, &params.member_id, "agent paused");
            }
        }
    } else if request.is::<org_proto::hiring::AgentOffboard>() {
        if let Ok(params) = request.params::<org_proto::hiring::AgentOffboard>() {
            stop_agent_and_revoke(shared, &params.member_id, "agent offboarded");
        }
    }
}
