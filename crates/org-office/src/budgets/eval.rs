//! Pure deterministic `doc-92` budget evaluator: calendar period management, exact integer
//! 80%/100% thresholds, deduplicated notices, independent holds, one-time +20% extension, and
//! hold release on raise/clear/extend/reset (`TASK-53`, `FC-089..093`).

mod checkpoint;
mod extend;
mod thresholds;

use org_proto::budgets::{BudgetHoldState, BudgetPeriodKind, BudgetScopeKind, BudgetUnit};
use org_proto::costs::{CompletenessStatus, UsageCostBasis, UsageRecordView};
use org_proto::tz::{format_utc_bounds, local_day_bounds_utc, local_month_bounds_utc};
use org_proto::{ApiError, ErrorCode};
use serde_json::json;

pub use self::checkpoint::checkpoint_ref_regressed;
use self::checkpoint::merge_checkpoint_refs;
pub use self::extend::{apply_extend_once, ExtendOnceRequest};
use self::thresholds::apply_thresholds_and_holds;
use super::{BudgetAggregate, BudgetEventRecord, BudgetPeriodRecord, BudgetPolicyVersion};
use crate::accounting::{record_is_api_equivalent, record_known_tokens, UsageCoverageSummary};

/// Result of evaluating one budget against effective usage records at `now_ms`.
#[derive(Debug, Clone, Default)]
pub struct EvalOutcome {
    pub changed: bool,
    pub newly_reached_task_id: Option<String>,
    pub released_hold: bool,
}

/// Computes `(period_key, starts_at, ends_at)` for `period_kind` in `time_zone` at `now_ms`.
pub fn current_period_window(
    period_kind: BudgetPeriodKind,
    time_zone: &str,
    now_ms: u64,
) -> Result<(String, Option<String>, Option<String>), ApiError> {
    match period_kind {
        BudgetPeriodKind::Lifetime => Ok(("lifetime".into(), None, None)),
        BudgetPeriodKind::Day => {
            let (key, s, e) = local_day_bounds_utc(time_zone, now_ms).ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "invalid IANA time zone for daily budget",
                )
            })?;
            let (sa, ea) = format_utc_bounds(s, e).ok_or_else(|| {
                ApiError::new(ErrorCode::StorageError, "failed to format period bounds")
            })?;
            Ok((key, Some(sa), Some(ea)))
        }
        BudgetPeriodKind::Month => {
            let (key, s, e) = local_month_bounds_utc(time_zone, now_ms).ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "invalid IANA time zone for monthly budget",
                )
            })?;
            let (sa, ea) = format_utc_bounds(s, e).ok_or_else(|| {
                ApiError::new(ErrorCode::StorageError, "failed to format period bounds")
            })?;
            Ok((key, Some(sa), Some(ea)))
        }
        _ => Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "unknown budget period kind",
        )),
    }
}

/// Evaluates `agg` against `effective` usage records at `now_ms`, updating period totals,
/// performing calendar resets, emitting deduplicated 80%/100% notices, and managing independent holds.
pub fn evaluate_budget(
    agg: &mut BudgetAggregate,
    effective: &[UsageRecordView],
    coverage: &UsageCoverageSummary,
    now_ms: u64,
    now_utc: &str,
    continues: &mut dyn FnMut(
        &crate::accounting::VerifiedStreamWatermark,
    ) -> Result<bool, ApiError>,
    next_id: &mut dyn FnMut() -> String,
) -> Result<EvalOutcome, ApiError> {
    let mut out = EvalOutcome::default();
    let policy = agg
        .current_policy()
        .cloned()
        .ok_or_else(|| ApiError::new(ErrorCode::StorageError, "budget missing current policy"))?;
    let (period_key, starts_at, ends_at) =
        current_period_window(agg.budget.period_kind, &agg.budget.time_zone, now_ms)?;

    ensure_current_period(
        agg,
        PeriodSpec {
            policy: &policy,
            period_key: &period_key,
            starts_at,
            ends_at,
            now_utc,
        },
        next_id,
        &mut out,
    )?;

    let period_idx = agg
        .periods
        .iter()
        .position(|p| p.state == "open")
        .ok_or_else(|| ApiError::new(ErrorCode::StorageError, "budget missing open period"))?;

    let stats = compute_period_usage(agg, &agg.periods[period_idx], &policy, effective);
    let prev_ref = agg.periods[period_idx].ledger_checkpoint_ref.as_deref();
    let regressed = checkpoint_ref_regressed(prev_ref, continues)?;
    let effective_incomplete = coverage
        .incomplete_streams
        .saturating_add(u64::from(regressed));
    let effective_status =
        if effective_incomplete > 0 && stats.status == CompletenessStatus::Complete {
            CompletenessStatus::Partial
        } else {
            stats.status
        };
    let next_checkpoint_ref =
        merge_checkpoint_refs(prev_ref, coverage.checkpoint_ref.as_deref(), continues)?;
    let p = &mut agg.periods[period_idx];
    if p.used_amount != stats.used
        || p.usage_status != effective_status
        || p.policy_revision != policy.revision
        || p.includes_estimates != stats.includes_estimates
        || p.includes_api_equivalent != stats.includes_api_equivalent
        || p.unpriced_records != stats.unpriced
        || p.incomplete_records != stats.incomplete
        || p.ledger_checkpoint_ref != next_checkpoint_ref
        || p.evaluated_at.is_none()
    {
        p.used_amount = stats.used;
        p.usage_status = effective_status;
        p.includes_estimates = stats.includes_estimates;
        p.includes_api_equivalent = stats.includes_api_equivalent;
        p.unpriced_records = stats.unpriced;
        p.incomplete_records = stats.incomplete;
        p.policy_revision = policy.revision;
        p.evaluated_at = Some(now_utc.to_owned());
        p.ledger_checkpoint_ref = next_checkpoint_ref;
        out.changed = true;
    }

    let period_snap = agg.periods[period_idx].clone();
    apply_thresholds_and_holds(
        agg,
        &policy,
        &period_snap,
        effective_incomplete,
        now_utc,
        next_id,
        &mut out,
    );
    Ok(out)
}

