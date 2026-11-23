//! Wire contracts for `L2` immutable usage & cost ledger, versioned USD model pricing,
//! historical attribution corrections, timezone-aware cost queries, and CSV export
//! (`TASK-53`, `doc-87`, `doc-51`, `FC-084..088`).

use serde::{Deserialize, Serialize};

use crate::Call;

/// Completeness status of token counts or cost valuation (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CompletenessStatus {
    Complete,
    Partial,
    Unavailable,
    #[serde(other)]
    Unknown,
}

impl CompletenessStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unavailable",
        }
    }
}

/// Basis for a usage record's USD cost valuation (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UsageCostBasis {
    RuntimeEstimate,
    TokenEstimate,
    Unpriced,
    #[serde(other)]
    Unknown,
}

impl UsageCostBasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeEstimate => "runtime_estimate",
            Self::TokenEstimate => "token_estimate",
            Self::Unpriced => "unpriced",
            Self::Unknown => "unknown",
        }
    }
}

/// Consuming actor kind for a usage record (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UsageActorKind {
    Agent,
    OneOff,
    #[serde(other)]
    Unknown,
}

impl UsageActorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::OneOff => "one_off",
            Self::Unknown => "one_off",
        }
    }
}

/// Historical work scope of a usage record (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UsageScopeKind {
    Task,
    Project,
    Team,
    Organization,
    #[serde(other)]
    Unknown,
}

impl UsageScopeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Project => "project",
            Self::Team => "team",
            Self::Organization => "organization",
            Self::Unknown => "organization",
        }
    }
}

/// Build vs Run work classification (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WorkType {
    Build,
    Run,
    #[serde(other)]
    Unknown,
}

impl WorkType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Run => "run",
            Self::Unknown => "build",
        }
    }
}

/// How work attribution was captured (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AttributionBasis {
    WorkFor,
    CurrentTask,
    ExplicitContext,
    Unattributed,
    ManualCorrection,
    #[serde(other)]
    Unknown,
}

impl AttributionBasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkFor => "work_for",
            Self::CurrentTask => "current_task",
            Self::ExplicitContext => "explicit_context",
            Self::Unattributed => "unattributed",
            Self::ManualCorrection => "manual_correction",
            Self::Unknown => "unattributed",
        }
    }
}

/// Kind of immutable usage correction (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CorrectionKind {
    Usage,
    Attribution,
    #[serde(other)]
    Unknown,
}

impl CorrectionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Attribution => "attribution",
            Self::Unknown => "attribution",
        }
    }
}

/// Versioned USD model price (micro-USD per 1,000,000 tokens; `doc-87`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPriceView {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub effective_from: String,
    pub currency: String,
    pub input_micro_usd_per_million: Option<u64>,
    pub output_micro_usd_per_million: Option<u64>,
    pub cache_read_micro_usd_per_million: Option<u64>,
    pub cache_write_micro_usd_per_million: Option<u64>,
    pub created_at: String,
    pub created_by_kind: String,
    pub created_by_id: Option<String>,
}

/// Result of exact integer micro-USD token valuation (`doc-87` §Token normalization and valuation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenValuation {
    pub cost_micro_usd: Option<u64>,
    pub cost_basis: UsageCostBasis,
    pub cost_status: CompletenessStatus,
    pub cost_issue: Option<&'static str>,
}

