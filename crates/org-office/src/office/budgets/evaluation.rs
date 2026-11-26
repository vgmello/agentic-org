//! Re-evaluating budgets against usage records, and the execution holds they produce.

use org_proto::budgets::{BudgetHoldState, BudgetPeriodKind, BudgetScopeKind, BudgetView};
use org_proto::costs::UsageRecordView;
use org_proto::{ApiError, ErrorCode};

use crate::budgets::eval::{evaluate_budget, EvalOutcome};
use crate::budgets::{ActiveBudgetHoldInfo, BudgetAggregate, OpenBudgetInboxNotice};
use crate::error::OfficeError;
use crate::office::Office;
use crate::time::format_rfc3339_ms;

use super::authority::collect_employee_kinds;
use super::queries::render_budget_view;
use super::stop::{ensure_held_task_blocked, reconcile_pending_stop};

pub(in crate::office) fn op_reevaluate_for_record(
    office: &Office,
    rec: &UsageRecordView,
) -> Result<(), OfficeError> {
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    for mut agg in office.budgets.list_all(&office.info.org_id)? {
        let matches = match agg.budget.scope_kind {
            BudgetScopeKind::Task => rec.task_id.is_some() && rec.task_id == agg.budget.task_id,
            BudgetScopeKind::Agent => {
                rec.employee_id.is_some() && rec.employee_id == agg.budget.agent_id
            }
            BudgetScopeKind::Release => {
                rec.release_id.is_some() && rec.release_id == agg.budget.release_id
            }
            _ => false,
        };
        if !matches {
            continue;
        }
        evaluate_and_save_budget(office, &mut agg, now_ms, &now_utc)?;
    }
    Ok(())
}

pub(in crate::office) fn op_is_task_reached(
    office: &Office,
    task_id: &str,
) -> Result<bool, OfficeError> {
    let Some(mut agg) = office.budgets.find_by_target(
        &office.info.org_id,
        BudgetScopeKind::Task,
        Some(task_id),
        None,
        None,
        BudgetPeriodKind::Lifetime,
    )?
    else {
        return Ok(false);
    };
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    evaluate_and_save_budget(office, &mut agg, now_ms, &now_utc)?;
    Ok(agg.active_hold().is_some())
}

pub(in crate::office) fn op_active_holds(
    office: &Office,
    agent_id: Option<&str>,
    task_id: Option<&str>,
) -> Result<Vec<ActiveBudgetHoldInfo>, OfficeError> {
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    let mut active = Vec::new();
    for mut agg in office.budgets.list_all(&office.info.org_id)? {
        let relevant = match agg.budget.scope_kind {
            BudgetScopeKind::Agent => {
                agent_id.is_some() && agg.budget.agent_id.as_deref() == agent_id
            }
            BudgetScopeKind::Task => task_id.is_some() && agg.budget.task_id.as_deref() == task_id,
            _ => false,
        };
        if !relevant {
            continue;
        }
        evaluate_and_save_budget(office, &mut agg, now_ms, &now_utc)?;
        if let Some(h) = agg.active_hold() {
            active.push(ActiveBudgetHoldInfo {
                budget_id: agg.budget.id.clone(),
                scope_kind: agg.budget.scope_kind,
                task_id: agg.budget.task_id.clone(),
                agent_id: agg.budget.agent_id.clone(),
                period_kind: agg.budget.period_kind,
                state: h.state,
                stage: h.stage,
            });
        }
    }
    Ok(active)
}

pub(super) fn op_mark_holds_stopped(office: &Office, agent_id: &str) -> Result<bool, OfficeError> {
    let now_utc = format_rfc3339_ms(office.monotonic_now_ms());
    let mut any = false;
    for mut agg in office.budgets.list_all(&office.info.org_id)? {
        let relevant = match agg.budget.scope_kind {
            BudgetScopeKind::Agent => agg.budget.agent_id.as_deref() == Some(agent_id),
            // Also transition `stop_pending -> held` for task budgets assigned to `agent_id` (`Pass 1 Med #12`).
            BudgetScopeKind::Task => agg
                .budget
                .task_id
                .as_deref()
                .and_then(|tid| office.task_inspect(tid).ok()?.ok())
                .is_some_and(|t| t.task.assignee_id.as_deref() == Some(agent_id)),
            _ => false,
        };
        if !relevant {
            continue;
        }
        let mut changed = false;
        for h in &mut agg.holds {
            if h.state == BudgetHoldState::StopPending {
                h.state = BudgetHoldState::Held;
                h.updated_at = now_utc.clone();
                changed = true;
                any = true;
            }
        }
        if changed {
            if let Err(err) = save_budget_aggregate(office, &agg)? {
                return Err(OfficeError::storage(std::io::Error::other(err.message)));
            }
        }
    }
    Ok(any)
}

