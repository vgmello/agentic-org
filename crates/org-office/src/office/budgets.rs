//! Office facade for `L3` task, agent (daily/monthly), and release budgets, 80/100 notices,
//! one-time +20% extensions, independent execution holds, and own-agent budget status
//! (`TASK-53`, `doc-92`, `doc-51`, `FC-089..093`, `ASK-8`).

use org_proto::budgets::{
    BudgetAgentOwnParams, BudgetAgentOwnResult, BudgetClearParams, BudgetExtendOnceParams,
    BudgetGetParams, BudgetGetResult, BudgetHoldState, BudgetKeepPausedParams, BudgetListParams,
    BudgetListResult, BudgetMutationResult, BudgetPeriodKind, BudgetPolicyState, BudgetScopeKind,
    BudgetSetParams, BudgetStopParams, BudgetTimeZoneSource, BudgetUnit, BudgetView,
};
use org_proto::costs::UsageRecordView;
use org_proto::tasks::{TaskStatus, TasksTransitionParams};
use org_proto::tz::validate_iana_timezone;
use org_proto::{ApiError, ErrorCode};
use org_store::Sha256;
use serde_json::json;

use crate::budgets::eval::{apply_extend_once, evaluate_budget, EvalOutcome, ExtendOnceRequest};
use crate::budgets::{
    ActiveBudgetHoldInfo, BudgetAggregate, BudgetEventRecord, BudgetPolicyVersion, BudgetRecord,
    OpenBudgetInboxNotice, PendingStopIntent,
};
use crate::error::OfficeError;
use crate::organization::records::{Employee, EmployeeKind, EmployeeStatus};
use crate::time::format_rfc3339_ms;

use super::Office;

const PERSON_ONLY_MSG: &str = "Only a person can raise, clear or extend a budget.";

struct ResolvedTarget {
    task_id: Option<String>,
    agent_id: Option<String>,
    release_id: Option<String>,
    project_id: Option<String>,
}

struct ResolvedTz {
    zone: String,
    source: BudgetTimeZoneSource,
    subject: Option<String>,
}

impl Office {
    pub(super) fn lock_budget_ops(&self) -> std::sync::MutexGuard<'_, ()> {
        self.budget_op_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `budget.set`: sets, raises, or lowers a task, agent (day/month), or release budget.
    pub fn budget_set(
        &self,
        actor: &str,
        params: BudgetSetParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        let _guard = self.lock_budget_ops();
        op_budget_set(self, actor, params)
    }

    /// `budget.clear`: clears an active budget via a new immutable `cleared` policy revision.
    pub fn budget_clear(
        &self,
        actor: &str,
        params: BudgetClearParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        let _guard = self.lock_budget_ops();
        op_budget_clear(self, actor, params)
    }

    /// `budget.extend_once`: applies the one-time +20% (`L + floor(L/5)`) extension (`doc-92`).
    pub fn budget_extend_once(
        &self,
        actor: &str,
        params: BudgetExtendOnceParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        let _guard = self.lock_budget_ops();
        op_budget_extend_once(self, actor, params)
    }

    /// `budget.stop`: stops a reached task budget (cancelling the task) or keeps an agent paused.
    pub fn budget_stop(
        &self,
        actor: &str,
        params: BudgetStopParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        let _guard = self.lock_budget_ops();
        op_budget_stop(self, actor, params)
    }

    /// `budget.keep_paused`: owner keeps a reached agent budget paused until calendar reset.
    pub fn budget_keep_paused(
        &self,
        actor: &str,
        params: BudgetKeepPausedParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        let _guard = self.lock_budget_ops();
        op_budget_keep_paused(self, actor, params)
    }

    /// `budget.get`: fetches one budget with role-appropriate amount visibility (`doc-51` §1).
    pub fn budget_get(
        &self,
        actor: &str,
        params: BudgetGetParams,
    ) -> Result<Result<BudgetGetResult, ApiError>, OfficeError> {
        let _guard = self.lock_budget_ops();
        op_budget_get(self, actor, params)
    }

    /// `budget.list`: lists budgets visible to the caller, filtering out unauthorized agent/release
    /// budgets and stripping amounts for non-PM task viewers (`doc-51` §1).
    pub fn budget_list(
        &self,
        actor: &str,
        params: BudgetListParams,
    ) -> Result<Result<BudgetListResult, ApiError>, OfficeError> {
        let _guard = self.lock_budget_ops();
        op_budget_list(self, actor, params)
    }

