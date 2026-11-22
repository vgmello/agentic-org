//! Consumer-owned port and domain contract for `L2` immutable usage & cost accounting,
//! versioned USD model pricing, durable JSONL/checkpoint streams, and attribution corrections
//! (`TASK-53`, `doc-87`, `doc-51`, `FC-084..088`).

pub mod memory;
pub mod query;

use std::collections::BTreeMap;

use org_proto::costs::{
    compute_token_cost_micro_usd, AttributionBasis, CompletenessStatus, CorrectionKind,
    CostPriceListParams, CostPriceSetParams, CostUsageCorrectParams, CostUsageRecordParams,
    ModelPriceView, UsageActorKind, UsageCostBasis, UsageRecordView, UsageScopeKind, WorkType,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Sha256;
use serde::{Deserialize, Serialize};

use crate::error::OfficeError;

/// Durable producer checkpoint (`doc-87` §Durable storage and multi-office recovery).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageStreamCheckpoint {
    pub organization_id: String,
    pub origin_office_id: String,
    pub producer_id: String,
    pub high_water_sequence: u64,
    pub head_sha256: Option<String>,
    pub state: String,
    pub checkpointed_at: String,
}

/// Summary of durable stream coverage (`usage_stream_coverage` / `ledger_incomplete_streams`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageCoverageSummary {
    pub incomplete_streams: u64,
    pub coverage_as_of: Option<String>,
    pub checkpoint_ref: Option<String>,
}

/// Caller's resolved cost visibility scope (`doc-51` §1 "Who sees amounts").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostViewerScope {
    /// Organization owner: sees all org, project, team, task, agent, and release costs.
    Owner,
    /// Project manager (lead of one or more projects' accountable teams): sees ONLY those projects.
    ProjectManager { project_ids: Vec<String> },
}

impl CostViewerScope {
    pub fn allows_project(&self, project_id: Option<&str>) -> bool {
        match self {
            Self::Owner => true,
            Self::ProjectManager { project_ids } => {
                project_id.is_some_and(|pid| project_ids.iter().any(|p| p == pid))
            }
        }
    }

    pub fn is_scoped_to_projects(&self) -> bool {
        matches!(self, Self::ProjectManager { .. })
    }
}

/// Consumer-owned storage port for immutable usage records, checkpoints, and versioned prices.
/// Implemented by `SqliteAccountingStore` (production, durable JSONL+YAML + isolated
/// `protected/accounting.db`) and [`memory::MemoryAccountingStore`] (tests).
pub trait AccountingStore: Send + Sync {
    fn set_price(
        &self,
        org_id: &str,
        price: ModelPriceView,
    ) -> Result<Result<ModelPriceView, ApiError>, OfficeError>;

    fn list_prices(
        &self,
        org_id: &str,
        params: &CostPriceListParams,
    ) -> Result<Vec<ModelPriceView>, OfficeError>;

    fn effective_price(
        &self,
        org_id: &str,
        provider: &str,
        model: &str,
        occurred_at: &str,
    ) -> Result<Option<ModelPriceView>, OfficeError>;

    fn append_usage(
        &self,
        org_id: &str,
        record: UsageRecordView,
    ) -> Result<Result<(UsageRecordView, bool), ApiError>, OfficeError>;

    fn get_usage(&self, org_id: &str, id: &str) -> Result<Option<UsageRecordView>, OfficeError>;

    fn find_by_source_event(
        &self,
        org_id: &str,
        origin_office_id: &str,
        producer_id: &str,
        source_event_id: &str,
    ) -> Result<Option<UsageRecordView>, OfficeError>;

    fn is_superseded(&self, org_id: &str, id: &str) -> Result<bool, OfficeError>;

    fn effective_records(&self, org_id: &str) -> Result<Vec<UsageRecordView>, OfficeError>;

    fn coverage_summary(&self, org_id: &str) -> Result<UsageCoverageSummary, OfficeError>;
}

/// Compares semantic fields of two model price versions (ignoring wall-clock `created_at`
/// so idempotent retries with the same `id` replay cleanly).
pub fn same_price_semantics(a: &ModelPriceView, b: &ModelPriceView) -> bool {
    a.id == b.id
        && a.provider == b.provider
        && a.model == b.model
        && a.effective_from == b.effective_from
        && a.currency == b.currency
        && a.input_micro_usd_per_million == b.input_micro_usd_per_million
        && a.output_micro_usd_per_million == b.output_micro_usd_per_million
        && a.cache_read_micro_usd_per_million == b.cache_read_micro_usd_per_million
        && a.cache_write_micro_usd_per_million == b.cache_write_micro_usd_per_million
}

