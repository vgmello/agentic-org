//! Versioned USD model price publication/listing and immutable usage-record capture,
//! including idempotent source-event replay matching and budget reevaluation.

use org_proto::costs::{
    CostPriceListParams, CostPriceListResult, CostPriceSetParams, CostPriceSetResult,
    CostUsageRecordParams, CostUsageRecordResult, UsageRecordView, UsageScopeKind,
};
use org_proto::{ApiError, ErrorCode};

use super::super::Office;
use super::access::{require_active_member_for, resolve_viewer_scope};
use crate::accounting::{build_model_price, build_usage_record, UsageBuildMeta};
use crate::error::OfficeError;
use crate::time::format_rfc3339_ms;

pub(super) fn op_cost_price_set(
    office: &Office,
    actor: &str,
    params: CostPriceSetParams,
) -> Result<Result<CostPriceSetResult, ApiError>, OfficeError> {
    let caller = match require_active_member_for(office, actor)? {
        Ok(emp) => emp,
        Err(e) => return Ok(Err(e)),
    };
    if caller.id.as_str() != office.owner.as_str() {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the organization owner can manage model prices",
        )));
    }
    let now = format_rfc3339_ms(office.monotonic_now_ms());
    let price = match build_model_price(
        params,
        &now,
        "employee",
        Some(caller.id.as_str().to_owned()),
        office.next_uuid_v7(),
    ) {
        Ok(p) => p,
        Err(e) => return Ok(Err(e)),
    };
    match office.accounting.set_price(&office.info.org_id, price)? {
        Ok(saved) => Ok(Ok(CostPriceSetResult { price: saved })),
        Err(e) => Ok(Err(e)),
    }
}

pub(super) fn op_cost_price_list(
    office: &Office,
    actor: &str,
    params: CostPriceListParams,
) -> Result<Result<CostPriceListResult, ApiError>, OfficeError> {
    if let Err(e) = resolve_viewer_scope(office, actor)? {
        return Ok(Err(e));
    }
    let prices = office
        .accounting
        .list_prices(&office.info.org_id, &params)?;
    Ok(Ok(CostPriceListResult { prices }))
}

pub(super) fn op_cost_usage_record(
    office: &Office,
    actor: &str,
    params: CostUsageRecordParams,
) -> Result<Result<CostUsageRecordResult, ApiError>, OfficeError> {
    let caller = match require_active_member_for(office, actor)? {
        Ok(emp) => emp,
        Err(e) => return Ok(Err(e)),
    };
    if caller.id.as_str() != office.owner.as_str() {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the organization owner can submit manual usage records",
        )));
    }
    op_record_usage_internal(
        office,
        "employee",
        Some(caller.id.as_str().to_owned()),
        params,
    )
}