    /// `budget.agent_own`: own-agent CLI exception (`agent-os budget status`, `doc-92`, `doc-51`).
    pub fn budget_agent_own(
        &self,
        actor: &str,
        params: BudgetAgentOwnParams,
    ) -> Result<Result<BudgetAgentOwnResult, ApiError>, OfficeError> {
        let _guard = self.lock_budget_ops();
        op_budget_agent_own(self, actor, params)
    }

    /// Returns any unreleased holds affecting `agent_id` or `task_id`, catching calendar resets first.
    pub fn active_budget_holds(
        &self,
        agent_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<Vec<ActiveBudgetHoldInfo>, OfficeError> {
        let _guard = self.lock_budget_ops();
        op_active_holds(self, agent_id, task_id)
    }

    /// Transitions any `stop_pending` holds for `agent_id` (and tasks assigned to it) to `held`.
    pub fn mark_budget_holds_stopped(&self, agent_id: &str) -> Result<bool, OfficeError> {
        let _guard = self.lock_budget_ops();
        op_mark_holds_stopped(self, agent_id)
    }

    /// Returns all unresolved owner `budget_reached` notices for `inbox.list` & `inbox.changed`.
    pub fn open_budget_inbox_notices(&self) -> Result<Vec<OpenBudgetInboxNotice>, OfficeError> {
        let _guard = self.lock_budget_ops();
        op_open_inbox_notices(self)
    }
}

fn request_fingerprint(val: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(val).unwrap_or_default();
    let mut hex = String::with_capacity(64);
    for b in Sha256::of(&bytes).as_bytes() {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{b:02x}");
    }
    hex
}

/// Checks `operation_id` replay AFTER caller authorization has already passed (`Pass 1 High #2`).
fn check_operation_replay(
    agg: &BudgetAggregate,
    operation_id: &str,
    caller_id: &str,
    action: &str,
    req_hash: &str,
) -> Result<bool, ApiError> {
    let Some(ev) = agg.events.iter().find(|e| e.operation_id == operation_id) else {
        return Ok(false);
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&ev.payload_json).unwrap_or(serde_json::Value::Null);
    let same_actor = ev.actor_id.as_deref() == Some(caller_id);
    let same_action = parsed.get("action").and_then(|v| v.as_str()) == Some(action);
    let same_hash = parsed.get("request_hash").and_then(|v| v.as_str()) == Some(req_hash);
    if same_actor && same_action && same_hash {
        Ok(true)
    } else {
        Err(ApiError::new(
            ErrorCode::Conflict,
            "operation_id reused with different actor, action, or parameters",
        ))
    }
}

fn op_budget_set(
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

fn push_budget_event(
    office: &Office,
    agg: &mut BudgetAggregate,
    operation_id: &str,
    kind: &str,
    actor_id: &str,
    now_utc: &str,
    payload_json: String,
) {
    let Some(period_id) = agg.open_period().map(|p| p.id.clone()) else {
        return;
    };
    agg.events.push(BudgetEventRecord {
        organization_id: office.info.org_id.clone(),
        id: office.next_uuid_v7(),
        budget_id: agg.budget.id.clone(),
        period_id,
        operation_id: operation_id.to_owned(),
        kind: kind.to_owned(),
        policy_revision: agg.budget.current_revision,
        actor_kind: "employee".into(),
        actor_id: Some(actor_id.to_owned()),
        occurred_at: now_utc.to_owned(),
        payload_json,
    });
}

fn op_budget_clear(
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

fn op_budget_extend_once(
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

fn op_budget_stop(
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

fn op_budget_keep_paused(
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

fn mark_open_periods_kept_paused(agg: &mut BudgetAggregate) {
    for p in &mut agg.periods {
        if p.state == "open" {
            p.kept_paused = true;
        }
    }
}

fn resolve_reached_notices(agg: &mut BudgetAggregate, now_utc: &str) {
    for n in &mut agg.notices {
        if n.kind == "budget_reached" && n.delivery_state != "resolved" {
            n.delivery_state = "resolved".into();
            n.resolved_at = Some(now_utc.to_owned());
        }
    }
}

fn op_budget_get(
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

fn op_budget_list(
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

fn op_budget_agent_own(
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

pub(super) fn op_reevaluate_for_record(
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

pub(super) fn op_is_task_reached(office: &Office, task_id: &str) -> Result<bool, OfficeError> {
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

pub(super) fn op_active_holds(
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

fn op_mark_holds_stopped(office: &Office, agent_id: &str) -> Result<bool, OfficeError> {
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

fn op_open_inbox_notices(office: &Office) -> Result<Vec<OpenBudgetInboxNotice>, OfficeError> {
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

fn load_and_eval_agent_budget(
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

fn period_watermark_regressed(office: &Office, agg: &BudgetAggregate) -> Result<bool, OfficeError> {
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

fn evaluate_in_memory(
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

fn evaluate_and_save_budget(
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

fn save_budget_aggregate(
    office: &Office,
    agg: &BudgetAggregate,
) -> Result<Result<(), ApiError>, OfficeError> {
    let emp_kinds = collect_employee_kinds(office, agg)?;
    office.budgets.save_aggregate(agg, &emp_kinds)
}

fn collect_employee_kinds(
    office: &Office,
    agg: &BudgetAggregate,
) -> Result<Vec<(String, &'static str)>, OfficeError> {
    let mut ids = std::collections::BTreeSet::new();
    ids.extend(agg.budget.agent_id.iter().cloned());
    ids.extend(agg.budget.time_zone_subject_ref.iter().cloned());
    for v in &agg.versions {
        ids.insert(v.changed_by_id.clone());
        ids.extend(v.time_zone_subject_ref.iter().cloned());
    }
    ids.extend(agg.events.iter().filter_map(|e| e.actor_id.clone()));
    ids.extend(agg.notices.iter().filter_map(|n| n.recipient_id.clone()));
    if let Some(intent) = &agg.pending_stop {
        ids.insert(intent.actor_id.clone());
    }
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let kind = match office.load_employee(&id)? {
            Some(emp) if emp.kind == EmployeeKind::Agent => "agent",
            _ => "person",
        };
        out.push((id, kind));
    }
    Ok(out)
}

fn resolve_budget_target(
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

fn authorize_budget_set(
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

fn require_budget_actor(
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

fn is_pm_of_task(office: &Office, caller: &Employee, task_id: &str) -> Result<bool, OfficeError> {
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

fn render_budget_view(
    office: &Office,
    agg: &BudgetAggregate,
    amounts_visible: bool,
) -> Result<BudgetView, OfficeError> {
    let mut view = agg.to_view(amounts_visible);
    if view.can_extend_once {
        let cov = office.accounting.coverage_summary(&office.info.org_id)?;
        if cov.incomplete_streams > 0 || period_watermark_regressed(office, agg)? {
            view.can_extend_once = false;
        }
    }
    Ok(view)
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

const MAX_TASK_TRANSITION_RETRIES: usize = 8;

/// Reconciles any active (unreleased) task budget hold by transitioning an `InProgress` task to
/// `Blocked`, retrying revision `Conflict`s cleanly (`doc-92`, `TASK-97` Pass 7 Finding #3).
fn ensure_held_task_blocked(office: &Office, agg: &BudgetAggregate) -> Result<(), OfficeError> {
    if agg.budget.scope_kind != BudgetScopeKind::Task || agg.active_hold().is_none() {
        return Ok(());
    }
    if let Some(tid) = agg.budget.task_id.as_deref() {
        block_task_for_budget(office, tid)?;
    }
    Ok(())
}

fn block_task_for_budget(office: &Office, task_id: &str) -> Result<(), OfficeError> {
    for _ in 0..MAX_TASK_TRANSITION_RETRIES {
        let inspected = match office.task_inspect(task_id)? {
            Ok(v) => v,
            Err(e) if e.code == ErrorCode::NotFound => return Ok(()),
            Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
        };
        if inspected.task.status != TaskStatus::InProgress {
            // Confirm durable flush & host custody of the observed non-InProgress status before
            // treating blocking as complete (`TASK-97` / Pass 11 Finding #4).
            office
                .store
                .sync()
                .map_err(|e| OfficeError::storage(std::io::Error::other(e.to_string())))?;
            if office.store.status().stopped.is_some() {
                return Err(OfficeError::storage(std::io::Error::other(
                    "store stopped before confirming task block state",
                )));
            }
            let confirmed = match office.task_inspect(task_id)? {
                Ok(v) => v,
                Err(e) if e.code == ErrorCode::NotFound => return Ok(()),
                Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
            };
            if confirmed.task.revision == inspected.task.revision
                && confirmed.task.status != TaskStatus::InProgress
            {
                return Ok(());
            }
            continue;
        }
        match office.task_transition(
            office.owner.as_str(),
            TasksTransitionParams {
                command_id: office.next_uuid_v7(),
                task: task_id.to_owned(),
                expected_revision: inspected.task.revision,
                status: TaskStatus::Blocked,
                blocked_reason: Some("Budget reached".into()),
                resolution: None,
                note: Some("Task blocked because its budget was reached.".into()),
            },
        )? {
            Ok(_) => return Ok(()),
            Err(e) if e.code == ErrorCode::Conflict => continue,
            Err(e) if e.code == ErrorCode::NotFound => return Ok(()),
            Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
        }
    }
    Err(OfficeError::storage(std::io::Error::other(
        "budget hold task transition conflicted after retries",
    )))
}

enum StopExecutionOutcome {
    Closed(&'static str),
    Refused(String),
}

fn stop_refusal_for_operation(agg: &BudgetAggregate, operation_id: &str) -> Option<ApiError> {
    let ev = agg
        .events
        .iter()
        .find(|e| e.operation_id == operation_id && e.kind == "stop_requested")?;
    let parsed: serde_json::Value = serde_json::from_str(&ev.payload_json).ok()?;
    let outcome = parsed.get("outcome").and_then(|v| v.as_str())?;
    let reason = outcome.strip_prefix("transition_refused:")?;
    Some(ApiError::new(ErrorCode::Conflict, reason.to_owned()))
}

/// Reconciles a persisted `pending_stop` intent against `org-store`'s flushed and host-confirmed
/// task transition receipt before finalizing the `stop_requested` event (`TASK-97` / Pass 7 & 8).
///
/// Resolves `budget_reached` notices ONLY on a verified closing outcome (`cancelled` or
/// `already_closed`). Retryable revision conflicts and unconfirmed receipts keep `pending_stop`
/// intact and return `Err`, while terminal task-transition refusals leave the notice open and
/// return `Ok(Some(ApiError))` so callers observe the refusal.
fn reconcile_pending_stop(
    office: &Office,
    agg: &mut BudgetAggregate,
) -> Result<Option<ApiError>, OfficeError> {
    let Some(mut intent) = agg.pending_stop.clone() else {
        return Ok(None);
    };
    if agg.has_operation(&intent.operation_id) {
        let refusal = stop_refusal_for_operation(agg, &intent.operation_id);
        agg.pending_stop = None;
        if let Err(err) = save_budget_aggregate(office, agg)? {
            return Err(OfficeError::storage(std::io::Error::other(err.message)));
        }
        return Ok(refusal);
    }
    let outcome = execute_or_reconcile_stop_intent(office, agg, &mut intent)?;
    let now_utc = format_rfc3339_ms(office.monotonic_now_ms());
    let (outcome_str, refusal) = match outcome {
        StopExecutionOutcome::Closed(label) => {
            resolve_reached_notices(agg, &now_utc);
            (label.to_owned(), None)
        }
        StopExecutionOutcome::Refused(msg) => (
            format!("transition_refused:{msg}"),
            Some(ApiError::new(ErrorCode::Conflict, msg)),
        ),
    };
    push_budget_event(
        office,
        agg,
        &intent.operation_id,
        "stop_requested",
        &intent.actor_id,
        &now_utc,
        json!({
            "action": "budget.stop",
            "request_hash": intent.request_hash,
            "reason": intent.reason,
            "outcome": outcome_str,
            "task_command_id": intent.task_command_id,
        })
        .to_string(),
    );
    agg.pending_stop = None;
    if let Err(err) = save_budget_aggregate(office, agg)? {
        return Err(OfficeError::storage(std::io::Error::other(err.message)));
    }
    Ok(refusal)
}

fn rotate_stop_intent_command(
    office: &Office,
    agg: &mut BudgetAggregate,
    intent: &mut PendingStopIntent,
) -> Result<(), OfficeError> {
    intent.task_command_id = office.next_uuid_v7();
    intent.task_command_fingerprint = None;
    agg.pending_stop = Some(intent.clone());
    if let Err(err) = save_budget_aggregate(office, agg)? {
        return Err(OfficeError::storage(std::io::Error::other(err.message)));
    }
    Ok(())
}

fn confirm_terminal_task_state(
    office: &Office,
    task_id: &str,
    observed_revision: u64,
) -> Result<Option<StopExecutionOutcome>, OfficeError> {
    // Confirm that the observed terminal task state is flushed and host-confirmed before
    // finalizing `already_closed` (`TASK-97` / Pass 9 Finding #1).
    office
        .store
        .sync()
        .map_err(|e| OfficeError::storage(std::io::Error::other(e.to_string())))?;
    if office.store.status().stopped.is_some() {
        return Err(OfficeError::storage(std::io::Error::other(
            "store stopped before confirming terminal task state",
        )));
    }
    let confirmed = match office.task_inspect(task_id)? {
        Ok(v) => v,
        Err(e) if e.code == ErrorCode::NotFound => {
            return Ok(Some(StopExecutionOutcome::Refused("task not found".into())));
        }
        Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
    };
    // Require the post-sync read to match the exact pre-sync revision so a newly appended close
    // after `sync()` started can never bypass durability (`Pass 10 #1`).
    if confirmed.task.revision == observed_revision
        && matches!(
            confirmed.task.status,
            TaskStatus::Cancelled | TaskStatus::Done
        )
    {
        return Ok(Some(StopExecutionOutcome::Closed("already_closed")));
    }
    Ok(None)
}

fn execute_or_reconcile_stop_intent(
    office: &Office,
    agg: &mut BudgetAggregate,
    intent: &mut PendingStopIntent,
) -> Result<StopExecutionOutcome, OfficeError> {
    for _ in 0..MAX_TASK_TRANSITION_RETRIES {
        if let Some(receipt_outcome) = lookup_task_transition_receipt(office, intent)? {
            match receipt_outcome {
                TaskReceiptOutcome::Final(outcome) => return Ok(outcome),
                TaskReceiptOutcome::RevisionConflict => {
                    rotate_stop_intent_command(office, agg, intent)?;
                    continue;
                }
            }
        }
        let inspected = match office.task_inspect(&intent.task_id)? {
            Ok(v) => v,
            Err(e) if e.code == ErrorCode::NotFound => {
                return Ok(StopExecutionOutcome::Refused("task not found".into()));
            }
            Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
        };
        if matches!(
            inspected.task.status,
            TaskStatus::Cancelled | TaskStatus::Done
        ) {
            if let Some(outcome) =
                confirm_terminal_task_state(office, &intent.task_id, inspected.task.revision)?
            {
                return Ok(outcome);
            }
            continue;
        }
        let transition_params = TasksTransitionParams {
            command_id: intent.task_command_id.clone(),
            task: intent.task_id.clone(),
            expected_revision: inspected.task.revision,
            status: TaskStatus::Cancelled,
            blocked_reason: None,
            resolution: Some(
                intent
                    .reason
                    .as_deref()
                    .unwrap_or("Stopped at budget limit")
                    .to_owned(),
            ),
            note: Some(
                intent
                    .reason
                    .as_deref()
                    .unwrap_or("Task stopped at budget limit.")
                    .to_owned(),
            ),
        };
        let fp = task_transition_fingerprint(&transition_params);
        if intent.task_command_fingerprint.as_deref() != Some(fp.as_str()) {
            // Persist the exact transition fingerprint BEFORE submitting `tasks.transition` so a
            // clock-rollback command ID collision with another task can never falsely finalize
            // this stop (`TASK-97` / Final Gate Finding #3).
            intent.task_command_fingerprint = Some(fp);
            agg.pending_stop = Some(intent.clone());
            if let Err(err) = save_budget_aggregate(office, agg)? {
                return Err(OfficeError::storage(std::io::Error::other(err.message)));
            }
        }
        match office.task_transition(&intent.actor_id, transition_params)? {
            Ok(_) => return Ok(StopExecutionOutcome::Closed("cancelled")),
            Err(e) if e.code == ErrorCode::Conflict => {
                rotate_stop_intent_command(office, agg, intent)?;
                continue;
            }
            Err(e)
                if matches!(
                    e.code,
                    ErrorCode::InvalidRequest | ErrorCode::NotAuthorized | ErrorCode::NotFound
                ) =>
            {
                // Flush & confirm the journaled domain refusal receipt before clearing
                // `pending_stop` (`TASK-97` / Pass 14 Finding #2).
                office
                    .store
                    .sync()
                    .map_err(|err| OfficeError::storage(std::io::Error::other(err.to_string())))?;
                if office.store.status().stopped.is_some() {
                    return Err(OfficeError::storage(std::io::Error::other(
                        "store stopped before confirming task refusal receipt",
                    )));
                }
                return Ok(StopExecutionOutcome::Refused(e.message));
            }
            // Ambiguous/storage errors (e.g., commit succeeded before snapshot read failed)
            // keep `pending_stop` intact so reconciliation resolves via the confirmed receipt
            // (`TASK-97` / Pass 13 Finding #1).
            Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
        }
    }
    Err(OfficeError::storage(std::io::Error::other(
        "budget stop task cancellation conflicted after retries",
    )))
}

enum TaskReceiptOutcome {
    Final(StopExecutionOutcome),
    RevisionConflict,
}

fn sha256_hex(digest: &Sha256) -> String {
    let mut hex = String::with_capacity(64);
    for b in digest.as_bytes() {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{b:02x}");
    }
    hex
}

/// Computes the exact `org-store` operation fingerprint produced by `task_ops::task_op`
/// for `"tasks.transition"` so stop recovery can reject command ID collisions from another task.
fn task_transition_fingerprint(params: &TasksTransitionParams) -> String {
    let fields = serde_json::to_vec(params).unwrap_or_default();
    sha256_hex(&Sha256::of_parts(&[b"tasks.transition", b"\0", &fields]))
}

fn lookup_task_transition_receipt(
    office: &Office,
    intent: &PendingStopIntent,
) -> Result<Option<TaskReceiptOutcome>, OfficeError> {
    let (Ok(op_id), Ok(actor_id)) = (
        org_store::Id::parse(&intent.task_command_id),
        org_store::Id::parse(&intent.actor_id),
    ) else {
        return Ok(None);
    };
    let (receipt, _) = office
        .store
        .read(crate::store::open::ReceiptById(op_id))
        .map_err(|e| OfficeError::storage(std::io::Error::other(e.to_string())))?;
    let Some(receipt) = receipt else {
        return Ok(None);
    };
    if receipt.kind != "tasks.transition" || receipt.actor != org_store::Actor::Employee(actor_id) {
        return Ok(Some(TaskReceiptOutcome::RevisionConflict));
    }
    let Some(expected_fp) = intent.task_command_fingerprint.as_deref() else {
        // An unfingerprinted intent has not yet submitted `tasks.transition`; any existing
        // receipt for `task_command_id` is a collision (e.g., clock rollback) and must rotate
        // to a fresh command ID rather than finalizing this stop (`Final Gate Pass 2 #2`).
        return Ok(Some(TaskReceiptOutcome::RevisionConflict));
    };
    if sha256_hex(&receipt.fingerprint) != expected_fp {
        return Ok(Some(TaskReceiptOutcome::RevisionConflict));
    }
    // Never finalize a budget stop from an unconfirmed projection receipt (`TASK-97` / Pass 8):
    // require both journal flush and host custody confirmation through `receipt.ordinal`.
    let mut status = office.store.status();
    if status.stopped.is_none()
        && (status.flushed < receipt.ordinal || status.confirmed < receipt.ordinal)
    {
        office
            .store
            .sync()
            .map_err(|e| OfficeError::storage(std::io::Error::other(e.to_string())))?;
        status = office.store.status();
    }
    if status.stopped.is_some()
        || status.flushed < receipt.ordinal
        || status.confirmed < receipt.ordinal
    {
        return Err(OfficeError::storage(std::io::Error::other(
            "task cancellation receipt is not yet flushed and confirmed",
        )));
    }
    match receipt.outcome {
        org_store::Outcome::Completed => {
            let reply_task_id = serde_json::from_slice::<serde_json::Value>(&receipt.reply)
                .ok()
                .and_then(|v| v.get("task_id")?.as_str().map(str::to_owned));
            if reply_task_id.as_deref() != Some(intent.task_id.as_str()) {
                return Ok(Some(TaskReceiptOutcome::RevisionConflict));
            }
            Ok(Some(TaskReceiptOutcome::Final(
                StopExecutionOutcome::Closed("cancelled"),
            )))
        }
        org_store::Outcome::Rejected => {
            let api_err = serde_json::from_slice::<ApiError>(&receipt.reply).unwrap_or_else(|_| {
                ApiError::new(ErrorCode::InvalidRequest, "task operation refused")
            });
            if matches!(
                api_err.code,
                ErrorCode::Conflict
                    | ErrorCode::StorageError
                    | ErrorCode::Unavailable
                    | ErrorCode::Timeout
            ) {
                Ok(Some(TaskReceiptOutcome::RevisionConflict))
            } else {
                Ok(Some(TaskReceiptOutcome::Final(
                    StopExecutionOutcome::Refused(api_err.message),
                )))
            }
        }
    }
}

fn invalid_op_id() -> ApiError {
    ApiError::new(
        ErrorCode::InvalidRequest,
        "operation_id must be a canonical lowercase UUIDv7",
    )
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
