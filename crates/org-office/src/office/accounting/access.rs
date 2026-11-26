//! Cost-API caller checks: active membership, owner-only access and the owner / project
//! manager cost viewer scope derived from led teams' accountable projects.

use org_proto::{ApiError, ErrorCode};

use super::super::Office;
use crate::accounting::CostViewerScope;
use crate::error::OfficeError;
use crate::organization::records::{Employee, EmployeeStatus};

pub(super) fn require_active_member_for(
    office: &Office,
    actor: &str,
) -> Result<Result<Employee, ApiError>, OfficeError> {
    let Some(emp) = office.load_employee(actor)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller is not a member of this organization",
        )));
    };
    if emp.status != EmployeeStatus::Active {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "inactive or paused employees cannot perform this operation",
        )));
    }
    Ok(Ok(emp))
}

pub(super) fn resolve_viewer_scope(
    office: &Office,
    actor: &str,
) -> Result<Result<(Employee, CostViewerScope), ApiError>, OfficeError> {
    let caller = match require_active_member_for(office, actor)? {
        Ok(emp) => emp,
        Err(e) => return Ok(Err(e)),
    };
    if caller.id.as_str() == office.owner.as_str() {
        return Ok(Ok((caller, CostViewerScope::Owner)));
    }
    let project_ids = list_pm_project_ids(office, caller.id.as_str())?;
    if project_ids.is_empty() {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller is not authorized to access cost data",
        )));
    }
    Ok(Ok((
        caller,
        CostViewerScope::ProjectManager { project_ids },
    )))
}

pub(super) fn list_pm_project_ids(
    office: &Office,
    member_id: &str,
) -> Result<Vec<String>, OfficeError> {
    let snap = office.org_snapshot()?;
    let led_teams: std::collections::BTreeSet<&str> = snap
        .teams
        .iter()
        .filter(|t| t.lead_member_id.as_deref() == Some(member_id))
        .map(|t| t.id.as_str())
        .collect();
    if led_teams.is_empty() {
        return Ok(Vec::new());
    }
    let projects = office.list_projects(true)?;
    Ok(projects
        .projects
        .into_iter()
        .filter(|p| led_teams.contains(p.owner_team_id.as_str()))
        .map(|p| p.id)
        .collect())
}

pub(super) fn require_owner_for(
    office: &Office,
    actor: &str,
) -> Result<Result<Employee, ApiError>, OfficeError> {
    let caller = match require_active_member_for(office, actor)? {
        Ok(emp) => emp,
        Err(e) => return Ok(Err(e)),
    };
    if caller.id.as_str() != office.owner.as_str() {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the organization owner can access subscription usage data",
        )));
    }
    Ok(Ok(caller))
}
