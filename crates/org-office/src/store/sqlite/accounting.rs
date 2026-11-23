//! Durable JSONL + isolated SQLite adapter for `L2` usage & cost accounting
//! (`TASK-53`, `doc-87`, `ADR-0016`).
//!
//! Durable source of truth (never in shared org Git for raw usage):
//! - `<durable>/accounting/prices/<price-id>.json`
//! - `<durable>/accounting/usage/<origin-office-id>/<producer-id>/<UTC-date>.jsonl`
//! - `<durable>/accounting/checkpoints/<origin-office-id>/<producer-id>.json`
//!
//! Rebuildable indexed projection (isolated 0700/0600, never plaintext `office.db`):
//! - `<runtime>/<office-id>/protected/accounting.db`

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use org_proto::costs::{CostPriceListParams, ModelPriceView, UsageRecordView};
use org_proto::{ApiError, ErrorCode};
use rusqlite::{params, Connection, OptionalExtension};

use crate::accounting::{
    same_price_semantics, AccountingStore, StoredSubscriptionUsageSnapshot, UsageCoverageSummary,
    UsageStreamCheckpoint, VerifiedStreamWatermark,
};
use crate::error::OfficeError;
use crate::private_fs::{
    absolute, full_sync, private_dir, private_dir_synced, sync_dir, tighten_dir, tighten_file,
    write_private_atomic,
};

use super::companion::open_and_reconcile_companion_db;

const ACCOUNTING_ADAPTER_FORMAT: u32 = 3;
const ACCOUNTING_PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
";

const ACCOUNTING_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS model_prices (
    organization_id TEXT NOT NULL,
    id TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    effective_from TEXT NOT NULL,
    json_payload TEXT NOT NULL,
    PRIMARY KEY (organization_id, id),
    UNIQUE (organization_id, provider, model, effective_from)
) STRICT;

CREATE TABLE IF NOT EXISTS usage_records (
    organization_id TEXT NOT NULL,
    id TEXT NOT NULL,
    origin_office_id TEXT NOT NULL,
    producer_id TEXT NOT NULL,
    source_event_id TEXT NOT NULL,
    source_sequence INTEGER NOT NULL CHECK (source_sequence >= 1),
    occurred_at TEXT NOT NULL,
    payload_sha256 TEXT NOT NULL,
    chain_sha256 TEXT,
    supersedes_id TEXT,
    json_payload TEXT NOT NULL,
    PRIMARY KEY (organization_id, id),
    UNIQUE (organization_id, origin_office_id, producer_id, source_event_id),
    UNIQUE (organization_id, origin_office_id, producer_id, source_sequence),
    UNIQUE (organization_id, supersedes_id)
) STRICT;

CREATE INDEX IF NOT EXISTS usage_records_org_occurred
    ON usage_records(organization_id, occurred_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS usage_stream_checkpoints (
    organization_id TEXT NOT NULL,
    origin_office_id TEXT NOT NULL,
    producer_id TEXT NOT NULL,
    high_water_sequence INTEGER NOT NULL CHECK (high_water_sequence >= 0),
    head_sha256 TEXT,
    chain_sha256 TEXT,
    state TEXT NOT NULL,
    checkpointed_at TEXT NOT NULL,
    PRIMARY KEY (organization_id, origin_office_id, producer_id)
) STRICT;

CREATE TABLE IF NOT EXISTS staged_usage_records (
    organization_id TEXT NOT NULL,
    id TEXT NOT NULL,
    origin_office_id TEXT NOT NULL,
    producer_id TEXT NOT NULL,
    source_event_id TEXT NOT NULL,
    source_sequence INTEGER NOT NULL CHECK (source_sequence >= 1),
    supersedes_id TEXT,
    json_payload TEXT NOT NULL,
    PRIMARY KEY (organization_id, id)
) STRICT;

CREATE INDEX IF NOT EXISTS staged_usage_stream
    ON staged_usage_records(organization_id, origin_office_id, producer_id);

CREATE TABLE IF NOT EXISTS subscription_usage_snapshots (
    organization_id TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    office_id TEXT NOT NULL,
    status TEXT NOT NULL,
    collected_at TEXT NOT NULL,
    fresh_until TEXT,
    json_payload TEXT NOT NULL,
    PRIMARY KEY (organization_id, connection_id)
) STRICT;
"#;

#[derive(Clone, Debug, PartialEq, Eq)]
struct VerifiedStreamHead {
    high_water_sequence: u64,
    head_sha256: Option<String>,
    chain_sha256: Option<String>,
}

type VerifiedStreamMap = BTreeMap<(String, String, String), VerifiedStreamHead>;
type StagedStreamSet = BTreeSet<(String, String, String)>;

struct ReconcileMetadata {
    verified_streams: VerifiedStreamMap,
    verified_prefixes: VerifiedStreamMap,
    staged_streams: StagedStreamSet,
}

struct PartitionedStreams {
    projected: Vec<(UsageRecordView, Option<String>)>,
    staged: Vec<UsageRecordView>,
    verified: VerifiedStreamMap,
    verified_prefixes: VerifiedStreamMap,
    staged_streams: StagedStreamSet,
}

struct AccountingState {
    conn: Connection,
    /// Streams whose complete `1..=hw` prefix passed contiguous `head_sha256` + `chain_sha256`
    /// verification during the last reconcile (with zero staged rows) and are advanced
    /// transactionally on every append so `append_usage` and `coverage_summary` stay O(1)
    /// (`TASK-97` / Final Gate Pass 6 #4).
    verified_streams: VerifiedStreamMap,
    /// Every stream whose `1..=hw` checkpoint prefix passed contiguous `head_sha256` +
    /// `chain_sha256` verification — INCLUDING streams that also carry staged tail/orphan rows —
    /// so projected prefix spend always records its watermark in `checkpoint_ref` (`Pass 9 #1`).
    verified_prefixes: VerifiedStreamMap,
    /// Distinct `(org_id, origin_office_id, producer_id)` streams currently holding staged rows so
    /// `coverage_summary` and `append_usage` stay O(streams) even with thousands of staged rows
    /// (`TASK-97` / Final Gate Pass 8 #7).
    staged_streams: StagedStreamSet,
}

impl std::ops::Deref for AccountingState {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}

impl std::ops::DerefMut for AccountingState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.conn
    }
}

/// Isolated durable JSONL + SQLite [`AccountingStore`].
pub struct SqliteAccountingStore {
    durable_org_root: PathBuf,
    durable_root: PathBuf,
    local_usage_dir: PathBuf,
    office_id: String,
    /// Set whenever a durable append or SQLite commit fails mid-flight so the next append
    /// reconciles durable JSONL into SQLite BEFORE allocating the next `source_sequence`
    /// (`TASK-97` / Finding #5).
    needs_reconcile: AtomicBool,
    state: Mutex<AccountingState>,
}

