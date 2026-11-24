//! Office facade for `L4` provider connection pools, office-local bindings, per-connection
//! model discovery, readiness, and dual-authority quota permits & cooldowns (`TASK-54`, `FC-098`,
//! `FC-099`, `doc-88`, `doc-89`).
//!
//! `impl Office` holds thin 1-line delegators to module-level functions to stay modular.

use std::collections::BTreeMap;

use org_proto::hiring::EntityOpResult;
use org_proto::providers::{
    AgentReadinessCode, AgentReadinessGetParams, AgentReadinessResult, BoundConnectionSummary,
    ConnectionCandidateStatus, ConnectionQuotaStatus, ConnectionReadinessStatus,
    DiscoveredModelEntitlement, ModelProfileCreateParams, ModelProfileModelsParams,
    ModelProfileModelsResult, ModelProfileUpdateParams, PermitAuthority, ProfileModelEntry,
    ProviderBindingListParams, ProviderBindingListResult, ProviderBindingSetParams,
    ProviderBindingSetResult, ProviderConnectionListParams, ProviderConnectionListResult,
    ProviderConnectionRefreshParams, ProviderConnectionRefreshResult,
    ProviderConnectionUpdateParams, ProviderConnectionView, ProviderCooldownRecordParams,
    ProviderCooldownRecordResult, ProviderQuotaStatusParams, ProviderQuotaStatusResult,
    TurnPermitState,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Actor, Id, Sha256};

use crate::error::OfficeError;
use crate::organization::features::provider_pools::{
    CreateModelProfile, UpdateModelProfile, UpdateProviderConnection,
};
use crate::organization::features::Feature;
use crate::organization::records::{ModelProfile, ProviderAuthMode, ProviderConnection};
use crate::providers::{
    quota_scope_key, ConnectionQuotaState, OfficeConnectionBinding, OfficeConnectionModel,
    OfficeConnectionState, PermitAcquireOutcome, PermitAcquireRequest, DEFAULT_COOLDOWN_SECS,
    MAX_OBSERVATION_TTL_SECS, PERMIT_LEASE_MS,
};
use crate::store::open::{ModelProfilesList, OfficesCount, ProviderConnectionsList, StructureOp};
use crate::time::format_rfc3339_ms;

use super::Office;

impl Office {
    pub fn load_provider_connections(&self) -> Result<Vec<ProviderConnection>, OfficeError> {
        Ok(self
            .store
            .read(ProviderConnectionsList)
            .map_err(OfficeError::storage)?
            .0)
    }

    pub fn load_model_profiles(&self) -> Result<Vec<ModelProfile>, OfficeError> {
        Ok(self
            .store
            .read(ModelProfilesList)
            .map_err(OfficeError::storage)?
            .0)
    }

    pub fn provider_connection_list(
        &self,
        actor: &str,
        params: ProviderConnectionListParams,
    ) -> Result<Result<ProviderConnectionListResult, ApiError>, OfficeError> {
        op_connection_list(self, actor, params)
    }

    pub fn provider_connection_update(
        &self,
        actor: &str,
        params: ProviderConnectionUpdateParams,
    ) -> Result<Result<EntityOpResult, ApiError>, OfficeError> {
        op_connection_update(self, actor, params)
    }

    pub fn model_profile_create(
        &self,
        actor: &str,
        params: ModelProfileCreateParams,
    ) -> Result<Result<EntityOpResult, ApiError>, OfficeError> {
        op_profile_create(self, actor, params)
    }

    pub fn model_profile_update(
        &self,
        actor: &str,
        params: ModelProfileUpdateParams,
    ) -> Result<Result<EntityOpResult, ApiError>, OfficeError> {
        op_profile_update(self, actor, params)
    }

    pub fn provider_binding_set(
        &self,
        actor: &str,
        params: ProviderBindingSetParams,
    ) -> Result<Result<ProviderBindingSetResult, ApiError>, OfficeError> {
        op_binding_set(self, actor, params)
    }

    pub fn provider_binding_list(
        &self,
        actor: &str,
        params: ProviderBindingListParams,
    ) -> Result<Result<ProviderBindingListResult, ApiError>, OfficeError> {
        op_binding_list(self, actor, params)
    }

    pub fn provider_connection_refresh(
        &self,
        actor: &str,
        params: ProviderConnectionRefreshParams,
    ) -> Result<Result<ProviderConnectionRefreshResult, ApiError>, OfficeError> {
        op_connection_refresh(self, actor, params)
    }

    pub fn model_profile_models(
        &self,
        actor: &str,
        params: ModelProfileModelsParams,
    ) -> Result<Result<ModelProfileModelsResult, ApiError>, OfficeError> {
        op_profile_models(self, actor, params)
    }

    pub fn agent_readiness_get(
        &self,
        actor: &str,
        params: AgentReadinessGetParams,
    ) -> Result<Result<AgentReadinessResult, ApiError>, OfficeError> {
        op_agent_readiness(self, actor, params)
    }

    pub fn provider_quota_status(
        &self,
        actor: &str,
        params: ProviderQuotaStatusParams,
    ) -> Result<Result<ProviderQuotaStatusResult, ApiError>, OfficeError> {
        op_quota_status(self, actor, params)
    }

