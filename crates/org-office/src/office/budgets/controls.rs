//! Clearing a budget, its one-time +20% extension, and keeping a reached budget paused.

use org_proto::budgets::{
    BudgetClearParams, BudgetExtendOnceParams, BudgetKeepPausedParams, BudgetMutationResult,
    BudgetPolicyState, BudgetScopeKind, BudgetUnit,
};
use org_proto::{ApiError, ErrorCode};
use serde_json::json;

use crate::budgets::eval::{apply_extend_once, ExtendOnceRequest};
use crate::budgets::BudgetPolicyVersion;
use crate::error::OfficeError;
use crate::office::Office;
use crate::time::format_rfc3339_ms;

use super::authority::require_budget_actor;
use super::evaluation::{evaluate_in_memory, period_watermark_regressed, render_budget_view};
use super::receipts::invalid_op_id;
use super::reconcile::{ensure_held_task_blocked, reconcile_pending_stop};
use super::{
    check_operation_replay, mark_open_periods_kept_paused, push_budget_event, request_fingerprint,
    save_budget_aggregate,
};

pub(super) fn op_budget_clear(
    office: &Office,
    actor: &str,
    params: BudgetClearParams,
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
    if let Err(e) = require_budget_actor(office, &caller, &agg, true)? {
        return Ok(Err(e));
    }
    reconcile_pending_stop(office, &mut agg)?;
    let req_hash = request_fingerprint(&json!({
        "budget_id": params.budget_id,
        "reason": params.reason,
    }));
    match check_operation_replay(
        &agg,
        &params.operation_id,
        caller.id.as_str(),
        "budget.clear",
        &req_hash,
    ) {
        Ok(true) => {
            ensure_held_task_blocked(office, &agg)?;
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
    let unit = agg
        .current_policy()
        .map_or(BudgetUnit::MicroUsd, |p| p.unit);
    let next_rev = agg.budget.current_revision + 1;
    agg.budget.current_revision = next_rev;
    agg.versions.push(BudgetPolicyVersion {
        organization_id: office.info.org_id.clone(),
        budget_id: agg.budget.id.clone(),
        revision: next_rev,
        state: BudgetPolicyState::Cleared,
        unit,
        limit_amount: None,
        time_zone: agg.budget.time_zone.clone(),
        time_zone_source: agg.budget.time_zone_source,
        time_zone_subject_ref: agg.budget.time_zone_subject_ref.clone(),
        changed_by_id: caller.id.as_str().to_owned(),
        changed_at: now_utc.clone(),
        reason: params.reason.clone(),
    });
    if let Err(e) = evaluate_in_memory(office, &mut agg, now_ms, &now_utc)? {
        return Ok(Err(e));
    }
    push_budget_event(
        office,
        &mut agg,
        &params.operation_id,
        "cleared",
        caller.id.as_str(),
        &now_utc,
        json!({ "action": "budget.clear", "request_hash": req_hash, "reason": params.reason })
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

pub(super) fn op_budget_extend_once(
    office: &Office,
    actor: &str,
    params: BudgetExtendOnceParams,
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
    if let Err(e) = require_budget_actor(office, &caller, &agg, true)? {
        return Ok(Err(e));
    }
    reconcile_pending_stop(office, &mut agg)?;
    let req_hash = request_fingerprint(&json!({
        "budget_id": params.budget_id,
        "reason": params.reason,
    }));
    match check_operation_replay(
        &agg,
        &params.operation_id,
        caller.id.as_str(),
        "budget.extend_once",
        &req_hash,
    ) {
        Ok(true) => {
            ensure_held_task_blocked(office, &agg)?;
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
    let cov = office.accounting.coverage_summary(&office.info.org_id)?;
    let regressed = period_watermark_regressed(office, &agg)?;
    let incomplete_streams = cov.incomplete_streams.saturating_add(u64::from(regressed));
    let mut next_id = || office.next_uuid_v7();
    if let Err(e) = apply_extend_once(
        &mut agg,
        ExtendOnceRequest {
            operation_id: &params.operation_id,
            actor_id: caller.id.as_str(),
            req_hash: &req_hash,
            reason: params.reason,
            incomplete_streams,
            now_utc: &now_utc,
        },
        &mut next_id,
    ) {
        return Ok(Err(e));
    }
    if let Err(err) = save_budget_aggregate(office, &agg)? {
        return Ok(Err(err));
    }
    Ok(Ok(BudgetMutationResult {
        budget: render_budget_view(office, &agg, true)?,
        replayed: false,
    }))
}

pub(super) fn op_budget_keep_paused(
    office: &Office,
    actor: &str,
    params: BudgetKeepPausedParams,
) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
    let caller = match office.require_active_member(actor)? {
        Ok(emp) => emp,
        Err(e) => return Ok(Err(e)),
    };
    // Authorize BEFORE loading budget or checking scope (`Pass 3 Low`).
    if caller.id.as_str() != office.owner.as_str() {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the organization owner can keep an agent budget paused",
        )));
    }
    if !org_proto::is_uuid_v7(&params.operation_id) {
        return Ok(Err(invalid_op_id()));
    }
    let Some(mut agg) = office
        .budgets
        .get_by_id(&office.info.org_id, &params.budget_id)?
    else {
        return Ok(Err(ApiError::new(ErrorCode::NotFound, "budget not found")));
    };
    if agg.budget.scope_kind != BudgetScopeKind::Agent {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "keep_paused applies only to agent budgets",
        )));
    }
    let req_hash = request_fingerprint(&json!({
        "budget_id": params.budget_id,
        "reason": params.reason,
    }));
    match check_operation_replay(
        &agg,
        &params.operation_id,
        caller.id.as_str(),
        "budget.keep_paused",
        &req_hash,
    ) {
        Ok(true) => {
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
    mark_open_periods_kept_paused(&mut agg);
    // Do NOT resolve `budget_reached` notice: `doc-92:49` & `doc-51:155` retain item & hold until reset/resolution (`Pass 1 Med #11`).
    push_budget_event(
        office,
        &mut agg,
        &params.operation_id,
        "stop_requested",
        caller.id.as_str(),
        &now_utc,
        json!({
            "action": "budget.keep_paused",
            "request_hash": req_hash,
            "reason": params.reason,
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
