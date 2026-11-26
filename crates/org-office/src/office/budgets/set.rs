//! Setting a budget: parameter and timezone validation, the aggregate, its policy and events.

use org_proto::budgets::{
    BudgetMutationResult, BudgetPeriodKind, BudgetPolicyState, BudgetScopeKind, BudgetSetParams,
    BudgetTimeZoneSource, BudgetUnit,
};
use org_proto::tz::validate_iana_timezone;
use org_proto::{ApiError, ErrorCode};
use serde_json::json;

use crate::budgets::{BudgetAggregate, BudgetPolicyVersion, BudgetRecord};
use crate::error::OfficeError;
use crate::office::Office;
use crate::organization::records::Employee;
use crate::time::format_rfc3339_ms;

use super::authority::{authorize_budget_set, resolve_budget_target};
use super::evaluation::{evaluate_in_memory, render_budget_view};
use super::receipts::invalid_op_id;
use super::reconcile::{ensure_held_task_blocked, reconcile_pending_stop};
use super::{
    check_operation_replay, push_budget_event, request_fingerprint, save_budget_aggregate,
    ResolvedTarget, ResolvedTz,
};

pub(super) fn op_budget_set(
    office: &Office,
    actor: &str,
    params: BudgetSetParams,
) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
    let caller = match office.require_active_member(actor)? {
        Ok(emp) => emp,
        Err(e) => return Ok(Err(e)),
    };
    if let Err(e) = validate_set_params(&params) {
        return Ok(Err(e));
    }
    let target = match resolve_budget_target(office, &params)? {
        Ok(t) => t,
        Err(e) => return Ok(Err(e)),
    };
    let tz = match resolve_budget_timezone(&params, &caller, &office.owner) {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    let mut existing = office.budgets.find_by_target(
        &office.info.org_id,
        params.scope_kind,
        target.task_id.as_deref(),
        target.agent_id.as_deref(),
        target.release_id.as_deref(),
        params.period_kind,
    )?;
    // Authorize BEFORE checking operation_id replay (`Pass 1 High #2`).
    if let Err(e) = authorize_budget_set(office, &caller, &params, &target, existing.as_ref())? {
        return Ok(Err(e));
    }
    if let Some(ref mut agg) = existing {
        reconcile_pending_stop(office, agg)?;
    }
    let req_hash = request_fingerprint(&json!({
        "scope_kind": params.scope_kind.as_str(),
        "task_id": target.task_id,
        "agent_id": target.agent_id,
        "release_id": target.release_id,
        "period_kind": params.period_kind.as_str(),
        "unit": params.unit.as_str(),
        "limit_amount": params.limit_amount,
        "time_zone": tz.zone,
        "reason": params.reason,
    }));
    if let Some(ref agg) = existing {
        match check_operation_replay(
            agg,
            &params.operation_id,
            caller.id.as_str(),
            "budget.set",
            &req_hash,
        ) {
            Ok(true) => {
                // Reconcile any active task hold before acknowledging a replay in case a crash
                // occurred between saving the hold and transitioning the task (`TASK-97` / Pass 8).
                ensure_held_task_blocked(office, agg)?;
                return Ok(Ok(BudgetMutationResult {
                    budget: render_budget_view(office, agg, true)?,
                    replayed: true,
                }));
            }
            Ok(false) => {}
            Err(e) => return Ok(Err(e)),
        }
    }
    apply_set_and_persist(office, &caller, params, &target, tz, existing, &req_hash)
}

fn apply_set_and_persist(
    office: &Office,
    caller: &Employee,
    params: BudgetSetParams,
    target: &ResolvedTarget,
    tz: ResolvedTz,
    existing: Option<BudgetAggregate>,
    req_hash: &str,
) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    let mut agg = existing.unwrap_or_else(|| new_aggregate(office, &params, target, &tz, &now_utc));
    let (same_policy, limit_or_unit_changed) =
        apply_policy_if_changed(&mut agg, caller, &params, &tz, &now_utc);
    // Resolve next period key BEFORE evaluating so an open period's `extension_used` / `kept_paused`
    // is cleared prior to a single `evaluate_in_memory` call (`Pass 2 High #3`).
    let (next_period_key, _, _) = match crate::budgets::eval::current_period_window(
        agg.budget.period_kind,
        &agg.budget.time_zone,
        now_ms,
    ) {
        Ok(w) => w,
        Err(e) => return Ok(Err(e)),
    };
    if limit_or_unit_changed
        && agg.open_period().map(|p| p.period_key.as_str()) == Some(next_period_key.as_str())
    {
        for p in &mut agg.periods {
            if p.state == "open" {
                p.extension_used = false;
                p.kept_paused = false;
            }
        }
    }
    // Evaluate in-memory once; if timezone shift collides with a closed period, refuse cleanly.
    let _outcome = match evaluate_in_memory(office, &mut agg, now_ms, &now_utc)? {
        Ok(o) => o,
        Err(e) => return Ok(Err(e)),
    };
    // Emit schema-valid event kind (`budgets-schema.sql:117`, `Pass 1 High #1`).
    let kind = if same_policy {
        "evaluated"
    } else {
        "policy_changed"
    };
    let payload = json!({
        "action": "budget.set",
        "request_hash": req_hash,
        "comment_only": same_policy,
        "unit": params.unit.as_str(),
        "limit_amount": params.limit_amount,
        "reason": params.reason,
    })
    .to_string();
    push_budget_event(
        office,
        &mut agg,
        &params.operation_id,
        kind,
        caller.id.as_str(),
        &now_utc,
        payload,
    );
    // Single atomic durable + SQLite save (`Pass 1 Med #14`, `TASK-97` Pass 5 #2).
    if let Err(err) = save_budget_aggregate(office, &agg)? {
        return Ok(Err(err));
    }
    ensure_held_task_blocked(office, &agg)?;
    Ok(Ok(BudgetMutationResult {
        budget: render_budget_view(office, &agg, true)?,
        replayed: false,
    }))
}

