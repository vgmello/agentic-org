//! Timezone-aware cost queries, breakdown aggregation, and RFC 4180 CSV export over
//! effective unsuperseded usage records (`TASK-53`, `doc-87`, `doc-51`, `FC-084..088`).

use std::collections::BTreeMap;

use org_proto::costs::{
    CompletenessStatus, CostBreakdownGroup, CostBreakdownRow, CostExportResult, CostPeriodPreset,
    CostQueryParams, CostQueryResult, CostSummaryView, UsageCostBasis, UsageRecordView,
};
use org_proto::tz::{
    date_range_to_utc_bounds, format_utc_bounds, local_date_key_for_rfc3339, local_day_bounds_utc,
    local_month_bounds_utc, local_week_bounds_utc, validate_iana_timezone,
};
use org_proto::{ApiError, ErrorCode};

use crate::{
    default_work_type_label, record_is_api_equivalent, record_known_tokens, CostViewerScope,
    UsageCoverageSummary,
};

const MAX_RECORDS_RETURNED: usize = 500;
const DAY_MS: u64 = 86_400_000;

/// Resolves `[starts_at_ms, ends_at_ms)` from [`CostQueryParams`] in `zone` (`doc-51` §3).
pub fn resolve_query_bounds(
    params: &CostQueryParams,
    zone: &str,
    now_ms: u64,
) -> Result<(Option<u64>, Option<u64>), ApiError> {
    if !validate_iana_timezone(zone) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "time_zone must be a valid IANA time zone",
        ));
    }
    let has_dates = params.from_date.is_some() || params.to_date.is_some();
    if params.to_date.is_some() && params.from_date.is_none() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "to_date requires from_date",
        ));
    }
    if has_dates && params.period.is_some_and(|p| p != CostPeriodPreset::Custom) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "cannot combine from_date/to_date with a non-custom period preset",
        ));
    }
    if let Some(from) = params.from_date.as_deref() {
        let (s, e) = date_range_to_utc_bounds(zone, from, params.to_date.as_deref(), now_ms)
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "invalid date range for the selected time zone",
                )
            })?;
        return Ok((Some(s), Some(e)));
    }
    resolve_preset_bounds(
        params.period.unwrap_or(CostPeriodPreset::AllTime),
        params.release_id.as_deref(),
        zone,
        now_ms,
    )
}

fn resolve_preset_bounds(
    preset: CostPeriodPreset,
    release_id: Option<&str>,
    zone: &str,
    now_ms: u64,
) -> Result<(Option<u64>, Option<u64>), ApiError> {
    match preset {
        CostPeriodPreset::AllTime => Ok((None, None)),
        CostPeriodPreset::Today => {
            let (_, s, e) = local_day_bounds_utc(zone, now_ms).ok_or_else(invalid_tz)?;
            Ok((Some(s), Some(e)))
        }
        CostPeriodPreset::ThisWeek => {
            let (_, s, e) = local_week_bounds_utc(zone, now_ms).ok_or_else(invalid_tz)?;
            Ok((Some(s), Some(e)))
        }
        CostPeriodPreset::ThisMonth => {
            let (_, s, e) = local_month_bounds_utc(zone, now_ms).ok_or_else(invalid_tz)?;
            Ok((Some(s), Some(e)))
        }
        CostPeriodPreset::LastMonth => {
            let (_, this_start, _) = local_month_bounds_utc(zone, now_ms).ok_or_else(invalid_tz)?;
            let prev_probe = this_start.saturating_sub(1_000);
            let (_, prev_start, prev_end) =
                local_month_bounds_utc(zone, prev_probe).ok_or_else(invalid_tz)?;
            Ok((Some(prev_start), Some(prev_end)))
        }
        CostPeriodPreset::Rolling7d => Ok((Some(now_ms.saturating_sub(7 * DAY_MS)), Some(now_ms))),
        CostPeriodPreset::Rolling30d => {
            Ok((Some(now_ms.saturating_sub(30 * DAY_MS)), Some(now_ms)))
        }
        CostPeriodPreset::Rolling90d => {
            Ok((Some(now_ms.saturating_sub(90 * DAY_MS)), Some(now_ms)))
        }
        CostPeriodPreset::Release if release_id.is_some() => Ok((None, Some(now_ms))),
        CostPeriodPreset::Release => Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "release period requires a release_id scope",
        )),
        CostPeriodPreset::Custom | _ => Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid or incomplete cost period",
        )),
    }
}

