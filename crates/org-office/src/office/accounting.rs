//! Office facade for `L2` immutable usage & cost accounting, versioned USD model pricing,
//! attribution corrections, timezone-aware queries, and CSV export (`TASK-53`, `doc-87`, `doc-51`).

use std::sync::atomic::{AtomicU64, Ordering};

use org_proto::costs::{
    CostExportResult, CostPriceListParams, CostPriceListResult, CostPriceSetParams,
    CostPriceSetResult, CostQueryParams, CostQueryResult, CostUsageCorrectParams,
    CostUsageCorrectResult, CostUsageRecordParams, CostUsageRecordResult, UsageRecordView,
    UsageScopeKind,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Sha256;

use crate::accounting::query::{execute_cost_query, export_cost_csv};
use crate::accounting::{
    build_attribution_correction, build_model_price, build_usage_record, CostViewerScope,
    UsageBuildMeta,
};
use crate::error::OfficeError;
use crate::organization::records::{Employee, EmployeeKind, EmployeeStatus};
use crate::time::format_rfc3339_ms;

use super::Office;

static UUID_SEQ: AtomicU64 = AtomicU64::new(1);

impl Office {
    /// Mints a valid canonical lowercase RFC 9562 UUIDv7 using monotonic ms + counter + office hash.
    pub(crate) fn next_uuid_v7(&self) -> String {
        mint_uuid_v7(self)
    }

    pub(crate) fn require_active_member(
        &self,
        actor: &str,
    ) -> Result<Result<Employee, ApiError>, OfficeError> {
        require_active_member_for(self, actor)
    }

    /// Returns all project IDs whose accountable (`owner_team_id`) team is led by `member_id`.
    pub(crate) fn pm_project_ids(&self, member_id: &str) -> Result<Vec<String>, OfficeError> {
        list_pm_project_ids(self, member_id)
    }

    /// `cost.price.set`: publishes a new immutable USD model price version (owner only).
    pub fn cost_price_set(
        &self,
        actor: &str,
        params: CostPriceSetParams,
    ) -> Result<Result<CostPriceSetResult, ApiError>, OfficeError> {
        op_cost_price_set(self, actor, params)
    }

    /// `cost.price.list`: lists versioned USD model prices (owner or PM).
    pub fn cost_price_list(
        &self,
        actor: &str,
        params: CostPriceListParams,
    ) -> Result<Result<CostPriceListResult, ApiError>, OfficeError> {
        op_cost_price_list(self, actor, params)
    }

    /// `cost.usage.record`: owner RPC to append a normalized immutable usage record.
    pub fn cost_usage_record(
        &self,
        actor: &str,
        params: CostUsageRecordParams,
    ) -> Result<Result<CostUsageRecordResult, ApiError>, OfficeError> {
        op_cost_usage_record(self, actor, params)
    }

    /// Office-internal capture hook for agent sessions & one-off watchers (`doc-87`, `doc-92`).
    pub fn record_office_usage(
        &self,
        params: CostUsageRecordParams,
    ) -> Result<Result<CostUsageRecordResult, ApiError>, OfficeError> {
        op_record_usage_internal(self, "system", None, params)
    }

    /// `cost.usage.correct`: appends an immutable attribution correction superseding a leaf record.
    pub fn cost_usage_correct(
        &self,
        actor: &str,
        params: CostUsageCorrectParams,
    ) -> Result<Result<CostUsageCorrectResult, ApiError>, OfficeError> {
        op_cost_usage_correct(self, actor, params)
    }

    /// `cost.query`: timezone-aware scoped cost totals, breakdowns, and records.
    pub fn cost_query(
        &self,
        actor: &str,
        params: CostQueryParams,
    ) -> Result<Result<CostQueryResult, ApiError>, OfficeError> {
        op_cost_query(self, actor, params)
    }

    /// `cost.export_csv`: scoped RFC 4180 CSV export of effective usage records (`FC-088`).
    pub fn cost_export_csv(
        &self,
        actor: &str,
        params: CostQueryParams,
    ) -> Result<Result<CostExportResult, ApiError>, OfficeError> {
        op_cost_export_csv(self, actor, params)
    }
}

fn mint_uuid_v7(office: &Office) -> String {
    let now_ms = office.monotonic_now_ms();
    let seq = UUID_SEQ.fetch_add(1, Ordering::Relaxed);
    let seed = format!("{}:{now_ms}:{seq}", office.info.office_id);
    let hash = Sha256::of(seed.as_bytes());
    let mut bytes = [0u8; 16];
    bytes[0] = ((now_ms >> 40) & 0xff) as u8;
    bytes[1] = ((now_ms >> 32) & 0xff) as u8;
    bytes[2] = ((now_ms >> 24) & 0xff) as u8;
    bytes[3] = ((now_ms >> 16) & 0xff) as u8;
    bytes[4] = ((now_ms >> 8) & 0xff) as u8;
    bytes[5] = (now_ms & 0xff) as u8;
    // 12-bit monotonic sequence in version-7 field.
    bytes[6] = 0x70 | (((seq >> 8) & 0x0f) as u8);
    bytes[7] = (seq & 0xff) as u8;
    bytes[8..16].copy_from_slice(&hash.as_bytes()[0..8]);
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format_uuid_bytes(&bytes)
}

fn require_active_member_for(
    office: &Office,
    actor: &str,
) -> Result<Result<Employee, ApiError>, OfficeError> {
    let Some(emp) = office.load_employee(actor)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller is not a member of this organization",
        )));
    };
    if emp.status != EmployeeStatus::Active {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "inactive or paused employees cannot perform this operation",
        )));
    }
    Ok(Ok(emp))
}