fn new_aggregate(
    office: &Office,
    params: &BudgetSetParams,
    target: &ResolvedTarget,
    tz: &ResolvedTz,
    now_utc: &str,
) -> BudgetAggregate {
    BudgetAggregate {
        budget: BudgetRecord {
            organization_id: office.info.org_id.clone(),
            id: office.next_uuid_v7(),
            current_revision: 0,
            scope_kind: params.scope_kind,
            task_id: target.task_id.clone(),
            agent_id: target.agent_id.clone(),
            release_id: target.release_id.clone(),
            period_kind: params.period_kind,
            time_zone: tz.zone.clone(),
            time_zone_source: tz.source,
            time_zone_subject_ref: tz.subject.clone(),
            created_at: now_utc.to_owned(),
        },
        versions: Vec::new(),
        periods: Vec::new(),
        events: Vec::new(),
        notices: Vec::new(),
        holds: Vec::new(),
        pending_stop: None,
    }
}

fn apply_policy_if_changed(
    agg: &mut BudgetAggregate,
    caller: &Employee,
    params: &BudgetSetParams,
    tz: &ResolvedTz,
    now_utc: &str,
) -> (bool, bool) {
    let prev = agg.current_policy();
    let same = prev.is_some_and(|p| {
        p.state == BudgetPolicyState::Active
            && p.unit == params.unit
            && p.limit_amount == Some(params.limit_amount)
            && p.time_zone == tz.zone
    });
    if same {
        return (true, false);
    }
    let limit_or_unit_changed = prev.is_none_or(|p| {
        p.state != BudgetPolicyState::Active
            || p.unit != params.unit
            || p.limit_amount != Some(params.limit_amount)
    });
    let next_rev = agg.budget.current_revision + 1;
    agg.budget.current_revision = next_rev;
    agg.budget.time_zone = tz.zone.clone();
    agg.budget.time_zone_source = tz.source;
    agg.budget.time_zone_subject_ref = tz.subject.clone();
    agg.versions.push(BudgetPolicyVersion {
        organization_id: agg.budget.organization_id.clone(),
        budget_id: agg.budget.id.clone(),
        revision: next_rev,
        state: BudgetPolicyState::Active,
        unit: params.unit,
        limit_amount: Some(params.limit_amount),
        time_zone: tz.zone.clone(),
        time_zone_source: tz.source,
        time_zone_subject_ref: tz.subject.clone(),
        changed_by_id: caller.id.as_str().to_owned(),
        changed_at: now_utc.to_owned(),
        reason: params.reason.clone(),
    });
    (false, limit_or_unit_changed)
}

/// Maximum representable base limit whose +20% extension (`L + L/5`) stays `<= i64::MAX`,
/// keeping validation, display, extension, and SQLite enforcement thresholds identical.
const MAX_BUDGET_LIMIT: u64 = (i64::MAX as u64) / 6 * 5;

fn validate_set_params(params: &BudgetSetParams) -> Result<(), ApiError> {
    if !org_proto::is_uuid_v7(&params.operation_id) {
        return Err(invalid_op_id());
    }
    if params.limit_amount == 0 || params.limit_amount > MAX_BUDGET_LIMIT {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "limit_amount must be between 1 and the maximum extendable 64-bit budget limit",
        ));
    }
    if !matches!(params.unit, BudgetUnit::MicroUsd | BudgetUnit::Tokens) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "unit must be micro_usd or tokens",
        ));
    }
    let period_ok = match params.scope_kind {
        BudgetScopeKind::Task | BudgetScopeKind::Release => {
            params.period_kind == BudgetPeriodKind::Lifetime
        }
        BudgetScopeKind::Agent => {
            matches!(
                params.period_kind,
                BudgetPeriodKind::Day | BudgetPeriodKind::Month
            )
        }
        _ => false,
    };
    if !period_ok {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "period_kind does not match scope_kind (task/release use lifetime; agent uses day/month)",
        ));
    }
    Ok(())
}

fn resolve_budget_timezone(
    params: &BudgetSetParams,
    caller: &Employee,
    owner: &org_store::Id,
) -> Result<ResolvedTz, ApiError> {
    let zone = match params.time_zone.as_deref().map(str::trim) {
        Some(raw) if !raw.is_empty() => {
            if !validate_iana_timezone(raw) {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "time_zone must be a valid IANA time zone",
                ));
            }
            raw.to_owned()
        }
        _ if params.scope_kind == BudgetScopeKind::Agent => {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "a valid IANA time_zone is required for daily and monthly agent budgets",
            ));
        }
        _ => "UTC".to_owned(),
    };
    let is_owner = caller.id.as_str() == owner.as_str();
    let source = match params.time_zone_source {
        Some(BudgetTimeZoneSource::OwnerPerson) if is_owner => BudgetTimeZoneSource::OwnerPerson,
        Some(BudgetTimeZoneSource::Hq) => BudgetTimeZoneSource::Hq,
        _ if is_owner && params.time_zone.is_some() => BudgetTimeZoneSource::OwnerPerson,
        _ => BudgetTimeZoneSource::Hq,
    };
    let subject = match source {
        BudgetTimeZoneSource::OwnerPerson => Some(owner.as_str().to_owned()),
        _ => None,
    };
    Ok(ResolvedTz {
        zone,
        source,
        subject,
    })
}
