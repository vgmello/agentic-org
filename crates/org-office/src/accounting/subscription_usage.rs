//! Domain types, validation, freshness evaluation, and cross-subscription model aggregation
//! for `TASK-79` provider subscription model usage snapshots (total, daily, weekly, short-term).

use std::collections::{BTreeMap, BTreeSet};

use org_proto::costs::{
    SubscriptionModelUsageView, SubscriptionUsageAuthSource, SubscriptionUsageSnapshotView,
    SubscriptionUsageStatus, SubscriptionUsageWindowView,
};
use org_proto::{ApiError, ErrorCode};
use serde::{Deserialize, Serialize};

use crate::organization::records::ProviderConnection;

/// Default freshness window for collected subscription usage snapshots (15 minutes).
pub const SUBSCRIPTION_USAGE_FRESHNESS_MS: u64 = 15 * 60 * 1_000;

/// Maximum number of model rows allowed in one persisted subscription usage snapshot.
pub const MAX_SUBSCRIPTION_MODELS: usize = 64;
/// Maximum byte length for one model identifier.
pub const MAX_MODEL_NAME_BYTES: usize = 128;
/// Maximum byte length for `issue_code`.
pub const MAX_ISSUE_CODE_BYTES: usize = 64;
/// Maximum byte length for `message`.
pub const MAX_MESSAGE_BYTES: usize = 512;
/// Maximum serialized JSON size for the `models` array (leaves 2 KiB envelope headroom so
/// inheriting last-good models into a transient `Unavailable` snapshot never overflows the cap).
pub const MAX_MODELS_JSON_BYTES: usize = 30 * 1024;
/// Maximum serialized JSON size for one stored snapshot (keeps list responses well under 1 MiB).
pub const MAX_SNAPSHOT_JSON_BYTES: usize = 32 * 1024;

/// Persisted office-local subscription usage snapshot (stored outside shared org Git).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSubscriptionUsageSnapshot {
    pub organization_id: String,
    pub office_id: String,
    pub connection_id: String,
    /// Monotonic per-office attempt generation fencing concurrent refreshes.
    #[serde(default)]
    pub attempt_seq: u64,
    pub status: SubscriptionUsageStatus,
    pub auth_source: SubscriptionUsageAuthSource,
    /// Timestamp of the most recent collection attempt.
    pub collected_at: String,
    /// Timestamp when `models` were last successfully collected (`Ok`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ok_collected_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub models: Vec<SubscriptionModelUsageView>,
}

/// Live credential custody metadata supplied by the session/credential vault layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubscriptionCredentialPresence {
    pub has_usage_credential: bool,
    pub has_enrolled_credential: bool,
    pub account_hint: Option<String>,
}

/// Aggregate result across fresh `Ok` subscription snapshots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelRollupResult {
    pub totals_by_model: Vec<SubscriptionModelUsageView>,
    pub partial_totals: bool,
    pub excluded_snapshot_count: u64,
}

/// Validates a snapshot before durable persistence or rebuild ingestion.
pub fn validate_stored_snapshot(
    snapshot: &StoredSubscriptionUsageSnapshot,
) -> Result<(), ApiError> {
    if !is_safe_connection_id(&snapshot.connection_id) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "subscription usage snapshot connection_id must be 1..=128 ASCII [A-Za-z0-9_-] chars",
        ));
    }
    validate_snapshot_timestamps_and_strings(snapshot)?;
    validate_snapshot_models(&snapshot.models)?;
    let models_len = serde_json::to_vec(&snapshot.models)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed encoding snapshot models"))?
        .len();
    if models_len > MAX_MODELS_JSON_BYTES {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            "subscription usage models exceed maximum serialized size",
        ));
    }
    let encoded_len = serde_json::to_vec(snapshot)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed encoding snapshot"))?
        .len();
    if encoded_len > MAX_SNAPSHOT_JSON_BYTES {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            "subscription usage snapshot exceeds maximum serialized size",
        ));
    }
    Ok(())
}

