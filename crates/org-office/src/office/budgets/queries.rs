//! Reading budgets: get, list, an agent's own budgets, and the views each caller may see.

use org_proto::budgets::{
    BudgetAgentOwnParams, BudgetAgentOwnResult, BudgetGetParams, BudgetGetResult, BudgetListParams,
    BudgetListResult, BudgetPeriodKind, BudgetScopeKind, BudgetView,
};
use org_proto::{ApiError, ErrorCode};

use crate::budgets::BudgetAggregate;
use crate::error::OfficeError;
use crate::office::Office;
use crate::organization::records::{Employee, EmployeeKind, EmployeeStatus};
use crate::time::format_rfc3339_ms;

use super::authority::is_pm_of_task;
use super::evaluation::{evaluate_and_save_budget, load_and_eval_agent_budget, render_budget_view};

pub(super) fn op_budget_get(
    office: &Office,
    actor: &str,
    params: BudgetGetParams,
) -> Result<Result<BudgetGetResult, ApiError>, OfficeError> {
    let caller = match office.require_active_member(actor)? {
        Ok(emp) => emp,
        Err(e) => return Ok(Err(e)),
    };
    let found = match load_budget_for_get(office, &params)? {
        Ok(opt) => opt,
        Err(e) => return Ok(Err(e)),
    };
    let Some(mut agg) = found else {
        return Ok(Ok(BudgetGetResult { budget: None }));
    };
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    evaluate_and_save_budget(office, &mut agg, now_ms, &now_utc)?;
    match budget_view_for_caller(office, &caller, &agg)? {
        Some(view) => Ok(Ok(BudgetGetResult { budget: Some(view) })),
        None => Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller is not authorized to view this budget",
        ))),
    }
}

fn load_budget_for_get(
    office: &Office,
    params: &BudgetGetParams,
) -> Result<Result<Option<BudgetAggregate>, ApiError>, OfficeError> {
    if let Some(ref bid) = params.budget_id {
        return Ok(Ok(office.budgets.get_by_id(&office.info.org_id, bid)?));
    }
    let Some(scope) = params.scope_kind else {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "budget_id or scope_kind is required",
        )));
    };
    let period = params.period_kind.unwrap_or(BudgetPeriodKind::Lifetime);
    let task_id = match params.task_id.as_deref() {
        Some(tref) => office.task_inspect(tref)?.ok().map(|r| r.task.id),
        None => None,
    };
    Ok(Ok(office.budgets.find_by_target(
        &office.info.org_id,
        scope,
        task_id.as_deref(),
        params.agent_id.as_deref(),
        params.release_id.as_deref(),
        period,
    )?))
}

pub(super) fn op_budget_list(
    office: &Office,
    actor: &str,
    params: BudgetListParams,
) -> Result<Result<BudgetListResult, ApiError>, OfficeError> {
    let caller = match office.require_active_member(actor)? {
        Ok(emp) => emp,
        Err(e) => return Ok(Err(e)),
    };
    let task_filter = match params.task_id.as_deref() {
        Some(tref) => match office.task_inspect(tref)? {
            Ok(r) => Some(r.task.id),
            Err(_) => {
                return Ok(Ok(BudgetListResult {
                    budgets: Vec::new(),
                }))
            }
        },
        None => None,
    };
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    let mut views = Vec::new();
    for mut agg in office.budgets.list_all(&office.info.org_id)? {
        if !matches_list_filter(&agg, &params, task_filter.as_deref()) {
            continue;
        }
        evaluate_and_save_budget(office, &mut agg, now_ms, &now_utc)?;
        if let Some(view) = budget_view_for_caller(office, &caller, &agg)? {
            views.push(view);
        }
    }
    Ok(Ok(BudgetListResult { budgets: views }))
}

fn matches_list_filter(
    agg: &BudgetAggregate,
    params: &BudgetListParams,
    task_filter: Option<&str>,
) -> bool {
    params.scope_kind.is_none_or(|s| s == agg.budget.scope_kind)
        && task_filter.is_none_or(|t| agg.budget.task_id.as_deref() == Some(t))
        && params
            .agent_id
            .as_deref()
            .is_none_or(|a| agg.budget.agent_id.as_deref() == Some(a))
        && params
            .release_id
            .as_deref()
            .is_none_or(|r| agg.budget.release_id.as_deref() == Some(r))
}

pub(super) fn op_budget_agent_own(
    office: &Office,
    actor: &str,
    params: BudgetAgentOwnParams,
) -> Result<Result<BudgetAgentOwnResult, ApiError>, OfficeError> {
    let Some(emp) = office.load_employee(actor)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller is not a member of this organization",
        )));
    };
    if emp.kind != EmployeeKind::Agent || matches!(emp.status, EmployeeStatus::Inactive) {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "budget.agent_own is only available to bound organization agents",
        )));
    }
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    let daily = load_and_eval_agent_budget(
        office,
        emp.id.as_str(),
        BudgetPeriodKind::Day,
        now_ms,
        &now_utc,
    )?;
    let monthly = load_and_eval_agent_budget(
        office,
        emp.id.as_str(),
        BudgetPeriodKind::Month,
        now_ms,
        &now_utc,
    )?;
    let task =
        match load_own_task_budget(office, &emp, params.task_id.as_deref(), now_ms, &now_utc)? {
            Ok(v) => v,
            Err(e) => return Ok(Err(e)),
        };
    Ok(Ok(BudgetAgentOwnResult {
        agent_id: emp.id.as_str().to_owned(),
        daily,
        monthly,
        task,
    }))
}

fn load_own_task_budget(
    office: &Office,
    emp: &Employee,
    task_ref: Option<&str>,
    now_ms: u64,
    now_utc: &str,
) -> Result<Result<Option<BudgetView>, ApiError>, OfficeError> {
    let Some(tref) = task_ref else {
        return Ok(Ok(None));
    };
    let inspected = match office.task_inspect(tref)? {
        Ok(r) => r,
        Err(e) => return Ok(Err(e)),
    };
    // `doc-92:72`, `doc-51:75`, `Pass 1 Med #13`: own-agent CLI exception applies ONLY to the
    // agent's own assigned task (with amounts visible) — refuse unrelated tasks!
    if inspected.task.assignee_id.as_deref() != Some(emp.id.as_str()) {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "budget.agent_own task lookup is limited to the agent's own assigned task",
        )));
    }
    let Some(mut agg) = office.budgets.find_by_target(
        &office.info.org_id,
        BudgetScopeKind::Task,
        Some(&inspected.task.id),
        None,
        None,
        BudgetPeriodKind::Lifetime,
    )?
    else {
        return Ok(Ok(None));
    };
    evaluate_and_save_budget(office, &mut agg, now_ms, now_utc)?;
    Ok(Ok(Some(render_budget_view(office, &agg, true)?)))
}

fn budget_view_for_caller(
    office: &Office,
    caller: &Employee,
    agg: &BudgetAggregate,
) -> Result<Option<BudgetView>, OfficeError> {
    if caller.id.as_str() == office.owner.as_str() {
        return Ok(Some(render_budget_view(office, agg, true)?));
    }
    match agg.budget.scope_kind {
        // Release & Agent budgets fail closed to owner-only until release ownership exists (`Pass 1 High #4`).
        BudgetScopeKind::Agent | BudgetScopeKind::Release => Ok(None),
        BudgetScopeKind::Task => {
            let is_pm = match agg.budget.task_id.as_deref() {
                Some(tid) => is_pm_of_task(office, caller, tid)?,
                None => false,
            };
            Ok(Some(render_budget_view(office, agg, is_pm)?))
        }
        _ => Ok(None),
    }
}