    pub fn provider_cooldown_record(
        &self,
        actor: &str,
        params: ProviderCooldownRecordParams,
    ) -> Result<Result<ProviderCooldownRecordResult, ApiError>, OfficeError> {
        op_cooldown_record(self, actor, params)
    }

    // ---- Internal supervisor APIs (`org-office-sessions`) ----

    /// Acquires a turn permit on `connection_id` under `authority` (`hq_pool` or `office_local`).
    /// When `is_failover_switch == false`, checks unreleased budget holds first (`doc-88` §6 step 3);
    /// when `is_failover_switch == true`, skips budget holds so a running turn can fail over cleanly.
    pub fn acquire_turn_permit(
        &self,
        connection_id: &str,
        employee_id: &str,
        task_id: Option<&str>,
        model_id: &str,
        authority: PermitAuthority,
        is_failover_switch: bool,
    ) -> Result<Result<PermitAcquireOutcome, ApiError>, OfficeError> {
        op_acquire_turn_permit(
            self,
            connection_id,
            employee_id,
            task_id,
            model_id,
            authority,
            is_failover_switch,
        )
    }

    /// Renews an active turn permit's lease **in memory only** (zero durable/SQLite writes; `TASK-2`).
    pub fn renew_turn_permit(
        &self,
        permit_id: &str,
        coordinator_generation: u64,
    ) -> Result<Result<u64, ApiError>, OfficeError> {
        self.providers.renew_permit_in_memory(
            permit_id,
            coordinator_generation,
            self.monotonic_now_ms(),
        )
    }

    pub fn mark_turn_permit_release_pending(&self, permit_id: &str) -> Result<bool, OfficeError> {
        self.providers.mark_permit_release_pending(permit_id)
    }

    pub fn release_turn_permit(&self, permit_id: &str) -> Result<bool, OfficeError> {
        self.providers.release_permit(permit_id)
    }

    pub fn record_connection_cooldown_internal(
        &self,
        connection_id: &str,
        model_id: Option<&str>,
        cooldown_until: Option<&str>,
        retry_after_seconds: Option<u64>,
    ) -> Result<ConnectionQuotaState, OfficeError> {
        record_cooldown_inner(
            self,
            connection_id,
            model_id,
            cooldown_until,
            retry_after_seconds,
        )
    }

    pub fn load_connection_binding(
        &self,
        connection_id: &str,
    ) -> Result<Option<OfficeConnectionBinding>, OfficeError> {
        self.providers
            .get_binding(&self.info.org_id, &self.info.office_id, connection_id)
    }

    pub fn load_connection_state(
        &self,
        connection_id: &str,
    ) -> Result<Option<OfficeConnectionState>, OfficeError> {
        self.providers
            .get_state(&self.info.org_id, &self.info.office_id, connection_id)
    }

    pub fn load_connection_models(
        &self,
        connection_id: &str,
    ) -> Result<Vec<OfficeConnectionModel>, OfficeError> {
        self.providers
            .list_models(&self.info.org_id, &self.info.office_id, connection_id)
    }

    pub fn mark_connection_auth_expired_internal(
        &self,
        connection_id: &str,
        issue_code: &str,
    ) -> Result<(), OfficeError> {
        let Some(binding) = self.load_connection_binding(connection_id)? else {
            return Ok(());
        };
        let now_utc = format_rfc3339_ms(self.monotonic_now_ms());
        let state = OfficeConnectionState {
            organization_id: self.info.org_id.clone(),
            office_id: self.info.office_id.clone(),
            connection_id: connection_id.to_owned(),
            status: ConnectionReadinessStatus::AuthExpired,
            binding_revision: binding.revision,
            driver_version: None,
            observed_at: Some(now_utc),
            valid_until: None,
            issue_code: Some(issue_code.to_owned()),
        };
        self.providers.save_observation(&state, &[])
    }

    pub fn provider_durable_write_count(&self) -> u64 {
        self.providers.durable_write_count()
    }

    /// Current monotonic office timestamp in milliseconds (uses the office's configured clock).
    pub fn current_time_ms(&self) -> u64 {
        self.monotonic_now_ms()
    }

    /// Returns `true` when this organization has more than one registered office (`doc-88` §6).
    pub fn is_multi_office(&self) -> Result<bool, OfficeError> {
        Ok(self
            .store
            .read(OfficesCount)
            .map_err(OfficeError::storage)?
            .0
            > 1)
    }
}

// ---- Implementation functions ----

fn require_owner(office: &Office, actor: &str) -> Result<Result<Id, ApiError>, OfficeError> {
    let caller = match office.require_active_member(actor)? {
        Ok(e) => e,
        Err(e) => return Ok(Err(e)),
    };
    if caller.id.as_str() != office.owner.as_str() {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the organization owner can manage provider connections and pools",
        )));
    }
    Ok(Ok(caller.id))
}

fn parse_uuid(raw: &str, label: &str) -> Result<Id, ApiError> {
    if !org_proto::is_uuid_v7(raw) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("{label} must be a canonical lowercase UUIDv7"),
        ));
    }
    Id::parse(raw).map_err(|_| ApiError::new(ErrorCode::InvalidRequest, format!("invalid {label}")))
}

