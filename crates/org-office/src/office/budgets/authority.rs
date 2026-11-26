//! Who may set or act on a budget, and which target a budget request names.

use org_proto::budgets::{BudgetPolicyState, BudgetScopeKind, BudgetSetParams};
use org_proto::{ApiError, ErrorCode};

use crate::budgets::BudgetAggregate;
use crate::error::OfficeError;
use crate::office::Office;
use crate::organization::records::{Employee, EmployeeKind};

use super::{ResolvedTarget, PERSON_ONLY_MSG};

pub(super) fn resolve_budget_target(
    office: &Office,
    params: &BudgetSetParams,
) -> Result<Result<ResolvedTarget, ApiError>, OfficeError> {
    match params.scope_kind {
        BudgetScopeKind::Task => {
            let Some(ref tref) = params.task_id else {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "task_id is required for a task budget",
                )));
            };
            match office.task_inspect(tref)? {
                Ok(r) => Ok(Ok(ResolvedTarget {
                    task_id: Some(r.task.id),
                    agent_id: None,
                    release_id: None,
                    project_id: r.task.project_id,
                })),
                Err(e) => Ok(Err(e)),
            }
        }
        BudgetScopeKind::Agent => {
            let Some(ref aid) = params.agent_id else {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "agent_id is required for an agent budget",
                )));
            };
            let Some(emp) = office.load_employee(aid)? else {
                return Ok(Err(ApiError::new(ErrorCode::NotFound, "agent not found")));
            };
            if emp.kind != EmployeeKind::Agent {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "agent budgets apply only to agent employees",
                )));
            }
            Ok(Ok(ResolvedTarget {
                task_id: None,
                agent_id: Some(emp.id.as_str().to_owned()),
                release_id: None,
                project_id: None,
            }))
        }
        BudgetScopeKind::Release => {
            let Some(rid) = params
                .release_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "release_id must be non-empty for a release warning cap",
                )));
            };
            Ok(Ok(ResolvedTarget {
                task_id: None,
                agent_id: None,
                release_id: Some(rid.to_owned()),
                project_id: None,
            }))
        }
        _ => Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "unknown budget scope_kind",
        ))),
    }
}

pub(super) fn authorize_budget_set(
    office: &Office,
    caller: &Employee,
    params: &BudgetSetParams,
    target: &ResolvedTarget,
    existing: Option<&BudgetAggregate>,
) -> Result<Result<(), ApiError>, OfficeError> {
    let is_owner = caller.id.as_str() == office.owner.as_str();
    // Agent budgets are owner-only; Release budgets fail closed to owner-only until Release
    // entities carry authoritative project ownership (`L11` TASK-55, `Pass 1 High #4`).
    if matches!(
        params.scope_kind,
        BudgetScopeKind::Agent | BudgetScopeKind::Release
    ) {
        if !is_owner {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "only the organization owner can manage this budget",
            )));
        }
        if caller.kind == EmployeeKind::Agent {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                PERSON_ONLY_MSG,
            )));
        }
        return Ok(Ok(()));
    }
    if !is_owner {
        let pm_projects = office.pm_project_ids(caller.id.as_str())?;
        let allowed = target
            .project_id
            .as_deref()
            .is_some_and(|pid| pm_projects.iter().any(|p| p == pid));
        if !allowed {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller is not authorized to manage this budget",
            )));
        }
    }
    if caller.kind == EmployeeKind::Agent {
        if let Some(prev) = existing.and_then(BudgetAggregate::current_policy) {
            let allowed_lower = prev.state == BudgetPolicyState::Active
                && prev.unit == params.unit
                && prev.limit_amount.is_some_and(|l| params.limit_amount <= l);
            if !allowed_lower {
                return Ok(Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    PERSON_ONLY_MSG,
                )));
            }
        }
    }
    Ok(Ok(()))
}

pub(super) fn require_budget_actor(
    office: &Office,
    caller: &Employee,
    agg: &BudgetAggregate,
    person_only: bool,
) -> Result<Result<(), ApiError>, OfficeError> {
    if person_only && caller.kind == EmployeeKind::Agent {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            PERSON_ONLY_MSG,
        )));
    }
    if caller.id.as_str() == office.owner.as_str() {
        return Ok(Ok(()));
    }
    if matches!(
        agg.budget.scope_kind,
        BudgetScopeKind::Agent | BudgetScopeKind::Release
    ) {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the organization owner can manage this budget",
        )));
    }
    if let Some(tid) = agg.budget.task_id.as_deref() {
        if is_pm_of_task(office, caller, tid)? {
            return Ok(Ok(()));
        }
    }
    Ok(Err(ApiError::new(
        ErrorCode::NotAuthorized,
        "caller is not authorized to manage this budget",
    )))
}

pub(super) fn is_pm_of_task(
    office: &Office,
    caller: &Employee,
    task_id: &str,
) -> Result<bool, OfficeError> {
    if caller.id.as_str() == office.owner.as_str() {
        return Ok(true);
    }
    let Ok(inspected) = office.task_inspect(task_id)? else {
        return Ok(false);
    };
    let Some(pid) = inspected.task.project_id else {
        return Ok(false);
    };
    let pm_projects = office.pm_project_ids(caller.id.as_str())?;
    Ok(pm_projects.iter().any(|p| p == &pid))
}