pub(super) fn op_record_usage_internal(
    office: &Office,
    recorded_by_kind: &str,
    recorded_by_id: Option<String>,
    params: CostUsageRecordParams,
) -> Result<Result<CostUsageRecordResult, ApiError>, OfficeError> {
    // Serialize usage writes and budget reevaluation under `budget_op_lock` (matching the
    // `budget_op_lock -> accounting` order used throughout `office/budgets.rs`).
    let _budget_guard = office.lock_budget_ops();
    let trimmed_producer = params.producer_id.trim();
    let trimmed_source_event = params.source_event_id.trim();
    let existing = office.accounting.find_by_source_event(
        &office.info.org_id,
        &office.info.office_id,
        trimmed_producer,
        trimmed_source_event,
    )?;
    let recorded_at = format_rfc3339_ms(office.monotonic_now_ms());
    let meta = UsageBuildMeta {
        org_id: &office.info.org_id,
        origin_office_id: &office.info.office_id,
        sequence: 1,
        recorded_at: &recorded_at,
        recorded_by_kind,
        recorded_by_id,
        default_id: office.next_uuid_v7(),
    };
    if let Some(existing) = existing {
        // Match replays against the saved record's price-table valuation so later price updates
        // or partial/missing-rate unpriced records never turn an identical retry into a false
        // Conflict, while still rejecting any changed request field (`TASK-97` / Pass 10 & 11).
        let explicit_id = params.id.clone();
        let runtime_cost_supplied = params.runtime_cost_micro_usd.is_some();
        let mut candidate = match build_usage_record(meta, params, None) {
            Ok(r) => r,
            Err(e) => return Ok(Err(e)),
        };
        if let Some(ref req_id) = explicit_id {
            if req_id != &existing.id {
                return Ok(Err(ApiError::new(
                    ErrorCode::Conflict,
                    "source_event_id already recorded with a different usage record id",
                )));
            }
        }
        if !runtime_cost_supplied
            && existing.cost_basis != org_proto::costs::UsageCostBasis::RuntimeEstimate
        {
            candidate.price_id = existing.price_id.clone();
            candidate.cost_micro_usd = existing.cost_micro_usd;
            candidate.cost_basis = existing.cost_basis;
            candidate.cost_status = existing.cost_status;
            candidate.cost_issue = existing.cost_issue.clone();
            candidate.payload_sha256 =
                crate::accounting::compute_usage_payload_sha256(&office.info.org_id, &candidate);
        }
        if existing.payload_sha256 != candidate.payload_sha256
            || !same_usage_request_inputs(&existing, &candidate)
        {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "source_event_id already recorded with a different payload",
            )));
        }
        super::super::budgets::op_reevaluate_for_record(office, &existing)?;
        return Ok(Ok(CostUsageRecordResult {
            record: existing,
            replayed: true,
        }));
    }
    let price = match (params.provider.as_deref(), params.model.as_deref()) {
        (Some(prov), Some(model)) => office.accounting.effective_price(
            &office.info.org_id,
            prov,
            model,
            &params.occurred_at,
        )?,
        _ => None,
    };
    let record = match build_usage_record(meta, params, price.as_ref()) {
        Ok(r) => r,
        Err(e) => return Ok(Err(e)),
    };
    let (saved, replayed) = match office
        .accounting
        .append_usage(&office.info.org_id, record)?
    {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    super::super::budgets::op_reevaluate_for_record(office, &saved)?;
    Ok(Ok(CostUsageRecordResult {
        record: saved,
        replayed,
    }))
}

#[derive(PartialEq)]
struct UsageRequestInputs<'a> {
    producer_id: &'a str,
    source_event_id: &'a str,
    run_id: Option<&'a str>,
    actor_kind: org_proto::costs::UsageActorKind,
    employee_id: Option<&'a str>,
    runtime: &'a str,
    provider: Option<&'a str>,
    model: Option<&'a str>,
    occurred_at: &'a str,
    source_metadata_json: &'a str,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    token_status: org_proto::costs::CompletenessStatus,
    token_issue: Option<&'a str>,
    scope_kind: UsageScopeKind,
    task_id: Option<&'a str>,
    project_id: Option<&'a str>,
    team_id: Option<&'a str>,
    feature_id: Option<&'a str>,
    milestone_id: Option<&'a str>,
    release_id: Option<&'a str>,
    task_type_snapshot: Option<&'a str>,
    work_type: Option<org_proto::costs::WorkType>,
    attribution_basis: org_proto::costs::AttributionBasis,
}

impl<'a> UsageRequestInputs<'a> {
    fn from_record(r: &'a UsageRecordView) -> Self {
        Self {
            producer_id: &r.producer_id,
            source_event_id: &r.source_event_id,
            run_id: r.run_id.as_deref(),
            actor_kind: r.actor_kind,
            employee_id: r.employee_id.as_deref(),
            runtime: &r.runtime,
            provider: r.provider.as_deref(),
            model: r.model.as_deref(),
            occurred_at: &r.occurred_at,
            source_metadata_json: &r.source_metadata_json,
            input_tokens: r.input_tokens,
            output_tokens: r.output_tokens,
            cache_read_tokens: r.cache_read_tokens,
            cache_write_tokens: r.cache_write_tokens,
            reasoning_tokens: r.reasoning_tokens,
            token_status: r.token_status,
            token_issue: r.token_issue.as_deref(),
            scope_kind: r.scope_kind,
            task_id: r.task_id.as_deref(),
            project_id: r.project_id.as_deref(),
            team_id: r.team_id.as_deref(),
            feature_id: r.feature_id.as_deref(),
            milestone_id: r.milestone_id.as_deref(),
            release_id: r.release_id.as_deref(),
            task_type_snapshot: r.task_type_snapshot.as_deref(),
            work_type: r.work_type,
            attribution_basis: r.attribution_basis,
        }
    }
}

fn same_usage_request_inputs(a: &UsageRecordView, b: &UsageRecordView) -> bool {
    UsageRequestInputs::from_record(a) == UsageRequestInputs::from_record(b)
}