fn invalid_tz() -> ApiError {
    ApiError::new(
        ErrorCode::InvalidRequest,
        "could not resolve period bounds for time zone",
    )
}

/// Filters effective records by viewer authorization scope, target filters, and UTC bounds.
pub fn filter_records<'a>(
    records: &'a [UsageRecordView],
    viewer: &CostViewerScope,
    params: &CostQueryParams,
    bounds: (Option<u64>, Option<u64>),
) -> Vec<&'a UsageRecordView> {
    records
        .iter()
        .filter(|rec| matches_viewer_and_filters(rec, viewer, params, bounds))
        .collect()
}

fn matches_viewer_and_filters(
    rec: &UsageRecordView,
    viewer: &CostViewerScope,
    params: &CostQueryParams,
    bounds: (Option<u64>, Option<u64>),
) -> bool {
    if !viewer.allows_project(rec.project_id.as_deref()) {
        return false;
    }
    if params.scope_kind.is_some_and(|s| rec.scope_kind != s)
        || params
            .task_id
            .as_deref()
            .is_some_and(|id| rec.task_id.as_deref() != Some(id))
        || params
            .project_id
            .as_deref()
            .is_some_and(|id| rec.project_id.as_deref() != Some(id))
        || params
            .team_id
            .as_deref()
            .is_some_and(|id| rec.team_id.as_deref() != Some(id))
        || params
            .agent_id
            .as_deref()
            .is_some_and(|id| rec.employee_id.as_deref() != Some(id))
        || params
            .release_id
            .as_deref()
            .is_some_and(|id| rec.release_id.as_deref() != Some(id))
        || params
            .milestone_id
            .as_deref()
            .is_some_and(|id| rec.milestone_id.as_deref() != Some(id))
    {
        return false;
    }
    let Some(occ_ms) = org_proto::parse_rfc3339_ms(&rec.occurred_at) else {
        return false;
    };
    if bounds.0.is_some_and(|start| occ_ms < start) || bounds.1.is_some_and(|end| occ_ms >= end) {
        return false;
    }
    true
}

/// Aggregates a slice of effective records into [`CostSummaryView`] preserving `None` when all
/// values are unknown and reporting unpriced/incomplete counts (`doc-87` §One ledger; derived totals).
pub fn aggregate_summary(
    records: &[&UsageRecordView],
    coverage: &UsageCoverageSummary,
) -> CostSummaryView {
    let mut out = CostSummaryView {
        incomplete_streams: coverage.incomplete_streams,
        coverage_as_of: coverage.coverage_as_of.clone(),
        ..Default::default()
    };
    for rec in records {
        out.record_count = out.record_count.saturating_add(1);
        add_opt(&mut out.known_cost_micro_usd, rec.cost_micro_usd);
        add_opt(&mut out.known_tokens, record_known_tokens(rec));
        add_opt(&mut out.input_tokens, rec.input_tokens);
        add_opt(&mut out.output_tokens, rec.output_tokens);
        add_opt(&mut out.cache_read_tokens, rec.cache_read_tokens);
        add_opt(&mut out.cache_write_tokens, rec.cache_write_tokens);
        add_opt(&mut out.reasoning_tokens, rec.reasoning_tokens);
        if rec.cost_micro_usd.is_none() {
            out.unpriced_records = out.unpriced_records.saturating_add(1);
        }
        if rec.token_status != CompletenessStatus::Complete
            || rec.cost_status != CompletenessStatus::Complete
        {
            out.incomplete_records = out.incomplete_records.saturating_add(1);
        }
        if matches!(
            rec.cost_basis,
            UsageCostBasis::RuntimeEstimate | UsageCostBasis::TokenEstimate
        ) {
            out.includes_estimates = true;
        }
        if record_is_api_equivalent(rec) {
            out.includes_api_equivalent = true;
        }
    }
    out
}

