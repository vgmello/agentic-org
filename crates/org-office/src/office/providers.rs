//! Office facade for `L4` provider connection pools, office-local bindings, per-connection
//! model discovery, readiness, and dual-authority quota permits & cooldowns (`TASK-54`, `FC-098`,
//! `FC-099`, `doc-88`, `doc-89`).
//!
//! `impl Office` holds thin 1-line delegators to module-level functions to stay modular.

use org_proto::hiring::EntityOpResult;
use org_proto::providers::{
    AgentReadinessGetParams, AgentReadinessResult, ConnectionReadinessStatus,
    ModelProfileCreateParams, ModelProfileModelsParams, ModelProfileModelsResult,
    ModelProfileUpdateParams, ProviderBindingListParams, ProviderBindingListResult,
    ProviderBindingSetParams, ProviderBindingSetResult, ProviderConnectionListParams,
    ProviderConnectionListResult, ProviderConnectionRefreshParams, ProviderConnectionRefreshResult,
    ProviderConnectionUpdateParams, ProviderCooldownRecordParams, ProviderCooldownRecordResult,
    ProviderQuotaStatusParams, ProviderQuotaStatusResult,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Actor, Id};

use crate::error::OfficeError;
use crate::organization::features::Feature;
use crate::organization::records::{ModelProfile, ProviderConnection};
use crate::providers::{
    OfficeConnectionBinding, OfficeConnectionModel, OfficeConnectionState, MAX_OBSERVATION_TTL_SECS,
};
use crate::store::open::{ModelProfilesList, OfficesCount, ProviderConnectionsList, StructureOp};
use crate::time::format_rfc3339_ms;

use super::Office;

mod connections;
mod discovery;
mod permits;
mod readiness;

use connections::{
    op_binding_list, op_binding_set, op_connection_list, op_connection_update, op_profile_create,
    op_profile_update,
};
use discovery::{op_connection_refresh, op_profile_models};
pub use permits::OfficePermitSpec;
use permits::{op_cooldown_record, op_quota_status};
use readiness::op_agent_readiness;

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

    /// Atomically renews a locally bound `Ready` provider observation ONLY if the binding
    /// revision has not changed and the current readiness status is still `Ready`.
    /// Never promotes `AuthExpired`, `Unavailable`, or `Unknown` connections.
    pub fn renew_ready_provider_observation_if_unchanged(
        &self,
        connection_id: &str,
        expected_binding_revision: u64,
        driver_version: Option<String>,
        models: Vec<org_proto::providers::DiscoveredModelInput>,
        ttl_seconds: u64,
    ) -> Result<bool, OfficeError> {
        let now_ms = self.monotonic_now_ms();
        let now_utc = format_rfc3339_ms(now_ms);
        let ttl = ttl_seconds.clamp(1, MAX_OBSERVATION_TTL_SECS);
        let valid_until = format_rfc3339_ms(now_ms.saturating_add(ttl * 1000));
        let state = OfficeConnectionState {
            organization_id: self.info.org_id.clone(),
            office_id: self.info.office_id.clone(),
            connection_id: connection_id.to_owned(),
            status: ConnectionReadinessStatus::Ready,
            binding_revision: expected_binding_revision,
            driver_version,
            observed_at: Some(now_utc.clone()),
            valid_until: Some(valid_until.clone()),
            issue_code: None,
        };
        let model_records: Vec<OfficeConnectionModel> = models
            .into_iter()
            .map(|m| OfficeConnectionModel {
                organization_id: self.info.org_id.clone(),
                office_id: self.info.office_id.clone(),
                connection_id: connection_id.to_owned(),
                model_id: m.model_id,
                driver_model_id: m.driver_model_id,
                entitlement: m.entitlement,
                capabilities_json: m.capabilities_json,
                observed_at: now_utc.clone(),
                valid_until: valid_until.clone(),
            })
            .collect();
        self.providers.renew_ready_observation_if_unchanged(
            expected_binding_revision,
            &state,
            &model_records,
        )
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

    /// Returns all canonical office IDs registered in this organization (`TASK-52` H-13).
    pub fn enrolled_office_ids(&self) -> Result<Vec<String>, OfficeError> {
        Ok(self
            .store
            .read(crate::store::open::OfficeIdsList)
            .map_err(OfficeError::storage)?
            .0)
    }

    /// Returns all offices registered in this organization's replicated state (`TASK-120`).
    pub fn list_org_offices(
        &self,
    ) -> Result<Vec<crate::organization::records::Office>, OfficeError> {
        Ok(self
            .store
            .read(crate::store::open::OfficesList)
            .map_err(OfficeError::storage)?
            .0)
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

fn require_hq_owner(office: &Office, actor: &str) -> Result<Result<Id, ApiError>, OfficeError> {
    if office.info.role != crate::OfficeRole::Hq {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "organization provider connections and model profiles can only be mutated on HQ",
        )));
    }
    require_owner(office, actor)
}

fn require_office_operator(
    office: &Office,
    actor: &str,
) -> Result<Result<(), ApiError>, OfficeError> {
    if office.info.role == crate::OfficeRole::Satellite && actor == office.info.office_id {
        return Ok(Ok(()));
    }
    match require_owner(office, actor)? {
        Ok(_) => Ok(Ok(())),
        Err(e) => Ok(Err(e)),
    }
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
