//! Consumer-owned port and pure domain model for `L4` office-local provider connection bindings,
//! freshness-bounded readiness/model discovery, and dual-authority quota permit/cooldown coordination
//! (`TASK-54`, `FC-098`, `FC-099`, `doc-88`, `doc-89`, `TASK-2` offline execution).

pub mod memory;

use org_proto::providers::{
    ConnectionQuotaStatus, ConnectionQuotaView, ConnectionReadinessStatus,
    DiscoveredModelEntitlement, OfficeConnectionBindingView, OfficeConnectionModelView,
    OfficeConnectionStateView, PermitAuthority, TurnPermitState, TurnPermitView,
};
use org_proto::ApiError;
use serde::{Deserialize, Serialize};

use crate::error::OfficeError;

/// Default turn permit lease duration (`doc-88` §6: 60 seconds, renewed every 10s in memory).
pub const PERMIT_LEASE_MS: u64 = 60_000;
/// Default cooldown duration when a provider omits `Retry-After` / `resets_at` (`doc-88` §6: 900s).
pub const DEFAULT_COOLDOWN_SECS: u64 = 900;
/// Maximum auth/model discovery observation freshness window (`doc-88` §4: 300s).
pub const MAX_OBSERVATION_TTL_SECS: u64 = 300;

/// Office-local binding record persisted at
/// `<durable>/local/offices/<office-id>/connections/<connection-id>/binding.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeConnectionBinding {
    pub organization_id: String,
    pub office_id: String,
    pub connection_id: String,
    pub revision: u64,
    pub auth_context_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_binary_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_identity_ref: Option<String>,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_operation_hash: Option<String>,
}

impl OfficeConnectionBinding {
    pub fn to_view(&self) -> OfficeConnectionBindingView {
        OfficeConnectionBindingView {
            office_id: self.office_id.clone(),
            connection_id: self.connection_id.clone(),
            revision: self.revision,
            auth_context_ref: self.auth_context_ref.clone(),
            runtime_binary_path: self.runtime_binary_path.clone(),
            account_identity_ref: self.account_identity_ref.clone(),
            updated_at: self.updated_at.clone(),
        }
    }
}

/// Supervisor-observed readiness state for one office connection binding (`office_connection_state`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeConnectionState {
    pub organization_id: String,
    pub office_id: String,
    pub connection_id: String,
    pub status: ConnectionReadinessStatus,
    pub binding_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_code: Option<String>,
}

impl OfficeConnectionState {
    pub fn is_stale(&self, binding_revision: u64, now_ms: u64) -> bool {
        if self.binding_revision != binding_revision {
            return true;
        }
        let Some(ref until) = self.valid_until else {
            return true;
        };
        org_proto::parse_rfc3339_ms(until).is_none_or(|exp| now_ms >= exp)
    }

    pub fn to_view(&self, binding_revision: u64, now_ms: u64) -> OfficeConnectionStateView {
        OfficeConnectionStateView {
            office_id: self.office_id.clone(),
            connection_id: self.connection_id.clone(),
            status: self.status,
            binding_revision: self.binding_revision,
            driver_version: self.driver_version.clone(),
            observed_at: self.observed_at.clone(),
            valid_until: self.valid_until.clone(),
            issue_code: self.issue_code.clone(),
            stale: self.is_stale(binding_revision, now_ms),
        }
    }
}

/// Discovered model capability & entitlement observation (`office_connection_models`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeConnectionModel {
    pub organization_id: String,
    pub office_id: String,
    pub connection_id: String,
    pub model_id: String,
    pub driver_model_id: String,
    pub entitlement: DiscoveredModelEntitlement,
    pub capabilities_json: String,
    pub observed_at: String,
    pub valid_until: String,
}

impl OfficeConnectionModel {
    pub fn is_stale(&self, now_ms: u64) -> bool {
        org_proto::parse_rfc3339_ms(&self.valid_until).is_none_or(|exp| now_ms >= exp)
    }