struct PeriodSpec<'a> {
    policy: &'a BudgetPolicyVersion,
    period_key: &'a str,
    starts_at: Option<String>,
    ends_at: Option<String>,
    now_utc: &'a str,
}

fn ensure_current_period(
    agg: &mut BudgetAggregate,
    spec: PeriodSpec<'_>,
    next_id: &mut dyn FnMut() -> String,
    out: &mut EvalOutcome,
) -> Result<(), ApiError> {
    let PeriodSpec {
        policy,
        period_key,
        starts_at,
        ends_at,
        now_utc,
    } = spec;
    if let Some(open_idx) = agg.periods.iter().position(|p| p.state == "open") {
        if agg.periods[open_idx].period_key == period_key {
            return Ok(());
        }
        if agg
            .periods
            .iter()
            .any(|p| p.period_key == period_key && p.state != "open")
        {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "time_zone change collides with an already-closed budget period",
            ));
        }
        // Calendar reset (`doc-92`): close old period, release its hold, emit period_reset event.
        let old_id = agg.periods[open_idx].id.clone();
        agg.periods[open_idx].state = "closed".into();
        for hold in &mut agg.holds {
            if hold.period_id == old_id && hold.state != BudgetHoldState::Released {
                hold.state = BudgetHoldState::Released;
                hold.updated_at = now_utc.to_owned();
                hold.release_reason = Some("period_reset".into());
                out.released_hold = true;
            }
        }
        for notice in &mut agg.notices {
            if notice.period_id == old_id && notice.delivery_state != "resolved" {
                notice.delivery_state = "resolved".into();
                notice.resolved_at = Some(now_utc.to_owned());
            }
        }
        let new_period_id = next_id();
        agg.periods.push(BudgetPeriodRecord {
            organization_id: agg.budget.organization_id.clone(),
            budget_id: agg.budget.id.clone(),
            id: new_period_id.clone(),
            policy_revision: policy.revision,
            period_key: period_key.to_owned(),
            starts_at,
            ends_at,
            used_amount: 0,
            usage_status: CompletenessStatus::Unavailable,
            includes_estimates: false,
            includes_api_equivalent: false,
            unpriced_records: 0,
            incomplete_records: 0,
            extension_used: false,
            kept_paused: false,
            state: "open".into(),
            evaluated_at: None,
            ledger_checkpoint_ref: None,
        });
        agg.events.push(BudgetEventRecord {
            organization_id: agg.budget.organization_id.clone(),
            id: next_id(),
            budget_id: agg.budget.id.clone(),
            period_id: new_period_id.clone(),
            operation_id: format!("period_reset:{old_id}:{new_period_id}"),
            kind: "period_reset".into(),
            policy_revision: policy.revision,
            actor_kind: "system".into(),
            actor_id: None,
            occurred_at: now_utc.to_owned(),
            payload_json: json!({
                "old_period_id": old_id,
                "new_period_id": new_period_id,
                "new_period_key": period_key,
            })
            .to_string(),
        });
        out.changed = true;
        return Ok(());
    }
    agg.periods.push(BudgetPeriodRecord {
        organization_id: agg.budget.organization_id.clone(),
        budget_id: agg.budget.id.clone(),
        id: next_id(),
        policy_revision: policy.revision,
        period_key: period_key.to_owned(),
        starts_at,
        ends_at,
        used_amount: 0,
        usage_status: CompletenessStatus::Unavailable,
        includes_estimates: false,
        includes_api_equivalent: false,
        unpriced_records: 0,
        incomplete_records: 0,
        extension_used: false,
        kept_paused: false,
        state: "open".into(),
        evaluated_at: None,
        ledger_checkpoint_ref: None,
    });
    out.changed = true;
    Ok(())
}

