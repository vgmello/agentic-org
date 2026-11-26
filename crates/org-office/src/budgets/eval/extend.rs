//! The one-time +20% budget extension (`doc-92`): validation, period/policy updates and
//! release of the holds it satisfies.

use org_proto::budgets::{extended_limit, BudgetPolicyState, BudgetScopeKind};
use org_proto::{ApiError, ErrorCode};
use serde_json::json;

use super::super::{BudgetAggregate, BudgetEventRecord};
use super::thresholds::release_satisfied_holds;
use super::EvalOutcome;

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
