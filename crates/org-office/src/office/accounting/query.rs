//! Scoped `cost.query` totals and `cost.export_csv` over effective usage records.

use org_proto::costs::{CostExportResult, CostQueryParams, CostQueryResult};
use org_proto::ApiError;

use super::super::Office;
use super::access::resolve_viewer_scope;
use crate::accounting::query::{execute_cost_query, export_cost_csv};
use crate::error::OfficeError;

pub(super) fn op_cost_query(
    office: &Office,
    actor: &str,
    params: CostQueryParams,
) -> Result<Result<CostQueryResult, ApiError>, OfficeError> {
    let (_, viewer) = match resolve_viewer_scope(office, actor)? {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    let zone = params.time_zone.as_deref().unwrap_or("UTC");
    let effective = office.accounting.effective_records(&office.info.org_id)?;
    let coverage = office.accounting.coverage_summary(&office.info.org_id)?;
    let now_ms = office.monotonic_now_ms();
    Ok(execute_cost_query(
        &effective, &coverage, &viewer, &params, zone, now_ms,
    ))
}

pub(super) fn op_cost_export_csv(
    office: &Office,
    actor: &str,
    params: CostQueryParams,
) -> Result<Result<CostExportResult, ApiError>, OfficeError> {
    let (_, viewer) = match resolve_viewer_scope(office, actor)? {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    let zone = params.time_zone.as_deref().unwrap_or("UTC");
    let effective = office.accounting.effective_records(&office.info.org_id)?;
    let coverage = office.accounting.coverage_summary(&office.info.org_id)?;
    let now_ms = office.monotonic_now_ms();
    Ok(export_cost_csv(
        &effective, &coverage, &viewer, &params, zone, now_ms,
    ))
}