fn run_org_feature<F: Feature>(
    office: &Office,
    command_id: &str,
    actor: Id,
    command: F,
) -> Result<Result<EntityOpResult, ApiError>, OfficeError> {
    let op_id = match parse_uuid(command_id, "command_id") {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let reply = match office.store.run(StructureOp {
        operation_id: op_id,
        here: office.identity.clone(),
        owner: office.owner.clone(),
        by: Actor::Employee(actor),
        command,
    }) {
        Ok(r) => r,
        Err(e) => return Ok(Err(map_store_err(e))),
    };
    match reply.outcome {
        org_store::Outcome::Completed => {
            let changed: crate::organization::features::Changed =
                serde_json::from_slice(&reply.bytes).map_err(OfficeError::storage)?;
            Ok(Ok(EntityOpResult {
                id: changed.id.as_str().to_owned(),
                revision: changed.revision,
                generation: changed.generation,
            }))
        }
        org_store::Outcome::Rejected => {
            if let Ok(rej) =
                serde_json::from_slice::<crate::organization::features::OrgRejection>(&reply.bytes)
            {
                return Ok(Err(ApiError::new(map_reject_code(rej.code), rej.message)));
            }
            Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "operation rejected",
            )))
        }
    }
}

fn map_store_err(err: org_store::StoreError) -> ApiError {
    match err {
        org_store::StoreError::Conflict => ApiError::new(
            ErrorCode::OperationReused,
            "command_id reused with different payload",
        ),
        org_store::StoreError::Invalid(msg) => {
            if msg == crate::organization::features::NOT_AUTHORIZED {
                ApiError::new(ErrorCode::NotAuthorized, msg)
            } else {
                ApiError::new(ErrorCode::InvalidRequest, msg)
            }
        }
        other => ApiError::new(ErrorCode::StorageError, other.to_string()),
    }
}

fn map_reject_code(code: crate::organization::features::RejectCode) -> ErrorCode {
    use crate::organization::features::RejectCode;
    match code {
        RejectCode::NotFound => ErrorCode::NotFound,
        RejectCode::InvalidRequest => ErrorCode::InvalidRequest,
        RejectCode::InvalidName => ErrorCode::InvalidName,
        RejectCode::OwnerProtected => ErrorCode::OwnerProtected,
        RejectCode::Cycle => ErrorCode::Cycle,
        RejectCode::NotEmpty => ErrorCode::NotEmpty,
        RejectCode::Conflict => ErrorCode::Conflict,
        RejectCode::AlreadyExists => ErrorCode::AlreadyExists,
    }
}

fn connection_to_view(c: &ProviderConnection) -> ProviderConnectionView {
    ProviderConnectionView {
        id: c.id.as_str().to_owned(),
        revision: c.meta.revision,
        key: c.key.clone(),
        name: c.name.clone(),
        provider_type: c.provider_type.clone(),
        auth_mode: c.auth_mode.as_str().to_owned(),
        subscription_identity: c.subscription_identity.clone(),
        runtime_driver: c.runtime_driver.clone(),
        endpoint_url: c.endpoint_url.clone(),
        enabled: c.enabled,
        archived: c.archived,
        max_concurrent_turns: c.max_concurrent_turns,
        created_at: c.meta.created_at.clone().unwrap_or_default(),
        updated_at: c.meta.updated_at.clone().unwrap_or_default(),
    }
}

fn op_connection_list(
    office: &Office,
    actor: &str,
    params: ProviderConnectionListParams,
) -> Result<Result<ProviderConnectionListResult, ApiError>, OfficeError> {
    if let Err(e) = office.require_active_member(actor)? {
        return Ok(Err(e));
    }
    let conns = office
        .load_provider_connections()?
        .into_iter()
        .filter(|c| params.include_archived || !c.archived)
        .map(|c| connection_to_view(&c))
        .collect();
    Ok(Ok(ProviderConnectionListResult { connections: conns }))
}

