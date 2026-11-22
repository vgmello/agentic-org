//! Wire contracts for `L4` provider connection pools, office-local bindings, per-connection
//! model discovery, readiness, shared/local quota permits, and cooldown coordination
//! (`TASK-54`, `FC-098`, `FC-099`, `doc-88`, `doc-89`).

use serde::{Deserialize, Serialize};

use crate::hiring::EntityOpResult;
use crate::Call;

// ---- Shared enums ----

/// Local readiness status of an office's binding to a provider connection (`office_connection_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConnectionReadinessStatus {
    Unknown,
    Ready,
    AuthExpired,
    Unavailable,
}

impl ConnectionReadinessStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Ready => "ready",
            Self::AuthExpired => "auth_expired",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "unknown" => Some(Self::Unknown),
            "ready" => Some(Self::Ready),
            "auth_expired" => Some(Self::AuthExpired),
            "unavailable" => Some(Self::Unavailable),
            _ => None,
        }
    }
}

/// Entitlement proof state for a discovered model (`office_connection_models`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveredModelEntitlement {
    Confirmed,
    Denied,
    Unknown,
}

impl DiscoveredModelEntitlement {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Denied => "denied",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "confirmed" => Some(Self::Confirmed),
            "denied" => Some(Self::Denied),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// Turn permit authority (`doc-88` §6, `TASK-2` offline execution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PermitAuthority {
    /// Shared multi-office connection coordinated by HQ.
    HqPool,
    /// Office-local / unshared connection admitted locally so offline satellites keep running.
    OfficeLocal,
}

impl PermitAuthority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HqPool => "hq_pool",
            Self::OfficeLocal => "office_local",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "hq_pool" => Some(Self::HqPool),
            "office_local" => Some(Self::OfficeLocal),
            _ => None,
        }
    }
}

/// State of a turn permit (`connection_turn_permits`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TurnPermitState {
    Active,
    ReleasePending,
    Released,
}

impl TurnPermitState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::ReleasePending => "release_pending",
            Self::Released => "released",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "active" => Some(Self::Active),
            "release_pending" => Some(Self::ReleasePending),
            "released" => Some(Self::Released),
            _ => None,
        }
    }
}

/// Quota / cooldown status (`connection_quota_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConnectionQuotaStatus {
    Unknown,
    Open,
    Cooldown,
}

impl ConnectionQuotaStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Open => "open",
            Self::Cooldown => "cooldown",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "unknown" => Some(Self::Unknown),
            "open" => Some(Self::Open),
            "cooldown" => Some(Self::Cooldown),
            _ => None,
        }
    }
}

/// Readiness status returned by `member.agent_readiness` (`doc-88` §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum AgentReadinessCode {
    Ready,
    NoExecutionConfig,
    ExecutionDisabled,
    ProfileArchived,
    ModelUnverified,
    BudgetHeld,
    PoolExhausted,
    NoReadyConnection,
}

// ---- Organization-wide connection & pool records ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConnectionView {
    pub id: String,
    pub revision: u64,
    pub key: String,
    pub name: String,
    pub provider_type: String,
    pub auth_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_identity: Option<String>,
    pub runtime_driver: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_url: Option<String>,
    pub enabled: bool,
    pub archived: bool,
    pub max_concurrent_turns: u64,
    pub created_at: String,
    pub updated_at: String,
}