fn resolve_viewer_scope(
    office: &Office,
    actor: &str,
) -> Result<Result<(Employee, CostViewerScope), ApiError>, OfficeError> {
    let caller = match require_active_member_for(office, actor)? {
        Ok(emp) => emp,
        Err(e) => return Ok(Err(e)),
    };
    if caller.id.as_str() == office.owner.as_str() {
        return Ok(Ok((caller, CostViewerScope::Owner)));
    }
    let project_ids = list_pm_project_ids(office, caller.id.as_str())?;
    if project_ids.is_empty() {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller is not authorized to access cost data",
        )));
    }
    Ok(Ok((
        caller,
        CostViewerScope::ProjectManager { project_ids },
    )))
}

fn list_pm_project_ids(office: &Office, member_id: &str) -> Result<Vec<String>, OfficeError> {
    let snap = office.org_snapshot()?;
    let led_teams: std::collections::BTreeSet<&str> = snap
        .teams
        .iter()
        .filter(|t| t.lead_member_id.as_deref() == Some(member_id))
        .map(|t| t.id.as_str())
        .collect();
    if led_teams.is_empty() {
        return Ok(Vec::new());
    }
    let projects = office.list_projects(true)?;
    Ok(projects
        .projects
        .into_iter()
        .filter(|p| led_teams.contains(p.owner_team_id.as_str()))
        .map(|p| p.id)
        .collect())
}

fn op_cost_price_set(
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

fn op_cost_price_list(
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

fn op_cost_usage_record(
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

fn op_record_usage_internal(
    office: &Office,
    recorded_by_kind: &str,
    recorded_by_id: Option<String>,
    params: CostUsageRecordParams,
) -> Result<Result<CostUsageRecordResult, ApiError>, OfficeError> {
    let price = match (params.provider.as_deref(), params.model.as_deref()) {
        (Some(prov), Some(model)) => office.accounting.effective_price(
            &office.info.org_id,
            prov,
            model,
            &params.occurred_at,
        )?,
        _ => None,
    };
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
    if !replayed {
        office.reevaluate_budgets_for_record(&saved)?;
    }
    Ok(Ok(CostUsageRecordResult {
        record: saved,
        replayed,
    }))
}

fn op_cost_usage_correct(
    office: &Office,
    actor: &str,
    mut params: CostUsageCorrectParams,
) -> Result<Result<CostUsageCorrectResult, ApiError>, OfficeError> {
    let (caller, viewer) = match resolve_viewer_scope(office, actor)? {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    if !org_proto::is_uuid_v7(&params.operation_id) || !org_proto::is_uuid_v7(&params.supersedes_id)
    {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "operation_id and supersedes_id must be canonical lowercase UUIDv7",
        )));
    }
    let Some(parent) = office
        .accounting
        .get_usage(&office.info.org_id, &params.supersedes_id)?
    else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "usage record not found",
        )));
    };
    if let Err(e) = authorize_correction(office, &caller, &viewer, &parent, &mut params)? {
        return Ok(Err(e));
    }
    let producer_id = format!("hq-corrections-{}", office.info.office_id);
    let recorded_at = format_rfc3339_ms(office.monotonic_now_ms());
    let corr = match build_attribution_correction(
        UsageBuildMeta {
            org_id: &office.info.org_id,
            origin_office_id: &office.info.office_id,
            sequence: 1,
            recorded_at: &recorded_at,
            recorded_by_kind: "employee",
            recorded_by_id: Some(caller.id.as_str().to_owned()),
            default_id: office.next_uuid_v7(),
        },
        &parent,
        &params,
    ) {
        Ok(r) => r,
        Err(e) => return Ok(Err(e)),
    };
    if let Some(existing) = office.accounting.find_by_source_event(
        &office.info.org_id,
        &office.info.office_id,
        &producer_id,
        &corr.source_event_id,
    )? {
        if existing.payload_sha256 == corr.payload_sha256 {
            return Ok(Ok(CostUsageCorrectResult { record: existing }));
        }
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "operation_id reused with different correction payload",
        )));
    }
    if office
        .accounting
        .is_superseded(&office.info.org_id, &parent.id)?
    {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "usage record has already been superseded; correct the latest leaf record",
        )));
    }
    let (saved, replayed) = match office.accounting.append_usage(&office.info.org_id, corr)? {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    if !replayed {
        office.reevaluate_budgets_for_record(&parent)?;
        office.reevaluate_budgets_for_record(&saved)?;
    }
    Ok(Ok(CostUsageCorrectResult { record: saved }))
}