/// Validates and builds an immutable [`ModelPriceView`] from [`CostPriceSetParams`].
pub fn build_model_price(
    params: CostPriceSetParams,
    created_at: &str,
    created_by_kind: &str,
    created_by_id: Option<String>,
    default_id: String,
) -> Result<ModelPriceView, ApiError> {
    let provider = params.provider.trim();
    let model = params.model.trim();
    if provider.is_empty() || model.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "provider and model must be non-empty",
        ));
    }
    let effective_from =
        org_proto::canonicalize_rfc3339_utc(&params.effective_from).ok_or_else(|| {
            ApiError::new(
                ErrorCode::InvalidRequest,
                "effective_from must be a valid RFC 3339 timestamp",
            )
        })?;
    let id = match params.id {
        Some(raw) if org_proto::is_uuid_v7(&raw) => raw,
        Some(_) => {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "price id must be a canonical lowercase UUIDv7",
            ));
        }
        None => default_id,
    };
    Ok(ModelPriceView {
        id,
        provider: provider.to_owned(),
        model: model.to_owned(),
        effective_from,
        currency: "USD".into(),
        input_micro_usd_per_million: params.input_micro_usd_per_million,
        output_micro_usd_per_million: params.output_micro_usd_per_million,
        cache_read_micro_usd_per_million: params.cache_read_micro_usd_per_million,
        cache_write_micro_usd_per_million: params.cache_write_micro_usd_per_million,
        created_at: created_at.to_owned(),
        created_by_kind: created_by_kind.to_owned(),
        created_by_id,
    })
}

/// Computes `canonical-json/2` SHA-256 (`payload_sha256`, 64 lowercase hex chars) over all
/// canonical fields of a usage record (`doc-87` §Durable storage).
pub fn compute_usage_payload_sha256(org_id: &str, rec: &UsageRecordView) -> String {
    let mut map: BTreeMap<&'static str, serde_json::Value> = BTreeMap::new();
    map.insert("actor_kind", rec.actor_kind.as_str().into());
    map.insert("attribution_basis", rec.attribution_basis.as_str().into());
    map.insert("cache_read_tokens", rec.cache_read_tokens.into());
    map.insert("cache_write_tokens", rec.cache_write_tokens.into());
    map.insert(
        "correction_kind",
        rec.correction_kind.map(|k| k.as_str()).into(),
    );
    map.insert("correction_reason", rec.correction_reason.clone().into());
    map.insert("cost_basis", rec.cost_basis.as_str().into());
    map.insert("cost_issue", rec.cost_issue.clone().into());
    map.insert("cost_micro_usd", rec.cost_micro_usd.into());
    map.insert("cost_status", rec.cost_status.as_str().into());
    map.insert("employee_id", rec.employee_id.clone().into());
    map.insert(
        "execution_office_id",
        rec.execution_office_id.clone().into(),
    );
    map.insert("feature_id", rec.feature_id.clone().into());
    map.insert("input_tokens", rec.input_tokens.into());
    map.insert("milestone_id", rec.milestone_id.clone().into());
    map.insert("model", rec.model.clone().into());
    map.insert("normalization_version", rec.normalization_version.into());
    map.insert("occurred_at", rec.occurred_at.clone().into());
    map.insert("organization_id", org_id.into());
    map.insert("origin_office_id", rec.origin_office_id.clone().into());
    map.insert("output_tokens", rec.output_tokens.into());
    map.insert("price_id", rec.price_id.clone().into());
    map.insert("producer_id", rec.producer_id.clone().into());
    map.insert("project_id", rec.project_id.clone().into());
    map.insert("provider", rec.provider.clone().into());
    map.insert("reasoning_tokens", rec.reasoning_tokens.into());
    map.insert("recorded_by_id", rec.recorded_by_id.clone().into());
    map.insert("recorded_by_kind", rec.recorded_by_kind.clone().into());
    map.insert("release_id", rec.release_id.clone().into());
    map.insert("run_id", rec.run_id.clone().into());
    map.insert("schema_version", 1u64.into());
    map.insert("scope_kind", rec.scope_kind.as_str().into());
    map.insert("source_event_id", rec.source_event_id.clone().into());
    map.insert(
        "source_metadata_json",
        rec.source_metadata_json.clone().into(),
    );
    map.insert("supersedes_id", rec.supersedes_id.clone().into());
    map.insert("task_id", rec.task_id.clone().into());
    map.insert("task_kind", "task".into());
    map.insert("task_type_snapshot", rec.task_type_snapshot.clone().into());
    map.insert("team_id", rec.team_id.clone().into());
    map.insert("token_issue", rec.token_issue.clone().into());
    map.insert("token_status", rec.token_status.as_str().into());
    map.insert("work_type", rec.work_type.map(|w| w.as_str()).into());
    let bytes = serde_json::to_vec(&map).unwrap_or_default();
    let mut hex = String::with_capacity(64);
    for b in Sha256::of(&bytes).as_bytes() {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{b:02x}");
    }
    hex
}

