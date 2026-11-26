//! Subscription model usage snapshots (`TASK-79`: total, daily, weekly per model).

use serde::{Deserialize, Serialize};

use crate::Call;

/// Collection health status for one provider subscription connection (`TASK-79`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SubscriptionUsageStatus {
    /// Usage metrics were successfully collected from the provider.
    Ok,
    /// Credentials are missing, expired, or rejected by the provider.
    AuthRequired,
    /// Provider usage endpoint is temporarily unreachable, rate-limited, or returned invalid data.
    Unavailable,
    /// Provider or connection type does not expose a standalone subscription usage endpoint.
    Unsupported,
    /// No collection attempt has been recorded yet for this subscription connection.
    NeverCollected,
    #[serde(other)]
    Unknown,
}

impl SubscriptionUsageStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::AuthRequired => "auth_required",
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
            Self::NeverCollected => "never_collected",
            Self::Unknown => "unavailable",
        }
    }
}

/// Which private office-local credential authenticated a subscription usage collection (`TASK-79`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SubscriptionUsageAuthSource {
    None,
    DedicatedUsageCredential,
    EnrolledSubscriptionCredential,
    #[serde(other)]
    Unknown,
}

impl SubscriptionUsageAuthSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DedicatedUsageCredential => "dedicated_usage_credential",
            Self::EnrolledSubscriptionCredential => "enrolled_subscription_credential",
            Self::Unknown => "none",
        }
    }
}

/// Provider-reported usage metrics for one time window (`total`, `daily`, or `weekly`).
///
/// All counters are nullable: `None` means unknown/not reported by the provider, never fabricated
/// as zero (`doc-87`, `TASK-79`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionUsageWindowView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    /// Provider-reported window duration in seconds (e.g. `18000` for 5h, `86400` for 24h, `604800` for 7d).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_seconds: Option<u64>,
    /// Utilization percentage in basis points (`0..=10000`, i.e. `0.00%..=100.00%`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization_bps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

/// Total, daily, weekly, and optional short-term rolling provider-side usage for one model (`TASK-79`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionModelUsageView {
    pub model: String,
    #[serde(default)]
    pub total: SubscriptionUsageWindowView,
    #[serde(default)]
    pub daily: SubscriptionUsageWindowView,
    #[serde(default)]
    pub weekly: SubscriptionUsageWindowView,
    /// Optional sub-daily rolling window (e.g. Claude/Codex 5-hour quota) kept distinct from 24h daily.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_term: Option<SubscriptionUsageWindowView>,
}

/// Persisted and live-enriched subscription usage snapshot for one provider connection (`TASK-79`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionUsageSnapshotView {
    pub connection_id: String,
    pub connection_key: String,
    pub connection_name: String,
    pub provider_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_identity: Option<String>,
    pub office_id: String,
    pub status: SubscriptionUsageStatus,
    pub auth_source: SubscriptionUsageAuthSource,
    pub has_usage_credential: bool,
    pub has_enrolled_credential: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_hint: Option<String>,
    /// Timestamp of the most recent collection attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collected_at: Option<String>,
    /// Timestamp when `models` were last successfully collected (`Ok`), preserved across transient outages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ok_collected_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_until: Option<String>,
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Always `false`: explicitly distinguishes provider subscription usage from API-equivalent estimates.
    pub is_api_equivalent_estimate: bool,
    #[serde(default)]
    pub models: Vec<SubscriptionModelUsageView>,
}

/// Non-secret status metadata for a connection's dedicated usage-collection credential (`TASK-79`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionUsageCredentialView {
    pub connection_id: String,
    pub office_id: String,
    pub configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_hint: Option<String>,
}

/// `cost.subscription_usage.list`: owner-scoped read of persisted subscription model usage snapshots.
pub struct CostSubscriptionUsageList;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageListParams {
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub provider_type: Option<String>,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub totals_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageListResult {
    pub snapshots: Vec<SubscriptionUsageSnapshotView>,
    pub totals_by_model: Vec<SubscriptionModelUsageView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collected_as_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_totals_cursor: Option<String>,
    /// True when any subscription was excluded (stale/non-Ok/duplicate) or had missing/incompatible counters.
    #[serde(default)]
    pub partial_totals: bool,
    /// Number of returned subscription snapshots excluded from `totals_by_model` (stale, non-Ok, or duplicate identity).
    #[serde(default)]
    pub excluded_snapshot_count: u64,
    /// Always `false`: distinguishes subscription usage totals from API-equivalent USD cost estimates.
    pub is_api_equivalent_estimate: bool,
}

impl Call for CostSubscriptionUsageList {
    const METHOD: &'static str = "cost.subscription_usage.list";
    type Params = CostSubscriptionUsageListParams;
    type Result = CostSubscriptionUsageListResult;
}

/// `cost.subscription_usage.collect`: owner-scoped authenticated collection for one or all active subscriptions.
pub struct CostSubscriptionUsageCollect;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCollectParams {
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCollectResult {
    pub snapshots: Vec<SubscriptionUsageSnapshotView>,
    pub collected_at: String,
    /// Connection IDs deferred because the batch deadline or response byte budget was reached.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deferred_connection_ids: Vec<String>,
}

impl Call for CostSubscriptionUsageCollect {
    const METHOD: &'static str = "cost.subscription_usage.collect";
    type Params = CostSubscriptionUsageCollectParams;
    type Result = CostSubscriptionUsageCollectResult;
}

/// `cost.subscription_usage.credential_set`: store a wrapped private usage-API credential outside org Git.
pub struct CostSubscriptionUsageCredentialSet;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCredentialSetParams {
    pub connection_id: String,
    pub secret: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub account_hint: Option<String>,
    /// Optional expected destination HTTPS origin (`https://host[:port]`); required for custom endpoint URLs.
    #[serde(default)]
    pub expected_origin: Option<String>,
}

impl std::fmt::Debug for CostSubscriptionUsageCredentialSetParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CostSubscriptionUsageCredentialSetParams")
            .field("connection_id", &self.connection_id)
            .field("secret", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("account_hint", &self.account_hint)
            .field("expected_origin", &self.expected_origin)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCredentialSetResult {
    pub credential: SubscriptionUsageCredentialView,
}

impl Call for CostSubscriptionUsageCredentialSet {
    const METHOD: &'static str = "cost.subscription_usage.credential_set";
    type Params = CostSubscriptionUsageCredentialSetParams;
    type Result = CostSubscriptionUsageCredentialSetResult;
}

/// `cost.subscription_usage.credential_revoke`: remove a private usage-API credential from office storage.
pub struct CostSubscriptionUsageCredentialRevoke;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCredentialRevokeParams {
    pub connection_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSubscriptionUsageCredentialRevokeResult {
    pub credential: SubscriptionUsageCredentialView,
}

impl Call for CostSubscriptionUsageCredentialRevoke {
    const METHOD: &'static str = "cost.subscription_usage.credential_revoke";
    type Params = CostSubscriptionUsageCredentialRevokeParams;
    type Result = CostSubscriptionUsageCredentialRevokeResult;
}
