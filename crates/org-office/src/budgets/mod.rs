//! Consumer-owned port and domain contract for `L3` task, agent (daily/monthly), and release
//! budgets, 80/100 notices, one-time 20% extensions, and independent execution holds
//! (`TASK-53`, `doc-92`, `doc-51`, `FC-089..093`, `ASK-8`).
//!
//! Budget data is stored durably under `<durable>/budgets/<budget-id>/` (never in shared org Git)
//! and projected into isolated 0700/0600 `<runtime>/<office-id>/protected/budgets.db`
//! (`budgets-schema.sql`), never plaintext `office.db`.

pub mod eval;
pub mod memory;

use org_proto::budgets::{
    BudgetHoldState, BudgetPeriodKind, BudgetPolicyState, BudgetScopeKind, BudgetStage,
    BudgetStateSummary, BudgetTimeZoneSource, BudgetUnit, BudgetView,
};
use org_proto::costs::CompletenessStatus;
use org_proto::ApiError;
use serde::{Deserialize, Serialize};

use crate::error::OfficeError;

/// Canonical `budgets` target metadata + `current_revision` pointer (`doc-92`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetRecord {
    pub organization_id: String,
    pub id: String,
    pub current_revision: u64,
    pub scope_kind: BudgetScopeKind,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
    pub release_id: Option<String>,
    pub period_kind: BudgetPeriodKind,
    pub time_zone: String,
    pub time_zone_source: BudgetTimeZoneSource,
    pub time_zone_subject_ref: Option<String>,
    pub created_at: String,
}

/// Immutable append-only `budget_policy_versions` row (`doc-92`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetPolicyVersion {
    pub organization_id: String,
    pub budget_id: String,
    pub revision: u64,
    pub state: BudgetPolicyState,
    pub unit: BudgetUnit,
    pub limit_amount: Option<u64>,
    pub time_zone: String,
    pub time_zone_source: BudgetTimeZoneSource,
    pub time_zone_subject_ref: Option<String>,
    pub changed_by_id: String,
    pub changed_at: String,
    pub reason: Option<String>,
}

/// One `budget_periods` row (`lifetime` for task/release; `YYYY-MM-DD`/`YYYY-MM` for agent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetPeriodRecord {
    pub organization_id: String,
    pub budget_id: String,
    pub id: String,
    pub policy_revision: u64,
    pub period_key: String,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    pub used_amount: u64,
    pub usage_status: CompletenessStatus,
    pub includes_estimates: bool,
    pub includes_api_equivalent: bool,
    pub unpriced_records: u64,
    pub incomplete_records: u64,
    pub extension_used: bool,
    pub kept_paused: bool,
    pub state: String,
    pub evaluated_at: Option<String>,
    pub ledger_checkpoint_ref: Option<String>,
}

/// Immutable `budget_events` audit entry (`doc-92`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetEventRecord {
    pub organization_id: String,
    pub id: String,
    pub budget_id: String,
    pub period_id: String,
    pub operation_id: String,
    pub kind: String,
    pub policy_revision: u64,
    pub actor_kind: String,
    pub actor_id: Option<String>,
    pub occurred_at: String,
    pub payload_json: String,
}

/// Deduplicated `budget_notices` row (`doc-92`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetNoticeRecord {
    pub organization_id: String,
    pub id: String,
    pub budget_id: String,
    pub period_id: String,
    pub policy_revision: u64,
    pub stage: BudgetStage,
    pub threshold: u8,
    pub recipient_kind: String,
    pub recipient_id: Option<String>,
    pub kind: String,
    pub delivery_state: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

/// Independent `budget_holds` row (`stop_pending`, `held`, `released`; `doc-92`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetHoldRecord {
    pub organization_id: String,
    pub budget_id: String,
    pub period_id: String,
    pub policy_revision: u64,
    pub stage: BudgetStage,
    pub state: BudgetHoldState,
    pub created_at: String,
    pub updated_at: String,
    pub release_reason: Option<String>,
}

/// Crash-recovery intent persisted before cancelling a task for `budget.stop` (`TASK-97`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingStopIntent {
    pub operation_id: String,
    pub actor_id: String,
    pub task_id: String,
    pub task_command_id: String,
    /// Hex SHA-256 of the exact `tasks.transition` operation fingerprint submitted for
    /// `task_command_id`, preventing false matches on clock-rollback command ID reuse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_command_fingerprint: Option<String>,
    pub request_hash: String,
    pub reason: Option<String>,
    pub created_at: String,
}

