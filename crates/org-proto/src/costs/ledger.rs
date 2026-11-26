//! The usage and cost ledger's calls: model prices, usage records and their corrections, cost
//! queries and CSV export.

use serde::{Deserialize, Serialize};

use super::{
    AttributionBasis, CompletenessStatus, CorrectionKind, ModelPriceView, UsageActorKind,
    UsageCostBasis, UsageScopeKind, WorkType,
};
use crate::Call;

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