fn validate_snapshot_timestamps_and_strings(
    snapshot: &StoredSubscriptionUsageSnapshot,
) -> Result<(), ApiError> {
    if org_proto::parse_rfc3339_ms(&snapshot.collected_at).is_none() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "subscription usage snapshot collected_at must be valid RFC 3339",
        ));
    }
    for ts in [&snapshot.last_ok_collected_at, &snapshot.fresh_until]
        .into_iter()
        .flatten()
    {
        if org_proto::parse_rfc3339_ms(ts).is_none() {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "subscription usage snapshot timestamp must be valid RFC 3339",
            ));
        }
    }
    if snapshot
        .issue_code
        .as_deref()
        .is_some_and(|s| s.len() > MAX_ISSUE_CODE_BYTES || s.bytes().any(|b| b < 0x20))
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "subscription usage issue_code is invalid or too long",
        ));
    }
    if snapshot
        .message
        .as_deref()
        .is_some_and(|s| s.len() > MAX_MESSAGE_BYTES || s.bytes().any(|b| b < 0x20 && b != b'\n'))
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "subscription usage message is invalid or too long",
        ));
    }
    Ok(())
}

fn validate_snapshot_models(models: &[SubscriptionModelUsageView]) -> Result<(), ApiError> {
    if models.len() > MAX_SUBSCRIPTION_MODELS {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            "subscription usage snapshot exceeds maximum model count",
        ));
    }
    let mut seen = BTreeSet::new();
    for row in models {
        let name = row.model.trim();
        if name.is_empty()
            || name.len() > MAX_MODEL_NAME_BYTES
            || name.bytes().any(|b| b < 0x20 || b == 0x7f)
            || !seen.insert(name)
        {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "subscription usage model identifiers must be unique, single-line, and <= 128 bytes",
            ));
        }
        validate_window(&row.total)?;
        validate_window(&row.daily)?;
        validate_window(&row.weekly)?;
        if let Some(ref st) = row.short_term {
            validate_window(st)?;
        }
    }
    Ok(())
}

fn is_safe_connection_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn validate_window(window: &SubscriptionUsageWindowView) -> Result<(), ApiError> {
    if let Some(bps) = window.utilization_bps {
        if bps > 10_000 {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "utilization_bps must be in 0..=10000",
            ));
        }
    }
    if window.window_seconds == Some(0) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "window_seconds must be positive when present",
        ));
    }
    for ts in [&window.starts_at, &window.ends_at, &window.resets_at]
        .into_iter()
        .flatten()
    {
        if org_proto::parse_rfc3339_ms(ts).is_none() {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "usage window timestamp must be valid RFC 3339",
            ));
        }
    }
    Ok(())
}

/// Builds a client-facing snapshot view by joining a shared `ProviderConnection`, optional
/// persisted office-local snapshot, live credential presence, and monotonic `now_ms`.
pub fn build_snapshot_view(
    conn: &ProviderConnection,
    office_id: &str,
    stored: Option<&StoredSubscriptionUsageSnapshot>,
    cred: &SubscriptionCredentialPresence,
    now_ms: u64,
) -> SubscriptionUsageSnapshotView {
    match stored {
        Some(snap) => {
            let has_any_cred = cred.has_usage_credential || cred.has_enrolled_credential;
            let resolved_missing_auth =
                snap.issue_code.as_deref() == Some("AUTH_MISSING") && has_any_cred;
            let (status, issue_code, message, stale) = if resolved_missing_auth {
                (
                    SubscriptionUsageStatus::NeverCollected,
                    Some("NOT_COLLECTED_YET".to_owned()),
                    Some("Run usage collection to fetch subscription usage from provider.".into()),
                    true,
                )
            } else {
                let is_stale = snap
                    .fresh_until
                    .as_deref()
                    .and_then(org_proto::parse_rfc3339_ms)
                    .is_none_or(|exp| now_ms >= exp);
                (
                    snap.status,
                    snap.issue_code.clone(),
                    snap.message.clone(),
                    is_stale,
                )
            };
            SubscriptionUsageSnapshotView {
                connection_id: conn.id.as_str().to_owned(),
                connection_key: conn.key.clone(),
                connection_name: conn.name.clone(),
                provider_type: conn.provider_type.clone(),
                subscription_identity: conn.subscription_identity.clone(),
                office_id: snap.office_id.clone(),
                status,
                auth_source: snap.auth_source,
                has_usage_credential: cred.has_usage_credential,
                has_enrolled_credential: cred.has_enrolled_credential,
                account_hint: cred.account_hint.clone(),
                collected_at: Some(snap.collected_at.clone()),
                last_ok_collected_at: snap.last_ok_collected_at.clone(),
                fresh_until: snap.fresh_until.clone(),
                stale,
                issue_code,
                message,
                is_api_equivalent_estimate: false,
                models: snap.models.clone(),
            }
        }
        None => build_uncollected_snapshot_view(conn, office_id, cred),
    }
}

