//! Pure deterministic `doc-92` budget evaluator: calendar period management, exact integer
//! 80%/100% thresholds, deduplicated notices, independent holds, one-time +20% extension, and
//! hold release on raise/clear/extend/reset (`TASK-53`, `FC-089..093`).

use org_proto::budgets::{
    extended_limit, reached_80_percent, BudgetHoldState, BudgetPeriodKind, BudgetPolicyState,
    BudgetScopeKind, BudgetStage, BudgetUnit,
};
use org_proto::costs::{CompletenessStatus, UsageCostBasis, UsageRecordView};
use org_proto::tz::{format_utc_bounds, local_day_bounds_utc, local_month_bounds_utc};
use org_proto::{ApiError, ErrorCode};
use serde_json::json;

use super::{
    BudgetAggregate, BudgetEventRecord, BudgetHoldRecord, BudgetNoticeRecord, BudgetPeriodRecord,
    BudgetPolicyVersion,
};
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

/// Returns `true` if any previously verified stream watermark in `prev` (structured JSON array of
/// `VerifiedStreamWatermark`) is malformed, missing, regressed, or replaced/forked according to
/// `continues` (`TASK-97` / Final Gate Pass 7 #2, Pass 8 #1/#3).
pub fn checkpoint_ref_regressed(
    prev: Option<&str>,
    continues: &mut dyn FnMut(
        &crate::accounting::VerifiedStreamWatermark,
    ) -> Result<bool, ApiError>,
) -> Result<bool, ApiError> {
    let Some(prev_watermarks) = parse_verified_watermarks(prev) else {
        return Ok(true);
    };
    for wm in &prev_watermarks {
        if wm.high_water_sequence > 0 && !continues(wm)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Monotonically merges previously saved verified watermarks with newly verified watermarks in
/// `current`, never overwriting a prior watermark whose chain did not continue and never saving
/// unverified checkpoints (`TASK-97` / Final Gate Pass 8 #1/#3).
fn merge_checkpoint_refs(
    prev: Option<&str>,
    current: Option<&str>,
    continues: &mut dyn FnMut(
        &crate::accounting::VerifiedStreamWatermark,
    ) -> Result<bool, ApiError>,
) -> Result<Option<String>, ApiError> {
    let Some(prev_list) = parse_verified_watermarks(prev) else {
        return Ok(prev.map(str::to_owned));
    };
    let mut merged: std::collections::BTreeMap<
        (String, String),
        crate::accounting::VerifiedStreamWatermark,
    > = std::collections::BTreeMap::new();
    for wm in prev_list {
        merged.insert((wm.origin_office_id.clone(), wm.producer_id.clone()), wm);
    }
    if let Some(cur_list) = parse_verified_watermarks(current) {
        for cur in cur_list {
            let key = (cur.origin_office_id.clone(), cur.producer_id.clone());
            match merged.get(&key) {
                Some(existing) => {
                    if cur.high_water_sequence >= existing.high_water_sequence
                        && (existing.high_water_sequence == 0 || continues(existing)?)
                    {
                        merged.insert(key, cur);
                    }
                }
                None => {
                    merged.insert(key, cur);
                }
            }
        }
    }
    if merged.is_empty() {
        Ok(None)
    } else {
        let vec: Vec<crate::accounting::VerifiedStreamWatermark> = merged.into_values().collect();
        serde_json::to_string(&vec)
            .map(Some)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to encode checkpoint_ref"))
    }
}

fn parse_verified_watermarks(
    raw: Option<&str>,
) -> Option<Vec<crate::accounting::VerifiedStreamWatermark>> {
    let Some(raw) = raw else {
        return Some(Vec::new());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(Vec::new());
    }
    serde_json::from_str(trimmed).ok()
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

fn apply_thresholds_and_holds(
    agg: &mut BudgetAggregate,
    policy: &BudgetPolicyVersion,
    period: &BudgetPeriodRecord,
    incomplete_streams: u64,
    now_utc: &str,
    next_id: &mut dyn FnMut() -> String,
    out: &mut EvalOutcome,
) {
    let Some(base_limit) = (policy.state == BudgetPolicyState::Active)
        .then_some(policy.limit_amount)
        .flatten()
    else {
        release_satisfied_holds(agg, &period.id, "policy_cleared", now_utc, out);
        return;
    };
    let stage = if period.extension_used {
        BudgetStage::Extended
    } else {
        BudgetStage::Base
    };
    let effective_limit = if period.extension_used {
        extended_limit(base_limit).unwrap_or(base_limit)
    } else {
        base_limit
    };
    // `doc-92`: an incomplete accounting archive cannot prove capacity remains, so never
    // release an existing hold solely because observed totals fell below the limit when
    // stream coverage has gaps (`TASK-97` / Final Gate Finding #1).
    if period.used_amount < effective_limit && incomplete_streams == 0 {
        release_satisfied_holds(agg, &period.id, "policy_satisfied", now_utc, out);
    }
    // 80% base threshold: one deduplicated neutral notice to last policy changer (never Inbox item).
    if stage == BudgetStage::Base && reached_80_percent(period.used_amount, base_limit) {
        ensure_notice(
            agg,
            NoticeSpec {
                period_id: &period.id,
                policy_revision: policy.revision,
                stage: BudgetStage::Base,
                threshold: 80,
                recipient_kind: "employee",
                recipient_id: Some(policy.changed_by_id.clone()),
                kind: "neutral",
                now_utc,
            },
            next_id,
            out,
        );
    }
    // 100% threshold at current stage.
    if period.used_amount >= effective_limit {
        apply_reached_threshold(
            agg,
            policy,
            period,
            ReachedCtx {
                stage,
                effective_limit,
                now_utc,
            },
            next_id,
            out,
        );
    }
}

struct ReachedCtx<'a> {
    stage: BudgetStage,
    effective_limit: u64,
    now_utc: &'a str,
}

fn apply_reached_threshold(
    agg: &mut BudgetAggregate,
    policy: &BudgetPolicyVersion,
    period: &BudgetPeriodRecord,
    ctx: ReachedCtx<'_>,
    next_id: &mut dyn FnMut() -> String,
    out: &mut EvalOutcome,
) {
    let ReachedCtx {
        stage,
        effective_limit,
        now_utc,
    } = ctx;
    if agg.budget.scope_kind == BudgetScopeKind::Release {
        // Release warning caps emit neutral notice ONLY — never hold or Budget reached Inbox item (`doc-92`).
        ensure_notice(
            agg,
            NoticeSpec {
                period_id: &period.id,
                policy_revision: policy.revision,
                stage,
                threshold: 100,
                recipient_kind: "employee",
                recipient_id: Some(policy.changed_by_id.clone()),
                kind: "neutral",
                now_utc,
            },
            next_id,
            out,
        );
        return;
    }
    release_superseded_holds_and_notices(agg, &period.id, policy.revision, stage, now_utc, out);
    let existing_idx = agg.holds.iter().position(|h| {
        h.period_id == period.id && h.policy_revision == policy.revision && h.stage == stage
    });
    let should_trigger = match existing_idx {
        None => {
            agg.holds.push(BudgetHoldRecord {
                organization_id: agg.budget.organization_id.clone(),
                budget_id: agg.budget.id.clone(),
                period_id: period.id.clone(),
                policy_revision: policy.revision,
                stage,
                state: BudgetHoldState::StopPending,
                created_at: now_utc.to_owned(),
                updated_at: now_utc.to_owned(),
                release_reason: None,
            });
            true
        }
        Some(idx) if agg.holds[idx].state == BudgetHoldState::Released => {
            // Re-activate hold if spend crossed back to/above limit after a correction (`Pass 1 High #7`).
            agg.holds[idx].state = BudgetHoldState::StopPending;
            agg.holds[idx].updated_at = now_utc.to_owned();
            agg.holds[idx].release_reason = None;
            true
        }
        Some(_) => false,
    };
    if should_trigger {
        ensure_open_budget_reached_notice(
            agg,
            NoticeSpec {
                period_id: &period.id,
                policy_revision: policy.revision,
                stage,
                threshold: 100,
                recipient_kind: "owner",
                recipient_id: None,
                kind: "budget_reached",
                now_utc,
            },
            next_id,
            out,
        );
        let reach_seq = agg
            .events
            .iter()
            .filter(|e| {
                e.period_id == period.id
                    && e.policy_revision == policy.revision
                    && e.kind == "reached"
            })
            .count();
        agg.events.push(BudgetEventRecord {
            organization_id: agg.budget.organization_id.clone(),
            id: next_id(),
            budget_id: agg.budget.id.clone(),
            period_id: period.id.clone(),
            operation_id: format!(
                "reached:{}:{}:{}:{reach_seq}",
                period.id,
                policy.revision,
                stage.as_str()
            ),
            kind: "reached".into(),
            policy_revision: policy.revision,
            actor_kind: "system".into(),
            actor_id: None,
            occurred_at: now_utc.to_owned(),
            payload_json: json!({
                "stage": stage.as_str(),
                "used_amount": period.used_amount,
                "effective_limit": effective_limit,
            })
            .to_string(),
        });
        if agg.budget.scope_kind == BudgetScopeKind::Task {
            out.newly_reached_task_id = agg.budget.task_id.clone();
        }
        out.changed = true;
    }
}

fn release_superseded_holds_and_notices(
    agg: &mut BudgetAggregate,
    period_id: &str,
    current_revision: u64,
    current_stage: BudgetStage,
    now_utc: &str,
    out: &mut EvalOutcome,
) {
    for hold in &mut agg.holds {
        if hold.period_id == period_id
            && (hold.policy_revision != current_revision || hold.stage != current_stage)
            && hold.state != BudgetHoldState::Released
        {
            hold.state = BudgetHoldState::Released;
            hold.updated_at = now_utc.to_owned();
            hold.release_reason = Some("policy_superseded".into());
            out.changed = true;
        }
    }
    for notice in &mut agg.notices {
        if notice.period_id == period_id
            && (notice.policy_revision != current_revision || notice.stage != current_stage)
            && notice.kind == "budget_reached"
            && notice.delivery_state != "resolved"
        {
            notice.delivery_state = "resolved".into();
            notice.resolved_at = Some(now_utc.to_owned());
            out.changed = true;
        }
    }
}

fn release_satisfied_holds(
    agg: &mut BudgetAggregate,
    period_id: &str,
    reason: &str,
    now_utc: &str,
    out: &mut EvalOutcome,
) {
    for hold in &mut agg.holds {
        if hold.period_id == period_id && hold.state != BudgetHoldState::Released {
            hold.state = BudgetHoldState::Released;
            hold.updated_at = now_utc.to_owned();
            hold.release_reason = Some(reason.to_owned());
            out.released_hold = true;
            out.changed = true;
        }
    }
    for notice in &mut agg.notices {
        if notice.period_id == period_id
            && notice.kind == "budget_reached"
            && notice.delivery_state != "resolved"
        {
            notice.delivery_state = "resolved".into();
            notice.resolved_at = Some(now_utc.to_owned());
            out.changed = true;
        }
    }
}

struct NoticeSpec<'a> {
    period_id: &'a str,
    policy_revision: u64,
    stage: BudgetStage,
    threshold: u8,
    recipient_kind: &'a str,
    recipient_id: Option<String>,
    kind: &'a str,
    now_utc: &'a str,
}

fn ensure_open_budget_reached_notice(
    agg: &mut BudgetAggregate,
    spec: NoticeSpec<'_>,
    next_id: &mut dyn FnMut() -> String,
    out: &mut EvalOutcome,
) {
    if let Some(existing) = agg.notices.iter_mut().find(|n| {
        n.period_id == spec.period_id
            && n.policy_revision == spec.policy_revision
            && n.stage == spec.stage
            && n.threshold == spec.threshold
    }) {
        if existing.delivery_state == "resolved" {
            existing.delivery_state = "pending".into();
            existing.resolved_at = None;
            out.changed = true;
        }
        return;
    }
    ensure_notice(agg, spec, next_id, out);
}

fn ensure_notice(
    agg: &mut BudgetAggregate,
    spec: NoticeSpec<'_>,
    next_id: &mut dyn FnMut() -> String,
    out: &mut EvalOutcome,
) {
    let exists = agg.notices.iter().any(|n| {
        n.period_id == spec.period_id
            && n.policy_revision == spec.policy_revision
            && n.stage == spec.stage
            && n.threshold == spec.threshold
    });
    if exists {
        return;
    }
    agg.notices.push(BudgetNoticeRecord {
        organization_id: agg.budget.organization_id.clone(),
        id: next_id(),
        budget_id: agg.budget.id.clone(),
        period_id: spec.period_id.to_owned(),
        policy_revision: spec.policy_revision,
        stage: spec.stage,
        threshold: spec.threshold,
        recipient_kind: spec.recipient_kind.to_owned(),
        recipient_id: spec.recipient_id,
        kind: spec.kind.to_owned(),
        delivery_state: "pending".into(),
        created_at: spec.now_utc.to_owned(),
        resolved_at: None,
    });
    out.changed = true;
}

/// Inputs for validating and applying a one-time +20% extension (`doc-92`).
pub struct ExtendOnceRequest<'a> {
    pub operation_id: &'a str,
    pub actor_id: &'a str,
    pub req_hash: &'a str,
    pub reason: Option<String>,
    pub incomplete_streams: u64,
    pub now_utc: &'a str,
}