    pub fn to_view(&self, now_ms: u64) -> OfficeConnectionModelView {
        OfficeConnectionModelView {
            office_id: self.office_id.clone(),
            connection_id: self.connection_id.clone(),
            model_id: self.model_id.clone(),
            driver_model_id: self.driver_model_id.clone(),
            entitlement: self.entitlement,
            capabilities_json: self.capabilities_json.clone(),
            observed_at: self.observed_at.clone(),
            valid_until: self.valid_until.clone(),
            stale: self.is_stale(now_ms),
        }
    }
}

/// Persisted cooldown / quota state (`connection_quota_state`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionQuotaState {
    pub organization_id: String,
    pub connection_id: String,
    pub scope_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub status: ConnectionQuotaStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_until: Option<String>,
    pub observed_at: String,
    pub observed_by_office_id: String,
    pub coordinator_generation: u64,
}

impl ConnectionQuotaState {
    pub fn is_cooling_down(&self, now_ms: u64) -> bool {
        if self.status != ConnectionQuotaStatus::Cooldown {
            return false;
        }
        self.cooldown_until
            .as_deref()
            .and_then(org_proto::parse_rfc3339_ms)
            .is_some_and(|until| now_ms < until)
    }

    pub fn to_view(&self) -> ConnectionQuotaView {
        ConnectionQuotaView {
            connection_id: self.connection_id.clone(),
            scope_key: self.scope_key.clone(),
            model_id: self.model_id.clone(),
            status: self.status,
            cooldown_until: self.cooldown_until.clone(),
            observed_at: self.observed_at.clone(),
            observed_by_office_id: self.observed_by_office_id.clone(),
            coordinator_generation: self.coordinator_generation,
        }
    }
}

/// Durable turn permit record (`connection_turn_permits`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnPermitRecord {
    pub organization_id: String,
    pub id: String,
    pub connection_id: String,
    pub office_id: String,
    pub employee_id: String,
    pub placement_generation: u64,
    pub model_id: String,
    pub authority: PermitAuthority,
    pub state: TurnPermitState,
    pub issued_at: String,
    pub expires_at: String,
    pub coordinator_generation: u64,
}

impl TurnPermitRecord {
    pub fn to_view(&self) -> TurnPermitView {
        TurnPermitView {
            id: self.id.clone(),
            connection_id: self.connection_id.clone(),
            office_id: self.office_id.clone(),
            employee_id: self.employee_id.clone(),
            placement_generation: self.placement_generation,
            model_id: self.model_id.clone(),
            authority: self.authority,
            state: self.state,
            issued_at: self.issued_at.clone(),
            expires_at: self.expires_at.clone(),
            coordinator_generation: self.coordinator_generation,
        }
    }
}

/// Atomic permit acquisition request (`doc-88` §6).
#[derive(Debug, Clone)]
pub struct PermitAcquireRequest {
    pub organization_id: String,
    pub permit_id: String,
    pub connection_id: String,
    pub office_id: String,
    pub employee_id: String,
    pub placement_generation: u64,
    pub model_id: String,
    pub authority: PermitAuthority,
    pub max_concurrent_turns: u64,
    pub now_ms: u64,
    pub issued_at: String,
    pub expires_at: String,
}

/// Outcome of attempting to acquire a turn permit on one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermitAcquireOutcome {
    Granted(TurnPermitRecord),
    CoolingDown { cooldown_until: String },
    CapacityFull { occupied: u64, max: u64 },
}

/// Snapshot of coordinator generation, cooldowns, and non-released permits.
#[derive(Debug, Clone, Default)]
pub struct PermitSnapshot {
    pub coordinator_generation: u64,
    pub cooldowns: Vec<ConnectionQuotaState>,
    pub permits: Vec<TurnPermitRecord>,
}