fn op_connection_update(
    office: &Office,
    actor: &str,
    params: ProviderConnectionUpdateParams,
) -> Result<Result<EntityOpResult, ApiError>, OfficeError> {
    let owner_id = match require_owner(office, actor)? {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let conn_id = match parse_uuid(&params.connection_id, "connection_id") {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let cmd = UpdateProviderConnection {
        id: conn_id,
        expected_revision: params.expected_version,
        name: params.name,
        endpoint_url: params.endpoint_url,
        clear_endpoint_url: params.clear_endpoint_url,
        enabled: params.enabled,
        archived: params.archived,
        max_concurrent_turns: params.max_concurrent_turns,
    };
    run_org_feature(office, &params.command_id, owner_id, cmd)
}

fn op_profile_create(
    office: &Office,
    actor: &str,
    params: ModelProfileCreateParams,
) -> Result<Result<EntityOpResult, ApiError>, OfficeError> {
    let owner_id = match require_owner(office, actor)? {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let profile_id = match parse_uuid(&params.profile_id, "profile_id") {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let mut connection_ids = Vec::with_capacity(params.connection_ids.len());
    for raw in &params.connection_ids {
        match parse_uuid(raw, "connection_id") {
            Ok(id) => connection_ids.push(id),
            Err(e) => return Ok(Err(e)),
        }
    }
    let cmd = CreateModelProfile {
        id: profile_id,
        key: params.key,
        name: params.name,
        description: params.description,
        connection_ids,
    };
    run_org_feature(office, &params.command_id, owner_id, cmd)
}

fn op_profile_update(
    office: &Office,
    actor: &str,
    params: ModelProfileUpdateParams,
) -> Result<Result<EntityOpResult, ApiError>, OfficeError> {
    let owner_id = match require_owner(office, actor)? {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let profile_id = match parse_uuid(&params.profile_id, "profile_id") {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let connection_ids = match params.connection_ids {
        Some(raws) => {
            let mut out = Vec::with_capacity(raws.len());
            for raw in &raws {
                match parse_uuid(raw, "connection_id") {
                    Ok(id) => out.push(id),
                    Err(e) => return Ok(Err(e)),
                }
            }
            Some(out)
        }
        None => None,
    };
    let cmd = UpdateModelProfile {
        id: profile_id,
        expected_revision: params.expected_version,
        name: params.name,
        description: params.description,
        archived: params.archived,
        connection_ids,
    };
    run_org_feature(office, &params.command_id, owner_id, cmd)
}

fn validate_binding_set_params(params: &ProviderBindingSetParams) -> Result<(), ApiError> {
    if !org_proto::is_uuid_v7(&params.operation_id) || !org_proto::is_uuid_v7(&params.connection_id)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "operation_id and connection_id must be canonical lowercase UUIDv7",
        ));
    }
    if !crate::organization::records::is_line(&params.auth_context_ref)
        || params.auth_context_ref.len() > 512
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "auth_context_ref must be a non-empty single line",
        ));
    }
    if params
        .runtime_binary_path
        .as_deref()
        .is_some_and(|v| !crate::organization::records::is_line(v) || v.len() > 1024)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "runtime_binary_path must be a valid single line",
        ));
    }
    if params
        .account_identity_ref
        .as_deref()
        .is_some_and(|v| !crate::organization::records::is_line(v) || v.len() > 512)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "account_identity_ref must be a valid single line",
        ));
    }
    Ok(())
}

fn op_binding_set(
    office: &Office,
    actor: &str,
    params: ProviderBindingSetParams,
) -> Result<Result<ProviderBindingSetResult, ApiError>, OfficeError> {
    if let Err(e) = require_owner(office, actor)? {
        return Ok(Err(e));
    }
    if let Err(e) = validate_binding_set_params(&params) {
        return Ok(Err(e));
    }
    let Some(conn) = office.load_provider_connection(&params.connection_id)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "provider connection not found",
        )));
    };
    if conn.archived {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "cannot bind an archived provider connection",
        )));
    }
    let req_hash = hex_sha256(
        &serde_json::to_vec(&serde_json::json!({
            "connection_id": params.connection_id,
            "expected_revision": params.expected_revision,
            "auth_context_ref": params.auth_context_ref,
            "runtime_binary_path": params.runtime_binary_path,
            "account_identity_ref": params.account_identity_ref,
        }))
        .unwrap_or_default(),
    );
    let existing = office.providers.get_binding(
        &office.info.org_id,
        &office.info.office_id,
        &params.connection_id,
    )?;
    if let Some(ref cur) = existing {
        if cur.last_operation_id.as_deref() == Some(params.operation_id.as_str()) {
            if cur.last_operation_hash.as_deref() == Some(req_hash.as_str()) {
                return Ok(Ok(ProviderBindingSetResult {
                    binding: cur.to_view(),
                    replayed: true,
                }));
            }
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "operation_id reused with different binding payload",
            )));
        }
        if let Some(exp) = params.expected_revision {
            if cur.revision != exp {
                return Ok(Err(ApiError::new(
                    ErrorCode::Conflict,
                    "stale binding revision",
                )));
            }
        }
    } else if params.expected_revision.is_some_and(|r| r != 0) {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "binding does not exist yet",
        )));
    }
    let next_rev = existing
        .as_ref()
        .map_or(1, |b| b.revision.saturating_add(1));
    let binding = OfficeConnectionBinding {
        organization_id: office.info.org_id.clone(),
        office_id: office.info.office_id.clone(),
        connection_id: params.connection_id,
        revision: next_rev,
        auth_context_ref: params.auth_context_ref,
        runtime_binary_path: params.runtime_binary_path,
        account_identity_ref: params.account_identity_ref,
        updated_at: format_rfc3339_ms(office.monotonic_now_ms()),
        last_operation_id: Some(params.operation_id),
        last_operation_hash: Some(req_hash),
    };
    office.providers.save_binding_and_invalidate(&binding)?;
    Ok(Ok(ProviderBindingSetResult {
        binding: binding.to_view(),
        replayed: false,
    }))
}

fn op_binding_list(
    office: &Office,
    actor: &str,
    params: ProviderBindingListParams,
) -> Result<Result<ProviderBindingListResult, ApiError>, OfficeError> {
    if let Err(e) = require_owner(office, actor)? {
        return Ok(Err(e));
    }
    let now_ms = office.monotonic_now_ms();
    let mut bindings = office
        .providers
        .list_bindings(&office.info.org_id, &office.info.office_id)?;
    if let Some(ref cid) = params.connection_id {
        bindings.retain(|b| b.connection_id == *cid);
    }
    let mut out = Vec::with_capacity(bindings.len());
    for b in bindings {
        let state = office
            .providers
            .get_state(
                &office.info.org_id,
                &office.info.office_id,
                &b.connection_id,
            )?
            .map(|s| s.to_view(b.revision, now_ms));
        let models = office
            .providers
            .list_models(
                &office.info.org_id,
                &office.info.office_id,
                &b.connection_id,
            )?
            .into_iter()
            .map(|m| m.to_view(now_ms))
            .collect();
        out.push(BoundConnectionSummary {
            binding: b.to_view(),
            state,
            models,
        });
    }
    Ok(Ok(ProviderBindingListResult { bindings: out }))
}

