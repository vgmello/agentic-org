//! `budget.stop`: records a task budget's durable stop intent, or keeps open periods paused.

use org_proto::budgets::{BudgetMutationResult, BudgetScopeKind, BudgetStopParams};
use org_proto::{ApiError, ErrorCode};
use serde_json::json;

use crate::budgets::{BudgetAggregate, PendingStopIntent};
use crate::error::OfficeError;
use crate::office::Office;
use crate::organization::records::Employee;
use crate::time::format_rfc3339_ms;

use super::authority::require_budget_actor;
use super::evaluation::{evaluate_in_memory, render_budget_view};
use super::receipts::invalid_op_id;
use super::reconcile::{
    ensure_held_task_blocked, reconcile_pending_stop, stop_refusal_for_operation,
};
use super::{
    check_operation_replay, mark_open_periods_kept_paused, push_budget_event, request_fingerprint,
    resolve_reached_notices, save_budget_aggregate,
};

pub(super) fn op_budget_stop(
    office: &Office,
    actor: &str,
    params: BudgetStopParams,
) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
    let caller = match office.require_active_member(actor)? {
        Ok(emp) => emp,
        Err(e) => return Ok(Err(e)),
    };
    if !org_proto::is_uuid_v7(&params.operation_id) {
        return Ok(Err(invalid_op_id()));
    }
    let Some(mut agg) = office
        .budgets
        .get_by_id(&office.info.org_id, &params.budget_id)?
    else {
        return Ok(Err(ApiError::new(ErrorCode::NotFound, "budget not found")));
    };
    // Authorize BEFORE replay check (`Pass 1 High #2`).
    if let Err(e) = require_budget_actor(office, &caller, &agg, false)? {
        return Ok(Err(e));
    }
    if agg.budget.scope_kind == BudgetScopeKind::Release {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "release warning caps do not stop or pause execution",
        )));
    }
    // Reconcile any previously crashed stop intent before replay check so a retry after a crash
    // preserves the original task-cancellation receipt outcome (`TASK-97` / Pass 7 Finding #2).
    reconcile_pending_stop(office, &mut agg)?;
    let req_hash = request_fingerprint(&json!({
        "budget_id": params.budget_id,
        "reason": params.reason,
    }));
    match check_operation_replay(
        &agg,
        &params.operation_id,
        caller.id.as_str(),
        "budget.stop",
        &req_hash,
    ) {
        Ok(true) => {
            ensure_held_task_blocked(office, &agg)?;
            if let Some(refusal) = stop_refusal_for_operation(&agg, &params.operation_id) {
                return Ok(Err(refusal));
            }
            return Ok(Ok(BudgetMutationResult {
                budget: render_budget_view(office, &agg, true)?,
                replayed: true,
            }));
        }
        Ok(false) => {}
        Err(e) => return Ok(Err(e)),
    }
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    if let Err(e) = evaluate_in_memory(office, &mut agg, now_ms, &now_utc)? {
        return Ok(Err(e));
    }
    if agg.budget.scope_kind == BudgetScopeKind::Task {
        return execute_task_budget_stop(office, &mut agg, &caller, &params, req_hash, now_utc);
    }
    mark_open_periods_kept_paused(&mut agg);
    push_budget_event(
        office,
        &mut agg,
        &params.operation_id,
        "stop_requested",
        caller.id.as_str(),
        &now_utc,
        json!({
            "action": "budget.stop",
            "request_hash": req_hash,
            "reason": params.reason,
            "outcome": "kept_paused",
        })
        .to_string(),
    );
    if let Err(err) = save_budget_aggregate(office, &agg)? {
        return Ok(Err(err));
    }
    Ok(Ok(BudgetMutationResult {
        budget: render_budget_view(office, &agg, true)?,
        replayed: false,
    }))
}

fn execute_task_budget_stop(
    office: &Office,
    agg: &mut BudgetAggregate,
    caller: &Employee,
    params: &BudgetStopParams,
    req_hash: String,
    now_utc: String,
) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
    if let Some(tid) = agg.budget.task_id.clone() {
        agg.pending_stop = Some(PendingStopIntent {
            operation_id: params.operation_id.clone(),
            actor_id: caller.id.as_str().to_owned(),
            task_id: tid,
            task_command_id: office.next_uuid_v7(),
            task_command_fingerprint: None,
            request_hash: req_hash,
            reason: params.reason.clone(),
            created_at: now_utc,
        });
        if let Err(err) = save_budget_aggregate(office, agg)? {
            return Ok(Err(err));
        }
        if let Some(refusal) = reconcile_pending_stop(office, agg)? {
            return Ok(Err(refusal));
        }
        return Ok(Ok(BudgetMutationResult {
            budget: render_budget_view(office, agg, true)?,
            replayed: false,
        }));
    }
    resolve_reached_notices(agg, &now_utc);
    push_budget_event(
        office,
        agg,
        &params.operation_id,
        "stop_requested",
        caller.id.as_str(),
        &now_utc,
        json!({
            "action": "budget.stop",
            "request_hash": req_hash,
            "reason": params.reason,
            "outcome": "missing_task",
        })
        .to_string(),
    );
    if let Err(err) = save_budget_aggregate(office, agg)? {
        return Ok(Err(err));
    }
    Ok(Ok(BudgetMutationResult {
        budget: render_budget_view(office, agg, true)?,
        replayed: false,
    }))
}