/// Computes exact integer micro-USD valuation from disjoint token counts and a versioned price,
/// summing category charges in `u128` and rounding half-up ONCE per event (`doc-87`).
///
/// A zero-count category needs no rate; a nonzero or unknown category with no rate makes the
/// valuation partial (when some nonzero category was priced) or unpriced.
pub fn compute_token_cost_micro_usd(
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
    price: &ModelPriceView,
) -> TokenValuation {
    let pairs = [
        (input_tokens, price.input_micro_usd_per_million),
        (output_tokens, price.output_micro_usd_per_million),
        (cache_read_tokens, price.cache_read_micro_usd_per_million),
        (cache_write_tokens, price.cache_write_micro_usd_per_million),
    ];
    let mut sum_num: u128 = 0;
    let mut any_priced = false;
    let mut any_missing_rate = false;
    let mut any_unknown_tokens = false;
    for (tokens_opt, rate_opt) in pairs {
        match (tokens_opt, rate_opt) {
            (Some(0), _) => {
                any_priced = true;
            }
            (Some(t), Some(r)) => {
                sum_num = sum_num.saturating_add(u128::from(t).saturating_mul(u128::from(r)));
                any_priced = true;
            }
            (Some(_), None) => {
                any_missing_rate = true;
            }
            (None, _) => {
                any_unknown_tokens = true;
            }
        }
    }
    if !any_priced {
        return TokenValuation {
            cost_micro_usd: None,
            cost_basis: UsageCostBasis::Unpriced,
            cost_status: CompletenessStatus::Unavailable,
            cost_issue: Some(if any_missing_rate {
                "missing_price_rate"
            } else {
                "tokens_unavailable"
            }),
        };
    }
    let rounded = (sum_num.saturating_add(500_000)) / 1_000_000;
    let cost_micro_usd = u64::try_from(rounded).unwrap_or(u64::MAX);
    if any_missing_rate || any_unknown_tokens {
        TokenValuation {
            cost_micro_usd: Some(cost_micro_usd),
            cost_basis: UsageCostBasis::TokenEstimate,
            cost_status: CompletenessStatus::Partial,
            cost_issue: Some(if any_missing_rate {
                "partial_price_rate"
            } else {
                "partial_tokens"
            }),
        }
    } else {
        TokenValuation {
            cost_micro_usd: Some(cost_micro_usd),
            cost_basis: UsageCostBasis::TokenEstimate,
            cost_status: CompletenessStatus::Complete,
            cost_issue: None,
        }
    }
}

/// `cost.price.set`: publish a new immutable USD model price version (`doc-87`, `FC-085`).
pub struct CostPriceSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostPriceSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub provider: String,
    pub model: String,
    pub effective_from: String,
    #[serde(default)]
    pub input_micro_usd_per_million: Option<u64>,
    #[serde(default)]
    pub output_micro_usd_per_million: Option<u64>,
    #[serde(default)]
    pub cache_read_micro_usd_per_million: Option<u64>,
    #[serde(default)]
    pub cache_write_micro_usd_per_million: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostPriceSetResult {
    pub price: ModelPriceView,
}

impl Call for CostPriceSet {
    const METHOD: &'static str = "cost.price.set";
    type Params = CostPriceSetParams;
    type Result = CostPriceSetResult;
}

/// `cost.price.list`: list versioned model prices.
pub struct CostPriceList;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostPriceListParams {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostPriceListResult {
    pub prices: Vec<ModelPriceView>,
}

impl Call for CostPriceList {
    const METHOD: &'static str = "cost.price.list";
    type Params = CostPriceListParams;
    type Result = CostPriceListResult;
}

/// One effective or historical immutable usage record (`doc-87`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageRecordView {
    pub id: String,
    pub origin_office_id: String,
    pub producer_id: String,
    pub source_event_id: String,
    pub source_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_office_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub actor_kind: UsageActorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    pub runtime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub occurred_at: String,
    pub recorded_at: String,
    pub recorded_by_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_by_id: Option<String>,
    pub normalization_version: u64,
    pub source_metadata_json: String,
    pub payload_sha256: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub token_status: CompletenessStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_issue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_id: Option<String>,
    pub cost_micro_usd: Option<u64>,
    pub cost_basis: UsageCostBasis,
    pub cost_status: CompletenessStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_issue: Option<String>,
    pub scope_kind: UsageScopeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_type: Option<WorkType>,
    pub attribution_basis: AttributionBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_kind: Option<CorrectionKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_reason: Option<String>,
}

/// `cost.usage.record`: append an immutable normalized usage record (owner / office capture).
pub struct CostUsageRecord;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostUsageRecordParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub producer_id: String,
    pub source_event_id: String,
    #[serde(default)]
    pub run_id: Option<String>,
    pub actor_kind: UsageActorKind,
    #[serde(default)]
    pub employee_id: Option<String>,
    pub runtime: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub occurred_at: String,
    #[serde(default)]
    pub source_metadata_json: Option<String>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_tokens: Option<u64>,
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub token_issue: Option<String>,
    /// Optional runtime-reported API-equivalent list-price estimate in micro-USD.
    #[serde(default)]
    pub runtime_cost_micro_usd: Option<u64>,
    #[serde(default)]
    pub is_subscription: bool,
    pub scope_kind: UsageScopeKind,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub feature_id: Option<String>,
    #[serde(default)]
    pub milestone_id: Option<String>,
    #[serde(default)]
    pub release_id: Option<String>,
    #[serde(default)]
    pub task_type_snapshot: Option<String>,
    #[serde(default)]
    pub work_type: Option<WorkType>,
    pub attribution_basis: AttributionBasis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostUsageRecordResult {
    pub record: UsageRecordView,
    pub replayed: bool,
}