/// Validates token completeness and reasoning subset invariant (`doc-87`).
pub fn classify_tokens(
    input: Option<u64>,
    output: Option<u64>,
    cache_read: Option<u64>,
    cache_write: Option<u64>,
    reasoning: Option<u64>,
    explicit_issue: Option<String>,
) -> Result<(CompletenessStatus, Option<String>), ApiError> {
    if let (Some(r), Some(o)) = (reasoning, output) {
        if r > o {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "reasoning_tokens cannot exceed output_tokens",
            ));
        }
    }
    let all_present =
        input.is_some() && output.is_some() && cache_read.is_some() && cache_write.is_some();
    let all_none = input.is_none()
        && output.is_none()
        && cache_read.is_none()
        && cache_write.is_none()
        && reasoning.is_none();
    if all_present {
        Ok((CompletenessStatus::Complete, None))
    } else if all_none {
        Ok((
            CompletenessStatus::Unavailable,
            Some(explicit_issue.unwrap_or_else(|| "runtime_no_usage".into())),
        ))
    } else {
        Ok((
            CompletenessStatus::Partial,
            Some(explicit_issue.unwrap_or_else(|| "partial_token_counts".into())),
        ))
    }
}

/// Resolves cost valuation for a new usage event (`doc-87` §Token normalization and valuation).
pub fn resolve_valuation(
    params: &CostUsageRecordParams,
    token_status: CompletenessStatus,
    price: Option<&ModelPriceView>,
) -> (
    Option<String>,
    Option<u64>,
    UsageCostBasis,
    CompletenessStatus,
    Option<String>,
) {
    if let Some(est) = params.runtime_cost_micro_usd {
        return (
            None,
            Some(est),
            UsageCostBasis::RuntimeEstimate,
            CompletenessStatus::Complete,
            None,
        );
    }
    if let Some(p) = price {
        if token_status != CompletenessStatus::Unavailable {
            let val = compute_token_cost_micro_usd(
                params.input_tokens,
                params.output_tokens,
                params.cache_read_tokens,
                params.cache_write_tokens,
                p,
            );
            let pid = (val.cost_basis == UsageCostBasis::TokenEstimate).then(|| p.id.clone());
            return (
                pid,
                val.cost_micro_usd,
                val.cost_basis,
                val.cost_status,
                val.cost_issue.map(str::to_owned),
            );
        }
    }
    if token_status == CompletenessStatus::Unavailable {
        (
            None,
            None,
            UsageCostBasis::Unknown,
            CompletenessStatus::Unavailable,
            Some("usage_unavailable".into()),
        )
    } else {
        (
            None,
            None,
            UsageCostBasis::Unpriced,
            CompletenessStatus::Unavailable,
            Some("no_effective_model_price".into()),
        )
    }
}

/// Context metadata for building a new immutable [`UsageRecordView`].
pub struct UsageBuildMeta<'a> {
    pub org_id: &'a str,
    pub origin_office_id: &'a str,
    pub sequence: u64,
    pub recorded_at: &'a str,
    pub recorded_by_kind: &'a str,
    pub recorded_by_id: Option<String>,
    pub default_id: String,
}