fn authorize_correction(
    office: &Office,
    caller: &Employee,
    viewer: &CostViewerScope,
    parent: &UsageRecordView,
    params: &mut CostUsageCorrectParams,
) -> Result<Result<(), ApiError>, OfficeError> {
    if let Err(e) = fill_correction_target_fields(office, parent, params)? {
        return Ok(Err(e));
    }
    if let Err(e) = check_correction_scope_auth(viewer, parent, params) {
        return Ok(Err(e));
    }
    check_reached_task_correction_guard(office, caller, parent, params)
}

fn fill_correction_target_fields(
    office: &Office,
    parent: &UsageRecordView,
    params: &mut CostUsageCorrectParams,
) -> Result<Result<(), ApiError>, OfficeError> {
    if params.scope_kind == UsageScopeKind::Task {
        if let Some(ref tref) = params.task_id {
            let inspected = match office.task_inspect(tref)? {
                Ok(v) => v,
                Err(e) => return Ok(Err(e)),
            };
            if params.project_id.is_some() && params.project_id != inspected.task.project_id {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "project_id does not match the target task's owning project",
                )));
            }
            if params
                .team_id
                .as_deref()
                .is_some_and(|t| t != inspected.task.team_id.as_str())
            {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "team_id does not match the target task's owning team",
                )));
            }
            params.task_id = Some(inspected.task.id);
            params.project_id = inspected.task.project_id;
            params.team_id = Some(inspected.task.team_id);
        }
    } else if params.scope_kind == UsageScopeKind::Project && params.project_id.is_none() {
        params.project_id = parent.project_id.clone();
    }
    Ok(Ok(()))
}

fn check_reached_task_correction_guard(
    office: &Office,
    caller: &Employee,
    parent: &UsageRecordView,
    params: &CostUsageCorrectParams,
) -> Result<Result<(), ApiError>, OfficeError> {
    // `doc-92`: reducing spend on a reached task budget requires a person (owner or human PM).
    if caller.kind != EmployeeKind::Agent {
        return Ok(Ok(()));
    }
    let Some(old_task) = parent.task_id.as_deref() else {
        return Ok(Ok(()));
    };
    if params.task_id.as_deref() != Some(old_task) && office.is_task_budget_reached(old_task)? {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "Only a person can raise, clear or extend a budget.",
        )));
    }
    Ok(Ok(()))
}

fn op_cost_query(
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

fn op_cost_export_csv(
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

fn check_correction_scope_auth(
    viewer: &CostViewerScope,
    parent: &UsageRecordView,
    params: &CostUsageCorrectParams,
) -> Result<(), ApiError> {
    if !viewer.allows_project(parent.project_id.as_deref()) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller is not authorized to correct usage from this source project",
        ));
    }
    match params.scope_kind {
        UsageScopeKind::Task | UsageScopeKind::Project
            if !viewer.allows_project(params.project_id.as_deref()) =>
        {
            Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller is not authorized to attribute usage to the target project",
            ))
        }
        UsageScopeKind::Team | UsageScopeKind::Organization
            if *viewer != CostViewerScope::Owner =>
        {
            Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "only the organization owner can attribute usage outside a project",
            ))
        }
        _ => Ok(()),
    }
}

fn format_uuid_bytes(b: &[u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(36);
    for (idx, &byte) in b.iter().enumerate() {
        if matches!(idx, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