fn build_uncollected_snapshot_view(
    conn: &ProviderConnection,
    office_id: &str,
    cred: &SubscriptionCredentialPresence,
) -> SubscriptionUsageSnapshotView {
    let has_any_cred = cred.has_usage_credential || cred.has_enrolled_credential;
    let (status, issue_code, message) = if has_any_cred {
        (
            SubscriptionUsageStatus::NeverCollected,
            Some("NOT_COLLECTED_YET".to_owned()),
            Some("Run usage collection to fetch subscription usage from provider.".into()),
        )
    } else {
        (
            SubscriptionUsageStatus::AuthRequired,
            Some("AUTH_MISSING".to_owned()),
            Some("No subscription or usage-API credential is configured on this office.".into()),
        )
    };
    SubscriptionUsageSnapshotView {
        connection_id: conn.id.as_str().to_owned(),
        connection_key: conn.key.clone(),
        connection_name: conn.name.clone(),
        provider_type: conn.provider_type.clone(),
        subscription_identity: conn.subscription_identity.clone(),
        office_id: office_id.to_owned(),
        status,
        auth_source: SubscriptionUsageAuthSource::None,
        has_usage_credential: cred.has_usage_credential,
        has_enrolled_credential: cred.has_enrolled_credential,
        account_hint: cred.account_hint.clone(),
        collected_at: None,
        last_ok_collected_at: None,
        fresh_until: None,
        stale: true,
        issue_code,
        message,
        is_api_equivalent_estimate: false,
        models: Vec::new(),
    }
}

/// Atomically preserves the last known good `last_ok_collected_at` and `models` when persisting
/// a transient `Unavailable` snapshot under the store's write lock.
pub fn merge_prior_for_unavailable(
    snapshot: &mut StoredSubscriptionUsageSnapshot,
    existing: Option<&StoredSubscriptionUsageSnapshot>,
) {
    if snapshot.status != SubscriptionUsageStatus::Unavailable
        || !snapshot.models.is_empty()
        || snapshot.last_ok_collected_at.is_some()
    {
        return;
    }
    if let Some(prev) = existing {
        snapshot.last_ok_collected_at = prev.last_ok_collected_at.clone();
        snapshot.models = prev.models.clone();
    }
}

pub fn canonical_provider_namespace(provider_type: &str) -> String {
    let lower = provider_type.trim().to_ascii_lowercase();
    match lower.as_str() {
        "anthropic" | "claude" | "claude-code" => "anthropic".into(),
        "openai" | "codex" | "codex-cli" | "gpt" => "openai".into(),
        "google" | "agy" | "antigravity" | "antigravity-cli" | "gemini" => "google".into(),
        _ => lower,
    }
}

/// Aggregates `total`, `daily`, `weekly`, and compatible `short_term` counters per model across
/// FRESH `Ok` snapshots only.
///
/// - Stale, non-`Ok`, or duplicate-identity snapshots are excluded and counted in
///   `excluded_snapshot_count` (`partial_totals = true`).
/// - Counters are only summed when EVERY contributor for that model reported that counter and
///   no `u64` overflow occurred; otherwise the counter stays `None` and `partial_totals = true`.
/// - Incompatible window durations or period boundaries across contributors mark
///   `partial_totals = true` and leave that window's merged counters `None`.
pub fn aggregate_totals_by_model(snapshots: &[SubscriptionUsageSnapshotView]) -> ModelRollupResult {
    let mut by_model: BTreeMap<String, ModelAccumulator> = BTreeMap::new();
    let mut seen_identities: BTreeSet<(String, String)> = BTreeSet::new();
    let mut excluded_snapshot_count: u64 = 0;
    for snap in snapshots {
        if snap.status != SubscriptionUsageStatus::Ok || snap.stale {
            excluded_snapshot_count = excluded_snapshot_count.saturating_add(1);
            continue;
        }
        if let Some(ident) = snap
            .subscription_identity
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let key = (
                canonical_provider_namespace(&snap.provider_type),
                ident.to_ascii_lowercase(),
            );
            if !seen_identities.insert(key) {
                excluded_snapshot_count = excluded_snapshot_count.saturating_add(1);
                continue;
            }
        }
        for row in &snap.models {
            let entry = by_model.entry(row.model.clone()).or_default();
            entry.total.merge(&row.total);
            entry.daily.merge(&row.daily);
            entry.weekly.merge(&row.weekly);
            if let Some(ref st) = row.short_term {
                entry.short_term.merge(st);
            } else {
                entry.short_term.record_absent();
            }
        }
    }
    let mut partial_totals = excluded_snapshot_count > 0;
    let totals_by_model = by_model
        .into_iter()
        .map(|(model, acc)| {
            let (total, p1) = acc.total.finish();
            let (daily, p2) = acc.daily.finish();
            let (weekly, p3) = acc.weekly.finish();
            let (short_term, p4) = acc.short_term.finish_optional();
            partial_totals = partial_totals || p1 || p2 || p3 || p4;
            SubscriptionModelUsageView {
                model,
                total,
                daily,
                weekly,
                short_term,
            }
        })
        .collect();
    ModelRollupResult {
        totals_by_model,
        partial_totals,
        excluded_snapshot_count,
    }
}