pub(super) fn op_open_inbox_notices(
    office: &Office,
) -> Result<Vec<OpenBudgetInboxNotice>, OfficeError> {
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    let mut out = Vec::new();
    for mut agg in office.budgets.list_all(&office.info.org_id)? {
        evaluate_and_save_budget(office, &mut agg, now_ms, &now_utc)?;
        let (Some(policy), Some(period)) =
            (agg.current_policy().cloned(), agg.open_period().cloned())
        else {
            continue;
        };
        let hold_state = agg.active_hold().map(|h| h.state);
        for notice in &agg.notices {
            if notice.kind == "budget_reached" && notice.delivery_state != "resolved" {
                out.push(OpenBudgetInboxNotice {
                    notice: notice.clone(),
                    budget: agg.budget.clone(),
                    period: period.clone(),
                    policy: policy.clone(),
                    hold_state,
                });
            }
        }
    }
    Ok(out)
}

pub(super) fn load_and_eval_agent_budget(
    office: &Office,
    agent_id: &str,
    period_kind: BudgetPeriodKind,
    now_ms: u64,
    now_utc: &str,
) -> Result<Option<BudgetView>, OfficeError> {
    let Some(mut agg) = office.budgets.find_by_target(
        &office.info.org_id,
        BudgetScopeKind::Agent,
        None,
        Some(agent_id),
        None,
        period_kind,
    )?
    else {
        return Ok(None);
    };
    evaluate_and_save_budget(office, &mut agg, now_ms, now_utc)?;
    Ok(Some(render_budget_view(office, &agg, true)?))
}

pub(super) fn period_watermark_regressed(
    office: &Office,
    agg: &BudgetAggregate,
) -> Result<bool, OfficeError> {
    let Some(prev) = agg
        .open_period()
        .and_then(|p| p.ledger_checkpoint_ref.as_deref())
    else {
        return Ok(false);
    };
    let mut storage_err: Option<OfficeError> = None;
    let res = crate::budgets::eval::checkpoint_ref_regressed(Some(prev), &mut |wm| match office
        .accounting
        .stream_continues_watermark(&office.info.org_id, wm)
    {
        Ok(ok) => Ok(ok),
        Err(err) => {
            storage_err = Some(err);
            Err(ApiError::new(
                ErrorCode::StorageError,
                "failed to verify stream watermark continuity",
            ))
        }
    });
    if let Some(err) = storage_err {
        return Err(err);
    }
    Ok(res.unwrap_or(true))
}

pub(super) fn evaluate_in_memory(
    office: &Office,
    agg: &mut BudgetAggregate,
    now_ms: u64,
    now_utc: &str,
) -> Result<Result<EvalOutcome, ApiError>, OfficeError> {
    let effective = office.accounting.effective_records(&office.info.org_id)?;
    let coverage = office.accounting.coverage_summary(&office.info.org_id)?;
    let mut storage_err: Option<OfficeError> = None;
    let mut continues = |wm: &crate::accounting::VerifiedStreamWatermark| match office
        .accounting
        .stream_continues_watermark(&office.info.org_id, wm)
    {
        Ok(ok) => Ok(ok),
        Err(err) => {
            storage_err = Some(err);
            Err(ApiError::new(
                ErrorCode::StorageError,
                "failed to verify stream watermark continuity",
            ))
        }
    };
    let mut next_id = || office.next_uuid_v7();
    let res = evaluate_budget(
        agg,
        &effective,
        &coverage,
        now_ms,
        now_utc,
        &mut continues,
        &mut next_id,
    );
    if let Some(err) = storage_err {
        return Err(err);
    }
    Ok(res)
}

pub(super) fn evaluate_and_save_budget(
    office: &Office,
    agg: &mut BudgetAggregate,
    now_ms: u64,
    now_utc: &str,
) -> Result<EvalOutcome, OfficeError> {
    reconcile_pending_stop(office, agg)?;
    let outcome = evaluate_in_memory(office, agg, now_ms, now_utc)?
        .map_err(|e| OfficeError::storage(std::io::Error::other(e.message)))?;
    if outcome.changed {
        if let Err(err) = save_budget_aggregate(office, agg)? {
            return Err(OfficeError::storage(std::io::Error::other(err.message)));
        }
    }
    ensure_held_task_blocked(office, agg)?;
    Ok(outcome)
}

pub(super) fn save_budget_aggregate(
    office: &Office,
    agg: &BudgetAggregate,
) -> Result<Result<(), ApiError>, OfficeError> {
    let emp_kinds = collect_employee_kinds(office, agg)?;
    office.budgets.save_aggregate(agg, &emp_kinds)
}