/// Complete durable bundle for one budget identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetAggregate {
    pub budget: BudgetRecord,
    pub versions: Vec<BudgetPolicyVersion>,
    pub periods: Vec<BudgetPeriodRecord>,
    pub events: Vec<BudgetEventRecord>,
    pub notices: Vec<BudgetNoticeRecord>,
    pub holds: Vec<BudgetHoldRecord>,
    /// In-flight task cancellation intent persisted before `task_transition` and cleared once
    /// the task transition outcome is finalized into `budget_events` (`TASK-97`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_stop: Option<PendingStopIntent>,
}

impl BudgetAggregate {
    pub fn current_policy(&self) -> Option<&BudgetPolicyVersion> {
        self.versions
            .iter()
            .find(|v| v.revision == self.budget.current_revision)
    }

    pub fn open_period(&self) -> Option<&BudgetPeriodRecord> {
        self.periods.iter().find(|p| p.state == "open")
    }

    pub fn active_hold(&self) -> Option<&BudgetHoldRecord> {
        self.holds
            .iter()
            .rev()
            .find(|h| h.state != BudgetHoldState::Released)
    }

    pub fn has_operation(&self, operation_id: &str) -> bool {
        self.events.iter().any(|e| e.operation_id == operation_id)
    }

    /// Renders a [`BudgetView`] enforcing amount visibility (`amounts_visible == false` strips all
    /// limits, usage, remaining amounts, and counts per `doc-51` §1 & `doc-92`).
    pub fn to_view(&self, amounts_visible: bool) -> BudgetView {
        let policy = self.current_policy();
        let period = self.open_period();
        let hold = self.active_hold();
        let policy_state = policy.map_or(BudgetPolicyState::Cleared, |p| p.state);
        let limit_amount = policy.and_then(|p| p.limit_amount);
        let extension_used = period.is_some_and(|p| p.extension_used);
        let effective_limit = match (policy_state, limit_amount, extension_used) {
            (BudgetPolicyState::Active, Some(base), true) => {
                org_proto::budgets::extended_limit(base).or(Some(base))
            }
            (BudgetPolicyState::Active, Some(base), false) => Some(base),
            _ => None,
        };
        let used = period.map(|p| p.used_amount);
        let state_summary = compute_state_summary(
            self.budget.scope_kind,
            policy_state,
            used,
            effective_limit,
            hold.map(|h| h.state),
        );
        let can_extend_once = policy_state == BudgetPolicyState::Active
            && self.budget.scope_kind != BudgetScopeKind::Release
            && !extension_used
            && limit_amount.is_some_and(|l| {
                used.is_some_and(|u| u >= l)
                    && org_proto::budgets::extended_limit(l)
                        .is_some_and(|ext| ext > used.unwrap_or(0))
            });
        let remaining = match (effective_limit, used) {
            (Some(lim), Some(u)) => Some(lim.saturating_sub(u)),
            _ => None,
        };
        BudgetView {
            id: self.budget.id.clone(),
            scope_kind: self.budget.scope_kind,
            task_id: self.budget.task_id.clone(),
            agent_id: self.budget.agent_id.clone(),
            release_id: self.budget.release_id.clone(),
            period_kind: self.budget.period_kind,
            current_revision: self.budget.current_revision,
            state: policy_state,
            time_zone: self.budget.time_zone.clone(),
            time_zone_source: self.budget.time_zone_source,
            state_summary,
            hold_state: hold.map(|h| h.state),
            amounts_visible,
            unit: amounts_visible.then(|| policy.map(|p| p.unit)).flatten(),
            limit_amount: if amounts_visible { limit_amount } else { None },
            effective_limit: if amounts_visible {
                effective_limit
            } else {
                None
            },
            used_amount: if amounts_visible { used } else { None },
            remaining_amount: if amounts_visible { remaining } else { None },
            period_id: period.map(|p| p.id.clone()),
            period_key: period.map(|p| p.period_key.clone()),
            starts_at: period.and_then(|p| p.starts_at.clone()),
            ends_at: period.and_then(|p| p.ends_at.clone()),
            usage_status: period.map(|p| p.usage_status),
            includes_estimates: period.is_some_and(|p| p.includes_estimates),
            includes_api_equivalent: period.is_some_and(|p| p.includes_api_equivalent),
            unpriced_records: if amounts_visible {
                period.map_or(0, |p| p.unpriced_records)
            } else {
                0
            },
            incomplete_records: if amounts_visible {
                period.map_or(0, |p| p.incomplete_records)
            } else {
                0
            },
            extension_used,
            can_extend_once,
            kept_paused: period.is_some_and(|p| p.kept_paused),
            ledger_checkpoint_ref: period.and_then(|p| p.ledger_checkpoint_ref.clone()),
        }
    }
}

