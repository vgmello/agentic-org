//! Office facade for `L2` immutable usage & cost accounting, versioned USD model pricing,
//! attribution corrections, timezone-aware queries, and CSV export (`TASK-53`, `doc-87`, `doc-51`).

use std::sync::atomic::{AtomicU64, Ordering};

use org_proto::costs::{
    CostExportResult, CostPriceListParams, CostPriceListResult, CostPriceSetParams,
    CostPriceSetResult, CostQueryParams, CostQueryResult, CostSubscriptionUsageListParams,
    CostSubscriptionUsageListResult, CostUsageCorrectParams, CostUsageCorrectResult,
    CostUsageRecordParams, CostUsageRecordResult, UsageRecordView, UsageScopeKind,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Sha256;

use crate::accounting::query::{execute_cost_query, export_cost_csv};
use crate::accounting::{
    aggregate_totals_by_model, build_attribution_correction, build_model_price,
    build_snapshot_view, build_usage_record, validate_stored_snapshot, CostViewerScope,
    StoredSubscriptionUsageSnapshot, SubscriptionCredentialPresence, UsageBuildMeta,
};
use crate::error::OfficeError;
use crate::organization::records::{Employee, EmployeeKind, EmployeeStatus, ProviderAuthMode};
use crate::time::format_rfc3339_ms;

use super::Office;

static UUID_SEQ: AtomicU64 = AtomicU64::new(1);
static SUBSCRIPTION_ATTEMPT_SEQ: AtomicU64 = AtomicU64::new(1);
const MAX_LIST_RESPONSE_BYTES: usize = 512 * 1024;

impl Office {
    /// Mints a restart-safe monotonic attempt generation seeded above persisted snapshot sequences.
    pub fn next_subscription_attempt_seq(&self) -> u64 {
        if let Ok(snaps) = self.accounting.list_subscription_usage(&self.info.org_id) {
            let max_stored = snaps.iter().map(|s| s.attempt_seq).max().unwrap_or(0);
            if max_stored > 0 {
                SUBSCRIPTION_ATTEMPT_SEQ.fetch_max(max_stored.saturating_add(1), Ordering::SeqCst);
            }
        }
        SUBSCRIPTION_ATTEMPT_SEQ.fetch_add(1, Ordering::SeqCst)
    }
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

    /// Ensures `actor` is the active organization owner (used by owner-only subscription usage APIs).
    pub fn require_owner_member(
        &self,
        actor: &str,
    ) -> Result<Result<Employee, ApiError>, OfficeError> {
        require_owner_for(self, actor)
    }

    /// Persists one validated office-local subscription usage snapshot (`TASK-79`).
    pub fn save_subscription_usage_snapshot(
        &self,
        snapshot: StoredSubscriptionUsageSnapshot,
    ) -> Result<Result<(), ApiError>, OfficeError> {
        op_save_subscription_usage(self, snapshot)
    }

    /// Reads one persisted subscription usage snapshot by `connection_id` (`TASK-79`).
    pub fn get_subscription_usage_snapshot(
        &self,
        connection_id: &str,
    ) -> Result<Option<StoredSubscriptionUsageSnapshot>, OfficeError> {
        self.accounting
            .get_subscription_usage(&self.info.org_id, connection_id)
    }

    /// `cost.subscription_usage.list` without live vault metadata (fallback when no session manager).
    pub fn cost_subscription_usage_list(
        &self,
        actor: &str,
        params: CostSubscriptionUsageListParams,
    ) -> Result<Result<CostSubscriptionUsageListResult, ApiError>, OfficeError> {
        op_subscription_usage_list(self, actor, params, |_| {
            SubscriptionCredentialPresence::default()
        })
    }

    /// `cost.subscription_usage.list` enriched with live private credential presence (`TASK-79`).
    pub fn subscription_usage_list_with_credentials(
        &self,
        actor: &str,
        params: CostSubscriptionUsageListParams,
        cred_lookup: impl Fn(&str) -> SubscriptionCredentialPresence,
    ) -> Result<Result<CostSubscriptionUsageListResult, ApiError>, OfficeError> {
        op_subscription_usage_list(self, actor, params, cred_lookup)
    }

    /// Returns `connection_id -> collected_at` for persisted snapshots (used to prioritize oldest targets).
    pub fn subscription_usage_collected_at_map(
        &self,
    ) -> Result<std::collections::BTreeMap<String, String>, OfficeError> {
        Ok(self
            .accounting
            .list_subscription_usage(&self.info.org_id)?
            .into_iter()
            .map(|s| (s.connection_id, s.collected_at))
            .collect())
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
        super::budgets::op_reevaluate_for_record(office, &existing)?;
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
    super::budgets::op_reevaluate_for_record(office, &saved)?;
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
    // Enforce actor and scope authorization BEFORE replay lookup, while deferring the
    // reached-budget guard until after exact-replay lookup under `budget_op_lock` (`Pass 10 #2/#3`).
    if let Err(e) = fill_correction_target_fields(office, &parent, &mut params)? {
        return Ok(Err(e));
    }
    if let Err(e) = check_correction_scope_auth(&viewer, &parent, &params) {
        return Ok(Err(e));
    }
    let _budget_guard = office.lock_budget_ops();
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
            super::budgets::op_reevaluate_for_record(office, &parent)?;
            super::budgets::op_reevaluate_for_record(office, &existing)?;
            return Ok(Ok(CostUsageCorrectResult { record: existing }));
        }
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "operation_id reused with different correction payload",
        )));
    }
    if let Err(e) = check_reached_task_correction_guard_locked(office, &caller, &parent, &params)? {
        return Ok(Err(e));
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
    let (saved, _replayed) = match office.accounting.append_usage(&office.info.org_id, corr)? {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    super::budgets::op_reevaluate_for_record(office, &parent)?;
    super::budgets::op_reevaluate_for_record(office, &saved)?;
    Ok(Ok(CostUsageCorrectResult { record: saved }))
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

fn check_reached_task_correction_guard_locked(
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
    if params.task_id.as_deref() != Some(old_task)
        && super::budgets::op_is_task_reached(office, old_task)?
    {
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

fn require_owner_for(
    office: &Office,
    actor: &str,
) -> Result<Result<Employee, ApiError>, OfficeError> {
    let caller = match require_active_member_for(office, actor)? {
        Ok(emp) => emp,
        Err(e) => return Ok(Err(e)),
    };
    if caller.id.as_str() != office.owner.as_str() {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the organization owner can access subscription usage data",
        )));
    }
    Ok(Ok(caller))
}

fn op_save_subscription_usage(
    office: &Office,
    snapshot: StoredSubscriptionUsageSnapshot,
) -> Result<Result<(), ApiError>, OfficeError> {
    if snapshot.organization_id != office.info.org_id || snapshot.office_id != office.info.office_id
    {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "subscription usage snapshot organization or office mismatch",
        )));
    }
    if let Err(err) = validate_stored_snapshot(&snapshot) {
        return Ok(Err(err));
    }
    office
        .accounting
        .save_subscription_usage(&office.info.org_id, snapshot)?;
    Ok(Ok(()))
}