fn op_connection_refresh(
    office: &Office,
    actor: &str,
    params: ProviderConnectionRefreshParams,
) -> Result<Result<ProviderConnectionRefreshResult, ApiError>, OfficeError> {
    if let Err(e) = require_owner(office, actor)? {
        return Ok(Err(e));
    }
    let Some(conn) = office.load_provider_connection(&params.connection_id)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "provider connection not found",
        )));
    };
    let Some(binding) = office.providers.get_binding(
        &office.info.org_id,
        &office.info.office_id,
        &params.connection_id,
    )?
    else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "office has no local binding for this connection",
        )));
    };
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    let ttl_secs = params
        .ttl_seconds
        .unwrap_or(MAX_OBSERVATION_TTL_SECS)
        .clamp(1, MAX_OBSERVATION_TTL_SECS);
    let valid_until = format_rfc3339_ms(now_ms.saturating_add(ttl_secs * 1000));

    let (status, issue_code) = evaluate_refresh_status(&conn, &binding, &params);
    let state = OfficeConnectionState {
        organization_id: office.info.org_id.clone(),
        office_id: office.info.office_id.clone(),
        connection_id: conn.id.as_str().to_owned(),
        status,
        binding_revision: binding.revision,
        driver_version: params.driver_version.clone(),
        observed_at: Some(now_utc.clone()),
        valid_until: (status == ConnectionReadinessStatus::Ready).then(|| valid_until.clone()),
        issue_code,
    };
    let mut models = Vec::new();
    let mut seen_models = std::collections::BTreeSet::new();
    if status == ConnectionReadinessStatus::Ready {
        for m in params.models {
            if m.model_id.trim().is_empty() || m.driver_model_id.trim().is_empty() {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "discovered model_id and driver_model_id must be non-empty",
                )));
            }
            if !seen_models.insert(m.model_id.clone()) {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "duplicate model_id in refresh payload",
                )));
            }
            let caps_ok = serde_json::from_str::<serde_json::Value>(&m.capabilities_json)
                .is_ok_and(|v| v.is_object());
            if !caps_ok {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "capabilities_json must be a JSON object",
                )));
            }
            models.push(OfficeConnectionModel {
                organization_id: office.info.org_id.clone(),
                office_id: office.info.office_id.clone(),
                connection_id: conn.id.as_str().to_owned(),
                model_id: m.model_id,
                driver_model_id: m.driver_model_id,
                entitlement: m.entitlement,
                capabilities_json: m.capabilities_json,
                observed_at: now_utc.clone(),
                valid_until: valid_until.clone(),
            });
        }
    }
    office.providers.save_observation(&state, &models)?;
    Ok(Ok(ProviderConnectionRefreshResult {
        state: state.to_view(binding.revision, now_ms),
        models: models.into_iter().map(|m| m.to_view(now_ms)).collect(),
    }))
}

fn evaluate_refresh_status(
    conn: &ProviderConnection,
    binding: &OfficeConnectionBinding,
    params: &ProviderConnectionRefreshParams,
) -> (ConnectionReadinessStatus, Option<String>) {
    if let Some(ref issue) = params.issue_code {
        return (ConnectionReadinessStatus::Unavailable, Some(issue.clone()));
    }
    match params.auth_signed_in {
        Some(false) => {
            return (
                ConnectionReadinessStatus::AuthExpired,
                Some("AUTH_EXPIRED".into()),
            );
        }
        None => {
            return (
                ConnectionReadinessStatus::Unknown,
                Some("AUTH_UNVERIFIED".into()),
            );
        }
        Some(true) => {}
    }
    // `doc-88` §4: compare subscription account evidence against registered subscription_identity!
    if conn.auth_mode == ProviderAuthMode::SubscriptionCli {
        let expected = conn.subscription_identity.as_deref();
        let observed = params
            .observed_subscription_identity
            .as_deref()
            .or(binding.account_identity_ref.as_deref());
        match (expected, observed, params.operator_attested_subscription) {
            (Some(exp), Some(obs), _) if obs != exp => {
                return (
                    ConnectionReadinessStatus::Unavailable,
                    Some("SUBSCRIPTION_IDENTITY_MISMATCH".into()),
                );
            }
            (Some(_), None, false) => {
                return (
                    ConnectionReadinessStatus::Unavailable,
                    Some("SUBSCRIPTION_IDENTITY_UNVERIFIED".into()),
                );
            }
            _ => {}
        }
    }
    if params
        .driver_version
        .as_deref()
        .is_none_or(|v| v.trim().is_empty())
    {
        return (
            ConnectionReadinessStatus::Unavailable,
            Some("DRIVER_VERSION_UNKNOWN".into()),
        );
    }
    (ConnectionReadinessStatus::Ready, None)
}

