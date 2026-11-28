//! Office facade for `L3` task, agent (daily/monthly), and release budgets, 80/100 notices,
//! one-time +20% extensions, independent execution holds, and own-agent budget status
//! (`TASK-53`, `doc-92`, `doc-51`, `FC-089..093`, `ASK-8`).

use org_proto::budgets::{
    BudgetAgentOwnParams, BudgetAgentOwnResult, BudgetClearParams, BudgetExtendOnceParams,
    BudgetGetParams, BudgetGetResult, BudgetKeepPausedParams, BudgetListParams, BudgetListResult,
    BudgetMutationResult, BudgetSetParams, BudgetStopParams, BudgetTimeZoneSource,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Sha256;

use crate::budgets::{
    check_operation_replay, mark_open_periods_kept_paused, resolve_reached_notices,
    ActiveBudgetHoldInfo, BudgetAggregate, BudgetEventRecord, OpenBudgetInboxNotice,
};
use crate::error::OfficeError;
use crate::organization::records::EmployeeKind;

use super::Office;

mod authority;
mod controls;
mod evaluation;
mod queries;
mod receipts;
mod reconcile;
mod set;
mod stop;

use controls::{op_budget_clear, op_budget_extend_once, op_budget_keep_paused};
pub(super) use evaluation::{op_active_holds, op_is_task_reached, op_reevaluate_for_record};
use evaluation::{op_mark_holds_stopped, op_open_inbox_notices};
use queries::{op_budget_agent_own, op_budget_get, op_budget_list};
use set::op_budget_set;
use stop::op_budget_stop;

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

    fn require_hq_for_budget_write(&self) -> Result<(), ApiError> {
        if self.info.role != crate::model::OfficeRole::Hq {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "budget policies are HQ-authoritative; modify budgets on HQ",
            ));
        }
        Ok(())
    }

    /// `budget.set`: sets, raises, or lowers a task, agent (day/month), or release budget.
    pub fn budget_set(
        &self,
        actor: &str,
        params: BudgetSetParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        if let Err(err) = self.require_hq_for_budget_write() {
            return Ok(Err(err));
        }
        let _guard = self.lock_budget_ops();
        op_budget_set(self, actor, params)
    }

    /// `budget.clear`: clears an active budget via a new immutable `cleared` policy revision.
    pub fn budget_clear(
        &self,
        actor: &str,
        params: BudgetClearParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        if let Err(err) = self.require_hq_for_budget_write() {
            return Ok(Err(err));
        }
        let _guard = self.lock_budget_ops();
        op_budget_clear(self, actor, params)
    }

    /// `budget.extend_once`: applies the one-time +20% (`L + floor(L/5)`) extension (`doc-92`).
    pub fn budget_extend_once(
        &self,
        actor: &str,
        params: BudgetExtendOnceParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        if let Err(err) = self.require_hq_for_budget_write() {
            return Ok(Err(err));
        }
        let _guard = self.lock_budget_ops();
        op_budget_extend_once(self, actor, params)
    }

    /// `budget.stop`: stops a reached task budget (cancelling the task) or keeps an agent paused.
    pub fn budget_stop(
        &self,
        actor: &str,
        params: BudgetStopParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        if let Err(err) = self.require_hq_for_budget_write() {
            return Ok(Err(err));
        }
        let _guard = self.lock_budget_ops();
        op_budget_stop(self, actor, params)
    }

    /// `budget.keep_paused`: owner keeps a reached agent budget paused until calendar reset.
    pub fn budget_keep_paused(
        &self,
        actor: &str,
        params: BudgetKeepPausedParams,
    ) -> Result<Result<BudgetMutationResult, ApiError>, OfficeError> {
        if let Err(err) = self.require_hq_for_budget_write() {
            return Ok(Err(err));
        }
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

enum StopExecutionOutcome {
    Closed(&'static str),
    Refused(String),
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

fn save_budget_aggregate(
    office: &Office,
    agg: &BudgetAggregate,
) -> Result<Result<(), ApiError>, OfficeError> {
    let emp_kinds = collect_employee_kinds(office, agg)?;
    Ok(office.budgets.save_aggregate(agg, &emp_kinds)?)
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