impl Call for CostUsageRecord {
    const METHOD: &'static str = "cost.usage.record";
    type Params = CostUsageRecordParams;
    type Result = CostUsageRecordResult;
}

/// `cost.usage.correct`: append an immutable attribution correction superseding a leaf record.
pub struct CostUsageCorrect;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostUsageCorrectParams {
    pub operation_id: String,
    pub supersedes_id: String,
    pub reason: String,
    pub scope_kind: UsageScopeKind,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub feature_id: Option<String>,
    #[serde(default)]
    pub milestone_id: Option<String>,
    #[serde(default)]
    pub release_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostUsageCorrectResult {
    pub record: UsageRecordView,
}

impl Call for CostUsageCorrect {
    const METHOD: &'static str = "cost.usage.correct";
    type Params = CostUsageCorrectParams;
    type Result = CostUsageCorrectResult;
}

/// Period preset for `cost.query` / `cost.export_csv` (`doc-51` §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CostPeriodPreset {
    Today,
    ThisWeek,
    ThisMonth,
    LastMonth,
    Rolling7d,
    Rolling30d,
    Rolling90d,
    AllTime,
    Custom,
    Release,
    #[serde(other)]
    Unknown,
}

/// Breakdown grouping for `cost.query` (`doc-51` §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CostBreakdownGroup {
    Project,
    Team,
    Consumer,
    Model,
    TaskType,
    WorkType,
    Period,
    Task,
    #[serde(other)]
    Unknown,
}

/// Derived totals over effective unsuperseded usage records (`doc-87`, `doc-51`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSummaryView {
    pub known_cost_micro_usd: Option<u64>,
    pub known_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub record_count: u64,
    pub unpriced_records: u64,
    pub incomplete_records: u64,
    pub includes_estimates: bool,
    pub includes_api_equivalent: bool,
    pub incomplete_streams: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_as_of: Option<String>,
}

/// One breakdown row in `cost.query`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostBreakdownRow {
    pub key: String,
    pub label: String,
    pub summary: CostSummaryView,
}

/// `cost.query`: timezone-aware scoped effective usage totals, breakdowns, and records.
pub struct CostQuery;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostQueryParams {
    #[serde(default)]
    pub scope_kind: Option<UsageScopeKind>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub release_id: Option<String>,
    #[serde(default)]
    pub milestone_id: Option<String>,
    #[serde(default)]
    pub period: Option<CostPeriodPreset>,
    #[serde(default)]
    pub from_date: Option<String>,
    #[serde(default)]
    pub to_date: Option<String>,
    #[serde(default)]
    pub time_zone: Option<String>,
    #[serde(default)]
    pub group_by: Option<CostBreakdownGroup>,
    #[serde(default)]
    pub include_records: bool,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostQueryResult {
    pub time_zone: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<String>,
    pub scoped_to_projects: bool,
    pub summary: CostSummaryView,
    pub breakdown: Vec<CostBreakdownRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<UsageRecordView>,
}

impl Call for CostQuery {
    const METHOD: &'static str = "cost.query";
    type Params = CostQueryParams;
    type Result = CostQueryResult;
}

/// `cost.export_csv`: scoped CSV export of effective usage records (`doc-51` §3, `FC-088`).
pub struct CostExportCsv;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostExportResult {
    pub csv: String,
    pub row_count: u64,
    pub scoped_to_projects: bool,
    pub incomplete_streams: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_as_of: Option<String>,
}

impl Call for CostExportCsv {
    const METHOD: &'static str = "cost.export_csv";
    type Params = CostQueryParams;
    type Result = CostExportResult;
}

// ---------------------------------------------------------------------------
// Subscription model usage snapshots (TASK-79: total, daily, weekly per model)
// ---------------------------------------------------------------------------

/// Collection health status for one provider subscription connection (`TASK-79`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SubscriptionUsageStatus {
    /// Usage metrics were successfully collected from the provider.
    Ok,
    /// Credentials are missing, expired, or rejected by the provider.
    AuthRequired,
    /// Provider usage endpoint is temporarily unreachable, rate-limited, or returned invalid data.
    Unavailable,
    /// Provider or connection type does not expose a standalone subscription usage endpoint.
    Unsupported,
    /// No collection attempt has been recorded yet for this subscription connection.
    NeverCollected,
    #[serde(other)]
    Unknown,
}

