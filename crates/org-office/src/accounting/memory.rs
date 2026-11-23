//! Engine-independent in-memory [`AccountingStore`] for `L2` conformance tests (`TASK-53`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use org_proto::costs::{CostPriceListParams, ModelPriceView, UsageRecordView};
use org_proto::{ApiError, ErrorCode};

use super::{
    same_price_semantics, AccountingStore, StoredSubscriptionUsageSnapshot, UsageCoverageSummary,
    UsageStreamCheckpoint,
};
use crate::error::OfficeError;

#[derive(Default)]
struct State {
    prices: BTreeMap<(String, String), ModelPriceView>,
    records: BTreeMap<(String, String), UsageRecordView>,
    source_keys: BTreeMap<(String, String, String, String), String>,
    superseded: BTreeSet<(String, String)>,
    checkpoints: BTreeMap<(String, String, String), UsageStreamCheckpoint>,
    subscription_usage: BTreeMap<(String, String), StoredSubscriptionUsageSnapshot>,
}

/// In-memory [`AccountingStore`] enforcing all `doc-87` immutability, replay, and supersession rules.
#[derive(Default)]
pub struct MemoryAccountingStore {
    inner: Mutex<State>,
}

impl MemoryAccountingStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, OfficeError> {
        self.inner
            .lock()
            .map_err(|_| OfficeError::Invalid("memory accounting store poisoned"))
    }
}