/// Consumer-owned storage port for office-local bindings, discovery caches, and dual-authority
/// quota permits/cooldowns (`AGENTS.md` Rule 1, `doc-6`).
pub trait ProviderPoolStore: Send + Sync {
    // ---- Bindings & discovery observations ----
    fn get_binding(
        &self,
        org_id: &str,
        office_id: &str,
        connection_id: &str,
    ) -> Result<Option<OfficeConnectionBinding>, OfficeError>;

    fn list_bindings(
        &self,
        org_id: &str,
        office_id: &str,
    ) -> Result<Vec<OfficeConnectionBinding>, OfficeError>;

    /// Saves binding durably and invalidates any existing state/model observations for this binding.
    fn save_binding_and_invalidate(
        &self,
        binding: &OfficeConnectionBinding,
    ) -> Result<(), OfficeError>;

    fn get_state(
        &self,
        org_id: &str,
        office_id: &str,
        connection_id: &str,
    ) -> Result<Option<OfficeConnectionState>, OfficeError>;

    fn list_models(
        &self,
        org_id: &str,
        office_id: &str,
        connection_id: &str,
    ) -> Result<Vec<OfficeConnectionModel>, OfficeError>;

    fn save_observation(
        &self,
        state: &OfficeConnectionState,
        models: &[OfficeConnectionModel],
    ) -> Result<(), OfficeError>;

    /// Atomically renews a `Ready` observation only if the current binding still matches
    /// `expected_binding_revision` and the current readiness state is still `Ready`.
    /// Returns `Ok(true)` if renewed, or `Ok(false)` if skipped due to a concurrent update.
    fn renew_ready_observation_if_unchanged(
        &self,
        expected_binding_revision: u64,
        state: &OfficeConnectionState,
        models: &[OfficeConnectionModel],
    ) -> Result<bool, OfficeError>;

    // ---- Quota permits & cooldowns ----
    fn coordinator_generation(&self) -> Result<u64, OfficeError>;

    /// Atomically expires overdue active permits to `release_pending`, checks cooldowns &
    /// capacity (`active + release_pending < max_concurrent_turns`), and durably issues a permit.
    fn acquire_permit(
        &self,
        req: PermitAcquireRequest,
    ) -> Result<PermitAcquireOutcome, OfficeError>;

    /// **In-memory heartbeat lease renewal ONLY** (`TASK-2` / Non-Negotiable #18):
    /// extends the permit's in-memory deadline by `PERMIT_LEASE_MS` with ZERO durable/SQLite writes.
    /// Returns `Ok(new_expires_at_ms)` if active and generation matches, else `Err(ApiError)`.
    fn renew_permit_in_memory(
        &self,
        permit_id: &str,
        coordinator_generation: u64,
        now_ms: u64,
    ) -> Result<Result<u64, ApiError>, OfficeError>;

    /// Durably transitions an `active` permit into `release_pending` (retaining capacity slot).
    fn mark_permit_release_pending(&self, permit_id: &str) -> Result<bool, OfficeError>;

    /// Durably transitions an `active` or `release_pending` permit into `released` (freeing slot).
    fn release_permit(&self, permit_id: &str) -> Result<bool, OfficeError>;

    /// Durably records a connection-wide (`model_id = None`) or model-specific cooldown.
    fn record_cooldown(&self, state: &ConnectionQuotaState) -> Result<(), OfficeError>;

    /// Lists current cooldowns & active/release_pending permits (optionally filtered by connection).
    fn quota_snapshot(
        &self,
        org_id: &str,
        connection_id: Option<&str>,
        now_ms: u64,
    ) -> Result<PermitSnapshot, OfficeError>;

    /// Counts durable permit/cooldown writes performed by this store (for testing that
    /// `renew_permit_in_memory` performs ZERO durable writes).
    fn durable_write_count(&self) -> u64;
}

/// Computes `scope_key` matching `schema.sql:1751` CHECK constraint.
pub fn quota_scope_key(model_id: Option<&str>) -> String {
    match model_id {
        Some(m) => format!("model:{m}"),
        None => "connection".into(),
    }
}
