//! Budget threshold application: deduplicated 80%/100% notices, independent execution
//! holds, and hold/notice release when a policy is cleared, satisfied or superseded.

use org_proto::budgets::{
    extended_limit, reached_80_percent, BudgetHoldState, BudgetPolicyState, BudgetScopeKind,
    BudgetStage,
};
use serde_json::json;

use super::EvalOutcome;
use crate::{
    BudgetAggregate, BudgetEventRecord, BudgetHoldRecord, BudgetNoticeRecord, BudgetPeriodRecord,
    BudgetPolicyVersion,
};

pub(super) fn apply_thresholds_and_holds(
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

pub(super) fn release_satisfied_holds(
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