pub struct ProviderConnectionList;
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConnectionListParams {
    #[serde(default)]
    pub include_archived: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConnectionListResult {
    pub connections: Vec<ProviderConnectionView>,
}
impl Call for ProviderConnectionList {
    const METHOD: &'static str = "provider.connection.list";
    type Params = ProviderConnectionListParams;
    type Result = ProviderConnectionListResult;
}

pub struct ProviderConnectionUpdate;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConnectionUpdateParams {
    pub command_id: String,
    pub connection_id: String,
    pub expected_version: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub endpoint_url: Option<String>,
    #[serde(default)]
    pub clear_endpoint_url: bool,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub max_concurrent_turns: Option<u64>,
}
impl Call for ProviderConnectionUpdate {
    const METHOD: &'static str = "provider.connection.update";
    type Params = ProviderConnectionUpdateParams;
    type Result = EntityOpResult;
}

pub struct ModelProfileCreate;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileCreateParams {
    pub command_id: String,
    pub profile_id: String,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub connection_ids: Vec<String>,
}
impl Call for ModelProfileCreate {
    const METHOD: &'static str = "model_profile.create";
    type Params = ModelProfileCreateParams;
    type Result = EntityOpResult;
}

pub struct ModelProfileUpdate;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileUpdateParams {
    pub command_id: String,
    pub profile_id: String,
    pub expected_version: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub connection_ids: Option<Vec<String>>,
}
impl Call for ModelProfileUpdate {
    const METHOD: &'static str = "model_profile.update";
    type Params = ModelProfileUpdateParams;
    type Result = EntityOpResult;
}

// ---- Office-local bindings, discovery & readiness ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeConnectionBindingView {
    pub office_id: String,
    pub connection_id: String,
    pub revision: u64,
    pub auth_context_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_binary_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_identity_ref: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeConnectionStateView {
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
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredModelInput {
    pub model_id: String,
    pub driver_model_id: String,
    pub entitlement: DiscoveredModelEntitlement,
    #[serde(default = "default_caps")]
    pub capabilities_json: String,
}

fn default_caps() -> String {
    "{}".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeConnectionModelView {
    pub office_id: String,
    pub connection_id: String,
    pub model_id: String,
    pub driver_model_id: String,
    pub entitlement: DiscoveredModelEntitlement,
    pub capabilities_json: String,
    pub observed_at: String,
    pub valid_until: String,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundConnectionSummary {
    pub binding: OfficeConnectionBindingView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<OfficeConnectionStateView>,
    pub models: Vec<OfficeConnectionModelView>,
}

pub struct ProviderBindingSet;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderBindingSetParams {
    pub operation_id: String,
    pub connection_id: String,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    pub auth_context_ref: String,
    #[serde(default)]
    pub runtime_binary_path: Option<String>,
    #[serde(default)]
    pub account_identity_ref: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderBindingSetResult {
    pub binding: OfficeConnectionBindingView,
    pub replayed: bool,
}
impl Call for ProviderBindingSet {
    const METHOD: &'static str = "provider.binding.set";
    type Params = ProviderBindingSetParams;
    type Result = ProviderBindingSetResult;
}

pub struct ProviderBindingList;
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderBindingListParams {
    #[serde(default)]
    pub connection_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderBindingListResult {
    pub bindings: Vec<BoundConnectionSummary>,
}
impl Call for ProviderBindingList {
    const METHOD: &'static str = "provider.binding.list";
    type Params = ProviderBindingListParams;
    type Result = ProviderBindingListResult;
}

pub struct ProviderConnectionRefresh;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConnectionRefreshParams {
    pub connection_id: String,
    #[serde(default)]
    pub driver_version: Option<String>,
    #[serde(default)]
    pub auth_signed_in: Option<bool>,
    #[serde(default)]
    pub observed_subscription_identity: Option<String>,
    #[serde(default)]
    pub operator_attested_subscription: bool,
    #[serde(default)]
    pub models: Vec<DiscoveredModelInput>,
    #[serde(default)]
    pub issue_code: Option<String>,
    /// Observation TTL in seconds (clamped to `1..=300` per `doc-88` §4).
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConnectionRefreshResult {
    pub state: OfficeConnectionStateView,
    pub models: Vec<OfficeConnectionModelView>,
}
impl Call for ProviderConnectionRefresh {
    const METHOD: &'static str = "provider.connection.refresh";
    type Params = ProviderConnectionRefreshParams;
    type Result = ProviderConnectionRefreshResult;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileModelEntry {
    pub model_id: String,
    pub supporting_connections: Vec<String>,
    pub first_priority: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    pub stale: bool,
    pub ready_connections: u64,
}

pub struct ModelProfileModels;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfileModelsParams {
    pub model_profile_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfileModelsResult {
    pub model_profile_id: String,
    pub models: Vec<ProfileModelEntry>,
}
impl Call for ModelProfileModels {
    const METHOD: &'static str = "model_profile.models";
    type Params = ModelProfileModelsParams;
    type Result = ModelProfileModelsResult;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionCandidateStatus {
    pub connection_id: String,
    pub priority: u64,
    pub eligible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_model_id: Option<String>,
}

pub struct AgentReadinessGet;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReadinessGetParams {
    pub employee_id: String,
    #[serde(default)]
    pub task_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReadinessResult {
    pub employee_id: String,
    pub ready: bool,
    pub status: AgentReadinessCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub candidates: Vec<ConnectionCandidateStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earliest_cooldown_until: Option<String>,
}
impl Call for AgentReadinessGet {
    const METHOD: &'static str = "member.agent_readiness";
    type Params = AgentReadinessGetParams;
    type Result = AgentReadinessResult;
}

// ---- Quota permits & cooldowns ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionQuotaView {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnPermitView {
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

pub struct ProviderQuotaStatus;
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderQuotaStatusParams {
    #[serde(default)]
    pub connection_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderQuotaStatusResult {
    pub coordinator_generation: u64,
    pub cooldowns: Vec<ConnectionQuotaView>,
    pub active_permits: Vec<TurnPermitView>,
    pub release_pending_permits: Vec<TurnPermitView>,
}
impl Call for ProviderQuotaStatus {
    const METHOD: &'static str = "provider.quota.status";
    type Params = ProviderQuotaStatusParams;
    type Result = ProviderQuotaStatusResult;
}

pub struct ProviderCooldownRecord;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCooldownRecordParams {
    pub connection_id: String,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub cooldown_until: Option<String>,
    #[serde(default)]
    pub retry_after_seconds: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCooldownRecordResult {
    pub quota: ConnectionQuotaView,
}
impl Call for ProviderCooldownRecord {
    const METHOD: &'static str = "provider.cooldown.record";
    type Params = ProviderCooldownRecordParams;
    type Result = ProviderCooldownRecordResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_enum_strings_and_methods_match_schema_sql() {
        assert_eq!(ConnectionReadinessStatus::Ready.as_str(), "ready");
        assert_eq!(
            ConnectionReadinessStatus::AuthExpired.as_str(),
            "auth_expired"
        );
        assert_eq!(DiscoveredModelEntitlement::Confirmed.as_str(), "confirmed");
        assert_eq!(PermitAuthority::HqPool.as_str(), "hq_pool");
        assert_eq!(PermitAuthority::OfficeLocal.as_str(), "office_local");
        assert_eq!(TurnPermitState::ReleasePending.as_str(), "release_pending");
        assert_eq!(ConnectionQuotaStatus::Cooldown.as_str(), "cooldown");
        assert_eq!(ProviderConnectionList::METHOD, "provider.connection.list");
        assert_eq!(ModelProfileCreate::METHOD, "model_profile.create");
        assert_eq!(AgentReadinessGet::METHOD, "member.agent_readiness");
    }
}