impl AccountingStore for MemoryAccountingStore {
    fn set_price(
        &self,
        org_id: &str,
        price: ModelPriceView,
    ) -> Result<Result<ModelPriceView, ApiError>, OfficeError> {
        let mut s = self.lock()?;
        if let Some(existing) = s.prices.get(&(org_id.to_owned(), price.id.clone())) {
            if same_price_semantics(existing, &price) {
                return Ok(Ok(existing.clone()));
            }
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "model price version is immutable once created",
            )));
        }
        let dup_key = s.prices.iter().any(|((o, _), p)| {
            o == org_id
                && p.provider == price.provider
                && p.model == price.model
                && p.effective_from == price.effective_from
        });
        if dup_key {
            return Ok(Err(ApiError::new(
                ErrorCode::AlreadyExists,
                "a price version already exists for this provider, model, and effective_from",
            )));
        }
        s.prices
            .insert((org_id.to_owned(), price.id.clone()), price.clone());
        Ok(Ok(price))
    }

    fn list_prices(
        &self,
        org_id: &str,
        params: &CostPriceListParams,
    ) -> Result<Vec<ModelPriceView>, OfficeError> {
        let s = self.lock()?;
        let mut out: Vec<ModelPriceView> = s
            .prices
            .iter()
            .filter(|((o, _), p)| {
                o == org_id
                    && params.provider.as_deref().is_none_or(|pr| p.provider == pr)
                    && params.model.as_deref().is_none_or(|m| p.model == m)
            })
            .map(|(_, p)| p.clone())
            .collect();
        out.sort_by(|a, b| {
            a.provider
                .cmp(&b.provider)
                .then_with(|| a.model.cmp(&b.model))
                .then_with(|| b.effective_from.cmp(&a.effective_from))
        });
        Ok(out)
    }

    fn effective_price(
        &self,
        org_id: &str,
        provider: &str,
        model: &str,
        occurred_at: &str,
    ) -> Result<Option<ModelPriceView>, OfficeError> {
        let s = self.lock()?;
        let occ_ms = org_proto::parse_rfc3339_ms(occurred_at);
        let best = s
            .prices
            .iter()
            .filter(|((o, _), p)| {
                o == org_id && p.provider == provider && p.model == model && {
                    match (org_proto::parse_rfc3339_ms(&p.effective_from), occ_ms) {
                        (Some(ef), Some(oc)) => ef <= oc,
                        _ => p.effective_from.as_str() <= occurred_at,
                    }
                }
            })
            .map(|(_, p)| p)
            .max_by(|a, b| {
                a.effective_from
                    .cmp(&b.effective_from)
                    .then_with(|| a.id.cmp(&b.id))
            })
            .cloned();
        Ok(best)
    }

    fn append_usage(
        &self,
        org_id: &str,
        mut record: UsageRecordView,
    ) -> Result<Result<(UsageRecordView, bool), ApiError>, OfficeError> {
        let mut s = self.lock()?;
        let src_key = (
            org_id.to_owned(),
            record.origin_office_id.clone(),
            record.producer_id.clone(),
            record.source_event_id.clone(),
        );
        if let Some(existing_id) = s.source_keys.get(&src_key).cloned() {
            if let Some(existing) = s.records.get(&(org_id.to_owned(), existing_id)) {
                if existing.payload_sha256 == record.payload_sha256 {
                    return Ok(Ok((existing.clone(), true)));
                }
                return Ok(Err(ApiError::new(
                    ErrorCode::Conflict,
                    "source_event_id already recorded with a different payload",
                )));
            }
        }
        if let Some(existing) = s.records.get(&(org_id.to_owned(), record.id.clone())) {
            if existing.payload_sha256 == record.payload_sha256 {
                return Ok(Ok((existing.clone(), true)));
            }
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "usage record id already exists with a different payload",
            )));
        }
        if let Some(parent_id) = &record.supersedes_id {
            if !s
                .records
                .contains_key(&(org_id.to_owned(), parent_id.clone()))
            {
                return Ok(Err(ApiError::new(
                    ErrorCode::NotFound,
                    "superseded usage record does not exist",
                )));
            }
            if !s.superseded.insert((org_id.to_owned(), parent_id.clone())) {
                return Ok(Err(ApiError::new(
                    ErrorCode::Conflict,
                    "usage record is already superseded; correct the current leaf instead",
                )));
            }
        }
        let cp_key = (
            org_id.to_owned(),
            record.origin_office_id.clone(),
            record.producer_id.clone(),
        );
        let next_seq = s
            .checkpoints
            .get(&cp_key)
            .map_or(0, |cp| cp.high_water_sequence)
            .saturating_add(1);
        record.source_sequence = next_seq;
        s.checkpoints.insert(
            cp_key,
            UsageStreamCheckpoint {
                organization_id: org_id.to_owned(),
                origin_office_id: record.origin_office_id.clone(),
                producer_id: record.producer_id.clone(),
                high_water_sequence: next_seq,
                head_sha256: Some(record.payload_sha256.clone()),
                state: "open".into(),
                checkpointed_at: record.recorded_at.clone(),
            },
        );
        s.source_keys.insert(src_key, record.id.clone());
        s.records
            .insert((org_id.to_owned(), record.id.clone()), record.clone());
        Ok(Ok((record, false)))
    }

    fn get_usage(&self, org_id: &str, id: &str) -> Result<Option<UsageRecordView>, OfficeError> {
        let s = self.lock()?;
        Ok(s.records.get(&(org_id.to_owned(), id.to_owned())).cloned())
    }

    fn find_by_source_event(
        &self,
        org_id: &str,
        origin_office_id: &str,
        producer_id: &str,
        source_event_id: &str,
    ) -> Result<Option<UsageRecordView>, OfficeError> {
        let s = self.lock()?;
        let key = (
            org_id.to_owned(),
            origin_office_id.to_owned(),
            producer_id.to_owned(),
            source_event_id.to_owned(),
        );
        Ok(s.source_keys
            .get(&key)
            .and_then(|id| s.records.get(&(org_id.to_owned(), id.clone())))
            .cloned())
    }

    fn is_superseded(&self, org_id: &str, id: &str) -> Result<bool, OfficeError> {
        let s = self.lock()?;
        Ok(s.superseded.contains(&(org_id.to_owned(), id.to_owned())))
    }

    fn effective_records(&self, org_id: &str) -> Result<Vec<UsageRecordView>, OfficeError> {
        let s = self.lock()?;
        let mut out: Vec<UsageRecordView> = s
            .records
            .iter()
            .filter(|((o, id), _)| o == org_id && !s.superseded.contains(&(o.clone(), id.clone())))
            .map(|(_, r)| r.clone())
            .collect();
        out.sort_by(|a, b| {
            b.occurred_at
                .cmp(&a.occurred_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(out)
    }

    fn coverage_summary(&self, org_id: &str) -> Result<UsageCoverageSummary, OfficeError> {
        let s = self.lock()?;
        let mut as_of: Option<String> = None;
        let mut refs: Vec<String> = Vec::new();
        for ((o, office, prod), cp) in &s.checkpoints {
            if o != org_id {
                continue;
            }
            if as_of
                .as_deref()
                .is_none_or(|cur| cp.checkpointed_at.as_str() > cur)
            {
                as_of = Some(cp.checkpointed_at.clone());
            }
            refs.push(format!("{office}/{prod}:{}", cp.high_water_sequence));
        }
        let checkpoint_ref = (!refs.is_empty()).then(|| refs.join(","));
        Ok(UsageCoverageSummary {
            incomplete_streams: 0,
            coverage_as_of: as_of,
            checkpoint_ref,
        })
    }

    fn save_subscription_usage(
        &self,
        org_id: &str,
        mut snapshot: StoredSubscriptionUsageSnapshot,
    ) -> Result<(), OfficeError> {
        let mut s = self.lock()?;
        let key = (org_id.to_owned(), snapshot.connection_id.clone());
        let existing = s.subscription_usage.get(&key);
        if let Some(prev) = existing {
            if snapshot.attempt_seq > 0 && prev.attempt_seq > snapshot.attempt_seq {
                return Ok(());
            }
        }
        super::merge_prior_for_unavailable(&mut snapshot, existing);
        if super::validate_stored_snapshot(&snapshot).is_err() {
            return Err(OfficeError::Invalid(
                "merged subscription usage snapshot exceeds validation bounds",
            ));
        }
        s.subscription_usage.insert(key, snapshot);
        Ok(())
    }

    fn list_subscription_usage(
        &self,
        org_id: &str,
    ) -> Result<Vec<StoredSubscriptionUsageSnapshot>, OfficeError> {
        let s = self.lock()?;
        let mut out: Vec<StoredSubscriptionUsageSnapshot> = s
            .subscription_usage
            .iter()
            .filter(|((o, _), _)| o == org_id)
            .map(|(_, v)| v.clone())
            .collect();
        out.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
        Ok(out)
    }

    fn get_subscription_usage(
        &self,
        org_id: &str,
        connection_id: &str,
    ) -> Result<Option<StoredSubscriptionUsageSnapshot>, OfficeError> {
        let s = self.lock()?;
        Ok(s.subscription_usage
            .get(&(org_id.to_owned(), connection_id.to_owned()))
            .cloned())
    }
}