fn op_subscription_usage_list(
    office: &Office,
    actor: &str,
    params: CostSubscriptionUsageListParams,
    cred_lookup: impl Fn(&str) -> SubscriptionCredentialPresence,
) -> Result<Result<CostSubscriptionUsageListResult, ApiError>, OfficeError> {
    if let Err(e) = require_owner_for(office, actor)? {
        return Ok(Err(e));
    }
    let conns = office.load_provider_connections()?;
    let stored_map: std::collections::BTreeMap<String, StoredSubscriptionUsageSnapshot> = office
        .accounting
        .list_subscription_usage(&office.info.org_id)?
        .into_iter()
        .map(|s| (s.connection_id.clone(), s))
        .collect();
    let now_ms = office.current_time_ms();
    let provider_filter = params
        .provider_type
        .as_deref()
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty());

    let mut all_snapshots = Vec::new();
    let mut collected_as_of: Option<String> = None;
    for conn in &conns {
        if conn.auth_mode != ProviderAuthMode::SubscriptionCli {
            continue;
        }
        if !params.include_archived && conn.archived {
            continue;
        }
        if let Some(ref cid) = params.connection_id {
            let matches = if org_proto::is_uuid_v7(cid) {
                conn.id.as_str() == cid
            } else {
                conn.key == *cid
            };
            if !matches {
                continue;
            }
        }
        if let Some(ref pf) = provider_filter {
            if conn.provider_type.to_ascii_lowercase() != *pf {
                continue;
            }
        }
        let stored = stored_map.get(conn.id.as_str());
        let cred = cred_lookup(conn.id.as_str());
        let view = build_snapshot_view(conn, &office.info.office_id, stored, &cred, now_ms);
        if let Some(ref at) = view.collected_at {
            if collected_as_of
                .as_deref()
                .is_none_or(|cur| at.as_str() > cur)
            {
                collected_as_of = Some(at.clone());
            }
        }
        all_snapshots.push(view);
    }
    all_snapshots.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
    let rollup = aggregate_totals_by_model(&all_snapshots);
    let (totals_by_model, next_totals_cursor, totals_bytes) =
        paginate_totals(rollup.totals_by_model, params.totals_cursor.as_deref());
    let (snapshots, next_cursor) = paginate_snapshots(
        all_snapshots,
        params.cursor.as_deref(),
        params.limit,
        totals_bytes,
    );
    Ok(Ok(CostSubscriptionUsageListResult {
        snapshots,
        totals_by_model,
        collected_as_of,
        next_cursor,
        next_totals_cursor,
        partial_totals: rollup.partial_totals,
        excluded_snapshot_count: rollup.excluded_snapshot_count,
        is_api_equivalent_estimate: false,
    }))
}