fn op_profile_models(
    office: &Office,
    actor: &str,
    params: ModelProfileModelsParams,
) -> Result<Result<ModelProfileModelsResult, ApiError>, OfficeError> {
    if let Err(e) = office.require_active_member(actor)? {
        return Ok(Err(e));
    }
    let Some(profile) = office.load_model_profile(&params.model_profile_id)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "model profile not found",
        )));
    };
    let now_ms = office.monotonic_now_ms();
    let mut entries: BTreeMap<String, ProfileModelEntry> = BTreeMap::new();
    for (idx, cid) in profile.connection_ids.iter().enumerate() {
        let priority = (idx as u64) + 1;
        let conn = office.load_provider_connection(cid.as_str())?;
        let enabled = conn.as_ref().is_some_and(|c| c.enabled && !c.archived);
        let binding = office.load_connection_binding(cid.as_str())?;
        let state = office.load_connection_state(cid.as_str())?;
        let conn_ready = enabled
            && binding.as_ref().is_some_and(|b| {
                state.as_ref().is_some_and(|s| {
                    s.status == ConnectionReadinessStatus::Ready && !s.is_stale(b.revision, now_ms)
                })
            });
        for m in office.load_connection_models(cid.as_str())? {
            if m.entitlement != DiscoveredModelEntitlement::Confirmed {
                continue;
            }
            let model_stale = m.is_stale(now_ms);
            let ready_inc = u64::from(conn_ready && !model_stale);
            let entry = entries
                .entry(m.model_id.clone())
                .or_insert_with(|| ProfileModelEntry {
                    model_id: m.model_id.clone(),
                    supporting_connections: Vec::new(),
                    first_priority: priority,
                    as_of: Some(m.observed_at.clone()),
                    stale: model_stale,
                    ready_connections: 0,
                });
            entry.supporting_connections.push(cid.as_str().to_owned());
            entry.first_priority = entry.first_priority.min(priority);
            entry.stale = entry.stale && model_stale;
            entry.ready_connections += ready_inc;
            if entry.as_of.as_deref() < Some(m.observed_at.as_str()) {
                entry.as_of = Some(m.observed_at);
            }
        }
    }
    Ok(Ok(ModelProfileModelsResult {
        model_profile_id: profile.id.as_str().to_owned(),
        models: entries.into_values().collect(),
    }))
}

fn op_agent_readiness(
    office: &Office,
    actor: &str,
    params: AgentReadinessGetParams,
) -> Result<Result<AgentReadinessResult, ApiError>, OfficeError> {
    if let Err(e) = authorize_agent_readiness(office, actor, &params)? {
        return Ok(Err(e));
    }
    let Some(exec) = office.load_agent_execution(&params.employee_id)? else {
        return Ok(Ok(empty_readiness(
            &params.employee_id,
            AgentReadinessCode::NoExecutionConfig,
        )));
    };
    if !exec.enabled {
        return Ok(Ok(AgentReadinessResult {
            employee_id: params.employee_id,
            ready: false,
            status: AgentReadinessCode::ExecutionDisabled,
            model_profile_id: Some(exec.model_profile_id.as_str().to_owned()),
            model_id: Some(exec.model_id),
            candidates: Vec::new(),
            earliest_cooldown_until: None,
        }));
    }
    let Some(profile) = office.load_model_profile(exec.model_profile_id.as_str())? else {
        return Ok(Ok(empty_readiness(
            &params.employee_id,
            AgentReadinessCode::ProfileArchived,
        )));
    };
    if profile.archived {
        return Ok(Ok(AgentReadinessResult {
            employee_id: params.employee_id,
            ready: false,
            status: AgentReadinessCode::ProfileArchived,
            model_profile_id: Some(profile.id.as_str().to_owned()),
            model_id: Some(exec.model_id),
            candidates: Vec::new(),
            earliest_cooldown_until: None,
        }));
    }
    let holds = office.active_budget_holds(Some(&params.employee_id), params.task_id.as_deref())?;
    let budget_held = !holds.is_empty();
    let now_ms = office.monotonic_now_ms();
    let snapshot = office
        .providers
        .quota_snapshot(&office.info.org_id, None, now_ms)?;
    let eval = evaluate_pool_candidates(office, &profile, &exec.model_id, &snapshot, now_ms)?;

    let (ready, status) = if budget_held {
        (false, AgentReadinessCode::BudgetHeld)
    } else if eval.any_ready {
        (true, AgentReadinessCode::Ready)
    } else if eval.any_exhausted {
        (false, AgentReadinessCode::PoolExhausted)
    } else if eval.any_unverified {
        (false, AgentReadinessCode::ModelUnverified)
    } else {
        (false, AgentReadinessCode::NoReadyConnection)
    };
    Ok(Ok(AgentReadinessResult {
        employee_id: params.employee_id,
        ready,
        status,
        model_profile_id: Some(profile.id.as_str().to_owned()),
        model_id: Some(exec.model_id),
        candidates: eval.candidates,
        earliest_cooldown_until: eval.earliest_cooldown,
    }))
}

fn empty_readiness(emp_id: &str, status: AgentReadinessCode) -> AgentReadinessResult {
    AgentReadinessResult {
        employee_id: emp_id.to_owned(),
        ready: false,
        status,
        model_profile_id: None,
        model_id: None,
        candidates: Vec::new(),
        earliest_cooldown_until: None,
    }
}