impl SubscriptionUsageStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::AuthRequired => "auth_required",
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
            Self::NeverCollected => "never_collected",
            Self::Unknown => "unavailable",
        }
    }
}

/// Which private office-local credential authenticated a subscription usage collection (`TASK-79`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SubscriptionUsageAuthSource {
    None,
    DedicatedUsageCredential,
    EnrolledSubscriptionCredential,
    #[serde(other)]
    Unknown,
}

impl SubscriptionUsageAuthSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DedicatedUsageCredential => "dedicated_usage_credential",
            Self::EnrolledSubscriptionCredential => "enrolled_subscription_credential",
            Self::Unknown => "none",
        }
    }
}

/// Provider-reported usage metrics for one time window (`total`, `daily`, or `weekly`).
///
/// All counters are nullable: `None` means unknown/not reported by the provider, never fabricated
/// as zero (`doc-87`, `TASK-79`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionUsageWindowView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    /// Provider-reported window duration in seconds (e.g. `18000` for 5h, `86400` for 24h, `604800` for 7d).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_seconds: Option<u64>,
    /// Utilization percentage in basis points (`0..=10000`, i.e. `0.00%..=100.00%`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization_bps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

/// Total, daily, weekly, and optional short-term rolling provider-side usage for one model (`TASK-79`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionModelUsageView {
    pub model: String,
    #[serde(default)]
    pub total: SubscriptionUsageWindowView,
    #[serde(default)]
    pub daily: SubscriptionUsageWindowView,
    #[serde(default)]
    pub weekly: SubscriptionUsageWindowView,
    /// Optional sub-daily rolling window (e.g. Claude/Codex 5-hour quota) kept distinct from 24h daily.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_term: Option<SubscriptionUsageWindowView>,
}

/// Persisted and live-enriched subscription usage snapshot for one provider connection (`TASK-79`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionUsageSnapshotView {
    pub connection_id: String,
    pub connection_key: String,
    pub connection_name: String,
    pub provider_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_identity: Option<String>,
    pub office_id: String,
    pub status: SubscriptionUsageStatus,
    pub auth_source: SubscriptionUsageAuthSource,
    pub has_usage_credential: bool,
    pub has_enrolled_credential: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_hint: Option<String>,
    /// Timestamp of the most recent collection attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collected_at: Option<String>,
    /// Timestamp when `models` were last successfully collected (`Ok`), preserved across transient outages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ok_collected_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_until: Option<String>,
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Always `false`: explicitly distinguishes provider subscription usage from API-equivalent estimates.
    pub is_api_equivalent_estimate: bool,
    #[serde(default)]
    pub models: Vec<SubscriptionModelUsageView>,
}

/// Non-secret status metadata for a connection's dedicated usage-collection credential (`TASK-79`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionUsageCredentialView {
    pub connection_id: String,
    pub office_id: String,
    pub configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_hint: Option<String>,
}