#[derive(Default)]
struct ModelAccumulator {
    total: WindowAccumulator,
    daily: WindowAccumulator,
    weekly: WindowAccumulator,
    short_term: WindowAccumulator,
}

#[derive(Default)]
struct CounterAcc {
    sum: u64,
    present_count: usize,
    missing_count: usize,
    overflowed: bool,
}

impl CounterAcc {
    fn add(&mut self, val: Option<u64>) {
        match val {
            Some(v) => {
                match self.sum.checked_add(v) {
                    Some(next) => self.sum = next,
                    None => self.overflowed = true,
                }
                self.present_count += 1;
            }
            None => {
                self.missing_count += 1;
            }
        }
    }

    fn finish(self) -> (Option<u64>, bool) {
        if self.overflowed {
            (None, true)
        } else if self.present_count == 0 {
            (None, false)
        } else if self.missing_count > 0 {
            (None, true)
        } else {
            (Some(self.sum), false)
        }
    }
}

#[derive(Default)]
struct WindowAccumulator {
    contributors: usize,
    absent_contributors: usize,
    starts_at: Option<String>,
    ends_at: Option<String>,
    resets_at: Option<String>,
    window_seconds: Option<u64>,
    window_incompatible: bool,
    utilization_bps: Option<u32>,
    requests: CounterAcc,
    input_tokens: CounterAcc,
    output_tokens: CounterAcc,
    cache_read_tokens: CounterAcc,
    cache_write_tokens: CounterAcc,
    total_tokens: CounterAcc,
}

impl WindowAccumulator {
    fn record_absent(&mut self) {
        self.absent_contributors += 1;
    }

    fn merge(&mut self, w: &SubscriptionUsageWindowView) {
        self.contributors += 1;
        if self.contributors == 1 {
            self.starts_at = w.starts_at.clone();
            self.ends_at = w.ends_at.clone();
            self.resets_at = w.resets_at.clone();
            self.window_seconds = w.window_seconds;
            self.utilization_bps = w.utilization_bps;
        } else {
            if self.window_seconds != w.window_seconds
                || self.starts_at != w.starts_at
                || self.ends_at != w.ends_at
                || self.resets_at != w.resets_at
            {
                self.window_incompatible = true;
            }
            self.utilization_bps = None;
        }
        self.requests.add(w.requests);
        self.input_tokens.add(w.input_tokens);
        self.output_tokens.add(w.output_tokens);
        self.cache_read_tokens.add(w.cache_read_tokens);
        self.cache_write_tokens.add(w.cache_write_tokens);
        self.total_tokens.add(w.total_tokens);
    }

    fn finish(self) -> (SubscriptionUsageWindowView, bool) {
        if self.window_incompatible {
            return (SubscriptionUsageWindowView::default(), true);
        }
        let (requests, p1) = self.requests.finish();
        let (input_tokens, p2) = self.input_tokens.finish();
        let (output_tokens, p3) = self.output_tokens.finish();
        let (cache_read_tokens, p4) = self.cache_read_tokens.finish();
        let (cache_write_tokens, p5) = self.cache_write_tokens.finish();
        let (total_tokens, p6) = self.total_tokens.finish();
        let partial = p1 || p2 || p3 || p4 || p5 || p6;
        (
            SubscriptionUsageWindowView {
                starts_at: self.starts_at,
                ends_at: self.ends_at,
                resets_at: self.resets_at,
                window_seconds: self.window_seconds,
                utilization_bps: self.utilization_bps,
                requests,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                total_tokens,
            },
            partial,
        )
    }

    fn finish_optional(self) -> (Option<SubscriptionUsageWindowView>, bool) {
        if self.contributors == 0 {
            return (None, false);
        }
        if self.absent_contributors > 0 {
            return (None, true);
        }
        let (view, partial) = self.finish();
        (Some(view), partial)
    }
}