struct PoolCandidateEvaluation {
    candidates: Vec<ConnectionCandidateStatus>,
    earliest_cooldown: Option<String>,
    any_ready: bool,
    any_unverified: bool,
    any_exhausted: bool,
}

fn evaluate_pool_candidates(
    office: &Office,
    profile: &ModelProfile,
    target_model: &str,
    snapshot: &crate::providers::PermitSnapshot,
    now_ms: u64,
) -> Result<PoolCandidateEvaluation, OfficeError> {
    let mut candidates = Vec::new();
    let mut earliest_cd: Option<String> = None;
    let mut any_ready = false;
    let mut any_unverified = false;
    let mut any_exhausted = false;

    for (idx, cid) in profile.connection_ids.iter().enumerate() {
        let priority = (idx as u64) + 1;
        let cand = evaluate_single_candidate(
            office,
            cid.as_str(),
            priority,
            target_model,
            snapshot,
            now_ms,
        )?;
        if cand.eligible {
            any_ready = true;
        } else {
            match cand.skip_reason.as_deref() {
                Some("MODEL_UNVERIFIED") => any_unverified = true,
                Some("COOLDOWN") | Some("CAPACITY_FULL") => any_exhausted = true,
                _ => {}
            }
        }
        if let Some(ref cd) = cand.cooldown_until {
            if earliest_cd.as_deref().is_none_or(|e| cd.as_str() < e) {
                earliest_cd = Some(cd.clone());
            }
        }
        candidates.push(cand);
    }
    Ok(PoolCandidateEvaluation {
        candidates,
        earliest_cooldown: earliest_cd,
        any_ready,
        any_unverified,
        any_exhausted,
    })
}

fn evaluate_single_candidate(
    office: &Office,
    cid: &str,
    priority: u64,
    target_model: &str,
    snapshot: &crate::providers::PermitSnapshot,
    now_ms: u64,
) -> Result<ConnectionCandidateStatus, OfficeError> {
    let Some(conn) = office.load_provider_connection(cid)? else {
        return Ok(skip_cand(cid, priority, "CONNECTION_NOT_FOUND", None));
    };
    if conn.archived || !conn.enabled {
        return Ok(skip_cand(cid, priority, "CONNECTION_DISABLED", None));
    }
    let Some(binding) = office.load_connection_binding(cid)? else {
        return Ok(skip_cand(cid, priority, "NOT_BOUND", None));
    };
    let Some(state) = office.load_connection_state(cid)? else {
        return Ok(skip_cand(cid, priority, "STATE_UNKNOWN", None));
    };
    if state.is_stale(binding.revision, now_ms) || state.status != ConnectionReadinessStatus::Ready
    {
        let code = if state.status == ConnectionReadinessStatus::Ready {
            "STATE_STALE".to_owned()
        } else {
            state
                .issue_code
                .unwrap_or_else(|| state.status.as_str().to_ascii_uppercase())
        };
        return Ok(skip_cand(cid, priority, &code, None));
    }
    let models = office.load_connection_models(cid)?;
    let matched = models.into_iter().find(|m| {
        m.model_id == target_model
            && m.entitlement == DiscoveredModelEntitlement::Confirmed
            && !m.is_stale(now_ms)
    });
    let Some(model) = matched else {
        return Ok(skip_cand(cid, priority, "MODEL_UNVERIFIED", None));
    };
    let cd = snapshot
        .cooldowns
        .iter()
        .filter(|q| {
            q.connection_id == cid
                && (q.model_id.is_none() || q.model_id.as_deref() == Some(target_model))
                && q.is_cooling_down(now_ms)
        })
        .filter_map(|q| q.cooldown_until.clone())
        .max();
    if let Some(until) = cd {
        return Ok(skip_cand(cid, priority, "COOLDOWN", Some(until)));
    }
    let occupied = snapshot
        .permits
        .iter()
        .filter(|p| p.connection_id == cid && p.state != TurnPermitState::Released)
        .count() as u64;
    if occupied >= conn.max_concurrent_turns {
        return Ok(skip_cand(cid, priority, "CAPACITY_FULL", None));
    }
    Ok(ConnectionCandidateStatus {
        connection_id: cid.to_owned(),
        priority,
        eligible: true,
        skip_reason: None,
        cooldown_until: None,
        driver_model_id: Some(model.driver_model_id),
    })
}

fn skip_cand(
    cid: &str,
    priority: u64,
    reason: &str,
    cooldown_until: Option<String>,
) -> ConnectionCandidateStatus {
    ConnectionCandidateStatus {
        connection_id: cid.to_owned(),
        priority,
        eligible: false,
        skip_reason: Some(reason.to_owned()),
        cooldown_until,
        driver_model_id: None,
    }
}

fn authorize_agent_readiness(
    office: &Office,
    actor: &str,
    params: &AgentReadinessGetParams,
) -> Result<Result<(), ApiError>, OfficeError> {
    if let Err(e) = office.require_active_member(actor)? {
        return Ok(Err(e));
    }
    if actor == office.owner_member_id() {
        return Ok(Ok(()));
    }
    let Some(emp) = office.load_employee(actor)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller not found",
        )));
    };
    if emp.kind == crate::organization::records::EmployeeKind::Person {
        return Ok(Ok(()));
    }
    if params.employee_id == actor && params.task_id.is_none() {
        return Ok(Ok(()));
    }
    Ok(Err(ApiError::new(
        ErrorCode::NotAuthorized,
        "agents may only inspect their own readiness without task_id probing",
    )))
}

