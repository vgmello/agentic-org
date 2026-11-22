//! Wire contracts for `L3` task, agent (daily/monthly), and release budgets, 80/100 notices,
//! one-time 20% extensions, independent execution holds, raises, and own-agent budget CLI
//! (`TASK-53`, `doc-92`, `doc-51`, `FC-089..093`, `ASK-8`).

use serde::{Deserialize, Serialize};

use crate::costs::CompletenessStatus;
use crate::Call;

/// Target scope of a budget (`doc-92` §Scope and units — never project/team/org/feature).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BudgetScopeKind {
    Task,
    Agent,
    Release,
    #[serde(other)]
    Unknown,
}

impl BudgetScopeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Agent => "agent",
            Self::Release => "release",
            Self::Unknown => "task",
        }
    }
}

/// Period kind for a budget (`doc-92`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BudgetPeriodKind {
    Lifetime,
    Day,
    Month,
    #[serde(other)]
    Unknown,
}

impl BudgetPeriodKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lifetime => "lifetime",
            Self::Day => "day",
            Self::Month => "month",
            Self::Unknown => "lifetime",
        }
    }
}

/// Unit of a budget: integer micro-USD or integer total tokens, never both (`doc-92`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BudgetUnit {
    MicroUsd,
    Tokens,
    #[serde(other)]
    Unknown,
}

impl BudgetUnit {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MicroUsd => "micro_usd",
            Self::Tokens => "tokens",
            Self::Unknown => "micro_usd",
        }
    }
}

/// Active vs cleared state of a budget policy revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BudgetPolicyState {
    Active,
    Cleared,
    #[serde(other)]
    Unknown,
}

impl BudgetPolicyState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Cleared => "cleared",
            Self::Unknown => "cleared",
        }
    }
}

/// Provenance of a budget's IANA time zone (`doc-92`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BudgetTimeZoneSource {
    OwnerPerson,
    Hq,
    #[serde(other)]
    Unknown,
}

impl BudgetTimeZoneSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OwnerPerson => "owner_person",
            Self::Hq => "hq",
            Self::Unknown => "hq",
        }
    }
}

/// Lifecycle state of an execution hold (`doc-92`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BudgetHoldState {
    StopPending,
    Held,
    Released,
    #[serde(other)]
    Unknown,
}

impl BudgetHoldState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StopPending => "stop_pending",
            Self::Held => "held",
            Self::Released => "released",
            Self::Unknown => "held",
        }
    }
}

/// Threshold stage (`base` or one-time `extended`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BudgetStage {
    Base,
    Extended,
    #[serde(other)]
    Unknown,
}

impl BudgetStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Extended => "extended",
            Self::Unknown => "base",
        }
    }
}

/// Non-monetary budget state visible to authorized members without amount visibility (`doc-51` §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BudgetStateSummary {
    None,
    Ok,
    Warning80,
    StopPending,
    Held,
    ReachedCap,
    #[serde(other)]
    Unknown,
}

/// Full budget view (amounts present only when caller is authorized per `doc-51` & `doc-92`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetView {
    pub id: String,
    pub scope_kind: BudgetScopeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_id: Option<String>,
    pub period_kind: BudgetPeriodKind,
    pub current_revision: u64,
    pub state: BudgetPolicyState,
    pub time_zone: String,
    pub time_zone_source: BudgetTimeZoneSource,
    pub state_summary: BudgetStateSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_state: Option<BudgetHoldState>,
    pub amounts_visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<BudgetUnit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_amount: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_amount: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_amount: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_status: Option<CompletenessStatus>,
    #[serde(default)]
    pub includes_estimates: bool,
    #[serde(default)]
    pub includes_api_equivalent: bool,
    #[serde(default)]
    pub unpriced_records: u64,
    #[serde(default)]
    pub incomplete_records: u64,
    #[serde(default)]
    pub extension_used: bool,
    #[serde(default)]
    pub can_extend_once: bool,
    #[serde(default)]
    pub kept_paused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger_checkpoint_ref: Option<String>,
}