fn add_opt(acc: &mut Option<u64>, val: Option<u64>) {
    if let Some(v) = val {
        *acc = Some(acc.unwrap_or(0).saturating_add(v));
    }
}

/// Groups filtered records into breakdown rows.
pub fn build_breakdown(
    records: &[&UsageRecordView],
    group_by: CostBreakdownGroup,
    zone: &str,
    coverage: &UsageCoverageSummary,
) -> Vec<CostBreakdownRow> {
    let mut groups: BTreeMap<String, Vec<&UsageRecordView>> = BTreeMap::new();
    for &rec in records {
        let key = breakdown_key(rec, group_by, zone);
        groups.entry(key).or_default().push(rec);
    }
    groups
        .into_iter()
        .map(|(key, list)| CostBreakdownRow {
            label: key.clone(),
            key,
            summary: aggregate_summary(&list, coverage),
        })
        .collect()
}

fn breakdown_key(rec: &UsageRecordView, group_by: CostBreakdownGroup, zone: &str) -> String {
    match group_by {
        CostBreakdownGroup::Project => rec
            .project_id
            .clone()
            .unwrap_or_else(|| "organization_overhead".into()),
        CostBreakdownGroup::Team => rec.team_id.clone().unwrap_or_else(|| "unassigned".into()),
        CostBreakdownGroup::Consumer => rec.employee_id.clone().unwrap_or_else(|| "one_off".into()),
        CostBreakdownGroup::Model => match (&rec.provider, &rec.model) {
            (Some(p), Some(m)) => format!("{p}/{m}"),
            (None, Some(m)) => m.clone(),
            _ => "unknown".into(),
        },
        CostBreakdownGroup::TaskType => rec
            .task_type_snapshot
            .clone()
            .unwrap_or_else(|| "not_on_a_task".into()),
        CostBreakdownGroup::WorkType => default_work_type_label(rec.work_type).into(),
        CostBreakdownGroup::Period => {
            local_date_key_for_rfc3339(zone, &rec.occurred_at).unwrap_or_else(|| "unknown".into())
        }
        CostBreakdownGroup::Task => rec
            .task_id
            .clone()
            .unwrap_or_else(|| "not_on_a_task".into()),
        _ => "unknown".into(),
    }
}

/// Runs a complete `cost.query` over effective records.
pub fn execute_cost_query(
    effective: &[UsageRecordView],
    coverage: &UsageCoverageSummary,
    viewer: &CostViewerScope,
    params: &CostQueryParams,
    zone: &str,
    now_ms: u64,
) -> Result<CostQueryResult, ApiError> {
    if let Some(pid) = params.project_id.as_deref() {
        if !viewer.allows_project(Some(pid)) {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller is not authorized to view costs for this project",
            ));
        }
    }
    let bounds = resolve_query_bounds(params, zone, now_ms)?;
    let filtered = filter_records(effective, viewer, params, bounds);
    let summary = aggregate_summary(&filtered, coverage);
    let breakdown = match params.group_by {
        Some(gb) => build_breakdown(&filtered, gb, zone, coverage),
        None => Vec::new(),
    };
    let records = if params.include_records {
        let limit = params
            .limit
            .map(|l| (l as usize).min(MAX_RECORDS_RETURNED))
            .unwrap_or(100);
        filtered.into_iter().take(limit).cloned().collect()
    } else {
        Vec::new()
    };
    let (starts_at, ends_at) = match bounds {
        (Some(s), Some(e)) => {
            let (a, b) = format_utc_bounds(s, e).unwrap_or_default();
            (Some(a), Some(b))
        }
        (Some(s), None) => (org_proto::format_rfc3339_ms(s), None),
        (None, Some(e)) => (None, org_proto::format_rfc3339_ms(e)),
        (None, None) => (None, None),
    };
    Ok(CostQueryResult {
        time_zone: zone.to_owned(),
        starts_at,
        ends_at,
        scoped_to_projects: viewer.is_scoped_to_projects(),
        summary,
        breakdown,
        records,
    })
}

