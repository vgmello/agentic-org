//! Office facade for `L2` immutable usage & cost accounting, versioned USD model pricing,
//! attribution corrections, timezone-aware queries, and CSV export (`TASK-53`, `doc-87`, `doc-51`).

mod access;
mod corrections;
mod query;
mod subscription;
mod usage;

use std::sync::atomic::{AtomicU64, Ordering};

use org_proto::costs::{
    CostExportResult, CostPriceListParams, CostPriceListResult, CostPriceSetParams,
    CostPriceSetResult, CostQueryParams, CostQueryResult, CostSubscriptionUsageListParams,
    CostSubscriptionUsageListResult, CostUsageCorrectParams, CostUsageCorrectResult,
    CostUsageRecordParams, CostUsageRecordResult,
};
use org_proto::ApiError;
use org_store::Sha256;

use self::access::{list_pm_project_ids, require_active_member_for, require_owner_for};
use self::corrections::op_cost_usage_correct;
use self::query::{op_cost_export_csv, op_cost_query};
use self::subscription::{op_save_subscription_usage, op_subscription_usage_list};
use self::usage::{
    op_cost_price_list, op_cost_price_set, op_cost_usage_record, op_record_usage_internal,
};
use crate::accounting::{StoredSubscriptionUsageSnapshot, SubscriptionCredentialPresence};
use crate::error::OfficeError;
use crate::organization::records::Employee;

use super::Office;

static UUID_SEQ: AtomicU64 = AtomicU64::new(1);
static SUBSCRIPTION_ATTEMPT_SEQ: AtomicU64 = AtomicU64::new(1);

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
    pub fn next_uuid_v7(&self) -> String {
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
        Ok(self
            .accounting
            .get_subscription_usage(&self.info.org_id, connection_id)?)
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