const MAX_TOTALS_RESPONSE_BYTES: usize = 256 * 1024;

fn paginate_totals(
    all: Vec<org_proto::costs::SubscriptionModelUsageView>,
    cursor: Option<&str>,
) -> (
    Vec<org_proto::costs::SubscriptionModelUsageView>,
    Option<String>,
    usize,
) {
    let mut used = 0usize;
    let mut page = Vec::new();
    let mut has_more = false;
    for row in all {
        if let Some(cur) = cursor {
            if row.model.as_str() <= cur {
                continue;
            }
        }
        let item_bytes = serde_json::to_vec(&row).map_or(0, |v| v.len()) + 16;
        if !page.is_empty() && used.saturating_add(item_bytes) > MAX_TOTALS_RESPONSE_BYTES {
            has_more = true;
            break;
        }
        used = used.saturating_add(item_bytes);
        page.push(row);
    }
    let next = has_more
        .then(|| page.last().map(|r| r.model.clone()))
        .flatten();
    (page, next, used)
}

fn paginate_snapshots(
    all: Vec<org_proto::costs::SubscriptionUsageSnapshotView>,
    cursor: Option<&str>,
    limit: Option<u32>,
    totals_bytes: usize,
) -> (
    Vec<org_proto::costs::SubscriptionUsageSnapshotView>,
    Option<String>,
) {
    let cap = limit.map_or(100, |n| (n as usize).clamp(1, 100));
    let mut used_bytes = totals_bytes.saturating_add(4096);
    let mut page = Vec::new();
    let mut has_more = false;
    for snap in all {
        if let Some(cur) = cursor {
            if snap.connection_id.as_str() <= cur {
                continue;
            }
        }
        let item_bytes = serde_json::to_vec(&snap).map_or(0, |v| v.len()) + 16;
        if page.len() >= cap
            || (!page.is_empty() && used_bytes.saturating_add(item_bytes) > MAX_LIST_RESPONSE_BYTES)
        {
            has_more = true;
            break;
        }
        used_bytes = used_bytes.saturating_add(item_bytes);
        page.push(snap);
    }
    let next_cursor = has_more
        .then(|| page.last().map(|s| s.connection_id.clone()))
        .flatten();
    (page, next_cursor)
}