fn compute_state_summary(
    scope: BudgetScopeKind,
    policy_state: BudgetPolicyState,
    used: Option<u64>,
    effective_limit: Option<u64>,
    hold_state: Option<BudgetHoldState>,
) -> BudgetStateSummary {
    if policy_state == BudgetPolicyState::Cleared {
        return BudgetStateSummary::None;
    }
    if let Some(h) = hold_state {
        return match h {
            BudgetHoldState::StopPending => BudgetStateSummary::StopPending,
            BudgetHoldState::Held => BudgetStateSummary::Held,
            _ => BudgetStateSummary::Ok,
        };
    }
    let (Some(u), Some(lim)) = (used, effective_limit) else {
        return BudgetStateSummary::Ok;
    };
    if u >= lim {
        if scope == BudgetScopeKind::Release {
            BudgetStateSummary::ReachedCap
        } else {
            BudgetStateSummary::Held
        }
    } else if org_proto::budgets::reached_80_percent(u, lim) {
        BudgetStateSummary::Warning80
    } else {
        BudgetStateSummary::Ok
    }
}

/// Active hold summary returned for admission checks (`ASK-8`, `LIVE-3`, `doc-92`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveBudgetHoldInfo {
    pub budget_id: String,
    pub scope_kind: BudgetScopeKind,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
    pub period_kind: BudgetPeriodKind,
    pub state: BudgetHoldState,
    pub stage: BudgetStage,
}

/// Unresolved owner `budget_reached` inbox item projected for `inbox.list` & `inbox.changed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenBudgetInboxNotice {
    pub notice: BudgetNoticeRecord,
    pub budget: BudgetRecord,
    pub period: BudgetPeriodRecord,
    pub policy: BudgetPolicyVersion,
    pub hold_state: Option<BudgetHoldState>,
}

/// Consumer-owned storage port for isolated `protected/budgets.db` + durable `durable/budgets/`.
pub trait BudgetStore: Send + Sync {
    fn save_aggregate(
        &self,
        agg: &BudgetAggregate,
        employee_kinds: &[(String, &'static str)],
    ) -> Result<Result<(), ApiError>, OfficeError>;

    fn get_by_id(
        &self,
        org_id: &str,
        budget_id: &str,
    ) -> Result<Option<BudgetAggregate>, OfficeError>;

    fn find_by_target(
        &self,
        org_id: &str,
        scope_kind: BudgetScopeKind,
        task_id: Option<&str>,
        agent_id: Option<&str>,
        release_id: Option<&str>,
        period_kind: BudgetPeriodKind,
    ) -> Result<Option<BudgetAggregate>, OfficeError>;

    fn list_all(&self, org_id: &str) -> Result<Vec<BudgetAggregate>, OfficeError>;
}

/// Fail-closed read-only wrapper for satellite offices (`TASK-52` M-20): budget policy mutations
/// are HQ-authoritative and must never be persisted on a satellite.
pub struct SatelliteReadOnlyBudgetStore<S> {
    inner: S,
}

impl<S> SatelliteReadOnlyBudgetStore<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: BudgetStore> BudgetStore for SatelliteReadOnlyBudgetStore<S> {
    fn save_aggregate(
        &self,
        _agg: &BudgetAggregate,
        _employee_kinds: &[(String, &'static str)],
    ) -> Result<Result<(), ApiError>, OfficeError> {
        Ok(Err(ApiError::new(
            org_proto::ErrorCode::NotAuthorized,
            "budget policies are HQ-authoritative; modify budgets on HQ",
        )))
    }

    fn get_by_id(
        &self,
        org_id: &str,
        budget_id: &str,
    ) -> Result<Option<BudgetAggregate>, OfficeError> {
        self.inner.get_by_id(org_id, budget_id)
    }

    fn find_by_target(
        &self,
        org_id: &str,
        scope_kind: BudgetScopeKind,
        task_id: Option<&str>,
        agent_id: Option<&str>,
        release_id: Option<&str>,
        period_kind: BudgetPeriodKind,
    ) -> Result<Option<BudgetAggregate>, OfficeError> {
        self.inner.find_by_target(
            org_id,
            scope_kind,
            task_id,
            agent_id,
            release_id,
            period_kind,
        )
    }

    fn list_all(&self, org_id: &str) -> Result<Vec<BudgetAggregate>, OfficeError> {
        self.inner.list_all(org_id)
    }
}
