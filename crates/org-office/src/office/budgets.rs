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

use crate::budgets::eval::{apply_extend_once, evaluate_budget, EvalOutcome};
use crate::budgets::{
    ActiveBudgetHoldInfo, BudgetAggregate, BudgetEventRecord, BudgetPolicyVersion, BudgetRecord,
    OpenBudgetInboxNotice,
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
    /// `budget.set`: sets, raises, or lowers a task, agent (day/month), or release budget.
    pub fn budget_set(
        &self,
        actor: &str,
        params: BudgetSetParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        op_budget_set(self, actor, params)
    }

    /// `budget.clear`: clears an active budget via a new immutable `cleared` policy revision.
    pub fn budget_clear(
        &self,
        actor: &str,
        params: BudgetClearParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        op_budget_clear(self, actor, params)
    }

    /// `budget.extend_once`: applies the one-time +20% (`L + floor(L/5)`) extension (`doc-92`).
    pub fn budget_extend_once(
        &self,
        actor: &str,
        params: BudgetExtendOnceParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        op_budget_extend_once(self, actor, params)
    }

    /// `budget.stop`: stops a reached task budget (cancelling the task) or keeps an agent paused.
    pub fn budget_stop(
        &self,
        actor: &str,
        params: BudgetStopParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        op_budget_stop(self, actor, params)
    }

    /// `budget.keep_paused`: owner keeps a reached agent budget paused until calendar reset.
    pub fn budget_keep_paused(
        &self,
        actor: &str,
        params: BudgetKeepPausedParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        op_budget_keep_paused(self, actor, params)
    }

    /// `budget.get`: fetches one budget with role-appropriate amount visibility (`doc-51` §1).
    pub fn budget_get(
        &self,
        actor: &str,
        params: BudgetGetParams,
    ) -> Result<Result<BudgetGetResult, ApiError>, OfficeError> {
        op_budget_get(self, actor, params)
    }

    /// `budget.list`: lists budgets visible to the caller, filtering out unauthorized agent/release
    /// budgets and stripping amounts for non-PM task viewers (`doc-51` §1).
    pub fn budget_list(
        &self,
        actor: &str,
        params: BudgetListParams,
    ) -> Result<Result<BudgetListResult, ApiError>, OfficeError> {
        op_budget_list(self, actor, params)
    }

    /// `budget.agent_own`: own-agent CLI exception (`agent-os budget status`, `doc-92`, `doc-51`).
    pub fn budget_agent_own(
        &self,
        actor: &str,
        params: BudgetAgentOwnParams,
    ) -> Result<Result<BudgetAgentOwnResult, ApiError>, OfficeError> {
        op_budget_agent_own(self, actor, params)
    }

    /// Re-evaluates all budgets potentially affected by `rec` (`doc-92`).
    pub(crate) fn reevaluate_budgets_for_record(
        &self,
        rec: &UsageRecordView,
    ) -> Result<(), OfficeError> {
        op_reevaluate_for_record(self, rec)
    }

    /// Checks whether `task_id` currently has a reached active task budget (`doc-92`).
    pub(crate) fn is_task_budget_reached(&self, task_id: &str) -> Result<bool, OfficeError> {
        op_is_task_reached(self, task_id)
    }

    /// Returns any unreleased holds affecting `agent_id` or `task_id`, catching calendar resets first.
    pub fn active_budget_holds(
        &self,
        agent_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<Vec<ActiveBudgetHoldInfo>, OfficeError> {
        op_active_holds(self, agent_id, task_id)
    }

    /// Transitions any `stop_pending` holds for `agent_id` (and tasks assigned to it) to `held`.
    pub fn mark_budget_holds_stopped(&self, agent_id: &str) -> Result<bool, OfficeError> {
        op_mark_holds_stopped(self, agent_id)
    }

    /// Returns all unresolved owner `budget_reached` notices for `inbox.list` & `inbox.changed`.
    pub fn open_budget_inbox_notices(&self) -> Result<Vec<OpenBudgetInboxNotice>, OfficeError> {
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
    let existing = office.budgets.find_by_target(
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
                return Ok(Ok(BudgetMutationResult {
                    budget: agg.to_view(true),
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
    let outcome = match evaluate_in_memory(office, &mut agg, now_ms, &now_utc)? {
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
    // Single atomic durable + SQLite save (`Pass 1 Med #14`).
    save_budget_aggregate(office, &agg)?;
    if let Some(tid) = outcome.newly_reached_task_id {
        block_task_for_budget(office, &tid)?;
    }
    Ok(Ok(BudgetMutationResult {
        budget: agg.to_view(true),
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
            return Ok(Ok(BudgetMutationResult {
                budget: agg.to_view(true),
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
    save_budget_aggregate(office, &agg)?;
    Ok(Ok(BudgetMutationResult {
        budget: agg.to_view(true),
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
            return Ok(Ok(BudgetMutationResult {
                budget: agg.to_view(true),
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
    let mut next_id = || office.next_uuid_v7();
    if let Err(e) = apply_extend_once(
        &mut agg,
        &params.operation_id,
        caller.id.as_str(),
        &req_hash,
        params.reason,
        &now_utc,
        &mut next_id,
    ) {
        return Ok(Err(e));
    }
    save_budget_aggregate(office, &agg)?;
    Ok(Ok(BudgetMutationResult {
        budget: agg.to_view(true),
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
            return Ok(Ok(BudgetMutationResult {
                budget: agg.to_view(true),
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
    let outcome = execute_stop_action(
        office,
        &mut agg,
        caller.id.as_str(),
        params.reason.as_deref(),
    )?;
    // Resolve owner Inbox notice ONLY for stopped tasks, NOT agent keep-paused (`Pass 1 Med #11`).
    if agg.budget.scope_kind == BudgetScopeKind::Task {
        resolve_reached_notices(&mut agg, &now_utc);
    }
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
            "outcome": outcome,
        })
        .to_string(),
    );
    save_budget_aggregate(office, &agg)?;
    Ok(Ok(BudgetMutationResult {
        budget: agg.to_view(true),
        replayed: false,
    }))
}

fn execute_stop_action(
    office: &Office,
    agg: &mut BudgetAggregate,
    actor: &str,
    reason: Option<&str>,
) -> Result<String, OfficeError> {
    if agg.budget.scope_kind == BudgetScopeKind::Task {
        return match agg.budget.task_id.as_deref() {
            Some(tid) => cancel_task_for_budget(office, actor, tid, reason),
            None => Ok("missing_task".into()),
        };
    }
    mark_open_periods_kept_paused(agg);
    Ok("kept_paused".into())
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
                budget: agg.to_view(true),
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
    save_budget_aggregate(office, &agg)?;
    Ok(Ok(BudgetMutationResult {
        budget: agg.to_view(true),
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
    Ok(Ok(Some(agg.to_view(true))))
}

fn op_reevaluate_for_record(office: &Office, rec: &UsageRecordView) -> Result<(), OfficeError> {
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    let mut newly_reached_tasks = Vec::new();
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
        let out = evaluate_and_save_budget(office, &mut agg, now_ms, &now_utc)?;
        if let Some(tid) = out.newly_reached_task_id {
            newly_reached_tasks.push(tid);
        }
    }
    for tid in newly_reached_tasks {
        block_task_for_budget(office, &tid)?;
    }
    Ok(())
}

fn op_is_task_reached(office: &Office, task_id: &str) -> Result<bool, OfficeError> {
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

fn op_active_holds(
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
            save_budget_aggregate(office, &agg)?;
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
    Ok(Some(agg.to_view(true)))
}

fn evaluate_in_memory(
    office: &Office,
    agg: &mut BudgetAggregate,
    now_ms: u64,
    now_utc: &str,
) -> Result<Result<EvalOutcome, ApiError>, OfficeError> {
    let effective = office.accounting.effective_records(&office.info.org_id)?;
    let coverage = office.accounting.coverage_summary(&office.info.org_id)?;
    let mut next_id = || office.next_uuid_v7();
    Ok(evaluate_budget(
        agg,
        &effective,
        &coverage,
        now_ms,
        now_utc,
        &mut next_id,
    ))
}

fn evaluate_and_save_budget(
    office: &Office,
    agg: &mut BudgetAggregate,
    now_ms: u64,
    now_utc: &str,
) -> Result<EvalOutcome, OfficeError> {
    let outcome = evaluate_in_memory(office, agg, now_ms, now_utc)?
        .map_err(|e| OfficeError::storage(std::io::Error::other(e.message)))?;
    if outcome.changed {
        save_budget_aggregate(office, agg)?;
    }
    Ok(outcome)
}

fn save_budget_aggregate(office: &Office, agg: &BudgetAggregate) -> Result<(), OfficeError> {
    let emp_kinds = collect_employee_kinds(office, agg)?;
    office
        .budgets
        .save_aggregate(agg, &emp_kinds)?
        .map_err(|e| OfficeError::storage(std::io::Error::other(e.message)))
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

fn budget_view_for_caller(
    office: &Office,
    caller: &Employee,
    agg: &BudgetAggregate,
) -> Result<Option<BudgetView>, OfficeError> {
    if caller.id.as_str() == office.owner.as_str() {
        return Ok(Some(agg.to_view(true)));
    }
    match agg.budget.scope_kind {
        // Release & Agent budgets fail closed to owner-only until release ownership exists (`Pass 1 High #4`).
        BudgetScopeKind::Agent | BudgetScopeKind::Release => Ok(None),
        BudgetScopeKind::Task => {
            let is_pm = match agg.budget.task_id.as_deref() {
                Some(tid) => is_pm_of_task(office, caller, tid)?,
                None => false,
            };
            Ok(Some(agg.to_view(is_pm)))
        }
        _ => Ok(None),
    }
}

fn block_task_for_budget(office: &Office, task_id: &str) -> Result<(), OfficeError> {
    let Ok(inspected) = office.task_inspect(task_id)? else {
        return Ok(());
    };
    if inspected.task.status != TaskStatus::InProgress {
        return Ok(());
    }
    let _ = office.task_transition(
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
    )?;
    Ok(())
}

fn cancel_task_for_budget(
    office: &Office,
    actor: &str,
    task_id: &str,
    reason: Option<&str>,
) -> Result<String, OfficeError> {
    let Ok(inspected) = office.task_inspect(task_id)? else {
        return Ok("task_not_found".into());
    };
    if matches!(
        inspected.task.status,
        TaskStatus::Cancelled | TaskStatus::Done
    ) {
        return Ok("already_closed".into());
    }
    match office.task_transition(
        actor,
        TasksTransitionParams {
            command_id: office.next_uuid_v7(),
            task: task_id.to_owned(),
            expected_revision: inspected.task.revision,
            status: TaskStatus::Cancelled,
            blocked_reason: None,
            resolution: Some(reason.unwrap_or("Stopped at budget limit").to_owned()),
            note: Some(reason.unwrap_or("Task stopped at budget limit.").to_owned()),
        },
    )? {
        Ok(_) => Ok("cancelled".into()),
        Err(e) => Ok(format!("transition_refused:{}", e.message)),
    }
}

fn invalid_op_id() -> ApiError {
    ApiError::new(
        ErrorCode::InvalidRequest,
        "operation_id must be a canonical lowercase UUIDv7",
    )
}

fn validate_set_params(params: &BudgetSetParams) -> Result<(), ApiError> {
    if !org_proto::is_uuid_v7(&params.operation_id) {
        return Err(invalid_op_id());
    }
    if params.limit_amount == 0 {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "limit_amount must be greater than zero",
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