/// Validates and constructs an immutable [`UsageRecordView`] ready for append.
pub fn build_usage_record(
    meta: UsageBuildMeta<'_>,
    params: CostUsageRecordParams,
    price: Option<&ModelPriceView>,
) -> Result<UsageRecordView, ApiError> {
    let UsageBuildMeta {
        org_id,
        origin_office_id,
        sequence,
        recorded_at,
        recorded_by_kind,
        recorded_by_id,
        default_id,
    } = meta;
    if params.producer_id.trim().is_empty()
        || params.source_event_id.trim().is_empty()
        || params.runtime.trim().is_empty()
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "producer_id, source_event_id, and runtime must be non-empty",
        ));
    }
    if matches!(params.actor_kind, UsageActorKind::Unknown)
        || matches!(params.work_type, Some(WorkType::Unknown))
        || matches!(params.attribution_basis, AttributionBasis::Unknown)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "unknown actor_kind, work_type, or attribution_basis is not accepted",
        ));
    }
    if matches!(params.actor_kind, UsageActorKind::Agent) && params.employee_id.is_none()
        || matches!(params.actor_kind, UsageActorKind::OneOff) && params.employee_id.is_some()
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "actor_kind=agent requires employee_id; actor_kind=one_off forbids employee_id",
        ));
    }
    validate_scope_fields(
        params.scope_kind,
        params.task_id.as_deref(),
        params.project_id.as_deref(),
        params.team_id.as_deref(),
    )?;
    validate_project_children(
        params.project_id.as_deref(),
        params.feature_id.as_deref(),
        params.milestone_id.as_deref(),
        params.release_id.as_deref(),
    )?;
    let occurred_at =
        org_proto::canonicalize_rfc3339_utc(&params.occurred_at).ok_or_else(|| {
            ApiError::new(
                ErrorCode::InvalidRequest,
                "occurred_at must be a valid RFC 3339 timestamp",
            )
        })?;
    let id = match params.id.clone() {
        Some(raw) if org_proto::is_uuid_v7(&raw) => raw,
        Some(_) => {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "usage record id must be a canonical lowercase UUIDv7",
            ));
        }
        None => default_id,
    };
    let (token_status, token_issue) = classify_tokens(
        params.input_tokens,
        params.output_tokens,
        params.cache_read_tokens,
        params.cache_write_tokens,
        params.reasoning_tokens,
        params.token_issue.clone(),
    )?;
    let (price_id, cost_micro_usd, cost_basis, cost_status, cost_issue) =
        resolve_valuation(&params, token_status, price);
    let source_metadata_json = normalize_source_metadata(
        params.source_metadata_json.as_deref(),
        params.is_subscription,
    )?;
    let mut rec = UsageRecordView {
        id,
        origin_office_id: origin_office_id.to_owned(),
        producer_id: params.producer_id.trim().to_owned(),
        source_event_id: params.source_event_id.trim().to_owned(),
        source_sequence: sequence.max(1),
        execution_office_id: Some(origin_office_id.to_owned()),
        run_id: params.run_id,
        actor_kind: params.actor_kind,
        employee_id: params.employee_id,
        runtime: params.runtime.trim().to_owned(),
        provider: params.provider,
        model: params.model,
        occurred_at,
        recorded_at: recorded_at.to_owned(),
        recorded_by_kind: recorded_by_kind.to_owned(),
        recorded_by_id,
        normalization_version: 1,
        source_metadata_json,
        payload_sha256: String::new(),
        input_tokens: params.input_tokens,
        output_tokens: params.output_tokens,
        cache_read_tokens: params.cache_read_tokens,
        cache_write_tokens: params.cache_write_tokens,
        reasoning_tokens: params.reasoning_tokens,
        token_status,
        token_issue,
        price_id,
        cost_micro_usd,
        cost_basis,
        cost_status,
        cost_issue,
        scope_kind: params.scope_kind,
        task_id: params.task_id,
        project_id: params.project_id,
        team_id: params.team_id,
        feature_id: params.feature_id,
        milestone_id: params.milestone_id,
        release_id: params.release_id,
        task_type_snapshot: params.task_type_snapshot,
        work_type: params.work_type,
        attribution_basis: params.attribution_basis,
        supersedes_id: None,
        correction_kind: None,
        correction_reason: None,
    };
    rec.payload_sha256 = compute_usage_payload_sha256(org_id, &rec);
    Ok(rec)
}

