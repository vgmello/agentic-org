//! The one authorization choke-point of the documentation tree (`TASK-55` AC #3,
//! `.agents/rules` §2): every `docs.*` read and write asks [`authorize`] on the same consistent
//! view it then reads or decides on, and nothing else decides.
//!
//! - Only an **active** employee is authorized for anything; anyone else (paused, offboarded,
//!   unknown) is `not_authorized` before a project is even resolved.
//! - **People** read and write every project's tree (Phase 1 organization policy).
//! - **Agents** read and write only the trees of projects they belong to: their home team, or
//!   a team above it, is the project's accountable `owner_team_id` or one of its
//!   `contributing_team_ids` (`doc-86`, as `crate::authz` admits project members).
//! - A project the caller may not see is `not_found` with the very message a missing project
//!   gets: neither its existence, its name nor its tree leaks.
//! - An archived project's tree is read-only.

use org_proto::docs::DOC_REF_MAX_BYTES;
use org_proto::{ApiError, ErrorCode};
use org_store::{EngineError, Id};

use super::ports::ProjectDocsRead;
use crate::organization::records::{Employee, EmployeeKind, EmployeeStatus};
use crate::projects::records::Project;

/// What a call does to a tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocsAccess {
    Read,
    Write,
}

/// A caller cleared for one project's tree.
#[derive(Clone, Debug)]
pub struct Authorized {
    pub caller: Employee,
    pub project: Project,
}

/// Why a call stops: a refusal the caller sees, or the storage under it failing.
#[derive(Debug)]
pub enum Denied {
    Refused(ApiError),
    Storage(EngineError),
}

impl From<EngineError> for Denied {
    fn from(error: EngineError) -> Self {
        Self::Storage(error)
    }
}

impl From<ApiError> for Denied {
    fn from(error: ApiError) -> Self {
        Self::Refused(error)
    }
}

/// The one answer for a project that is missing or that the caller may not see.
pub const PROJECT_NOT_FOUND: &str = "project not found";

pub fn refused(code: ErrorCode, message: impl Into<String>) -> Denied {
    Denied::Refused(ApiError::new(code, message.into()))
}

/// Whether `caller` belongs to `project` for its documentation: any person; an agent only
/// through its home team's chain (the home team and every team above it) holding the
/// accountable or a contributing team. A home team whose chain is broken admits nothing.
pub fn may_see<R: ProjectDocsRead>(
    view: &R,
    caller: &Employee,
    project: &Project,
) -> Result<bool, EngineError> {
    if caller.kind == EmployeeKind::Person {
        return Ok(true);
    }
    let Some(chain) = view.team_chain(&caller.home_team_id)? else {
        return Ok(false);
    };
    Ok(chain
        .iter()
        .any(|team| *team == project.owner_team_id || project.contributing_team_ids.contains(team)))
}

/// The active employee `caller_id` names, or `not_authorized`.
pub fn active_caller<R: ProjectDocsRead>(view: &R, caller_id: &str) -> Result<Employee, Denied> {
    let employee = match Id::parse(caller_id) {
        Ok(id) => view.employee(&id)?,
        Err(_) => None,
    };
    match employee {
        Some(employee) if employee.status == EmployeeStatus::Active => Ok(employee),
        Some(_) => Err(refused(
            ErrorCode::NotAuthorized,
            "only an active employee may use project documentation",
        )),
        None => Err(refused(
            ErrorCode::NotAuthorized,
            "the caller is not an employee of this organization",
        )),
    }
}

/// Decides whether `caller_id` may `access` the tree of the project `project_ref` names
/// (UUIDv7, `P-<number>` or slug), on `view`.
pub fn authorize<R: ProjectDocsRead>(
    view: &R,
    caller_id: &str,
    project_ref: &str,
    access: DocsAccess,
) -> Result<Authorized, Denied> {
    let caller = active_caller(view, caller_id)?;
    let reference = project_ref.trim();
    if reference.is_empty() || reference.len() > DOC_REF_MAX_BYTES {
        return Err(refused(ErrorCode::InvalidRequest, "project: required"));
    }
    let project = match view.resolve_project(reference)? {
        Some(project) if may_see(view, &caller, &project)? => project,
        _ => return Err(refused(ErrorCode::NotFound, PROJECT_NOT_FOUND)),
    };
    if access == DocsAccess::Write && project.archived {
        return Err(refused(
            ErrorCode::NotAuthorized,
            "the project is archived: its documentation is read-only",
        ));
    }
    Ok(Authorized { caller, project })
}
