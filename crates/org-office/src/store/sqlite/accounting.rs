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

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use org_proto::costs::{CostPriceListParams, ModelPriceView, UsageRecordView};
use org_proto::{ApiError, ErrorCode};
use rusqlite::{params, Connection, OptionalExtension};

use crate::accounting::{
    same_price_semantics, AccountingStore, UsageCoverageSummary, UsageStreamCheckpoint,
};
use crate::error::OfficeError;
use crate::private_fs::{absolute, private_dir, private_file, tighten_dir};

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
    state TEXT NOT NULL,
    checkpointed_at TEXT NOT NULL,
    PRIMARY KEY (organization_id, origin_office_id, producer_id)
) STRICT;
"#;

/// Isolated durable JSONL + SQLite [`AccountingStore`].
pub struct SqliteAccountingStore {
    durable_root: PathBuf,
    conn: Mutex<Connection>,
}

impl SqliteAccountingStore {
    /// Opens `<runtime_root>/<office_id>/protected/accounting.db`, rebuilding automatically
    /// from `<durable_root>/accounting/` whenever the projection is missing or empty.
    pub fn open(
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
    ) -> Result<Self, OfficeError> {
        let durable_acc = durable_root.join("accounting");
        private_dir(&durable_acc).map_err(OfficeError::storage)?;
        let office_dir = runtime_root.join(office_id);
        private_dir(&office_dir).map_err(OfficeError::storage)?;
        let protected = office_dir.join("protected");
        private_dir(&protected).map_err(OfficeError::storage)?;
        tighten_dir(&protected).map_err(OfficeError::storage)?;

        let db_path = absolute(protected.join("accounting.db")).map_err(OfficeError::storage)?;
        drop(private_file(&db_path).map_err(OfficeError::storage)?);
        let conn = Connection::open(&db_path).map_err(OfficeError::storage)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(OfficeError::storage)?;
        conn.execute_batch(ACCOUNTING_DDL)
            .map_err(OfficeError::storage)?;
        tighten_db_and_sidecars(&db_path)?;

        let store = Self {
            durable_root: durable_acc,
            conn: Mutex::new(conn),
        };
        store.reconcile_durable(org_id)?;
        Ok(store)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, OfficeError> {
        self.conn
            .lock()
            .map_err(|_| OfficeError::Invalid("accounting sqlite connection poisoned"))
    }

    /// Reconciles durable price JSON, usage JSONL, and checkpoints into `protected/accounting.db`
    /// on every startup so both full rebuilds and partial-crash tail writes recover (`doc-87`).
    fn reconcile_durable(&self, org_id: &str) -> Result<(), OfficeError> {
        let mut conn = self.lock()?;
        let prices = load_durable_prices(&self.durable_root)?;
        let records = load_durable_usage_records(&self.durable_root)?;
        let checkpoints = load_durable_checkpoints(&self.durable_root)?;
        if prices.is_empty() && records.is_empty() && checkpoints.is_empty() {
            return Ok(());
        }
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        for price in &prices {
            insert_price_row_ignore(&tx, org_id, price)?;
        }
        for rec in &records {
            insert_usage_row_ignore(&tx, org_id, rec)?;
        }
        for cp in &checkpoints {
            upsert_checkpoint_row(&tx, cp)?;
        }
        tx.commit().map_err(OfficeError::storage)?;
        Ok(())
    }

    fn write_price_durable(&self, price: &ModelPriceView) -> Result<(), OfficeError> {
        let dir = self.durable_root.join("prices");
        private_dir(&dir).map_err(OfficeError::storage)?;
        let path = dir.join(format!("{}.json", price.id));
        let bytes = serde_json::to_vec_pretty(price).map_err(OfficeError::storage)?;
        write_atomic(&path, &bytes)
    }

    fn append_usage_durable(
        &self,
        rec: &UsageRecordView,
        cp: &UsageStreamCheckpoint,
    ) -> Result<(), OfficeError> {
        let date = rec.recorded_at.get(0..10).unwrap_or("1970-01-01");
        let stream_dir = self
            .durable_root
            .join("usage")
            .join(safe_segment(&rec.origin_office_id))
            .join(safe_segment(&rec.producer_id));
        private_dir(&stream_dir).map_err(OfficeError::storage)?;
        let jsonl_path = stream_dir.join(format!("{date}.jsonl"));
        let mut line = serde_json::to_vec(rec).map_err(OfficeError::storage)?;
        line.push(b'\n');
        let mut file = open_private_append(&jsonl_path)?;
        file.write_all(&line).map_err(OfficeError::storage)?;
        file.sync_all().map_err(OfficeError::storage)?;
        tighten_file(&jsonl_path)?;

        let cp_dir = self
            .durable_root
            .join("checkpoints")
            .join(safe_segment(&cp.origin_office_id));
        private_dir(&cp_dir).map_err(OfficeError::storage)?;
        let cp_path = cp_dir.join(format!("{}.json", safe_segment(&cp.producer_id)));
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
        self.write_price_durable(&price)?;
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        insert_price_row(&tx, org_id, &price)?;
        tx.commit().map_err(OfficeError::storage)?;
        Ok(Ok(price))
    }

    fn list_prices(
        &self,
        org_id: &str,
        params: &CostPriceListParams,
    ) -> Result<Vec<ModelPriceView>, OfficeError> {
        let conn = self.lock()?;
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
        let conn = self.lock()?;
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
        let mut conn = self.lock()?;
        if let Some(res) = check_existing_usage(&conn, org_id, &record)? {
            return Ok(res);
        }
        if let Some(err) = check_supersedes_valid(&conn, org_id, &record)? {
            return Ok(Err(err));
        }
        // Allocate source_sequence atomically under the store mutex (`Pass 1 High #5`).
        let cur_seq: Option<i64> = conn
            .query_row(
                "SELECT MAX(seq) FROM (\
                    SELECT high_water_sequence AS seq FROM usage_stream_checkpoints \
                     WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3 \
                    UNION ALL \
                    SELECT COALESCE(MAX(source_sequence), 0) AS seq FROM usage_records \
                     WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3\
                 )",
                params![org_id, &record.origin_office_id, &record.producer_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(OfficeError::storage)?
            .flatten();
        record.source_sequence = cur_seq.map_or(0, |v| v.max(0) as u64).saturating_add(1);
        let cp = UsageStreamCheckpoint {
            organization_id: org_id.to_owned(),
            origin_office_id: record.origin_office_id.clone(),
            producer_id: record.producer_id.clone(),
            high_water_sequence: record.source_sequence,
            head_sha256: Some(record.payload_sha256.clone()),
            state: "open".into(),
            checkpointed_at: record.recorded_at.clone(),
        };
        self.append_usage_durable(&record, &cp)?;
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        insert_usage_row_strict(&tx, org_id, &record)?;
        upsert_checkpoint_row(&tx, &cp)?;
        tx.commit().map_err(OfficeError::storage)?;
        Ok(Ok((record, false)))
    }

    fn get_usage(&self, org_id: &str, id: &str) -> Result<Option<UsageRecordView>, OfficeError> {
        let conn = self.lock()?;
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
        let conn = self.lock()?;
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
        let conn = self.lock()?;
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM usage_records WHERE organization_id=?1 AND supersedes_id=?2)",
            params![org_id, id],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)
    }

    fn effective_records(&self, org_id: &str) -> Result<Vec<UsageRecordView>, OfficeError> {
        let conn = self.lock()?;
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
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT origin_office_id, producer_id, high_water_sequence, head_sha256, checkpointed_at \
                 FROM usage_stream_checkpoints WHERE organization_id=?1 \
                 ORDER BY origin_office_id, producer_id",
            )
            .map_err(OfficeError::storage)?;
        let rows = stmt
            .query_map([org_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .map_err(OfficeError::storage)?;
        let mut incomplete_streams: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM (\
                    SELECT DISTINCT origin_office_id, producer_id FROM usage_records u \
                     WHERE u.organization_id=?1 AND NOT EXISTS (\
                       SELECT 1 FROM usage_stream_checkpoints c \
                        WHERE c.organization_id=u.organization_id \
                          AND c.origin_office_id=u.origin_office_id \
                          AND c.producer_id=u.producer_id\
                     )\
                 )",
                [org_id],
                |r| r.get::<_, i64>(0),
            )
            .map_err(OfficeError::storage)?
            .max(0) as u64;
        let mut as_of: Option<String> = None;
        let mut refs = Vec::new();
        for row in rows {
            let (office, prod, hw, head_sha, at) = row.map_err(OfficeError::storage)?;
            if !stream_matches_checkpoint(&conn, org_id, &office, &prod, hw, head_sha.as_deref())? {
                incomplete_streams = incomplete_streams.saturating_add(1);
            }
            if as_of.as_deref().is_none_or(|cur| at.as_str() > cur) {
                as_of = Some(at);
            }
            refs.push(format!("{office}/{prod}:{hw}"));
        }
        Ok(UsageCoverageSummary {
            incomplete_streams,
            coverage_as_of: as_of,
            checkpoint_ref: (!refs.is_empty()).then(|| refs.join(",")),
        })
    }
}

type DedupReplay = Option<Result<(UsageRecordView, bool), ApiError>>;

fn check_existing_usage(
    conn: &Connection,
    org_id: &str,
    rec: &UsageRecordView,
) -> Result<DedupReplay, OfficeError> {
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
            "SELECT EXISTS(SELECT 1 FROM usage_records WHERE organization_id=?1 AND supersedes_id=?2)",
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

fn stream_matches_checkpoint(
    conn: &Connection,
    org_id: &str,
    office: &str,
    prod: &str,
    hw: i64,
    head_sha: Option<&str>,
) -> Result<bool, OfficeError> {
    let (cnt, max_seq): (i64, Option<i64>) = conn
        .query_row(
            "SELECT COUNT(*), MAX(source_sequence) FROM usage_records \
             WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3",
            params![org_id, office, prod],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(OfficeError::storage)?;
    if cnt != hw || max_seq.unwrap_or(0) != hw {
        return Ok(false);
    }
    if hw == 0 {
        return Ok(head_sha.is_none());
    }
    let actual_head: Option<String> = conn
        .query_row(
            "SELECT payload_sha256 FROM usage_records \
             WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3 AND source_sequence=?4",
            params![org_id, office, prod, hw],
            |r| r.get(0),
        )
        .optional()
        .map_err(OfficeError::storage)?;
    Ok(actual_head.as_deref() == head_sha)
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
) -> Result<(), OfficeError> {
    insert_usage_sql(tx, org_id, rec, "INSERT INTO usage_records")
}

fn insert_usage_row_ignore(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    rec: &UsageRecordView,
) -> Result<(), OfficeError> {
    insert_usage_sql(tx, org_id, rec, "INSERT OR IGNORE INTO usage_records")
}

fn insert_usage_sql(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    rec: &UsageRecordView,
    verb: &str,
) -> Result<(), OfficeError> {
    let payload = serde_json::to_string(rec).map_err(OfficeError::storage)?;
    let sql = format!(
        "{verb}(organization_id, id, origin_office_id, producer_id, source_event_id, \
         source_sequence, occurred_at, payload_sha256, supersedes_id, json_payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
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
         organization_id, origin_office_id, producer_id, high_water_sequence, head_sha256, state, checkpointed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(organization_id, origin_office_id, producer_id) DO UPDATE SET \
         high_water_sequence = MAX(high_water_sequence, excluded.high_water_sequence), \
         head_sha256 = CASE WHEN excluded.high_water_sequence >= high_water_sequence THEN excluded.head_sha256 ELSE head_sha256 END, \
         checkpointed_at = CASE WHEN excluded.high_water_sequence >= high_water_sequence THEN excluded.checkpointed_at ELSE checkpointed_at END",
        params![
            &cp.organization_id,
            &cp.origin_office_id,
            &cp.producer_id,
            cp.high_water_sequence as i64,
            cp.head_sha256.as_deref(),
            &cp.state,
            &cp.checkpointed_at,
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

fn load_durable_prices(root: &Path) -> Result<Vec<ModelPriceView>, OfficeError> {
    let dir = root.join("prices");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).map_err(OfficeError::storage)? {
        let path = entry.map_err(OfficeError::storage)?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let bytes = fs::read(&path).map_err(OfficeError::storage)?;
            out.push(serde_json::from_slice(&bytes).map_err(OfficeError::storage)?);
        }
    }
    Ok(out)
}

fn load_durable_usage_records(root: &Path) -> Result<Vec<UsageRecordView>, OfficeError> {
    let usage_dir = root.join("usage");
    if !usage_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_files_with_ext(&usage_dir, "jsonl", &mut files)?;
    files.sort();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for path in files {
        let text = fs::read_to_string(&path).map_err(OfficeError::storage)?;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let rec: UsageRecordView = serde_json::from_str(line).map_err(OfficeError::storage)?;
            if seen.insert(rec.id.clone()) {
                out.push(rec);
            }
        }
    }
    // Replay parents before correction children deterministically.
    out.sort_by(|a, b| {
        a.supersedes_id
            .is_some()
            .cmp(&b.supersedes_id.is_some())
            .then_with(|| a.recorded_at.cmp(&b.recorded_at))
            .then_with(|| a.source_sequence.cmp(&b.source_sequence))
    });
    Ok(out)
}

fn load_durable_checkpoints(root: &Path) -> Result<Vec<UsageStreamCheckpoint>, OfficeError> {
    let cp_dir = root.join("checkpoints");
    if !cp_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_files_with_ext(&cp_dir, "json", &mut files)?;
    let mut out = Vec::new();
    for path in files {
        let bytes = fs::read(&path).map_err(OfficeError::storage)?;
        out.push(serde_json::from_slice(&bytes).map_err(OfficeError::storage)?);
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

fn safe_segment(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn open_private_append(path: &Path) -> Result<fs::File, OfficeError> {
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path).map_err(OfficeError::storage)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), OfficeError> {
    let seq = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp.{}.{seq}", std::process::id()));
    {
        let mut opts = OpenOptions::new();
        opts.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(OfficeError::storage)?;
        f.write_all(bytes).map_err(OfficeError::storage)?;
        f.sync_all().map_err(OfficeError::storage)?;
    }
    tighten_file(&tmp)?;
    fs::rename(&tmp, path).map_err(OfficeError::storage)?;
    if let Some(parent) = path.parent() {
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    tighten_file(path)
}

#[cfg(unix)]
fn tighten_file(path: &Path) -> Result<(), OfficeError> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(OfficeError::storage)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten_file(_path: &Path) -> Result<(), OfficeError> {
    Err(OfficeError::Invalid(
        "protected accounting owner-only file permissions are unsupported on this OS",
    ))
}

fn tighten_db_and_sidecars(db_path: &Path) -> Result<(), OfficeError> {
    tighten_file(db_path)?;
    for suffix in ["-wal", "-shm"] {
        let mut s = db_path.as_os_str().to_owned();
        s.push(suffix);
        tighten_file(Path::new(&s))?;
    }
    Ok(())
}