/// Builds an immutable attribution correction record superseding `parent` (`doc-87` §Historical attribution).
pub fn build_attribution_correction(
    meta: UsageBuildMeta<'_>,
    parent: &UsageRecordView,
    params: &CostUsageCorrectParams,
) -> Result<UsageRecordView, ApiError> {
    let UsageBuildMeta {
        org_id,
        origin_office_id,
        sequence,
        recorded_at,
        recorded_by_kind,
        recorded_by_id,
        default_id,
    } = meta;
    let reason = params.reason.trim();
    if reason.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "correction reason is required",
        ));
    }
    // Moving a project task record to "Not on a task" retains its explicit source project as overhead (doc-87).
    let project_id = if params.scope_kind == UsageScopeKind::Project && params.project_id.is_none()
    {
        parent.project_id.clone()
    } else {
        params.project_id.clone()
    };
    validate_scope_fields(
        params.scope_kind,
        params.task_id.as_deref(),
        project_id.as_deref(),
        params.team_id.as_deref(),
    )?;
    validate_project_children(
        project_id.as_deref(),
        params.feature_id.as_deref(),
        params.milestone_id.as_deref(),
        params.release_id.as_deref(),
    )?;
    let mut rec = parent.clone();
    rec.id = default_id;
    rec.origin_office_id = origin_office_id.to_owned();
    rec.producer_id = format!("hq-corrections-{origin_office_id}");
    rec.source_event_id = format!("correction/{}", params.operation_id);
    rec.source_sequence = sequence.max(1);
    rec.recorded_at = recorded_at.to_owned();
    rec.recorded_by_kind = recorded_by_kind.to_owned();
    rec.recorded_by_id = recorded_by_id;
    rec.scope_kind = params.scope_kind;
    rec.task_id = params.task_id.clone();
    rec.project_id = project_id;
    rec.team_id = params.team_id.clone();
    rec.feature_id = params.feature_id.clone();
    rec.milestone_id = params.milestone_id.clone();
    rec.release_id = params.release_id.clone();
    rec.attribution_basis = AttributionBasis::ManualCorrection;
    rec.supersedes_id = Some(parent.id.clone());
    rec.correction_kind = Some(CorrectionKind::Attribution);
    rec.correction_reason = Some(reason.to_owned());
    rec.payload_sha256 = compute_usage_payload_sha256(org_id, &rec);
    Ok(rec)
}

fn validate_project_children(
    project_id: Option<&str>,
    feature_id: Option<&str>,
    milestone_id: Option<&str>,
    release_id: Option<&str>,
) -> Result<(), ApiError> {
    if project_id.is_none()
        && (feature_id.is_some() || milestone_id.is_some() || release_id.is_some())
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "feature_id, milestone_id, and release_id require project_id",
        ));
    }
    Ok(())
}

fn validate_scope_fields(
    scope: UsageScopeKind,
    task_id: Option<&str>,
    project_id: Option<&str>,
    team_id: Option<&str>,
) -> Result<(), ApiError> {
    let ok = match scope {
        UsageScopeKind::Task => task_id.is_some(),
        UsageScopeKind::Project => task_id.is_none() && project_id.is_some(),
        UsageScopeKind::Team => task_id.is_none() && project_id.is_none() && team_id.is_some(),
        UsageScopeKind::Organization => {
            task_id.is_none() && project_id.is_none() && team_id.is_none()
        }
        _ => false,
    };
    if !ok {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "scope_kind fields do not match required task/project/team constraints",
        ));
    }
    Ok(())
}

fn normalize_source_metadata(raw: Option<&str>, is_subscription: bool) -> Result<String, ApiError> {
    let mut map: serde_json::Map<String, serde_json::Value> = match raw {
        Some(text) if !text.trim().is_empty() => {
            let val: serde_json::Value = serde_json::from_str(text).map_err(|_| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "source_metadata_json must be a JSON object",
                )
            })?;
            val.as_object().cloned().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "source_metadata_json must be a JSON object",
                )
            })?
        }
        _ => serde_json::Map::new(),
    };
    if is_subscription {
        map.insert(
            "subscription_api_equivalent".into(),
            serde_json::Value::Bool(true),
        );
    }
    serde_json::to_string(&map).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "failed to serialize source_metadata_json",
        )
    })
}

/// Returns total known disjoint tokens for one usage record (reasoning is a subset of output).
pub fn record_known_tokens(rec: &UsageRecordView) -> Option<u64> {
    let vals = [
        rec.input_tokens,
        rec.output_tokens,
        rec.cache_read_tokens,
        rec.cache_write_tokens,
    ];
    if vals.iter().all(Option::is_none) {
        return None;
    }
    Some(
        vals.into_iter()
            .flatten()
            .fold(0u64, |acc, v| acc.saturating_add(v)),
    )
}

/// Whether `rec` carries API-equivalent subscription valuation.
pub fn record_is_api_equivalent(rec: &UsageRecordView) -> bool {
    serde_json::from_str::<serde_json::Value>(&rec.source_metadata_json)
        .ok()
        .and_then(|v| v.get("subscription_api_equivalent")?.as_bool())
        .unwrap_or(false)
}

pub fn default_work_type_label(w: Option<WorkType>) -> &'static str {
    match w {
        Some(WorkType::Build) => "build",
        Some(WorkType::Run) => "run",
        _ => "unknown",
    }
}