/// Exports filtered effective usage records as RFC 4180 CSV (`doc-51` §3, `FC-088`).
pub fn export_cost_csv(
    effective: &[UsageRecordView],
    coverage: &UsageCoverageSummary,
    viewer: &CostViewerScope,
    params: &CostQueryParams,
    zone: &str,
    now_ms: u64,
) -> Result<CostExportResult, ApiError> {
    if let Some(pid) = params.project_id.as_deref() {
        if !viewer.allows_project(Some(pid)) {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller is not authorized to export costs for this project",
            ));
        }
    }
    let bounds = resolve_query_bounds(params, zone, now_ms)?;
    let filtered = filter_records(effective, viewer, params, bounds);
    let mut csv = String::from(
        "id,origin_office_id,producer_id,source_event_id,occurred_at,recorded_at,actor_kind,\
         employee_id,runtime,provider,model,input_tokens,output_tokens,cache_read_tokens,\
         cache_write_tokens,reasoning_tokens,token_status,price_id,cost_micro_usd,cost_basis,\
         cost_status,scope_kind,task_id,project_id,team_id,release_id,work_type,\
         attribution_basis,supersedes_id,correction_kind,correction_reason,coverage_as_of\n",
    );
    let cov_ref = coverage.coverage_as_of.as_deref().unwrap_or("");
    for rec in &filtered {
        append_csv_row(&mut csv, rec, cov_ref);
    }
    Ok(CostExportResult {
        csv,
        row_count: filtered.len() as u64,
        scoped_to_projects: viewer.is_scoped_to_projects(),
        incomplete_streams: coverage.incomplete_streams,
        coverage_as_of: coverage.coverage_as_of.clone(),
    })
}

fn append_csv_row(out: &mut String, rec: &UsageRecordView, cov_ref: &str) {
    let opt_u64 = |v: Option<u64>| v.map(|n| n.to_string()).unwrap_or_default();
    let fields: [&str; 32] = [
        &rec.id,
        &rec.origin_office_id,
        &rec.producer_id,
        &rec.source_event_id,
        &rec.occurred_at,
        &rec.recorded_at,
        rec.actor_kind.as_str(),
        rec.employee_id.as_deref().unwrap_or(""),
        &rec.runtime,
        rec.provider.as_deref().unwrap_or(""),
        rec.model.as_deref().unwrap_or(""),
        &opt_u64(rec.input_tokens),
        &opt_u64(rec.output_tokens),
        &opt_u64(rec.cache_read_tokens),
        &opt_u64(rec.cache_write_tokens),
        &opt_u64(rec.reasoning_tokens),
        rec.token_status.as_str(),
        rec.price_id.as_deref().unwrap_or(""),
        &opt_u64(rec.cost_micro_usd),
        rec.cost_basis.as_str(),
        rec.cost_status.as_str(),
        rec.scope_kind.as_str(),
        rec.task_id.as_deref().unwrap_or(""),
        rec.project_id.as_deref().unwrap_or(""),
        rec.team_id.as_deref().unwrap_or(""),
        rec.release_id.as_deref().unwrap_or(""),
        rec.work_type.map(|w| w.as_str()).unwrap_or(""),
        rec.attribution_basis.as_str(),
        rec.supersedes_id.as_deref().unwrap_or(""),
        rec.correction_kind.map(|k| k.as_str()).unwrap_or(""),
        rec.correction_reason.as_deref().unwrap_or(""),
        cov_ref,
    ];
    for (idx, field) in fields.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        push_csv_cell(out, field);
    }
    out.push('\n');
}

fn push_csv_cell(out: &mut String, cell: &str) {
    // Guard against spreadsheet formula injection on untrusted text + RFC 4180 quoting.
    let needs_prefix = matches!(
        cell.as_bytes().first(),
        Some(b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r')
    );
    let needs_quote = needs_prefix || cell.contains([',', '"', '\n', '\r']);
    if !needs_quote {
        out.push_str(cell);
        return;
    }
    out.push('"');
    if needs_prefix {
        out.push('\'');
    }
    for ch in cell.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
}