fn op_quota_status(
    office: &Office,
    actor: &str,
    params: ProviderQuotaStatusParams,
) -> Result<Result<ProviderQuotaStatusResult, ApiError>, OfficeError> {
    if let Err(e) = require_owner(office, actor)? {
        return Ok(Err(e));
    }
    let snap = office.providers.quota_snapshot(
        &office.info.org_id,
        params.connection_id.as_deref(),
        office.monotonic_now_ms(),
    )?;
    let mut active = Vec::new();
    let mut release_pending = Vec::new();
    for p in snap.permits {
        match p.state {
            TurnPermitState::Active => active.push(p.to_view()),
            TurnPermitState::ReleasePending => release_pending.push(p.to_view()),
            TurnPermitState::Released | _ => {}
        }
    }
    Ok(Ok(ProviderQuotaStatusResult {
        coordinator_generation: snap.coordinator_generation,
        cooldowns: snap.cooldowns.iter().map(|c| c.to_view()).collect(),
        active_permits: active,
        release_pending_permits: release_pending,
    }))
}

fn op_cooldown_record(
    office: &Office,
    actor: &str,
    params: ProviderCooldownRecordParams,
) -> Result<Result<ProviderCooldownRecordResult, ApiError>, OfficeError> {
    if let Err(e) = require_owner(office, actor)? {
        return Ok(Err(e));
    }
    if office
        .load_provider_connection(&params.connection_id)?
        .is_none()
    {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "provider connection not found",
        )));
    }
    if let Some(ref until) = params.cooldown_until {
        if org_proto::parse_rfc3339_ms(until).is_none() {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "cooldown_until must be a valid RFC 3339 timestamp",
            )));
        }
    }
    let state = record_cooldown_inner(
        office,
        &params.connection_id,
        params.model_id.as_deref(),
        params.cooldown_until.as_deref(),
        params.retry_after_seconds,
    )?;
    Ok(Ok(ProviderCooldownRecordResult {
        quota: state.to_view(),
    }))
}

fn record_cooldown_inner(
    office: &Office,
    connection_id: &str,
    model_id: Option<&str>,
    cooldown_until: Option<&str>,
    retry_after_seconds: Option<u64>,
) -> Result<ConnectionQuotaState, OfficeError> {
    let now_ms = office.monotonic_now_ms();
    let until_ms = cooldown_until
        .and_then(org_proto::parse_rfc3339_ms)
        .unwrap_or_else(|| {
            let secs = retry_after_seconds.unwrap_or(DEFAULT_COOLDOWN_SECS).max(1);
            now_ms.saturating_add(secs.saturating_mul(1000))
        });
    let state = ConnectionQuotaState {
        organization_id: office.info.org_id.clone(),
        connection_id: connection_id.to_owned(),
        scope_key: quota_scope_key(model_id),
        model_id: model_id.map(str::to_owned),
        status: ConnectionQuotaStatus::Cooldown,
        cooldown_until: Some(format_rfc3339_ms(until_ms)),
        observed_at: format_rfc3339_ms(now_ms),
        observed_by_office_id: office.info.office_id.clone(),
        coordinator_generation: office.providers.coordinator_generation()?,
    };
    office.providers.record_cooldown(&state)?;
    Ok(state)
}

fn op_acquire_turn_permit(
    office: &Office,
    connection_id: &str,
    employee_id: &str,
    task_id: Option<&str>,
    model_id: &str,
    authority: PermitAuthority,
    is_failover_switch: bool,
) -> Result<Result<PermitAcquireOutcome, ApiError>, OfficeError> {
    // `doc-88` §6 step 3: serialize the final budget hold check with first-permit acquisition
    // under `budget_op_lock` so a concurrent budget hold cannot slip in between check and grant,
    // while preserving the §7 exemption for replacement switch permits on an already-running turn.
    let _budget_guard = (!is_failover_switch).then(|| office.lock_budget_ops());
    if !is_failover_switch {
        let holds = super::budgets::op_active_holds(office, Some(employee_id), task_id)?;
        if let Some(first) = holds.first() {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                format!("BUDGET_HELD: {}", first.budget_id),
            )));
        }
    }
    let Some(conn) = office.load_provider_connection(connection_id)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "provider connection not found",
        )));
    };
    if conn.archived || !conn.enabled {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "provider connection is disabled or archived",
        )));
    }
    let now_ms = office.monotonic_now_ms();
    let req = PermitAcquireRequest {
        organization_id: office.info.org_id.clone(),
        permit_id: office.next_uuid_v7(),
        connection_id: connection_id.to_owned(),
        office_id: office.info.office_id.clone(),
        employee_id: employee_id.to_owned(),
        placement_generation: 1,
        model_id: model_id.to_owned(),
        authority,
        max_concurrent_turns: conn.max_concurrent_turns,
        now_ms,
        issued_at: format_rfc3339_ms(now_ms),
        expires_at: format_rfc3339_ms(now_ms.saturating_add(PERMIT_LEASE_MS)),
    };
    Ok(Ok(office.providers.acquire_permit(req)?))
}

fn hex_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::of(bytes);
    let mut out = String::with_capacity(64);
    for &b in digest.as_bytes() {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}
