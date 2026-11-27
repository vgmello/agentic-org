//! [`AccountingStore`] implementation over the durable JSONL + isolated SQLite projection.

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use org_proto::costs::{CostPriceListParams, ModelPriceView, UsageRecordView};
use org_proto::{ApiError, ErrorCode};
use rusqlite::{params, OptionalExtension};

use org_office::accounting::{
    same_price_semantics, AccountingStore, StoredSubscriptionUsageSnapshot, UsageCoverageSummary,
    UsageStreamCheckpoint, VerifiedStreamWatermark,
};
use org_office::OfficeError;

use super::files::{
    read_durable_snapshot_if_present, sync_usage_dir_and_parent, write_durable_subscription_usage,
};
use super::rows::{
    insert_price_row, insert_usage_row_strict, upsert_checkpoint_row, upsert_subscription_usage_row,
};
use super::verify::{
    check_existing_usage, check_supersedes_valid, evaluate_stream_coverage_row,
    load_checkpoint_coverage_rows,
};
use super::{SqliteAccountingStore, VerifiedStreamHead};

impl AccountingStore for SqliteAccountingStore {
    fn set_price(
        &self,
        org_id: &str,
        price: ModelPriceView,
    ) -> Result<Result<ModelPriceView, ApiError>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        if let Some(existing_json) = conn
            .query_row(
                "SELECT json_payload FROM model_prices WHERE organization_id=?1 AND id=?2",
                params![org_id, &price.id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(OfficeError::storage)?
        {
            let existing: ModelPriceView =
                serde_json::from_str(&existing_json).map_err(OfficeError::storage)?;
            if same_price_semantics(&existing, &price) {
                return Ok(Ok(existing));
            }
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "model price version is immutable once created",
            )));
        }
        let dup: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM model_prices WHERE organization_id=?1 AND provider=?2 AND model=?3 AND effective_from=?4)",
                params![org_id, &price.provider, &price.model, &price.effective_from],
                |r| r.get(0),
            )
            .map_err(OfficeError::storage)?;
        if dup {
            return Ok(Err(ApiError::new(
                ErrorCode::AlreadyExists,
                "a price version already exists for this provider, model, and effective_from",
            )));
        }
        if let Err(err) = self.write_price_durable(&price) {
            self.needs_reconcile.store(true, Ordering::Release);
            return Err(err);
        }
        let proj = (|| -> Result<(), OfficeError> {
            let tx = conn.transaction().map_err(OfficeError::storage)?;
            insert_price_row(&tx, org_id, &price)?;
            tx.commit().map_err(OfficeError::storage)?;
            Ok(())
        })();
        if let Err(err) = proj {
            self.needs_reconcile.store(true, Ordering::Release);
            return Err(err);
        }
        Ok(Ok(price))
    }

    fn list_prices(
        &self,
        org_id: &str,
        params: &CostPriceListParams,
    ) -> Result<Vec<ModelPriceView>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        let mut stmt = conn
            .prepare(
                "SELECT json_payload FROM model_prices WHERE organization_id=?1 \
                 ORDER BY provider ASC, model ASC, effective_from DESC",
            )
            .map_err(OfficeError::storage)?;
        let rows = stmt
            .query_map([org_id], |r| r.get::<_, String>(0))
            .map_err(OfficeError::storage)?;
        let mut out = Vec::new();
        for row in rows {
            let p: ModelPriceView = serde_json::from_str(&row.map_err(OfficeError::storage)?)
                .map_err(OfficeError::storage)?;
            if params.provider.as_deref().is_none_or(|pr| p.provider == pr)
                && params.model.as_deref().is_none_or(|m| p.model == m)
            {
                out.push(p);
            }
        }
        Ok(out)
    }

    fn effective_price(
        &self,
        org_id: &str,
        provider: &str,
        model: &str,
        occurred_at: &str,
    ) -> Result<Option<ModelPriceView>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        let mut stmt = conn
            .prepare(
                "SELECT json_payload FROM model_prices \
                 WHERE organization_id=?1 AND provider=?2 AND model=?3 \
                 ORDER BY effective_from DESC, id DESC",
            )
            .map_err(OfficeError::storage)?;
        let occ_ms = org_proto::parse_rfc3339_ms(occurred_at);
        let rows = stmt
            .query_map(params![org_id, provider, model], |r| r.get::<_, String>(0))
            .map_err(OfficeError::storage)?;
        for row in rows {
            let p: ModelPriceView = serde_json::from_str(&row.map_err(OfficeError::storage)?)
                .map_err(OfficeError::storage)?;
            let eligible = match (org_proto::parse_rfc3339_ms(&p.effective_from), occ_ms) {
                (Some(ef), Some(oc)) => ef <= oc,
                _ => p.effective_from.as_str() <= occurred_at,
            };
            if eligible {
                return Ok(Some(p));
            }
        }
        Ok(None)
    }

    fn append_usage(
        &self,
        org_id: &str,
        mut record: UsageRecordView,
    ) -> Result<Result<(UsageRecordView, bool), ApiError>, OfficeError> {
        let mut state = self.lock()?;
        self.ensure_reconciled(&mut state, org_id)?;
        if let Some(res) = check_existing_usage(&state.conn, org_id, &record)? {
            return Ok(res);
        }
        if let Some(err) = check_supersedes_valid(&state.conn, org_id, &record)? {
            return Ok(Err(err));
        }
        let stream_key = (
            org_id.to_owned(),
            record.origin_office_id.clone(),
            record.producer_id.clone(),
        );
        // O(1) check against staged stream set and verified-stream map maintained across rebuild +
        // transactional appends (`TASK-97` / Final Gate Pass 5 #1, Pass 6 #1/#4, Pass 8 #7).
        if state.staged_streams.contains(&stream_key) {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "cannot append usage to a stream with staged or unverified records",
            )));
        }
        let existing_cp: Option<(i64, Option<String>, Option<String>)> = state
            .conn
            .query_row(
                "SELECT high_water_sequence, head_sha256, chain_sha256 \
                 FROM usage_stream_checkpoints \
                 WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3",
                params![org_id, &record.origin_office_id, &record.producer_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(OfficeError::storage)?;
        let (prev_chain, write_zero_anchor) = match existing_cp {
            Some((hw, head_sha, chain_sha)) => {
                let verified_match = state.verified_streams.get(&stream_key).filter(|h| {
                    hw >= 0
                        && h.high_water_sequence == (hw as u64)
                        && h.head_sha256 == head_sha
                        && h.chain_sha256 == chain_sha
                });
                let Some(head) = verified_match else {
                    return Ok(Err(ApiError::new(
                        ErrorCode::Conflict,
                        "cannot append usage to a stream whose checkpoint verification failed",
                    )));
                };
                (head.chain_sha256.clone(), false)
            }
            None => {
                let uncheckpointed_rows: bool = state
                    .conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM usage_records \
                         WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3)",
                        params![org_id, &record.origin_office_id, &record.producer_id],
                        |r| r.get(0),
                    )
                    .map_err(OfficeError::storage)?;
                if uncheckpointed_rows {
                    return Ok(Err(ApiError::new(
                        ErrorCode::Conflict,
                        "cannot append usage to an uncheckpointed stream",
                    )));
                }
                (None, true)
            }
        };
        // Allocate source_sequence atomically under the store mutex across checkpoints,
        // projected rows, and staged rows (`TASK-97` / Pass 1 #5, Final Gate Pass 5 #1).
        let cur_seq: Option<i64> = state
            .conn
            .query_row(
                "SELECT MAX(seq) FROM (\
                    SELECT high_water_sequence AS seq FROM usage_stream_checkpoints \
                     WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3 \
                    UNION ALL \
                    SELECT COALESCE(MAX(source_sequence), 0) AS seq FROM usage_records \
                     WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3 \
                    UNION ALL \
                    SELECT COALESCE(MAX(source_sequence), 0) AS seq FROM staged_usage_records \
                     WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3\
                 )",
                params![org_id, &record.origin_office_id, &record.producer_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(OfficeError::storage)?
            .flatten();
        record.source_sequence = cur_seq.map_or(0, |v| v.max(0) as u64).saturating_add(1);
        let next_chain =
            org_office::accounting::chain_usage_stream_head(prev_chain.as_deref(), &record);
        let cp = UsageStreamCheckpoint {
            organization_id: org_id.to_owned(),
            origin_office_id: record.origin_office_id.clone(),
            producer_id: record.producer_id.clone(),
            high_water_sequence: record.source_sequence,
            head_sha256: Some(record.payload_sha256.clone()),
            chain_sha256: Some(next_chain),
            state: "open".into(),
            checkpointed_at: record.recorded_at.clone(),
        };
        if let Err(err) = self.append_usage_durable(&record, &cp, write_zero_anchor) {
            // Event may have landed in durable JSONL before checkpoint failed; force
            // reconciliation before any subsequent sequence allocation (`TASK-97` / Finding #5).
            self.needs_reconcile.store(true, Ordering::Release);
            return Err(err);
        }
        let proj = (|| -> Result<(), OfficeError> {
            let tx = state.conn.transaction().map_err(OfficeError::storage)?;
            insert_usage_row_strict(&tx, org_id, &record, cp.chain_sha256.as_deref())?;
            upsert_checkpoint_row(&tx, &cp)?;
            tx.commit().map_err(OfficeError::storage)?;
            Ok(())
        })();
        if let Err(err) = proj {
            self.needs_reconcile.store(true, Ordering::Release);
            return Err(err);
        }
        let new_head = VerifiedStreamHead {
            high_water_sequence: cp.high_water_sequence,
            head_sha256: cp.head_sha256.clone(),
            chain_sha256: cp.chain_sha256.clone(),
        };
        state
            .verified_streams
            .insert(stream_key.clone(), new_head.clone());
        state.verified_prefixes.insert(stream_key, new_head);
        Ok(Ok((record, false)))
    }

    fn get_usage(&self, org_id: &str, id: &str) -> Result<Option<UsageRecordView>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        let found: Option<String> = conn
            .query_row(
                "SELECT json_payload FROM usage_records WHERE organization_id=?1 AND id=?2",
                params![org_id, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(OfficeError::storage)?;
        found
            .map(|s| serde_json::from_str(&s).map_err(OfficeError::storage))
            .transpose()
    }

    fn find_by_source_event(
        &self,
        org_id: &str,
        origin_office_id: &str,
        producer_id: &str,
        source_event_id: &str,
    ) -> Result<Option<UsageRecordView>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        let found: Option<String> = conn
            .query_row(
                "SELECT json_payload FROM usage_records \
                 WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3 AND source_event_id=?4",
                params![org_id, origin_office_id, producer_id, source_event_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(OfficeError::storage)?;
        found
            .map(|s| serde_json::from_str(&s).map_err(OfficeError::storage))
            .transpose()
    }

    fn is_superseded(&self, org_id: &str, id: &str) -> Result<bool, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM usage_records WHERE organization_id=?1 AND supersedes_id=?2)",
            params![org_id, id],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)
    }

    fn effective_records(&self, org_id: &str) -> Result<Vec<UsageRecordView>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        let mut stmt = conn
            .prepare(
                "SELECT u.json_payload FROM usage_records u \
                 WHERE u.organization_id=?1 \
                   AND NOT EXISTS (SELECT 1 FROM usage_records c \
                                   WHERE c.organization_id=u.organization_id AND c.supersedes_id=u.id) \
                 ORDER BY u.occurred_at DESC, u.id DESC",
            )
            .map_err(OfficeError::storage)?;
        let rows = stmt
            .query_map([org_id], |r| r.get::<_, String>(0))
            .map_err(OfficeError::storage)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(
                serde_json::from_str(&row.map_err(OfficeError::storage)?)
                    .map_err(OfficeError::storage)?,
            );
        }
        Ok(out)
    }

    fn coverage_summary(&self, org_id: &str) -> Result<UsageCoverageSummary, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        // O(streams) coverage from in-memory `staged_streams` + `verified_streams` + per-stream
        // `usage_stream_checkpoints` rows — zero full scans of `usage_records` or
        // `staged_usage_records` (`TASK-97` / Final Gate Pass 7 #5, Pass 8 #1/#3/#7).
        let mut incomplete: BTreeSet<(String, String)> = conn
            .staged_streams
            .iter()
            .filter(|(o, _, _)| o == org_id)
            .map(|(_, office, prod)| (office.clone(), prod.clone()))
            .collect();
        let rows = load_checkpoint_coverage_rows(&conn.conn, org_id)?;
        let mut as_of: Option<String> = None;
        let mut verified_watermarks: Vec<VerifiedStreamWatermark> = Vec::new();
        for row in rows {
            if let Some(wm) = evaluate_stream_coverage_row(&conn, org_id, &row, &mut incomplete)? {
                verified_watermarks.push(wm);
            }
            if as_of.as_deref().is_none_or(|cur| row.at.as_str() > cur) {
                as_of = Some(row.at);
            }
        }
        let checkpoint_ref = (!verified_watermarks.is_empty())
            .then(|| serde_json::to_string(&verified_watermarks))
            .transpose()
            .map_err(OfficeError::storage)?;
        Ok(UsageCoverageSummary {
            incomplete_streams: incomplete.len() as u64,
            coverage_as_of: as_of,
            checkpoint_ref,
        })
    }

    fn stream_continues_watermark(
        &self,
        org_id: &str,
        prior: &VerifiedStreamWatermark,
    ) -> Result<bool, OfficeError> {
        if prior.high_water_sequence == 0 {
            return Ok(true);
        }
        let mut state = self.lock()?;
        self.ensure_reconciled(&mut state, org_id)?;
        let key = (
            org_id.to_owned(),
            prior.origin_office_id.clone(),
            prior.producer_id.clone(),
        );
        let Some(head) = state.verified_prefixes.get(&key) else {
            return Ok(false);
        };
        if head.high_water_sequence < prior.high_water_sequence {
            return Ok(false);
        }
        let Some(expected_chain) = prior.chain_sha256.as_deref() else {
            return Ok(true);
        };
        if head.high_water_sequence == prior.high_water_sequence {
            return Ok(head.chain_sha256.as_deref() == Some(expected_chain));
        }
        // O(1) unique-index lookup verifying cryptographic chain continuity at `prior.high_water_sequence`.
        let actual_chain: Option<String> = state
            .conn
            .query_row(
                "SELECT chain_sha256 FROM usage_records \
                 WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3 \
                   AND source_sequence=?4",
                params![
                    org_id,
                    &prior.origin_office_id,
                    &prior.producer_id,
                    prior.high_water_sequence as i64,
                ],
                |r| r.get(0),
            )
            .optional()
            .map_err(OfficeError::storage)?
            .flatten();
        Ok(actual_chain.as_deref() == Some(expected_chain))
    }

    fn save_subscription_usage(
        &self,
        org_id: &str,
        mut snapshot: StoredSubscriptionUsageSnapshot,
    ) -> Result<(), OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        let sqlite_existing: Option<StoredSubscriptionUsageSnapshot> = conn
            .query_row(
                "SELECT json_payload FROM subscription_usage_snapshots \
                 WHERE organization_id=?1 AND connection_id=?2",
                params![org_id, &snapshot.connection_id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(OfficeError::storage)?
            .and_then(|raw| serde_json::from_str(&raw).ok());
        let durable_existing =
            read_durable_snapshot_if_present(&self.local_usage_dir, &snapshot.connection_id)?;
        let sqlite_seq = sqlite_existing.as_ref().map_or(0, |s| s.attempt_seq);
        let durable_seq = durable_existing.as_ref().map_or(0, |s| s.attempt_seq);
        if let Some(ref dur) = durable_existing {
            if durable_seq > sqlite_seq {
                // Ensure the on-disk snapshot directory entry is durably synced before
                // promoting a disk-only snapshot into SQLite.
                sync_usage_dir_and_parent(&self.local_usage_dir)?;
                let tx = conn.transaction().map_err(OfficeError::storage)?;
                upsert_subscription_usage_row(&tx, org_id, dur)?;
                tx.commit().map_err(OfficeError::storage)?;
            }
        }
        let winning_existing = if durable_seq >= sqlite_seq {
            durable_existing.as_ref().or(sqlite_existing.as_ref())
        } else {
            sqlite_existing.as_ref().or(durable_existing.as_ref())
        };
        let max_existing = sqlite_seq.max(durable_seq);
        if snapshot.attempt_seq > 0 && max_existing > snapshot.attempt_seq {
            return Ok(());
        }
        org_office::accounting::merge_prior_for_unavailable(&mut snapshot, winning_existing);
        if org_office::accounting::validate_stored_snapshot(&snapshot).is_err() {
            return Err(OfficeError::Invalid(
                "merged subscription usage snapshot exceeds validation bounds",
            ));
        }
        if let Err(err) = write_durable_subscription_usage(
            &self.durable_org_root,
            &self.local_usage_dir,
            &snapshot,
        ) {
            self.needs_reconcile.store(true, Ordering::Release);
            return Err(err);
        }
        let proj = (|| -> Result<(), OfficeError> {
            let tx = conn.transaction().map_err(OfficeError::storage)?;
            upsert_subscription_usage_row(&tx, org_id, &snapshot)?;
            tx.commit().map_err(OfficeError::storage)?;
            Ok(())
        })();
        if let Err(err) = proj {
            self.needs_reconcile.store(true, Ordering::Release);
            return Err(err);
        }
        Ok(())
    }

    fn list_subscription_usage(
        &self,
        org_id: &str,
    ) -> Result<Vec<StoredSubscriptionUsageSnapshot>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        let mut stmt = conn
            .prepare(
                "SELECT json_payload FROM subscription_usage_snapshots \
                 WHERE organization_id=?1 ORDER BY connection_id ASC",
            )
            .map_err(OfficeError::storage)?;
        let rows = stmt
            .query_map([org_id], |r| r.get::<_, String>(0))
            .map_err(OfficeError::storage)?;
        let mut out = Vec::new();
        for row in rows {
            let json = row.map_err(OfficeError::storage)?;
            out.push(serde_json::from_str(&json).map_err(OfficeError::storage)?);
        }
        Ok(out)
    }

    fn get_subscription_usage(
        &self,
        org_id: &str,
        connection_id: &str,
    ) -> Result<Option<StoredSubscriptionUsageSnapshot>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        let json: Option<String> = conn
            .query_row(
                "SELECT json_payload FROM subscription_usage_snapshots \
                 WHERE organization_id=?1 AND connection_id=?2",
                params![org_id, connection_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(OfficeError::storage)?;
        json.map(|raw| serde_json::from_str(&raw).map_err(OfficeError::storage))
            .transpose()
    }
}