/// Validates and applies the one-time +20% extension (`L + floor(L/5)`) (`doc-92`).
pub fn apply_extend_once(
    agg: &mut BudgetAggregate,
    req: ExtendOnceRequest<'_>,
    next_id: &mut dyn FnMut() -> String,
) -> Result<(), ApiError> {
    let ExtendOnceRequest {
        operation_id,
        actor_id,
        req_hash,
        reason,
        incomplete_streams,
        now_utc,
    } = req;
    if agg.budget.scope_kind == BudgetScopeKind::Release {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "release warning caps do not use one-time extensions",
        ));
    }
    if incomplete_streams > 0 {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "cannot grant one-time extension while accounting stream coverage is incomplete",
        ));
    }
    let policy = agg
        .current_policy()
        .cloned()
        .ok_or_else(|| ApiError::new(ErrorCode::StorageError, "budget missing policy"))?;
    let Some(base_limit) = (policy.state == BudgetPolicyState::Active)
        .then_some(policy.limit_amount)
        .flatten()
    else {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "cannot extend a cleared budget",
        ));
    };
    let period_idx = agg
        .periods
        .iter()
        .position(|p| p.state == "open")
        .ok_or_else(|| ApiError::new(ErrorCode::InvalidRequest, "no active budget period"))?;
    let period = &agg.periods[period_idx];
    if period.extension_used {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "This budget already used its one 20% extension.",
        ));
    }
    if period.used_amount < base_limit {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "Allow 20% once is available only when the budget is reached.",
        ));
    }
    let Some(new_limit) = extended_limit(base_limit) else {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "20% extension increment is zero; raise the budget instead",
        ));
    };
    if new_limit <= period.used_amount {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "20% extension does not exceed current known spend; raise the budget instead",
        ));
    }
    let period_id = period.id.clone();
    let used = period.used_amount;
    agg.periods[period_idx].extension_used = true;
    agg.events.push(BudgetEventRecord {
        organization_id: agg.budget.organization_id.clone(),
        id: next_id(),
        budget_id: agg.budget.id.clone(),
        period_id: period_id.clone(),
        operation_id: operation_id.to_owned(),
        kind: "extended".into(),
        policy_revision: policy.revision,
        actor_kind: "employee".into(),
        actor_id: Some(actor_id.to_owned()),
        occurred_at: now_utc.to_owned(),
        payload_json: json!({
            "action": "budget.extend_once",
            "request_hash": req_hash,
            "base_limit": base_limit,
            "effective_limit": new_limit,
            "used_amount": used,
            "reason": reason,
        })
        .to_string(),
    });
    let mut dummy_out = EvalOutcome::default();
    release_satisfied_holds(agg, &period_id, "extended_once", now_utc, &mut dummy_out);
    Ok(())
}