impl SqliteAccountingStore {
    /// Opens `<runtime_root>/<office_id>/protected/accounting.db`, rebuilding automatically
    /// from `<durable_root>/accounting/` and `<durable_root>/local/offices/<office_id>/subscription-usage/`
    /// whenever the projection is missing, empty, corrupt, or schema-mismatched.
    pub fn open(
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
    ) -> Result<Self, OfficeError> {
        let durable_acc = durable_root.join("accounting");
        private_dir_synced(durable_root, &durable_acc).map_err(OfficeError::storage)?;
        let local_usage_dir = durable_root
            .join("local")
            .join("offices")
            .join(office_id)
            .join("subscription-usage");
        let office_dir = runtime_root.join(office_id);
        private_dir(&office_dir).map_err(OfficeError::storage)?;
        let protected = office_dir.join("protected");
        private_dir(&protected).map_err(OfficeError::storage)?;
        tighten_dir(&protected).map_err(OfficeError::storage)?;

        let db_path = absolute(protected.join("accounting.db")).map_err(OfficeError::storage)?;
        let mut meta = ReconcileMetadata {
            verified_streams: BTreeMap::new(),
            verified_prefixes: BTreeMap::new(),
            staged_streams: BTreeSet::new(),
        };
        let conn = open_and_reconcile_companion_db(
            &db_path,
            "accounting",
            ACCOUNTING_ADAPTER_FORMAT,
            ACCOUNTING_DDL,
            ACCOUNTING_PRAGMAS,
            |conn| {
                meta = reconcile_durable_conn(
                    conn,
                    &durable_acc,
                    &local_usage_dir,
                    office_id,
                    org_id,
                    true,
                )?;
                Ok(())
            },
        )?;

        Ok(Self {
            durable_org_root: durable_root.to_path_buf(),
            durable_root: durable_acc,
            local_usage_dir,
            office_id: office_id.to_owned(),
            needs_reconcile: AtomicBool::new(false),
            state: Mutex::new(AccountingState {
                conn,
                verified_streams: meta.verified_streams,
                verified_prefixes: meta.verified_prefixes,
                staged_streams: meta.staged_streams,
            }),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, AccountingState>, OfficeError> {
        self.state
            .lock()
            .map_err(|_| OfficeError::Invalid("accounting sqlite connection poisoned"))
    }

    fn ensure_reconciled(
        &self,
        state: &mut AccountingState,
        org_id: &str,
    ) -> Result<(), OfficeError> {
        if self.needs_reconcile.load(Ordering::Acquire) {
            let meta = reconcile_durable_conn(
                &mut state.conn,
                &self.durable_root,
                &self.local_usage_dir,
                &self.office_id,
                org_id,
                true,
            )?;
            state.verified_streams = meta.verified_streams;
            state.verified_prefixes = meta.verified_prefixes;
            state.staged_streams = meta.staged_streams;
            self.needs_reconcile.store(false, Ordering::Release);
        }
        Ok(())
    }

    fn write_price_durable(&self, price: &ModelPriceView) -> Result<(), OfficeError> {
        let dir = self.durable_root.join("prices");
        private_dir_synced(&self.durable_root, &dir).map_err(OfficeError::storage)?;
        let path = dir.join(format!("{}.json", price.id));
        let bytes = serde_json::to_vec_pretty(price).map_err(OfficeError::storage)?;
        write_atomic(&path, &bytes)
    }

    fn append_usage_durable(
        &self,
        rec: &UsageRecordView,
        cp: &UsageStreamCheckpoint,
        write_zero_anchor: bool,
    ) -> Result<(), OfficeError> {
        let cp_dir = self
            .durable_root
            .join("checkpoints")
            .join(safe_segment(&cp.origin_office_id));
        private_dir_synced(&self.durable_root, &cp_dir).map_err(OfficeError::storage)?;
        let cp_path = cp_dir.join(format!("{}.json", safe_segment(&cp.producer_id)));
        // Persist a sequence-0 anchor checkpoint BEFORE writing the first event on a new stream
        // so crash recovery distinguishes an interrupted first write from an unanchored partial
        // restore whose checkpoint was lost (`TASK-97` / Final Gate Pass 6 #1).
        if write_zero_anchor && !cp_path.is_file() {
            let zero_cp = UsageStreamCheckpoint {
                organization_id: cp.organization_id.clone(),
                origin_office_id: cp.origin_office_id.clone(),
                producer_id: cp.producer_id.clone(),
                high_water_sequence: 0,
                head_sha256: None,
                chain_sha256: None,
                state: "open".into(),
                checkpointed_at: cp.checkpointed_at.clone(),
            };
            let zero_bytes = serde_json::to_vec_pretty(&zero_cp).map_err(OfficeError::storage)?;
            write_atomic(&cp_path, &zero_bytes)?;
        }

        let date = rec.recorded_at.get(0..10).unwrap_or("1970-01-01");
        let stream_dir = self
            .durable_root
            .join("usage")
            .join(safe_segment(&rec.origin_office_id))
            .join(safe_segment(&rec.producer_id));
        private_dir_synced(&self.durable_root, &stream_dir).map_err(OfficeError::storage)?;
        let jsonl_path = stream_dir.join(format!("{date}.jsonl"));
        // O(1) last-byte check before appending; only inspects/repairs the trailing fragment
        // if an interrupted write left a non-newline-terminated tail (`TASK-97` / Pass 2 #9).
        if jsonl_path.is_file() {
            ensure_jsonl_ends_with_newline(&jsonl_path)?;
        }
        let mut line = serde_json::to_vec(rec).map_err(OfficeError::storage)?;
        line.push(b'\n');
        let mut file = open_private_append(&jsonl_path)?;
        file.write_all(&line).map_err(OfficeError::storage)?;
        full_sync(&file).map_err(OfficeError::storage)?;
        tighten_file(&jsonl_path).map_err(OfficeError::storage)?;
        sync_dir(&stream_dir).map_err(OfficeError::storage)?;

        let cp_bytes = serde_json::to_vec_pretty(cp).map_err(OfficeError::storage)?;
        write_atomic(&cp_path, &cp_bytes)
    }
}

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
        let next_chain = crate::accounting::chain_usage_stream_head(prev_chain.as_deref(), &record);
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
        crate::accounting::merge_prior_for_unavailable(&mut snapshot, winning_existing);
        if crate::accounting::validate_stored_snapshot(&snapshot).is_err() {
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

fn upsert_subscription_usage_row(
    conn: &Connection,
    org_id: &str,
    snap: &StoredSubscriptionUsageSnapshot,
) -> Result<(), OfficeError> {
    let payload = serde_json::to_string(snap).map_err(OfficeError::storage)?;
    conn.execute(
        "INSERT INTO subscription_usage_snapshots \
         (organization_id, connection_id, office_id, status, collected_at, fresh_until, json_payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(organization_id, connection_id) DO UPDATE SET \
         office_id=excluded.office_id, status=excluded.status, \
         collected_at=excluded.collected_at, fresh_until=excluded.fresh_until, \
         json_payload=excluded.json_payload",
        params![
            org_id,
            &snap.connection_id,
            &snap.office_id,
            snap.status.as_str(),
            &snap.collected_at,
            &snap.fresh_until,
            &payload,
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

fn sync_usage_dir_and_parent(dir: &Path) -> Result<(), OfficeError> {
    sync_dir(dir).map_err(OfficeError::storage)?;
    if let Some(parent) = dir.parent() {
        sync_dir(parent).map_err(OfficeError::storage)?;
    }
    Ok(())
}

fn write_durable_subscription_usage(
    durable_org_root: &Path,
    dir: &Path,
    snap: &StoredSubscriptionUsageSnapshot,
) -> Result<(), OfficeError> {
    private_dir_synced(durable_org_root, dir).map_err(OfficeError::storage)?;
    sync_usage_dir_and_parent(dir)?;
    let path = dir.join(format!("{}.json", snap.connection_id));
    let bytes = serde_json::to_vec_pretty(snap).map_err(OfficeError::storage)?;
    write_atomic(&path, &bytes)
}

const MAX_DURABLE_SNAPSHOT_FILE_BYTES: u64 = 64 * 1024;

fn read_bounded_snapshot_file(path: &Path) -> Result<Vec<u8>, OfficeError> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(OfficeError::storage)?;
    let mut bytes = Vec::new();
    file.take(MAX_DURABLE_SNAPSHOT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(OfficeError::storage)?;
    if bytes.len() as u64 > MAX_DURABLE_SNAPSHOT_FILE_BYTES {
        return Err(OfficeError::Invalid(
            "durable subscription usage snapshot file exceeds size limit",
        ));
    }
    Ok(bytes)
}

fn read_durable_snapshot_if_present(
    dir: &Path,
    connection_id: &str,
) -> Result<Option<StoredSubscriptionUsageSnapshot>, OfficeError> {
    let path = dir.join(format!("{connection_id}.json"));
    match fs::File::open(&path) {
        Ok(_) => {
            let bytes = read_bounded_snapshot_file(&path)?;
            let snap: StoredSubscriptionUsageSnapshot =
                serde_json::from_slice(&bytes).map_err(OfficeError::storage)?;
            Ok(Some(snap))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(OfficeError::storage(err)),
    }
}

fn load_durable_subscription_usage(
    dir: &Path,
    org_id: &str,
    office_id: &str,
) -> Result<Vec<StoredSubscriptionUsageSnapshot>, OfficeError> {
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(OfficeError::storage(err)),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(OfficeError::storage)?.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            files.push(path);
        }
    }
    files.sort();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for path in files {
        let bytes = read_bounded_snapshot_file(&path)?;
        let snap: StoredSubscriptionUsageSnapshot =
            serde_json::from_slice(&bytes).map_err(OfficeError::storage)?;
        let expected_name = format!("{}.json", snap.connection_id);
        let actual_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if snap.organization_id != org_id
            || snap.office_id != office_id
            || actual_name != expected_name
            || crate::accounting::validate_stored_snapshot(&snap).is_err()
            || !seen.insert(snap.connection_id.clone())
        {
            return Err(OfficeError::Invalid(
                "invalid or mismatched durable subscription usage snapshot file",
            ));
        }
        out.push(snap);
    }
    Ok(out)
}

type DedupReplay = Option<Result<(UsageRecordView, bool), ApiError>>;

fn check_existing_usage(
    conn: &Connection,
    org_id: &str,
    rec: &UsageRecordView,
) -> Result<DedupReplay, OfficeError> {
    let staged_match: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM staged_usage_records \
             WHERE organization_id=?1 AND (\
               id=?2 OR (origin_office_id=?3 AND producer_id=?4 AND source_event_id=?5)\
             ))",
            params![
                org_id,
                &rec.id,
                &rec.origin_office_id,
                &rec.producer_id,
                &rec.source_event_id,
            ],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    if staged_match {
        return Ok(Some(Err(ApiError::new(
            ErrorCode::Conflict,
            "usage record belongs to a staged or unverified stream",
        ))));
    }
    let by_source: Option<(String, String)> = conn
        .query_row(
            "SELECT payload_sha256, json_payload FROM usage_records \
             WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3 AND source_event_id=?4",
            params![org_id, &rec.origin_office_id, &rec.producer_id, &rec.source_event_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(OfficeError::storage)?;
    if let Some((sha, payload)) = by_source {
        if sha == rec.payload_sha256 {
            let existing: UsageRecordView =
                serde_json::from_str(&payload).map_err(OfficeError::storage)?;
            return Ok(Some(Ok((existing, true))));
        }
        return Ok(Some(Err(ApiError::new(
            ErrorCode::Conflict,
            "source_event_id already recorded with a different payload",
        ))));
    }
    let by_id: Option<(String, String)> = conn
        .query_row(
            "SELECT payload_sha256, json_payload FROM usage_records \
             WHERE organization_id=?1 AND id=?2",
            params![org_id, &rec.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(OfficeError::storage)?;
    if let Some((sha, payload)) = by_id {
        if sha == rec.payload_sha256 {
            let existing: UsageRecordView =
                serde_json::from_str(&payload).map_err(OfficeError::storage)?;
            return Ok(Some(Ok((existing, true))));
        }
        return Ok(Some(Err(ApiError::new(
            ErrorCode::Conflict,
            "usage record id already exists with a different payload",
        ))));
    }
    Ok(None)
}

fn check_supersedes_valid(
    conn: &Connection,
    org_id: &str,
    rec: &UsageRecordView,
) -> Result<Option<ApiError>, OfficeError> {
    let Some(parent_id) = rec.supersedes_id.as_deref() else {
        return Ok(None);
    };
    let parent_staged: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM staged_usage_records WHERE organization_id=?1 AND id=?2)",
            params![org_id, parent_id],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    if parent_staged {
        return Ok(Some(ApiError::new(
            ErrorCode::Conflict,
            "superseded usage record belongs to a staged or unverified stream",
        )));
    }
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM usage_records WHERE organization_id=?1 AND id=?2)",
            params![org_id, parent_id],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    if !exists {
        return Ok(Some(ApiError::new(
            ErrorCode::NotFound,
            "superseded usage record does not exist",
        )));
    }
    let already_superseded: bool = conn
        .query_row(
            "SELECT EXISTS(\
                SELECT 1 FROM usage_records WHERE organization_id=?1 AND supersedes_id=?2 \
                UNION ALL \
                SELECT 1 FROM staged_usage_records WHERE organization_id=?1 AND supersedes_id=?2\
             )",
            params![org_id, parent_id],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    if already_superseded {
        return Ok(Some(ApiError::new(
            ErrorCode::Conflict,
            "usage record is already superseded; correct the current leaf instead",
        )));
    }
    Ok(None)
}

/// Verifies a stream's `1..=hw` prefix in `usage_records` against both `head_sha256` (last event's
/// `payload_sha256`, matching `docs/design/organization-data/schema.sql:1129`) and `chain_sha256`
/// (running SHA-256 chain over `id`, `source_sequence`, `recorded_at`, `payload_sha256`).
/// Returns `Ok(Some(running_chain))` when valid, or `Ok(None)` when incomplete or mismatched.
fn verify_stream_checkpoint(
    conn: &Connection,
    org_id: &str,
    office: &str,
    prod: &str,
    hw: i64,
    head_sha: Option<&str>,
    chain_sha: Option<&str>,
) -> Result<Option<Option<String>>, OfficeError> {
    if hw < 0 {
        return Ok(None);
    }
    let mut stmt = conn
        .prepare(
            "SELECT source_sequence, id, payload_sha256, json_payload FROM usage_records \
             WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3 \
             ORDER BY source_sequence ASC",
        )
        .map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map(params![org_id, office, prod], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(OfficeError::storage)?;
    let mut count: i64 = 0;
    let mut last_payload_sha: Option<String> = None;
    let mut running_chain: Option<String> = None;
    for row in rows {
        let (seq, id, stored_sha, json) = row.map_err(OfficeError::storage)?;
        count += 1;
        if seq != count {
            return Ok(None);
        }
        let Ok(rec) = serde_json::from_str::<UsageRecordView>(&json) else {
            return Ok(None);
        };
        if rec.id != id
            || rec.source_sequence != (seq as u64)
            || rec.payload_sha256 != stored_sha
            || crate::accounting::compute_usage_payload_sha256(org_id, &rec) != stored_sha
        {
            return Ok(None);
        }
        last_payload_sha = Some(stored_sha);
        running_chain = Some(crate::accounting::chain_usage_stream_head(
            running_chain.as_deref(),
            &rec,
        ));
    }
    if count != hw {
        return Ok(None);
    }
    if last_payload_sha.as_deref() != head_sha {
        return Ok(None);
    }
    if let Some(expected_chain) = chain_sha {
        if running_chain.as_deref() != Some(expected_chain) {
            return Ok(None);
        }
    }
    Ok(Some(running_chain))
}

fn insert_price_row(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    price: &ModelPriceView,
) -> Result<(), OfficeError> {
    let payload = serde_json::to_string(price).map_err(OfficeError::storage)?;
    tx.execute(
        "INSERT INTO model_prices(organization_id, id, provider, model, effective_from, json_payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![org_id, &price.id, &price.provider, &price.model, &price.effective_from, payload],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

fn insert_price_row_ignore(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    price: &ModelPriceView,
) -> Result<(), OfficeError> {
    let payload = serde_json::to_string(price).map_err(OfficeError::storage)?;
    tx.execute(
        "INSERT OR IGNORE INTO model_prices(organization_id, id, provider, model, effective_from, json_payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![org_id, &price.id, &price.provider, &price.model, &price.effective_from, payload],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

fn insert_usage_row_strict(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    rec: &UsageRecordView,
    chain_sha256: Option<&str>,
) -> Result<(), OfficeError> {
    insert_usage_sql(tx, org_id, rec, chain_sha256, "INSERT INTO usage_records")
}

fn reconcile_durable_conn(
    conn: &mut Connection,
    durable_root: &Path,
    local_usage_dir: &Path,
    office_id: &str,
    org_id: &str,
    flush_before_reconcile: bool,
) -> Result<ReconcileMetadata, OfficeError> {
    let prices = load_durable_prices(durable_root, flush_before_reconcile)?;
    let all_records = load_durable_usage_records(durable_root, org_id, flush_before_reconcile)?;
    let mut checkpoints = load_durable_checkpoints(durable_root)?;
    if flush_before_reconcile {
        // Advance only already-anchored streams whose durable JSONL advanced ahead of their
        // checkpoint during an interrupted write and whose parents are already checkpoint-covered;
        // never fabricate a checkpoint for an unanchored restore (`TASK-97` / Pass 6 #1, Pass 8 #6).
        sync_checkpoints_for_reconciled_records(
            durable_root,
            org_id,
            &all_records,
            &mut checkpoints,
        )?;
    }
    // Verify complete streams against their durable checkpoints BEFORE staging orphan corrections,
    // so a missing parent for correction N never undoes an earlier valid correction in the same
    // stream (`TASK-97` / Final Gate Pass 5 #2, Pass 6 #2).
    let PartitionedStreams {
        projected: records,
        staged: staged_records,
        verified: verified_streams,
        verified_prefixes,
        staged_streams,
    } = verify_and_partition_streams(org_id, &checkpoints, all_records);
    let sub_snapshots = load_durable_subscription_usage(local_usage_dir, org_id, office_id)?;
    if !sub_snapshots.is_empty() {
        sync_usage_dir_and_parent(local_usage_dir)?;
    }
    let tx = conn.transaction().map_err(OfficeError::storage)?;
    // Rebuild the organization's projection tables cleanly from the durable archive so any
    // truncated/lost trailing JSONL record cannot linger in SQLite and mask a coverage gap
    // (`TASK-97` / Final Gate Finding #2).
    tx.execute(
        "DELETE FROM usage_records WHERE organization_id=?1",
        [org_id],
    )
    .map_err(OfficeError::storage)?;
    tx.execute(
        "DELETE FROM staged_usage_records WHERE organization_id=?1",
        [org_id],
    )
    .map_err(OfficeError::storage)?;
    tx.execute(
        "DELETE FROM usage_stream_checkpoints WHERE organization_id=?1",
        [org_id],
    )
    .map_err(OfficeError::storage)?;
    tx.execute(
        "DELETE FROM model_prices WHERE organization_id=?1",
        [org_id],
    )
    .map_err(OfficeError::storage)?;
    tx.execute(
        "DELETE FROM subscription_usage_snapshots WHERE organization_id=?1",
        [org_id],
    )
    .map_err(OfficeError::storage)?;
    for price in &prices {
        insert_price_row_ignore(&tx, org_id, price)?;
    }
    for (rec, chain_sha) in &records {
        reconcile_usage_row(&tx, org_id, rec, chain_sha.as_deref())?;
    }
    for rec in &staged_records {
        insert_staged_usage_row(&tx, org_id, rec)?;
    }
    for cp in &checkpoints {
        upsert_checkpoint_row(&tx, cp)?;
    }
    for snap in &sub_snapshots {
        upsert_subscription_usage_row(&tx, org_id, snap)?;
    }
    tx.commit().map_err(OfficeError::storage)?;
    Ok(ReconcileMetadata {
        verified_streams,
        verified_prefixes,
        staged_streams,
    })
}

struct CheckpointCoverageRow {
    office: String,
    prod: String,
    hw: i64,
    head_sha: Option<String>,
    chain_sha: Option<String>,
    at: String,
}

fn load_checkpoint_coverage_rows(
    conn: &Connection,
    org_id: &str,
) -> Result<Vec<CheckpointCoverageRow>, OfficeError> {
    let mut stmt = conn
        .prepare(
            "SELECT origin_office_id, producer_id, high_water_sequence, head_sha256, \
                    chain_sha256, checkpointed_at \
             FROM usage_stream_checkpoints WHERE organization_id=?1 \
             ORDER BY origin_office_id, producer_id",
        )
        .map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map([org_id], |r| {
            Ok(CheckpointCoverageRow {
                office: r.get(0)?,
                prod: r.get(1)?,
                hw: r.get(2)?,
                head_sha: r.get(3)?,
                chain_sha: r.get(4)?,
                at: r.get(5)?,
            })
        })
        .map_err(OfficeError::storage)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(OfficeError::storage)?);
    }
    Ok(out)
}

fn evaluate_stream_coverage_row(
    conn: &AccountingState,
    org_id: &str,
    row: &CheckpointCoverageRow,
    incomplete: &mut BTreeSet<(String, String)>,
) -> Result<Option<VerifiedStreamWatermark>, OfficeError> {
    let key = (row.office.clone(), row.prod.clone());
    let stream_key = (org_id.to_owned(), row.office.clone(), row.prod.clone());
    let prefix_head = conn
        .verified_prefixes
        .get(&stream_key)
        .filter(|h| {
            row.hw >= 0
                && h.high_water_sequence == (row.hw as u64)
                && h.head_sha256 == row.head_sha
                && row
                    .chain_sha
                    .as_ref()
                    .is_none_or(|c| h.chain_sha256.as_ref() == Some(c))
        })
        .cloned();
    let stream_fully_verified = !incomplete.contains(&key)
        && row.hw >= 0
        && conn.verified_streams.get(&stream_key).is_some_and(|h| {
            h.high_water_sequence == (row.hw as u64)
                && h.head_sha256 == row.head_sha
                && h.chain_sha256 == row.chain_sha
        });
    let full_ok = if stream_fully_verified {
        true
    } else if incomplete.contains(&key) {
        false
    } else {
        verify_stream_checkpoint(
            &conn.conn,
            org_id,
            &row.office,
            &row.prod,
            row.hw,
            row.head_sha.as_deref(),
            row.chain_sha.as_deref(),
        )?
        .is_some()
    };
    if !full_ok {
        incomplete.insert(key);
    }
    if !(full_ok || prefix_head.is_some()) {
        return Ok(None);
    }
    let effective_chain = prefix_head
        .as_ref()
        .and_then(|h| h.chain_sha256.clone())
        .or_else(|| row.chain_sha.clone());
    Ok(Some(VerifiedStreamWatermark {
        origin_office_id: row.office.clone(),
        producer_id: row.prod.clone(),
        high_water_sequence: row.hw as u64,
        chain_sha256: effective_chain,
    }))
}

struct VerifiedPrefixOutcome {
    head: VerifiedStreamHead,
    prefix_chains: Vec<(String, String)>,
    tail_ids: Vec<String>,
}

fn verify_single_stream_prefix(
    cp: &UsageStreamCheckpoint,
    stream_recs: &[&UsageRecordView],
) -> Option<VerifiedPrefixOutcome> {
    if (stream_recs.len() as u64) < cp.high_water_sequence {
        return None;
    }
    let mut expected_seq = 0u64;
    let mut last_payload_sha: Option<&str> = None;
    let mut running_chain: Option<String> = None;
    let mut prefix_chains: Vec<(String, String)> =
        Vec::with_capacity(cp.high_water_sequence as usize);
    for rec in stream_recs.iter().take(cp.high_water_sequence as usize) {
        expected_seq += 1;
        if rec.source_sequence != expected_seq {
            return None;
        }
        last_payload_sha = Some(rec.payload_sha256.as_str());
        let next_c = crate::accounting::chain_usage_stream_head(running_chain.as_deref(), rec);
        prefix_chains.push((rec.id.clone(), next_c.clone()));
        running_chain = Some(next_c);
    }
    if last_payload_sha != cp.head_sha256.as_deref()
        || cp
            .chain_sha256
            .as_deref()
            .is_some_and(|c| running_chain.as_deref() != Some(c))
    {
        return None;
    }
    let tail_ids = stream_recs
        .iter()
        .skip(cp.high_water_sequence as usize)
        .map(|extra| extra.id.clone())
        .collect();
    Some(VerifiedPrefixOutcome {
        head: VerifiedStreamHead {
            high_water_sequence: cp.high_water_sequence,
            head_sha256: cp.head_sha256.clone(),
            chain_sha256: running_chain,
        },
        prefix_chains,
        tail_ids,
    })
}

fn partition_topological_records(
    candidates: Vec<UsageRecordView>,
    chain_by_id: &mut BTreeMap<String, String>,
) -> (Vec<(UsageRecordView, Option<String>)>, Vec<UsageRecordView>) {
    let mut valid_ids: BTreeSet<String> = BTreeSet::new();
    let mut remaining = candidates;
    let mut projected = Vec::with_capacity(remaining.len());
    loop {
        let mut progressed = false;
        let mut next_remaining = Vec::new();
        for rec in remaining {
            let ready = rec
                .supersedes_id
                .as_deref()
                .is_none_or(|pid| valid_ids.contains(pid));
            if ready {
                let chain = chain_by_id.remove(&rec.id);
                valid_ids.insert(rec.id.clone());
                projected.push((rec, chain));
                progressed = true;
            } else {
                next_remaining.push(rec);
            }
        }
        remaining = next_remaining;
        if !progressed {
            break;
        }
    }
    (projected, remaining)
}

/// 1. Verifies each stream's complete durable sequence (`1..=hw` + `head_sha256` + `chain_sha256`)
///    against its durable checkpoint BEFORE checking `supersedes_id` parents (`Pass 6 #2`).
///    Unanchored or mismatched streams move entirely into `staged_records`.
/// 2. Topologically partitions verified records so valid corrections stay projected while any
///    correction whose parent is missing (or was in a quarantined stream) is staged individually,
///    removing its stream from `verified_streams` and recording it in `staged_streams`.
fn verify_and_partition_streams(
    org_id: &str,
    checkpoints: &[UsageStreamCheckpoint],
    all_records: Vec<UsageRecordView>,
) -> PartitionedStreams {
    let cp_by_stream: BTreeMap<(&str, &str), &UsageStreamCheckpoint> = checkpoints
        .iter()
        .filter(|c| c.organization_id == org_id)
        .map(|c| ((c.origin_office_id.as_str(), c.producer_id.as_str()), c))
        .collect();
    let mut by_stream: BTreeMap<(String, String), Vec<&UsageRecordView>> = BTreeMap::new();
    for rec in &all_records {
        by_stream
            .entry((rec.origin_office_id.clone(), rec.producer_id.clone()))
            .or_default()
            .push(rec);
    }
    let mut bad_streams: BTreeSet<(String, String)> = BTreeSet::new();
    let mut tail_staged_ids: BTreeSet<String> = BTreeSet::new();
    let mut chain_by_id: BTreeMap<String, String> = BTreeMap::new();
    let mut verified_streams: VerifiedStreamMap = BTreeMap::new();
    let mut verified_prefixes: VerifiedStreamMap = BTreeMap::new();
    for (&(origin, producer), cp) in &cp_by_stream {
        if cp.high_water_sequence == 0
            && cp.head_sha256.is_none()
            && !by_stream.contains_key(&(origin.to_owned(), producer.to_owned()))
        {
            let head = VerifiedStreamHead {
                high_water_sequence: 0,
                head_sha256: None,
                chain_sha256: None,
            };
            let skey = (org_id.to_owned(), origin.to_owned(), producer.to_owned());
            verified_streams.insert(skey.clone(), head.clone());
            verified_prefixes.insert(skey, head);
        }
    }
    for (key, mut stream_recs) in by_stream {
        stream_recs.sort_by_key(|r| r.source_sequence);
        let outcome = cp_by_stream
            .get(&(key.0.as_str(), key.1.as_str()))
            .copied()
            .and_then(|cp| verify_single_stream_prefix(cp, &stream_recs));
        let Some(outcome) = outcome else {
            bad_streams.insert(key);
            continue;
        };
        chain_by_id.extend(outcome.prefix_chains);
        let skey = (org_id.to_owned(), key.0, key.1);
        verified_prefixes.insert(skey.clone(), outcome.head.clone());
        if outcome.tail_ids.is_empty() {
            verified_streams.insert(skey, outcome.head);
        } else {
            tail_staged_ids.extend(outcome.tail_ids);
        }
    }
    let (mut staged_records, candidates): (Vec<_>, Vec<_>) =
        all_records.into_iter().partition(|rec| {
            bad_streams.contains(&(rec.origin_office_id.clone(), rec.producer_id.clone()))
                || tail_staged_ids.contains(&rec.id)
        });
    let (projected, orphans) = partition_topological_records(candidates, &mut chain_by_id);
    for orphan in orphans {
        verified_streams.remove(&(
            org_id.to_owned(),
            orphan.origin_office_id.clone(),
            orphan.producer_id.clone(),
        ));
        staged_records.push(orphan);
    }
    let staged_streams: StagedStreamSet = staged_records
        .iter()
        .map(|r| {
            (
                org_id.to_owned(),
                r.origin_office_id.clone(),
                r.producer_id.clone(),
            )
        })
        .collect();
    PartitionedStreams {
        projected,
        staged: staged_records,
        verified: verified_streams,
        verified_prefixes,
        staged_streams,
    }
}

struct ReconciledStreamInfo<'a> {
    origin: String,
    producer: String,
    existing_idx: usize,
    existing_hw: u64,
    missing_chain: bool,
    prefix_ok: bool,
    contiguous: bool,
    full_chain: Option<String>,
    prefix_chain: Option<String>,
    recs: Vec<&'a UsageRecordView>,
    advanced: bool,
}

fn analyze_reconciled_streams<'a>(
    org_id: &str,
    records: &'a [UsageRecordView],
    checkpoints: &[UsageStreamCheckpoint],
) -> (Vec<ReconciledStreamInfo<'a>>, Vec<&'a UsageRecordView>) {
    let mut by_stream: BTreeMap<(String, String), Vec<&'a UsageRecordView>> = BTreeMap::new();
    for rec in records {
        by_stream
            .entry((rec.origin_office_id.clone(), rec.producer_id.clone()))
            .or_default()
            .push(rec);
    }
    let mut infos = Vec::new();
    let mut prefix_candidates = Vec::new();
    for ((origin, producer), mut stream_recs) in by_stream {
        stream_recs.sort_by_key(|r| r.source_sequence);
        let Some(existing_idx) = checkpoints.iter().position(|c| {
            c.organization_id == org_id && c.origin_office_id == origin && c.producer_id == producer
        }) else {
            continue;
        };
        let existing_cp = &checkpoints[existing_idx];
        let existing_hw = existing_cp.high_water_sequence;
        let (prefix_ok, contiguous, full_chain, prefix_chain) =
            scan_reconciled_stream_chain(existing_cp, &stream_recs);
        if prefix_ok && (stream_recs.len() as u64) >= existing_hw {
            prefix_candidates.extend(stream_recs.iter().take(existing_hw as usize).copied());
        }
        infos.push(ReconciledStreamInfo {
            origin,
            producer,
            existing_idx,
            existing_hw,
            missing_chain: existing_hw > 0 && existing_cp.chain_sha256.is_none(),
            prefix_ok,
            contiguous,
            full_chain,
            prefix_chain,
            recs: stream_recs,
            advanced: false,
        });
    }
    (infos, prefix_candidates)
}

fn scan_reconciled_stream_chain(
    existing_cp: &UsageStreamCheckpoint,
    stream_recs: &[&UsageRecordView],
) -> (bool, bool, Option<String>, Option<String>) {
    let existing_hw = existing_cp.high_water_sequence;
    let mut expected_seq = 0u64;
    let mut running_chain: Option<String> = None;
    let mut prefix_chain: Option<String> = None;
    let mut prefix_ok = existing_hw == 0 && existing_cp.head_sha256.is_none();
    let mut contiguous = true;
    for rec in stream_recs {
        expected_seq += 1;
        if rec.source_sequence != expected_seq {
            contiguous = false;
            break;
        }
        running_chain = Some(crate::accounting::chain_usage_stream_head(
            running_chain.as_deref(),
            rec,
        ));
        if expected_seq == existing_hw {
            let head_ok = existing_cp.head_sha256.as_deref() == Some(rec.payload_sha256.as_str());
            let chain_ok = existing_cp
                .chain_sha256
                .as_deref()
                .is_none_or(|c| running_chain.as_deref() == Some(c));
            prefix_ok = head_ok && chain_ok;
            prefix_chain = running_chain.clone();
        }
    }
    (prefix_ok, contiguous, running_chain, prefix_chain)
}

fn expand_covered_prefix_ids<'a>(
    prefix_candidates: &[&'a UsageRecordView],
    covered_ids: &mut BTreeSet<&'a str>,
) {
    loop {
        let mut dep_progressed = false;
        for rec in prefix_candidates {
            if !covered_ids.contains(rec.id.as_str())
                && rec
                    .supersedes_id
                    .as_deref()
                    .is_none_or(|pid| covered_ids.contains(pid))
            {
                covered_ids.insert(rec.id.as_str());
                dep_progressed = true;
            }
        }
        if !dep_progressed {
            break;
        }
    }
}

fn try_advance_stream_checkpoint<'a>(
    durable_root: &Path,
    org_id: &str,
    info: &mut ReconciledStreamInfo<'a>,
    checkpoints: &mut [UsageStreamCheckpoint],
    covered_ids: &mut BTreeSet<&'a str>,
) -> Result<bool, OfficeError> {
    if info.advanced || !info.prefix_ok || !info.contiguous {
        return Ok(false);
    }
    let Some(head) = info.recs.last().copied() else {
        return Ok(false);
    };
    if head.source_sequence <= info.existing_hw {
        return Ok(false);
    }
    let mut tentative: BTreeSet<&'a str> = BTreeSet::new();
    for rec in info.recs.iter().skip(info.existing_hw as usize) {
        let parent_ok = rec
            .supersedes_id
            .as_deref()
            .is_none_or(|pid| covered_ids.contains(pid) || tentative.contains(pid));
        if !parent_ok {
            return Ok(false);
        }
        tentative.insert(rec.id.as_str());
    }
    let cp_dir = durable_root
        .join("checkpoints")
        .join(safe_segment(&info.origin));
    private_dir_synced(durable_root, &cp_dir).map_err(OfficeError::storage)?;
    let cp_path = cp_dir.join(format!("{}.json", safe_segment(&info.producer)));
    let cp = UsageStreamCheckpoint {
        organization_id: org_id.to_owned(),
        origin_office_id: info.origin.clone(),
        producer_id: info.producer.clone(),
        high_water_sequence: head.source_sequence,
        head_sha256: Some(head.payload_sha256.clone()),
        chain_sha256: info.full_chain.clone(),
        state: "open".into(),
        checkpointed_at: head.recorded_at.clone(),
    };
    let cp_bytes = serde_json::to_vec_pretty(&cp).map_err(OfficeError::storage)?;
    write_atomic(&cp_path, &cp_bytes)?;
    checkpoints[info.existing_idx] = cp;
    covered_ids.extend(tentative);
    info.advanced = true;
    Ok(true)
}

fn sync_unadvanced_checkpoints(
    durable_root: &Path,
    infos: &[ReconciledStreamInfo<'_>],
    checkpoints: &mut [UsageStreamCheckpoint],
) -> Result<(), OfficeError> {
    for info in infos {
        if info.advanced {
            continue;
        }
        let cp_dir = durable_root
            .join("checkpoints")
            .join(safe_segment(&info.origin));
        private_dir_synced(durable_root, &cp_dir).map_err(OfficeError::storage)?;
        let cp_path = cp_dir.join(format!("{}.json", safe_segment(&info.producer)));
        if info.missing_chain
            && info.prefix_ok
            && (info.recs.len() as u64) >= info.existing_hw
            && info.prefix_chain.is_some()
        {
            let mut upgraded = checkpoints[info.existing_idx].clone();
            upgraded.chain_sha256 = info.prefix_chain.clone();
            let cp_bytes = serde_json::to_vec_pretty(&upgraded).map_err(OfficeError::storage)?;
            write_atomic(&cp_path, &cp_bytes)?;
            checkpoints[info.existing_idx] = upgraded;
        } else if cp_path.is_file() {
            let f = fs::File::open(&cp_path).map_err(OfficeError::storage)?;
            full_sync(&f).map_err(OfficeError::storage)?;
        }
    }
    Ok(())
}

/// Fixed-point checkpoint advancement (`TASK-97` / Pass 6 #1, Pass 7 #4, Pass 8 #6):
/// - Never fabricates a checkpoint for an unanchored stream.
/// - Seeds `covered_ids` ONLY from records ALREADY covered by verified `1..=existing_hw` prefixes.
/// - Advances a stream's checkpoint to `N` ONLY when its prefix matches, `1..=N` is contiguous,
///   and every tail record (`seq > existing_hw`) has its `supersedes_id` parent already covered by
///   an existing or newly advanced checkpoint (or an earlier record in the same advancing stream).
fn sync_checkpoints_for_reconciled_records(
    durable_root: &Path,
    org_id: &str,
    records: &[UsageRecordView],
    checkpoints: &mut [UsageStreamCheckpoint],
) -> Result<(), OfficeError> {
    let (mut infos, prefix_candidates) = analyze_reconciled_streams(org_id, records, checkpoints);
    let mut covered_ids: BTreeSet<&str> = BTreeSet::new();
    loop {
        expand_covered_prefix_ids(&prefix_candidates, &mut covered_ids);
        let mut progressed = false;
        for info in &mut infos {
            if try_advance_stream_checkpoint(
                durable_root,
                org_id,
                info,
                checkpoints,
                &mut covered_ids,
            )? {
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    sync_unadvanced_checkpoints(durable_root, &infos, checkpoints)
}

/// Inserts a replayed durable usage row, skipping ONLY an exact full-record duplicate across
/// ALL unique identities (`id`, `(origin, producer, source_event_id)`, `(origin, producer,
/// source_sequence)`) and failing closed on any conflict (`TASK-97` / Pass 3 Finding #4).
fn reconcile_usage_row(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    rec: &UsageRecordView,
    chain_sha256: Option<&str>,
) -> Result<(), OfficeError> {
    let mut stmt = tx
        .prepare(
            "SELECT json_payload FROM usage_records \
             WHERE organization_id=?1 AND (\
               id=?2 OR \
               (origin_office_id=?3 AND producer_id=?4 AND source_event_id=?5) OR \
               (origin_office_id=?3 AND producer_id=?4 AND source_sequence=?6)\
             )",
        )
        .map_err(OfficeError::storage)?;
    let matches: Vec<String> = stmt
        .query_map(
            params![
                org_id,
                &rec.id,
                &rec.origin_office_id,
                &rec.producer_id,
                &rec.source_event_id,
                rec.source_sequence as i64,
            ],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?
        .collect::<Result<_, _>>()
        .map_err(OfficeError::storage)?;
    if !matches.is_empty() {
        if matches.len() == 1 {
            let existing: UsageRecordView =
                serde_json::from_str(&matches[0]).map_err(OfficeError::storage)?;
            if existing == *rec {
                return Ok(());
            }
        }
        return Err(OfficeError::Invalid(
            "conflicting usage record detected during durable replay",
        ));
    }
    insert_usage_row_strict(tx, org_id, rec, chain_sha256)
}

fn insert_usage_sql(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    rec: &UsageRecordView,
    chain_sha256: Option<&str>,
    verb: &str,
) -> Result<(), OfficeError> {
    let payload = serde_json::to_string(rec).map_err(OfficeError::storage)?;
    let sql = format!(
        "{verb}(organization_id, id, origin_office_id, producer_id, source_event_id, \
         source_sequence, occurred_at, payload_sha256, chain_sha256, supersedes_id, json_payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
    );
    tx.execute(
        &sql,
        params![
            org_id,
            &rec.id,
            &rec.origin_office_id,
            &rec.producer_id,
            &rec.source_event_id,
            rec.source_sequence as i64,
            &rec.occurred_at,
            &rec.payload_sha256,
            chain_sha256,
            rec.supersedes_id.as_deref(),
            payload,
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

fn insert_staged_usage_row(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    rec: &UsageRecordView,
) -> Result<(), OfficeError> {
    let payload = serde_json::to_string(rec).map_err(OfficeError::storage)?;
    tx.execute(
        "INSERT INTO staged_usage_records(\
         organization_id, id, origin_office_id, producer_id, source_event_id, \
         source_sequence, supersedes_id, json_payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            org_id,
            &rec.id,
            &rec.origin_office_id,
            &rec.producer_id,
            &rec.source_event_id,
            rec.source_sequence as i64,
            rec.supersedes_id.as_deref(),
            payload,
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

fn upsert_checkpoint_row(
    tx: &rusqlite::Transaction<'_>,
    cp: &UsageStreamCheckpoint,
) -> Result<(), OfficeError> {
    tx.execute(
        "INSERT INTO usage_stream_checkpoints(\
         organization_id, origin_office_id, producer_id, high_water_sequence, head_sha256, \
         chain_sha256, state, checkpointed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(organization_id, origin_office_id, producer_id) DO UPDATE SET \
         high_water_sequence = MAX(high_water_sequence, excluded.high_water_sequence), \
         head_sha256 = CASE WHEN excluded.high_water_sequence >= high_water_sequence THEN excluded.head_sha256 ELSE head_sha256 END, \
         chain_sha256 = CASE WHEN excluded.high_water_sequence >= high_water_sequence THEN excluded.chain_sha256 ELSE chain_sha256 END, \
         checkpointed_at = CASE WHEN excluded.high_water_sequence >= high_water_sequence THEN excluded.checkpointed_at ELSE checkpointed_at END",
        params![
            &cp.organization_id,
            &cp.origin_office_id,
            &cp.producer_id,
            cp.high_water_sequence as i64,
            cp.head_sha256.as_deref(),
            cp.chain_sha256.as_deref(),
            &cp.state,
            &cp.checkpointed_at,
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

fn load_durable_prices(
    root: &Path,
    flush_files_and_dirs: bool,
) -> Result<Vec<ModelPriceView>, OfficeError> {
    let dir = root.join("prices");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).map_err(OfficeError::storage)? {
        let path = entry.map_err(OfficeError::storage)?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if flush_files_and_dirs {
                let f = fs::File::open(&path).map_err(OfficeError::storage)?;
                full_sync(&f).map_err(OfficeError::storage)?;
            }
            let bytes = fs::read(&path).map_err(OfficeError::storage)?;
            out.push(serde_json::from_slice(&bytes).map_err(OfficeError::storage)?);
        }
    }
    if flush_files_and_dirs {
        private_dir_synced(root, &dir).map_err(OfficeError::storage)?;
    }
    Ok(out)
}

fn load_usage_jsonl_file_into(
    root: &Path,
    path: &Path,
    org_id: &str,
    flush_files_and_dirs: bool,
    seen: &mut BTreeMap<String, UsageRecordView>,
    out: &mut Vec<UsageRecordView>,
) -> Result<(), OfficeError> {
    if flush_files_and_dirs {
        let f = fs::File::open(path).map_err(OfficeError::storage)?;
        full_sync(&f).map_err(OfficeError::storage)?;
        if let Some(parent) = path.parent() {
            private_dir_synced(root, parent).map_err(OfficeError::storage)?;
        }
    }
    for rec in read_and_repair_usage_jsonl_file(path)? {
        let expected_sha = crate::accounting::compute_usage_payload_sha256(org_id, &rec);
        if rec.payload_sha256 != expected_sha {
            return Err(OfficeError::Invalid(
                "durable usage record payload_sha256 mismatch",
            ));
        }
        match seen.get(&rec.id) {
            Some(existing) if existing == &rec => {}
            Some(_) => {
                return Err(OfficeError::Invalid(
                    "conflicting duplicate usage record id in durable JSONL",
                ));
            }
            None => {
                seen.insert(rec.id.clone(), rec.clone());
                out.push(rec);
            }
        }
    }
    Ok(())
}

fn validate_durable_usage_invariants(records: &[UsageRecordView]) -> Result<(), OfficeError> {
    let mut seen_supersedes: BTreeSet<String> = BTreeSet::new();
    let mut seen_source_events: BTreeSet<(String, String, String)> = BTreeSet::new();
    let mut seen_sequences: BTreeSet<(String, String, u64)> = BTreeSet::new();
    for rec in records {
        if !org_proto::is_uuid_v7(&rec.id) {
            return Err(OfficeError::Invalid(
                "durable usage record id is not a canonical UUIDv7",
            ));
        }
        if !seen_source_events.insert((
            rec.origin_office_id.clone(),
            rec.producer_id.clone(),
            rec.source_event_id.clone(),
        )) {
            return Err(OfficeError::Invalid(
                "conflicting duplicate usage source_event_id in durable JSONL",
            ));
        }
        if !seen_sequences.insert((
            rec.origin_office_id.clone(),
            rec.producer_id.clone(),
            rec.source_sequence,
        )) {
            return Err(OfficeError::Invalid(
                "conflicting duplicate usage source_sequence in durable JSONL",
            ));
        }
        if let Some(parent_id) = rec.supersedes_id.as_deref() {
            if !seen_supersedes.insert(parent_id.to_owned()) {
                return Err(OfficeError::Invalid(
                    "duplicate durable usage correction for the same parent id",
                ));
            }
        }
    }
    Ok(())
}

fn load_durable_usage_records(
    root: &Path,
    org_id: &str,
    flush_files_and_dirs: bool,
) -> Result<Vec<UsageRecordView>, OfficeError> {
    let usage_dir = root.join("usage");
    if !usage_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_files_with_ext(&usage_dir, "jsonl", &mut files)?;
    files.sort();
    let mut out = Vec::new();
    let mut seen: BTreeMap<String, UsageRecordView> = BTreeMap::new();
    for path in files {
        load_usage_jsonl_file_into(
            root,
            &path,
            org_id,
            flush_files_and_dirs,
            &mut seen,
            &mut out,
        )?;
    }
    validate_durable_usage_invariants(&out)?;
    out.sort_by(|a, b| {
        a.recorded_at
            .cmp(&b.recorded_at)
            .then_with(|| a.source_sequence.cmp(&b.source_sequence))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(out)
}

/// O(1) check before appending: verifies the last byte is `\n` and ONLY inspects/repairs the
/// trailing fragment when the file lacks a terminating newline (`TASK-97` / Pass 2 Finding #9).
fn ensure_jsonl_ends_with_newline(path: &Path) -> Result<(), OfficeError> {
    let mut f = fs::File::open(path).map_err(OfficeError::storage)?;
    let len = f.metadata().map_err(OfficeError::storage)?.len();
    if len == 0 {
        return Ok(());
    }
    f.seek(SeekFrom::End(-1)).map_err(OfficeError::storage)?;
    let mut last = [0u8; 1];
    f.read_exact(&mut last).map_err(OfficeError::storage)?;
    if last[0] == b'\n' {
        return Ok(());
    }
    drop(f);
    let _ = read_and_repair_usage_jsonl_file(path)?;
    Ok(())
}

/// Reads all complete usage records from `path`, durably repairing any non-newline-terminated
/// trailing fragment (`TASK-97` / Finding #4):
/// - Complete JSON without trailing `\n`: appends `\n` + `full_sync`.
/// - Crash-torn invalid suffix without trailing `\n`: truncates back to the last `\n` + `full_sync`.
/// - Newline-terminated corrupt line anywhere in the file: fails closed.
fn read_and_repair_usage_jsonl_file(path: &Path) -> Result<Vec<UsageRecordView>, OfficeError> {
    let raw = fs::read(path).map_err(OfficeError::storage)?;
    let (complete_part, tail_part) = match raw.iter().rposition(|&b| b == b'\n') {
        Some(pos) => (&raw[..=pos], &raw[pos + 1..]),
        None => (&[][..], raw.as_slice()),
    };
    let mut out = Vec::new();
    for line in complete_part.split(|&b| b == b'\n') {
        let trimmed = trim_ascii_whitespace(line);
        if !trimmed.is_empty() {
            out.push(parse_usage_jsonl_line(trimmed)?);
        }
    }
    if !tail_part.is_empty() {
        let trimmed_tail = trim_ascii_whitespace(tail_part);
        if trimmed_tail.is_empty() {
            truncate_and_sync(path, complete_part.len() as u64)?;
        } else if let Ok(rec) = parse_usage_jsonl_line(trimmed_tail) {
            append_newline_and_sync(path)?;
            out.push(rec);
        } else {
            eprintln!(
                "agentic-org-office: truncating torn trailing JSONL fragment in {}",
                path.display()
            );
            truncate_and_sync(path, complete_part.len() as u64)?;
        }
    }
    Ok(out)
}

fn truncate_and_sync(path: &Path, len: u64) -> Result<(), OfficeError> {
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(OfficeError::storage)?;
    file.set_len(len).map_err(OfficeError::storage)?;
    full_sync(&file).map_err(OfficeError::storage)
}

fn append_newline_and_sync(path: &Path) -> Result<(), OfficeError> {
    let mut file = open_private_append(path)?;
    file.write_all(b"\n").map_err(OfficeError::storage)?;
    full_sync(&file).map_err(OfficeError::storage)
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &bytes[start..end]
}

fn parse_usage_jsonl_line(bytes: &[u8]) -> Result<UsageRecordView, OfficeError> {
    let text = std::str::from_utf8(bytes).map_err(OfficeError::storage)?;
    serde_json::from_str(text).map_err(OfficeError::storage)
}

fn load_durable_checkpoints(root: &Path) -> Result<Vec<UsageStreamCheckpoint>, OfficeError> {
    let cp_dir = root.join("checkpoints");
    if !cp_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_files_with_ext(&cp_dir, "json", &mut files)?;
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for path in files {
        let bytes = fs::read(&path).map_err(OfficeError::storage)?;
        let cp: UsageStreamCheckpoint =
            serde_json::from_slice(&bytes).map_err(OfficeError::storage)?;
        let expected_path = cp_dir
            .join(safe_segment(&cp.origin_office_id))
            .join(format!("{}.json", safe_segment(&cp.producer_id)));
        if path != expected_path {
            return Err(OfficeError::Invalid(
                "durable usage checkpoint path does not match canonical stream path",
            ));
        }
        let stream_key = (
            cp.organization_id.clone(),
            cp.origin_office_id.clone(),
            cp.producer_id.clone(),
        );
        if !seen.insert(stream_key) {
            return Err(OfficeError::Invalid(
                "duplicate durable usage checkpoint for stream identity",
            ));
        }
        out.push(cp);
    }
    Ok(out)
}

fn collect_files_with_ext(
    dir: &Path,
    ext: &str,
    out: &mut Vec<PathBuf>,
) -> Result<(), OfficeError> {
    for entry in fs::read_dir(dir).map_err(OfficeError::storage)? {
        let path = entry.map_err(OfficeError::storage)?.path();
        if path.is_dir() {
            collect_files_with_ext(&path, ext, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some(ext) {
            out.push(path);
        }
    }
    Ok(())
}

/// Collision-free, case-insensitive-filesystem-safe path component encoding (`TASK-97` / Pass 13).
/// Keeps lowercase `[a-z0-9-]` literal and hex-escapes every other byte as `_xx`, with a SHA-256
/// suffix if the encoded component would exceed 180 bytes.
fn safe_segment(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for &b in raw.as_bytes() {
        if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' {
            encoded.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(&mut encoded, "_{b:02x}");
        }
    }
    if encoded.len() <= 180 {
        return encoded;
    }
    let mut hash_hex = String::with_capacity(64);
    for b in org_store::Sha256::of(raw.as_bytes()).as_bytes() {
        use std::fmt::Write;
        let _ = write!(&mut hash_hex, "{b:02x}");
    }
    encoded.truncate(112);
    format!("{encoded}__{hash_hex}")
}

fn open_private_append(path: &Path) -> Result<fs::File, OfficeError> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(OfficeError::storage)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), OfficeError> {
    write_private_atomic(path, bytes).map_err(OfficeError::storage)
}
