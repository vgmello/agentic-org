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
    repo_ref: Option<&str>,
) -> Result<(), ApiError> {
    let allowed_teams = caller_allowed_teams(office, member)?;
    let mut effective_project: Option<ProjectRecord> = None;
    if let Some(pref) = project_ref {
        let trimmed = pref.trim();
        if trimmed.is_empty() {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "project_id cannot be empty",
            ));
        }
        let proj = office
            .inspect_project(trimmed)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))??;
        check_project_visible(&proj, allowed_teams.as_ref(), "project not found")?;
        effective_project = Some(proj);
    }
    if let Some(tref) = task_ref {
        let task = office
            .task_inspect(tref)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))??;
        match task.task.project_id.as_deref() {
            Some(pid) => {
                let proj = office.inspect_project(pid).map_err(|_| {
                    ApiError::new(ErrorCode::StorageError, "the office could not answer")
                })??;
                check_project_visible(&proj, allowed_teams.as_ref(), "task not found")?;
                if let Some(exp) = effective_project.as_ref() {
                    if exp.id != proj.id {
                        return Err(ApiError::new(
                            ErrorCode::InvalidRequest,
                            "task does not belong to project",
                        ));
                    }
                } else {
                    effective_project = Some(proj);
                }
            }
            None if effective_project.is_some() => {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "task does not belong to project",
                ));
            }
            None => {}
        }
    }
    if let Some(raw_repo) = repo_ref {
        verify_repository_scope(
            office,
            allowed_teams.as_ref(),
            effective_project.as_ref(),
            raw_repo,
        )?;
    }
    Ok(())
}

pub(super) fn caller_allowed_teams(
    office: &Office,
    member: &str,
) -> Result<Option<BTreeSet<String>>, ApiError> {
    let emp = office
        .load_employee(member)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?
        .ok_or_else(|| ApiError::new(ErrorCode::NotAuthorized, "unknown employee"))?;
    if emp.status != EmployeeStatus::Active {
        return Err(ApiError::new(ErrorCode::NotAuthorized, "inactive employee"));
    }
    if emp.kind == EmployeeKind::Person {
        Ok(None)
    } else {
        Ok(Some(caller_team_chain(office, emp.home_team_id.as_str())?))
    }
}

pub(super) fn is_project_visible(
    proj: &ProjectRecord,
    allowed_teams: Option<&BTreeSet<String>>,
) -> bool {
    match allowed_teams {
        None => true,
        Some(chain) => {
            chain.contains(&proj.owner_team_id)
                || proj.contributing_team_ids.iter().any(|t| chain.contains(t))
        }
    }
}

fn verify_repository_scope(
    office: &Office,
    allowed_teams: Option<&BTreeSet<String>>,
    effective_project: Option<&ProjectRecord>,
    raw_repo: &str,
) -> Result<(), ApiError> {
    let repo_id = raw_repo.trim();
    if !org_proto::is_uuid_v7(repo_id) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "repository_id must be a canonical lowercase UUIDv7",
        ));
    }
    if let Some(proj) = effective_project {
        let proj_repos = office
            .list_repositories(Some(&proj.id))
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))??;
        if !proj_repos.iter().any(|r| r.record.id.as_str() == repo_id) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "repository is not attached to project",
            ));
        }
        return Ok(());
    }
    let Some(repo) = office
        .resolve_repository(repo_id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?
    else {
        return Err(ApiError::new(ErrorCode::NotFound, "repository not found"));
    };
    if repo.id.as_str() != repo_id {
        return Err(ApiError::new(ErrorCode::NotFound, "repository not found"));
    }
    let projects = office
        .list_projects(true)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?;
    let mut attached_any = false;
    let mut visible_any = false;
    for proj in &projects.projects {
        let proj_repos = office
            .list_repositories(Some(&proj.id))
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))??;
        if proj_repos.iter().any(|r| r.record.id.as_str() == repo_id) {
            attached_any = true;
            if is_project_visible(proj, allowed_teams) {
                visible_any = true;
                break;
            }
        }
    }
    if attached_any && !visible_any {
        return Err(ApiError::new(ErrorCode::NotFound, "repository not found"));
    }
    Ok(())
}

fn check_project_visible(
    proj: &ProjectRecord,
    allowed_teams: Option<&BTreeSet<String>>,
    missing_msg: &str,
) -> Result<(), ApiError> {
    if !is_project_visible(proj, allowed_teams) {
        return Err(ApiError::new(ErrorCode::NotFound, missing_msg));
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
