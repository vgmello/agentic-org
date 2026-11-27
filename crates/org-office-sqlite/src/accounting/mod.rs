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
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use org_proto::costs::{ModelPriceView, UsageRecordView};
use rusqlite::Connection;

use org_office::accounting::UsageStreamCheckpoint;
use org_office::private_fs::{
    absolute, full_sync, private_dir, private_dir_synced, sync_dir, tighten_dir, tighten_file,
};
use org_office::storage_layout as layout;
use org_office::OfficeError;

use super::companion::open_and_reconcile_companion_db;

mod accounting_store;
mod checkpoint_sync;
mod files;
mod partition;
mod reconcile;
mod rows;
mod schema;
mod verify;

use self::files::{
    ensure_jsonl_ends_with_newline, open_private_append, safe_segment, write_atomic,
};
use self::reconcile::reconcile_durable_conn;
use self::schema::{ACCOUNTING_ADAPTER_FORMAT, ACCOUNTING_DDL, ACCOUNTING_PRAGMAS};

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

struct CheckpointCoverageRow {
    office: String,
    prod: String,
    hw: i64,
    head_sha: Option<String>,
    chain_sha: Option<String>,
    at: String,
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
///
/// [`AccountingStore`]: org_office::accounting::AccountingStore
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
        let durable_acc = layout::accounting(durable_root);
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
