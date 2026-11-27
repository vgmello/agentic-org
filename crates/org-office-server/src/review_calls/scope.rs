//! Active-employee and project/task scope pre-check for `review.request` (`ADR-0031`).
//!
//! Runs BEFORE any `git-state.json` or bare-cache access so unauthorized outsiders or inactive
//! employees never probe repository or worktree state.

use std::collections::{BTreeMap, BTreeSet};

use org_office::organization_family::records::{EmployeeKind, EmployeeStatus};
use org_office::Office;
use org_proto::projects::ProjectRecord;
use org_proto::{ApiError, ErrorCode};

pub(super) fn verify_active_and_request_scope(
    office: &Office,
    member: &str,
    project_ref: Option<&str>,
    task_ref: Option<&str>,
) -> Result<(), ApiError> {
    let emp = office
        .load_employee(member)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?
        .ok_or_else(|| ApiError::new(ErrorCode::NotAuthorized, "unknown employee"))?;
    if emp.status != EmployeeStatus::Active {
        return Err(ApiError::new(ErrorCode::NotAuthorized, "inactive employee"));
    }
    let allowed_teams = if emp.kind == EmployeeKind::Person {
        None
    } else {
        Some(caller_team_chain(office, emp.home_team_id.as_str())?)
    };
    if let Some(pref) = project_ref {
        let proj = office
            .inspect_project(pref)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))??;
        check_project_visible(&proj, allowed_teams.as_ref(), "project not found")?;
    }
    if let Some(tref) = task_ref {
        let task = office
            .task_inspect(tref)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))??;
        if let Some(pid) = task.task.project_id.as_deref() {
            let proj = office.inspect_project(pid).map_err(|_| {
                ApiError::new(ErrorCode::StorageError, "the office could not answer")
            })??;
            check_project_visible(&proj, allowed_teams.as_ref(), "task not found")?;
        }
    }
    Ok(())
}

fn check_project_visible(
    proj: &ProjectRecord,
    allowed_teams: Option<&BTreeSet<String>>,
    missing_msg: &str,
) -> Result<(), ApiError> {
    if let Some(chain) = allowed_teams {
        let visible = chain.contains(&proj.owner_team_id)
            || proj.contributing_team_ids.iter().any(|t| chain.contains(t));
        if !visible {
            return Err(ApiError::new(ErrorCode::NotFound, missing_msg));
        }
    }
    if proj.archived {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "project is archived",
        ));
    }
    Ok(())
}

fn caller_team_chain(office: &Office, home_team_id: &str) -> Result<BTreeSet<String>, ApiError> {
    let snap = office
        .org_snapshot()
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?;
    let parents: BTreeMap<&str, Option<&str>> = snap
        .teams
        .iter()
        .map(|t| (t.id.as_str(), t.owner_team_id.as_deref()))
        .collect();
    let mut chain = BTreeSet::new();
    let mut cur = Some(home_team_id);
    while let Some(tid) = cur {
        if !chain.insert(tid.to_owned()) {
            break;
        }
        cur = parents.get(tid).copied().flatten();
    }
    Ok(chain)
}