/// `budget.set`: set, raise, or lower a task, agent (day/month), or release budget.
pub struct BudgetSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetSetParams {
    pub operation_id: String,
    pub scope_kind: BudgetScopeKind,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub release_id: Option<String>,
    pub period_kind: BudgetPeriodKind,
    pub unit: BudgetUnit,
    pub limit_amount: u64,
    #[serde(default)]
    pub time_zone: Option<String>,
    #[serde(default)]
    pub time_zone_source: Option<BudgetTimeZoneSource>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetMutationResult {
    pub budget: BudgetView,
    pub replayed: bool,
}

impl Call for BudgetSet {
    const METHOD: &'static str = "budget.set";
    type Params = BudgetSetParams;
    type Result = BudgetMutationResult;
}

/// `budget.clear`: explicitly clear a budget via a new policy revision (`state = cleared`).
pub struct BudgetClear;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetClearParams {
    pub operation_id: String,
    pub budget_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

impl Call for BudgetClear {
    const METHOD: &'static str = "budget.clear";
    type Params = BudgetClearParams;
    type Result = BudgetMutationResult;
}

/// `budget.extend_once`: apply the one-time +20% (`L + floor(L/5)`) extension at base reach.
pub struct BudgetExtendOnce;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetExtendOnceParams {
    pub operation_id: String,
    pub budget_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

impl Call for BudgetExtendOnce {
    const METHOD: &'static str = "budget.extend_once";
    type Params = BudgetExtendOnceParams;
    type Result = BudgetMutationResult;
}

/// `budget.stop`: stop a reached task budget (cancelling/closing the task) or keep an agent paused.
pub struct BudgetStop;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetStopParams {
    pub operation_id: String,
    pub budget_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

impl Call for BudgetStop {
    const METHOD: &'static str = "budget.stop";
    type Params = BudgetStopParams;
    type Result = BudgetMutationResult;
}

/// `budget.keep_paused`: keep a reached agent budget paused until period reset (`doc-92`).
pub struct BudgetKeepPaused;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetKeepPausedParams {
    pub operation_id: String,
    pub budget_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

impl Call for BudgetKeepPaused {
    const METHOD: &'static str = "budget.keep_paused";
    type Params = BudgetKeepPausedParams;
    type Result = BudgetMutationResult;
}

/// `budget.get`: inspect one budget by ID or by target scope.
pub struct BudgetGet;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetGetParams {
    #[serde(default)]
    pub budget_id: Option<String>,
    #[serde(default)]
    pub scope_kind: Option<BudgetScopeKind>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub release_id: Option<String>,
    #[serde(default)]
    pub period_kind: Option<BudgetPeriodKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetGetResult {
    pub budget: Option<BudgetView>,
}

impl Call for BudgetGet {
    const METHOD: &'static str = "budget.get";
    type Params = BudgetGetParams;
    type Result = BudgetGetResult;
}

/// `budget.list`: list budgets visible to the caller (filtered by scope/target).
pub struct BudgetList;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetListParams {
    #[serde(default)]
    pub scope_kind: Option<BudgetScopeKind>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub release_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetListResult {
    pub budgets: Vec<BudgetView>,
}

impl Call for BudgetList {
    const METHOD: &'static str = "budget.list";
    type Params = BudgetListParams;
    type Result = BudgetListResult;
}

/// `budget.agent_own`: authenticated agent own-budget CLI exception (`doc-92`, `doc-51` §1).
/// Returns ONLY the bound agent's own daily/monthly budgets and optional current task budget.
pub struct BudgetAgentOwn;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetAgentOwnParams {
    #[serde(default)]
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetAgentOwnResult {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily: Option<BudgetView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monthly: Option<BudgetView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<BudgetView>,
}

impl Call for BudgetAgentOwn {
    const METHOD: &'static str = "budget.agent_own";
    type Params = BudgetAgentOwnParams;
    type Result = BudgetAgentOwnResult;
}

/// Exact integer 80% threshold check without float rounding (`doc-92`: `used * 5 >= limit * 4`).
pub fn reached_80_percent(used: u64, limit: u64) -> bool {
    limit > 0 && u128::from(used).saturating_mul(5) >= u128::from(limit).saturating_mul(4)
}

/// Exact integer +20% one-time extension (`L + floor(L/5)`) (`doc-92`).
pub fn extended_limit(base_limit: u64) -> Option<u64> {
    let inc = base_limit / 5;
    if inc == 0 {
        return None;
    }
    base_limit.checked_add(inc)
}
