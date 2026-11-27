//! Active-employee and project/repository visibility gate (`ADR-0031`) for GitHub PR & CI reads.
//!
//! Enforced BEFORE any `github-context/` disk read so unauthorized outsiders, inactive employees,
//! or agents outside a private project's team hierarchy receive uniform `NotFound` / `NotAuthorized`
//! responses with zero cross-project existence oracle.

use std::collections::{BTreeMap, BTreeSet};

use org_office::organization_family::records::{EmployeeKind, EmployeeStatus, Repository};
use org_office::Office;
use org_proto::projects::ProjectRecord;
use org_proto::{ApiError, ErrorCode};

/// Authorized project scope and attached repositories accessible to the caller.
pub(super) struct AuthorizedProjectRepos {
    pub project_id: String,
    pub repos: Vec<Repository>,
}

/// Verifies that `member` is an active employee who may see `project_ref` (`ADR-0031`), and
/// returns the project's attached repositories (filtered to `repo_filter` if given).
///
/// Any invisible project or unattached repository returns uniform `ErrorCode::NotFound` to avoid
/// cross-project existence oracles.
pub(super) fn resolve_authorized_project_repos(
    office: &Office,
    member: &str,
    project_ref: &str,
    repo_filter: Option<&str>,
) -> Result<AuthorizedProjectRepos, ApiError> {
    let allowed_teams = caller_allowed_teams(office, member)?;
    let trimmed_project = project_ref.trim();
    if trimmed_project.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "project cannot be empty",
        ));
    }
    let proj = office
        .inspect_project(trimmed_project)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))??;
    if !is_project_visible(&proj, allowed_teams.as_ref()) {
        return Err(ApiError::new(ErrorCode::NotFound, "project not found"));
    }

    let attached = office
        .list_repositories(Some(&proj.id))
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))??;
    let mut repos: Vec<Repository> = attached.into_iter().map(|entry| entry.record).collect();

    if let Some(raw_repo) = repo_filter.map(str::trim).filter(|s| !s.is_empty()) {
        repos.retain(|r| {
            r.id.as_str() == raw_repo
                || r.key
                    .as_deref()
                    .is_some_and(|k| k.eq_ignore_ascii_case(raw_repo))
        });
        if repos.is_empty() {
            return Err(ApiError::new(ErrorCode::NotFound, "repository not found"));
        }
    }

    Ok(AuthorizedProjectRepos {
        project_id: proj.id,
        repos,
    })
}

fn caller_allowed_teams(
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

fn is_project_visible(proj: &ProjectRecord, allowed_teams: Option<&BTreeSet<String>>) -> bool {
    match allowed_teams {
        None => true,
        Some(chain) => {
            chain.contains(&proj.owner_team_id)
                || proj.contributing_team_ids.iter().any(|t| chain.contains(t))
        }
    }
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