/// `cost.subscription_usage.list`: owner-scoped read of persisted subscription model usage snapshots.
pub struct CostSubscriptionUsageList;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageListParams {
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub provider_type: Option<String>,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub totals_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageListResult {
    pub snapshots: Vec<SubscriptionUsageSnapshotView>,
    pub totals_by_model: Vec<SubscriptionModelUsageView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collected_as_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_totals_cursor: Option<String>,
    /// True when any subscription was excluded (stale/non-Ok/duplicate) or had missing/incompatible counters.
    #[serde(default)]
    pub partial_totals: bool,
    /// Number of returned subscription snapshots excluded from `totals_by_model` (stale, non-Ok, or duplicate identity).
    #[serde(default)]
    pub excluded_snapshot_count: u64,
    /// Always `false`: distinguishes subscription usage totals from API-equivalent USD cost estimates.
    pub is_api_equivalent_estimate: bool,
}

impl Call for CostSubscriptionUsageList {
    const METHOD: &'static str = "cost.subscription_usage.list";
    type Params = CostSubscriptionUsageListParams;
    type Result = CostSubscriptionUsageListResult;
}

/// `cost.subscription_usage.collect`: owner-scoped authenticated collection for one or all active subscriptions.
pub struct CostSubscriptionUsageCollect;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCollectParams {
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCollectResult {
    pub snapshots: Vec<SubscriptionUsageSnapshotView>,
    pub collected_at: String,
    /// Connection IDs deferred because the batch deadline or response byte budget was reached.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deferred_connection_ids: Vec<String>,
}

impl Call for CostSubscriptionUsageCollect {
    const METHOD: &'static str = "cost.subscription_usage.collect";
    type Params = CostSubscriptionUsageCollectParams;
    type Result = CostSubscriptionUsageCollectResult;
}

/// `cost.subscription_usage.credential_set`: store a wrapped private usage-API credential outside org Git.
pub struct CostSubscriptionUsageCredentialSet;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCredentialSetParams {
    pub connection_id: String,
    pub secret: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub account_hint: Option<String>,
    /// Optional expected destination HTTPS origin (`https://host[:port]`); required for custom endpoint URLs.
    #[serde(default)]
    pub expected_origin: Option<String>,
}

impl std::fmt::Debug for CostSubscriptionUsageCredentialSetParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CostSubscriptionUsageCredentialSetParams")
            .field("connection_id", &self.connection_id)
            .field("secret", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("account_hint", &self.account_hint)
            .field("expected_origin", &self.expected_origin)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCredentialSetResult {
    pub credential: SubscriptionUsageCredentialView,
}

impl Call for CostSubscriptionUsageCredentialSet {
    const METHOD: &'static str = "cost.subscription_usage.credential_set";
    type Params = CostSubscriptionUsageCredentialSetParams;
    type Result = CostSubscriptionUsageCredentialSetResult;
}

/// `cost.subscription_usage.credential_revoke`: remove a private usage-API credential from office storage.
pub struct CostSubscriptionUsageCredentialRevoke;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCredentialRevokeParams {
    pub connection_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCredentialRevokeResult {
    pub credential: SubscriptionUsageCredentialView,
}

impl Call for CostSubscriptionUsageCredentialRevoke {
    const METHOD: &'static str = "cost.subscription_usage.credential_revoke";
    type Params = CostSubscriptionUsageCredentialRevokeParams;
    type Result = CostSubscriptionUsageCredentialRevokeResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_integer_token_valuation_matches_doc_87_example() {
        // doc-87 example: $1/M input, $2/M output, $0.50/M cache-read, 1000 in / 200 out / 500 cr / 0 cw = 1650 micro-USD.
        let price = ModelPriceView {
            id: "p1".into(),
            provider: "example".into(),
            model: "m1".into(),
            effective_from: "2026-11-01T00:00:00.000Z".into(),
            currency: "USD".into(),
            input_micro_usd_per_million: Some(1_000_000),
            output_micro_usd_per_million: Some(2_000_000),
            cache_read_micro_usd_per_million: Some(500_000),
            cache_write_micro_usd_per_million: Some(1_250_000),
            created_at: "2026-10-31T12:00:00.000Z".into(),
            created_by_kind: "system".into(),
            created_by_id: None,
        };
        let val = compute_token_cost_micro_usd(Some(1000), Some(200), Some(500), Some(0), &price);
        assert_eq!(val.cost_micro_usd, Some(1650));
        assert_eq!(val.cost_basis, UsageCostBasis::TokenEstimate);
        assert_eq!(val.cost_status, CompletenessStatus::Complete);
        assert_eq!(val.cost_issue, None);

        // Zero cache_write needs no rate; nonzero missing rate yields Partial.
        let partial_price = ModelPriceView {
            cache_write_micro_usd_per_million: None,
            ..price.clone()
        };
        let zero_cw =
            compute_token_cost_micro_usd(Some(1000), Some(200), Some(500), Some(0), &partial_price);
        assert_eq!(zero_cw.cost_status, CompletenessStatus::Complete);
        let nonzero_cw = compute_token_cost_micro_usd(
            Some(1000),
            Some(200),
            Some(500),
            Some(100),
            &partial_price,
        );
        assert_eq!(nonzero_cw.cost_status, CompletenessStatus::Partial);
        assert_eq!(nonzero_cw.cost_issue, Some("partial_price_rate"));
    }
}