struct PeriodUsageStats {
    used: u64,
    status: CompletenessStatus,
    includes_estimates: bool,
    includes_api_equivalent: bool,
    unpriced: u64,
    incomplete: u64,
}

fn compute_period_usage(
    agg: &BudgetAggregate,
    period: &BudgetPeriodRecord,
    policy: &BudgetPolicyVersion,
    effective: &[UsageRecordView],
) -> PeriodUsageStats {
    let start_ms = period
        .starts_at
        .as_deref()
        .and_then(org_proto::parse_rfc3339_ms);
    let end_ms = period
        .ends_at
        .as_deref()
        .and_then(org_proto::parse_rfc3339_ms);
    let mut stats = PeriodUsageStats {
        used: 0,
        status: CompletenessStatus::Complete,
        includes_estimates: false,
        includes_api_equivalent: false,
        unpriced: 0,
        incomplete: 0,
    };
    let mut matched = 0u64;
    let mut known_any = false;
    for rec in effective {
        if !record_matches_budget(agg, rec, start_ms, end_ms) {
            continue;
        }
        matched += 1;
        if accumulate_record_usage(&mut stats, rec, policy.unit) {
            known_any = true;
        }
    }
    stats.status = if matched == 0 || !known_any {
        CompletenessStatus::Unavailable
    } else if stats.incomplete > 0 || stats.unpriced > 0 {
        CompletenessStatus::Partial
    } else {
        CompletenessStatus::Complete
    };
    stats
}

fn accumulate_record_usage(
    stats: &mut PeriodUsageStats,
    rec: &UsageRecordView,
    unit: BudgetUnit,
) -> bool {
    if rec.cost_micro_usd.is_none() {
        stats.unpriced += 1;
    }
    if matches!(
        rec.cost_basis,
        UsageCostBasis::RuntimeEstimate | UsageCostBasis::TokenEstimate
    ) {
        stats.includes_estimates = true;
    }
    if record_is_api_equivalent(rec) {
        stats.includes_api_equivalent = true;
    }
    const MAX_SQLITE_I64: u64 = i64::MAX as u64;
    match unit {
        BudgetUnit::MicroUsd => {
            if rec.cost_status != CompletenessStatus::Complete {
                stats.incomplete += 1;
            }
            if let Some(cost) = rec.cost_micro_usd {
                // Saturate at `i64::MAX` so accumulated spend always stays representable in
                // SQLite `INTEGER` columns while still triggering every valid budget cap
                // (`TASK-97` / Final Gate Finding #4).
                stats.used = stats.used.saturating_add(cost).min(MAX_SQLITE_I64);
                return true;
            }
            false
        }
        BudgetUnit::Tokens => {
            if rec.token_status != CompletenessStatus::Complete {
                stats.incomplete += 1;
            }
            if let Some(toks) = record_known_tokens(rec) {
                stats.used = stats.used.saturating_add(toks).min(MAX_SQLITE_I64);
                return true;
            }
            false
        }
        _ => false,
    }
}

fn record_matches_budget(
    agg: &BudgetAggregate,
    rec: &UsageRecordView,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
) -> bool {
    let target_ok = match agg.budget.scope_kind {
        BudgetScopeKind::Task => rec.task_id == agg.budget.task_id && agg.budget.task_id.is_some(),
        BudgetScopeKind::Agent => {
            rec.employee_id == agg.budget.agent_id && agg.budget.agent_id.is_some()
        }
        BudgetScopeKind::Release => {
            rec.release_id == agg.budget.release_id && agg.budget.release_id.is_some()
        }
        _ => false,
    };
    if !target_ok {
        return false;
    }
    if start_ms.is_some() || end_ms.is_some() {
        let Some(occ_ms) = org_proto::parse_rfc3339_ms(&rec.occurred_at) else {
            return false;
        };
        if start_ms.is_some_and(|s| occ_ms < s) || end_ms.is_some_and(|e| occ_ms >= e) {
            return false;
        }
    }
    true
}
