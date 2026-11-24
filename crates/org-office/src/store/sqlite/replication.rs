//! SQLite + durable-JSON adapter for [`crate::replication::ReplicationStore`]
//! (`TASK-52`, `FC-145`, `ADR-0018` `ENG-3`).
//!
//! Durable source of truth lives under `<durable>/local/offices/<office_id>/replication/state.v1.json`
//! (written atomically via `full_sync` + parent-dir sync). The rebuildable SQLite projection lives
//! at `<runtime>/<office_id>/protected/replication.db` and is reconciled via
//! [`super::companion::open_and_reconcile_companion_db`].

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use org_sync::{
    error::{StoreError, SyncError},
    messages::{MessageAttempt, MessageOutput, MessageStore, Queue},
    organization::{OrgAttempt, OrgCheckpoint, OrgEvent, OrgOutput, OrgStore},
    work::{Change as WorkChange, LocalOutbox, Origin},
};
use org_sync_proto::{
    Codec, Digest, Epoch, OfficeId, OrgId, OriginIncarnation, Position, SourceSeq,
};
use rusqlite::{params, Connection};

use super::companion::open_and_reconcile_companion_db;
use crate::error::OfficeError;
use crate::private_fs::{private_dir, private_dir_synced, tighten_dir, write_private_atomic};
use crate::replication::{
    decode_b64, decode_hex_digest, encode_b64,
    memory::{CoreMessageTxn, CoreOrgTxn, ReplicationCoreData},
    message_from_stored, message_to_stored, outbox_from_stored, receipt_from_stored,
    receipt_to_stored, ReplicationStateFile, ReplicationStore, StoredOrgEvent, StoredRelayItem,
    StoredWorkChange, SyncStatusSnapshot,
};

const REPLICATION_ADAPTER_FORMAT: u32 = 1;
const REPLICATION_PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
";

const REPLICATION_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS replication_meta (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    org_id TEXT NOT NULL,
    local_office TEXT NOT NULL,
    hq_office TEXT NOT NULL,
    epoch INTEGER NOT NULL,
    seq INTEGER NOT NULL,
    highest_epoch INTEGER NOT NULL,
    protocol_level INTEGER NOT NULL,
    restore_pending INTEGER NOT NULL,
    initial_snapshot_required INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS replication_org_events (
    epoch INTEGER NOT NULL,
    seq INTEGER NOT NULL,
    digest_hex TEXT NOT NULL,
    codec_name TEXT NOT NULL,
    codec_version INTEGER NOT NULL,
    PRIMARY KEY (epoch, seq)
) STRICT;

CREATE TABLE IF NOT EXISTS replication_work_outbox (
    seq INTEGER PRIMARY KEY,
    record_id TEXT NOT NULL,
    digest_hex TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS replication_message_outbox (
    message_id TEXT PRIMARY KEY,
    destination_office TEXT NOT NULL,
    deadline_ms INTEGER NOT NULL
) STRICT;
"#;

struct SqliteReplicationInner {
    data: ReplicationCoreData,
    conn: Connection,
    /// True if a prior SQLite projection failed after the durable JSON commit point succeeded;
    /// reconciled before serving any subsequent mutation (`TASK-52` review F19).
    projection_dirty: bool,
}

pub struct SqliteReplicationStore {
    state_path: PathBuf,
    inner: Mutex<SqliteReplicationInner>,
}

pub struct OpenReplicationParams<'a> {
    pub durable_root: &'a Path,
    pub runtime_root: &'a Path,
    pub org_id: OrgId,
    pub local_office: OfficeId,
    pub hq_office: OfficeId,
    pub ca: Digest,
    pub epoch: Epoch,
    pub incarnation: OriginIncarnation,
    pub initial_snapshot_required: bool,
}

impl SqliteReplicationStore {
    pub fn open(p: OpenReplicationParams<'_>) -> Result<Self, OfficeError> {
        let state_dir = p
            .durable_root
            .join("local")
            .join("offices")
            .join(p.local_office.as_str())
            .join("replication");
        let state_path = state_dir.join("state.v1.json");
        let protected_dir = p
            .runtime_root
            .join(p.local_office.as_str())
            .join("protected");
        private_dir_synced(p.durable_root, &state_dir).map_err(OfficeError::storage)?;
        private_dir(&protected_dir).map_err(OfficeError::storage)?;
        tighten_dir(&protected_dir).map_err(OfficeError::storage)?;

        let data = if state_path.is_file() {
            let bytes = fs::read(&state_path).map_err(OfficeError::storage)?;
            let parsed: ReplicationStateFile = serde_json::from_slice(&bytes)
                .map_err(|_| OfficeError::Invalid("malformed durable replication state"))?;
            core_from_file(&parsed)?
        } else {
            let initial = ReplicationCoreData::new(
                p.org_id,
                p.local_office.clone(),
                p.hq_office,
                p.ca,
                p.epoch,
                p.incarnation,
                p.initial_snapshot_required,
            );
            let encoded = encode_state_file(&initial)?;
            write_private_atomic(&state_path, &encoded).map_err(OfficeError::storage)?;
            initial
        };

        let db_path = protected_dir.join("replication.db");
        let conn = open_and_reconcile_companion_db(
            &db_path,
            "replication",
            REPLICATION_ADAPTER_FORMAT,
            REPLICATION_DDL,
            REPLICATION_PRAGMAS,
            |conn| project_into_sqlite(conn, &data),
        )?;

        Ok(Self {
            state_path,
            inner: Mutex::new(SqliteReplicationInner {
                data,
                conn,
                projection_dirty: false,
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SqliteReplicationInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn reconcile_if_dirty(guard: &mut SqliteReplicationInner) -> Result<(), OfficeError> {
        if guard.projection_dirty {
            project_into_sqlite(&mut guard.conn, &guard.data)?;
            guard.projection_dirty = false;
        }
        Ok(())
    }

    /// Single durable commit point: write `state.v1.json` atomically, update `guard.data`
    /// immediately so memory never diverges from the committed JSON, then project into the
    /// rebuildable SQLite index (marking `projection_dirty` if projection fails so the next
    /// operation reconciles from the committed state before mutating further).
    fn persist_and_project(
        &self,
        guard: &mut SqliteReplicationInner,
        candidate: ReplicationCoreData,
    ) -> Result<(), OfficeError> {
        Self::reconcile_if_dirty(guard)?;
        let encoded = encode_state_file(&candidate)?;
        write_private_atomic(&self.state_path, &encoded).map_err(OfficeError::storage)?;
        guard.data = candidate;
        if let Err(err) = project_into_sqlite(&mut guard.conn, &guard.data) {
            guard.projection_dirty = true;
            return Err(err);
        }
        Ok(())
    }
}

impl OrgStore for SqliteReplicationStore {
    fn transact(&self, attempt: Arc<OrgAttempt>) -> Result<OrgOutput, SyncError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        let output = attempt(&mut CoreOrgTxn {
            data: &mut candidate,
        })?;
        self.persist_and_project(&mut guard, candidate)
            .map_err(|e| StoreError::unavailable(e.to_string()))?;
        Ok(output)
    }
}

impl MessageStore for SqliteReplicationStore {
    fn transact(&self, attempt: Arc<MessageAttempt>) -> Result<MessageOutput, SyncError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        let output = attempt(&mut CoreMessageTxn {
            data: &mut candidate,
        })?;
        self.persist_and_project(&mut guard, candidate)
            .map_err(|e| StoreError::unavailable(e.to_string()))?;
        Ok(output)
    }
}

impl ReplicationStore for SqliteReplicationStore {
    fn org_checkpoint(&self) -> Result<OrgCheckpoint, OfficeError> {
        Ok(self.lock().data.checkpoint.clone())
    }

    fn org_events_after(&self, epoch: u64, seq: u64) -> Result<Vec<OrgEvent>, OfficeError> {
        Ok(self
            .lock()
            .data
            .events
            .range((epoch, seq.saturating_add(1))..)
            .filter(|((e, _), _)| *e == epoch)
            .map(|(_, ev)| ev.clone())
            .collect())
    }

    fn stage_snapshot(
        &self,
        staging_id: &str,
        payloads: Vec<Arc<[u8]>>,
    ) -> Result<(), OfficeError> {
        self.lock()
            .data
            .staging
            .insert(staging_id.to_owned(), payloads);
        Ok(())
    }

    fn applied_org_payloads(&self) -> Result<Vec<Arc<[u8]>>, OfficeError> {
        Ok(self.lock().data.applied_org.clone())
    }

    fn load_work_outbox(&self) -> Result<LocalOutbox, OfficeError> {
        Ok(self.lock().data.work_outbox.clone())
    }

    fn enqueue_work_change(&self, change: WorkChange) -> Result<LocalOutbox, SyncError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        let outbox = candidate.enqueue_work(change)?;
        self.persist_and_project(&mut guard, candidate)
            .map_err(|e| StoreError::unavailable(e.to_string()))?;
        Ok(outbox)
    }

    fn ack_work_through(&self, acked_seq: u64) -> Result<LocalOutbox, SyncError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        let outbox = candidate.ack_work(acked_seq)?;
        self.persist_and_project(&mut guard, candidate)
            .map_err(|e| StoreError::unavailable(e.to_string()))?;
        Ok(outbox)
    }

    fn record_remote_work_seq(&self, origin: &Origin, seq: SourceSeq) -> Result<bool, OfficeError> {
        let key = format!("{}:{}", origin.office.as_str(), origin.incarnation.as_str());
        let mut guard = self.lock();
        let current = guard
            .data
            .remote_work_high_water
            .get(&key)
            .copied()
            .unwrap_or(0);
        if seq.get() <= current {
            return Ok(false);
        }
        let mut candidate = guard.data.clone();
        candidate.remote_work_high_water.insert(key, seq.get());
        self.persist_and_project(&mut guard, candidate)?;
        Ok(true)
    }

    fn remote_work_high_water(&self, origin: &Origin) -> Result<u64, OfficeError> {
        let key = format!("{}:{}", origin.office.as_str(), origin.incarnation.as_str());
        Ok(self
            .lock()
            .data
            .remote_work_high_water
            .get(&key)
            .copied()
            .unwrap_or(0))
    }

    fn delivered_message_payloads(&self) -> Result<Vec<Arc<[u8]>>, OfficeError> {
        Ok(self.lock().data.delivered_messages.clone())
    }

    fn message_queue_snapshot(&self) -> Result<Queue, OfficeError> {
        Ok(self.lock().data.message_queue.clone())
    }

    fn outgoing_receipt_snapshot(
        &self,
        message_id: &str,
    ) -> Result<Option<org_sync::messages::Receipt>, OfficeError> {
        Ok(self.lock().data.outgoing_receipts.get(message_id).cloned())
    }

    fn has_received_message_receipt(
        &self,
        origin_office: &str,
        message_id: &str,
        fingerprint_hex: &str,
    ) -> Result<bool, OfficeError> {
        Ok(self.lock().data.has_received_message_receipt(
            origin_office,
            message_id,
            fingerprint_hex,
        ))
    }

    fn reset_for_re_enrollment(
        &self,
        new_hq_office: OfficeId,
        new_ca: Digest,
        new_epoch: Epoch,
    ) -> Result<(), OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        candidate.apply_re_enrollment(new_hq_office, new_ca, new_epoch)?;
        self.persist_and_project(&mut guard, candidate)
    }

    fn sync_status(&self) -> Result<SyncStatusSnapshot, OfficeError> {
        Ok(self.lock().data.status())
    }

    fn enqueue_relay_item(&self, item: StoredRelayItem) -> Result<(), OfficeError> {
        let mut guard = self.lock();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut candidate = guard.data.clone();
        let prev_len = candidate.pending_relays.len();
        let added = candidate.enqueue_relay_checked(item, now_ms)?;
        if added || candidate.pending_relays.len() != prev_len {
            self.persist_and_project(&mut guard, candidate)?;
        }
        Ok(())
    }

    fn pending_relays_for(
        &self,
        destination_office: &str,
    ) -> Result<Vec<StoredRelayItem>, OfficeError> {
        Ok(self
            .lock()
            .data
            .pending_relays
            .iter()
            .filter(|r| r.destination_office == destination_office)
            .cloned()
            .collect())
    }

    fn ack_relay_item(&self, relay_id: &str) -> Result<bool, OfficeError> {
        let mut guard = self.lock();
        if !guard.data.pending_relays.iter().any(|r| r.id == relay_id) {
            return Ok(false);
        }
        let mut candidate = guard.data.clone();
        candidate.pending_relays.retain(|r| r.id != relay_id);
        self.persist_and_project(&mut guard, candidate)?;
        Ok(true)
    }

    fn ack_relay_item_for_destination(
        &self,
        destination_office: &str,
        relay_id: &str,
    ) -> Result<bool, OfficeError> {
        let mut guard = self.lock();
        let matches =
            |r: &StoredRelayItem| r.id == relay_id && r.destination_office == destination_office;
        if !guard.data.pending_relays.iter().any(matches) {
            return Ok(false);
        }
        let mut candidate = guard.data.clone();
        candidate.pending_relays.retain(|r| !matches(r));
        self.persist_and_project(&mut guard, candidate)?;
        Ok(true)
    }

    fn ack_message_relay_checked(
        &self,
        destination_office: &str,
        origin_office: &str,
        message_id: &str,
        fingerprint_hex: &str,
    ) -> Result<bool, OfficeError> {
        self.ack_message_relay_and_enqueue_receipt(
            destination_office,
            origin_office,
            message_id,
            fingerprint_hex,
            None,
        )
    }

    fn ack_message_relay_and_enqueue_receipt(
        &self,
        destination_office: &str,
        origin_office: &str,
        message_id: &str,
        fingerprint_hex: &str,
        receipt_item: Option<StoredRelayItem>,
    ) -> Result<bool, OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let removed = candidate.ack_message_relay_and_enqueue_receipt_inner(
            destination_office,
            origin_office,
            message_id,
            fingerprint_hex,
            receipt_item,
            now_ms,
        )?;
        if removed {
            self.persist_and_project(&mut guard, candidate)?;
        }
        Ok(removed)
    }

    fn purge_relays_for_office(&self, office_id: &str) -> Result<usize, OfficeError> {
        let mut guard = self.lock();
        let before = guard.data.pending_relays.len();
        let had_wm = guard
            .data
            .last_published_message_journal_by_dest
            .contains_key(office_id);
        let mut candidate = guard.data.clone();
        candidate
            .pending_relays
            .retain(|r| r.destination_office != office_id);
        candidate
            .last_published_message_journal_by_dest
            .remove(office_id);
        let removed = before.saturating_sub(candidate.pending_relays.len());
        if removed > 0 || had_wm {
            self.persist_and_project(&mut guard, candidate)?;
        }
        Ok(removed)
    }

    fn retire_undeliverable_message(
        &self,
        message_id: &str,
        destination_office: &str,
    ) -> Result<bool, OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        let retired = candidate.retire_undeliverable_message_inner(message_id, destination_office);
        if retired {
            self.persist_and_project(&mut guard, candidate)?;
        }
        Ok(retired)
    }

    fn publish_message_journal_relay_for_dest(
        &self,
        destination_office: &str,
        messages: Vec<org_sync::messages::Message>,
        relays: Vec<StoredRelayItem>,
        incarnation: &str,
        ordinal: u64,
        now_ms: u64,
    ) -> Result<usize, OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        let count = candidate.publish_message_journal_relay_for_dest(
            destination_office,
            messages,
            relays,
            incarnation,
            ordinal,
            now_ms,
        )?;
        if count > 0
            || candidate.last_published_message_journal_by_dest
                != guard.data.last_published_message_journal_by_dest
        {
            self.persist_and_project(&mut guard, candidate)?;
        }
        Ok(count)
    }

    fn has_accepted_activity_from(
        &self,
        origin_office: &str,
        source_id: &str,
    ) -> Result<bool, OfficeError> {
        let guard = self.lock();
        let has_msg = guard
            .data
            .received_receipts
            .keys()
            .any(|(o, id)| o.as_str() == origin_office && id == source_id);
        if has_msg {
            return Ok(true);
        }
        let has_relay =
            guard.data.pending_relays.iter().any(|r| {
                r.id == source_id && r.kind == "message" && r.origin_office == origin_office
            });
        Ok(has_relay)
    }

    fn last_published_journal(&self) -> Result<Option<(String, u64)>, OfficeError> {
        Ok(self.lock().data.last_published_journal.clone())
    }

    fn record_published_journal(&self, incarnation: &str, ordinal: u64) -> Result<(), OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        candidate.last_published_journal = Some((incarnation.to_owned(), ordinal));
        self.persist_and_project(&mut guard, candidate)
    }

    fn publish_hq_journal_events(
        &self,
        codec: Codec,
        payloads: Vec<Arc<[u8]>>,
        incarnation: &str,
        ordinal: u64,
    ) -> Result<usize, OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        let count = candidate.publish_hq_journal_events(codec, payloads, incarnation, ordinal)?;
        if count > 0 || candidate.last_published_journal != guard.data.last_published_journal {
            self.persist_and_project(&mut guard, candidate)?;
        }
        Ok(count)
    }

    fn publish_satellite_work_entries(
        &self,
        codec: Codec,
        record_id: &str,
        payloads: Vec<Arc<[u8]>>,
        incarnation: &str,
        ordinal: u64,
        created_ms: u64,
    ) -> Result<usize, OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        let count = candidate.publish_satellite_work_entries(
            codec,
            record_id,
            payloads,
            incarnation,
            ordinal,
            created_ms,
        )?;
        if count > 0
            || candidate.last_published_work_journal != guard.data.last_published_work_journal
        {
            self.persist_and_project(&mut guard, candidate)?;
        }
        Ok(count)
    }

    fn publish_message_journal_relay(
        &self,
        messages: Vec<org_sync::messages::Message>,
        relays: Vec<StoredRelayItem>,
        incarnation: &str,
        ordinal: u64,
        now_ms: u64,
    ) -> Result<usize, OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.data.clone();
        let count = candidate.publish_message_journal_relay(
            messages,
            relays,
            incarnation,
            ordinal,
            now_ms,
        )?;
        if count > 0
            || candidate.last_published_message_journal != guard.data.last_published_message_journal
        {
            self.persist_and_project(&mut guard, candidate)?;
        }
        Ok(count)
    }

    fn record_replicated_op_id(&self, op_id: &str) -> Result<(), OfficeError> {
        let mut guard = self.lock();
        if guard.data.replicated_op_ids.iter().any(|s| s == op_id) {
            return Ok(());
        }
        let mut candidate = guard.data.clone();
        candidate.record_replicated_op(op_id);
        self.persist_and_project(&mut guard, candidate)
    }

    fn is_replicated_op_id(&self, op_id: &str) -> Result<bool, OfficeError> {
        Ok(self
            .lock()
            .data
            .replicated_op_ids
            .iter()
            .any(|s| s == op_id))
    }
}

fn encode_state_file(data: &ReplicationCoreData) -> Result<Vec<u8>, OfficeError> {
    let cp = &data.checkpoint;
    let file = ReplicationStateFile {
        version: 1,
        org_id: cp.org_id.as_str().to_owned(),
        local_office: cp.local_office.as_str().to_owned(),
        hq_office: cp.hq_office.as_str().to_owned(),
        ca_hex: cp.ca.to_hex(),
        epoch: cp.position.epoch.get(),
        seq: cp.position.seq,
        digest_hex: cp.digest.to_hex(),
        highest_epoch: cp.highest_epoch.get(),
        restore_pending: cp.restore_pending,
        pending_restore_base_hex: cp.pending_restore_base.map(|d| d.to_hex()),
        protocol_level: cp.protocol_level,
        initial_snapshot_required: data.initial_snapshot_required,
        events: data
            .events
            .values()
            .map(|e| StoredOrgEvent {
                epoch: e.position.epoch.get(),
                seq: e.position.seq,
                previous_hex: e.previous.to_hex(),
                digest_hex: e.digest.to_hex(),
                codec_name: e.codec.id().to_owned(),
                codec_version: e.codec.version(),
                payload_b64: encode_b64(&e.payload),
            })
            .collect(),
        work_incarnation: data.work_outbox.origin().incarnation.as_str().to_owned(),
        work_last_seq: data.work_outbox.last_seq(),
        work_acked_seq: data.work_outbox.acknowledged_seq(),
        work_pending: data
            .work_outbox
            .pending()
            .iter()
            .map(|c| StoredWorkChange {
                origin_office: c.key.origin.office.as_str().to_owned(),
                incarnation: c.key.origin.incarnation.as_str().to_owned(),
                seq: c.key.seq.get(),
                record_id: c.record_id.clone(),
                generation: c.version.generation,
                version: c.version.version,
                digest_hex: c.digest.to_hex(),
                record_digest_hex: c.record_digest.to_hex(),
                codec_name: c.codec.id().to_owned(),
                codec_version: c.codec.version(),
                payload_b64: encode_b64(&c.payload),
                required_org_epoch: c.required_org.epoch.get(),
                required_org_seq: c.required_org.seq,
                created_ms: c.created_ms,
            })
            .collect(),
        remote_work_high_water: data.remote_work_high_water.clone(),
        message_clock_high_ms: data.message_queue.clock_high_ms,
        message_pending: data
            .message_queue
            .pending
            .iter()
            .map(message_to_stored)
            .collect(),
        message_outgoing_receipts: data
            .outgoing_receipts
            .values()
            .map(receipt_to_stored)
            .collect(),
        message_received_receipts: data
            .received_receipts
            .values()
            .map(receipt_to_stored)
            .collect(),
        message_expired: data
            .expired_messages
            .values()
            .map(message_to_stored)
            .collect(),
        pending_relays: data.pending_relays.clone(),
        last_published_journal: data.last_published_journal.clone(),
        last_published_work_journal: data.last_published_work_journal.clone(),
        last_published_message_journal: data.last_published_message_journal.clone(),
        last_published_message_journal_by_dest: data.last_published_message_journal_by_dest.clone(),
        replicated_op_ids: data.replicated_op_ids.clone(),
    };
    serde_json::to_vec_pretty(&file).map_err(OfficeError::storage)
}

fn core_from_file(file: &ReplicationStateFile) -> Result<ReplicationCoreData, OfficeError> {
    if file.version != 1 {
        return Err(OfficeError::Invalid(
            "unsupported replication state version",
        ));
    }
    let org_id = OrgId::new(&file.org_id).map_err(|_| OfficeError::Invalid("org_id"))?;
    let local_office =
        OfficeId::new(&file.local_office).map_err(|_| OfficeError::Invalid("local_office"))?;
    let hq_office =
        OfficeId::new(&file.hq_office).map_err(|_| OfficeError::Invalid("hq_office"))?;
    let ca = decode_hex_digest(&file.ca_hex)?;
    let epoch = Epoch::new(file.epoch).map_err(|_| OfficeError::Invalid("epoch"))?;
    let highest_epoch =
        Epoch::new(file.highest_epoch).map_err(|_| OfficeError::Invalid("highest_epoch"))?;
    let pending_restore_base = file
        .pending_restore_base_hex
        .as_deref()
        .map(decode_hex_digest)
        .transpose()?;
    let checkpoint = OrgCheckpoint {
        org_id: org_id.clone(),
        local_office: local_office.clone(),
        hq_office,
        ca,
        position: Position {
            epoch,
            seq: file.seq,
        },
        digest: decode_hex_digest(&file.digest_hex)?,
        highest_epoch,
        restore_pending: file.restore_pending,
        pending_restore_base,
        protocol_level: file.protocol_level,
    };
    let mut events = BTreeMap::new();
    let mut applied_org = Vec::with_capacity(file.events.len());
    for ev in &file.events {
        let payload: Arc<[u8]> = Arc::from(decode_b64(&ev.payload_b64)?);
        let event = OrgEvent {
            org_id: org_id.clone(),
            position: Position {
                epoch: Epoch::new(ev.epoch).map_err(|_| OfficeError::Invalid("event epoch"))?,
                seq: ev.seq,
            },
            previous: decode_hex_digest(&ev.previous_hex)?,
            digest: decode_hex_digest(&ev.digest_hex)?,
            codec: Codec::new(&ev.codec_name, ev.codec_version)
                .map_err(|_| OfficeError::Invalid("event codec"))?,
            payload: Arc::clone(&payload),
        };
        events.insert((ev.epoch, ev.seq), event);
        applied_org.push(payload);
    }
    let work_outbox = outbox_from_stored(
        &local_office,
        &file.work_incarnation,
        file.work_last_seq,
        file.work_acked_seq,
        &file.work_pending,
        &org_id,
    )?;
    let mut message_queue = Queue::new(org_id, local_office);
    message_queue.clock_high_ms = file.message_clock_high_ms;
    for sm in &file.message_pending {
        message_queue.pending.push_back(message_from_stored(sm)?);
    }
    let mut outgoing_receipts = BTreeMap::new();
    for sr in &file.message_outgoing_receipts {
        let r = receipt_from_stored(sr)?;
        outgoing_receipts.insert(r.message.id.clone(), r);
    }
    let mut received_receipts = BTreeMap::new();
    let mut delivered_messages = Vec::new();
    for sr in &file.message_received_receipts {
        let r = receipt_from_stored(sr)?;
        if r.delivered {
            delivered_messages.push(Arc::clone(&r.message.payload));
        }
        received_receipts.insert((r.message.origin.clone(), r.message.id.clone()), r);
    }
    let mut expired_messages = BTreeMap::new();
    for sm in &file.message_expired {
        let m = message_from_stored(sm)?;
        expired_messages.insert((m.origin.clone(), m.id.clone()), m);
    }
    Ok(ReplicationCoreData {
        checkpoint,
        initial_snapshot_required: file.initial_snapshot_required,
        events,
        applied_org,
        staging: BTreeMap::new(),
        supported_protocol_max: 1,
        supported_codec_max: 1,
        work_outbox,
        remote_work_high_water: file.remote_work_high_water.clone(),
        message_queue,
        outgoing_receipts,
        received_receipts,
        delivered_messages,
        expired_messages,
        pending_relays: file.pending_relays.clone(),
        last_published_journal: file.last_published_journal.clone(),
        last_published_work_journal: file.last_published_work_journal.clone(),
        last_published_message_journal: file.last_published_message_journal.clone(),
        last_published_message_journal_by_dest: file.last_published_message_journal_by_dest.clone(),
        replicated_op_ids: file.replicated_op_ids.clone(),
    })
}

fn project_into_sqlite(
    conn: &mut Connection,
    data: &ReplicationCoreData,
) -> Result<(), OfficeError> {
    let tx = conn.transaction().map_err(OfficeError::storage)?;
    let cp = &data.checkpoint;
    tx.execute("DELETE FROM replication_meta", [])
        .map_err(OfficeError::storage)?;
    tx.execute(
        "INSERT INTO replication_meta \
         (id, org_id, local_office, hq_office, epoch, seq, highest_epoch, protocol_level, \
          restore_pending, initial_snapshot_required) \
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            cp.org_id.as_str(),
            cp.local_office.as_str(),
            cp.hq_office.as_str(),
            cp.position.epoch.get() as i64,
            cp.position.seq as i64,
            cp.highest_epoch.get() as i64,
            i64::from(cp.protocol_level),
            i64::from(cp.restore_pending),
            i64::from(data.initial_snapshot_required),
        ],
    )
    .map_err(OfficeError::storage)?;

    tx.execute("DELETE FROM replication_org_events", [])
        .map_err(OfficeError::storage)?;
    for ev in data.events.values() {
        tx.execute(
            "INSERT INTO replication_org_events (epoch, seq, digest_hex, codec_name, codec_version) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                ev.position.epoch.get() as i64,
                ev.position.seq as i64,
                ev.digest.to_hex(),
                ev.codec.id(),
                i64::from(ev.codec.version()),
            ],
        )
        .map_err(OfficeError::storage)?;
    }

    tx.execute("DELETE FROM replication_work_outbox", [])
        .map_err(OfficeError::storage)?;
    for change in data.work_outbox.pending() {
        tx.execute(
            "INSERT INTO replication_work_outbox (seq, record_id, digest_hex) VALUES (?1, ?2, ?3)",
            params![
                change.key.seq.get() as i64,
                &change.record_id,
                change.digest.to_hex(),
            ],
        )
        .map_err(OfficeError::storage)?;
    }

    tx.execute("DELETE FROM replication_message_outbox", [])
        .map_err(OfficeError::storage)?;
    for msg in &data.message_queue.pending {
        tx.execute(
            "INSERT INTO replication_message_outbox (message_id, destination_office, deadline_ms) \
             VALUES (?1, ?2, ?3)",
            params![&msg.id, msg.destination.as_str(), msg.deadline_ms as i64],
        )
        .map_err(OfficeError::storage)?;
    }
    tx.commit().map_err(OfficeError::storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_sync::organization::{append_hq, OrgStore};

    #[test]
    fn sqlite_replication_store_passes_eng3_conformance_and_rebuilds_after_runtime_delete() {
        let mut holders = Vec::new();
        crate::replication::conformance::run_eng3_replication_suite(|spec| {
            let dir = tempfile::tempdir().unwrap();
            let durable = dir.path().join("durable");
            let runtime = dir.path().join("runtime");
            fs::create_dir_all(&durable).unwrap();
            fs::create_dir_all(&runtime).unwrap();
            let store = SqliteReplicationStore::open(OpenReplicationParams {
                durable_root: &durable,
                runtime_root: &runtime,
                org_id: spec.org_id,
                local_office: spec.local_office,
                hq_office: spec.hq_office,
                ca: spec.ca,
                epoch: spec.epoch,
                incarnation: spec.incarnation,
                initial_snapshot_required: spec.initial_snapshot_required,
            })
            .unwrap();
            holders.push(dir);
            Box::new(store)
        });

        // Verify durable survival + runtime/ delete-and-rebuild on SQLite.
        let dir = tempfile::tempdir().unwrap();
        let durable = dir.path().join("durable");
        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&durable).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        let org_id = OrgId::new("0195a000-0000-7000-8000-000000000001").unwrap();
        let hq_id = OfficeId::new("0195a000-0000-7000-8000-000000000002").unwrap();
        let inc = OriginIncarnation::new("0195a000-0000-7000-8000-000000000004").unwrap();
        let ca = Digest::from_bytes([0x42; 32]);
        {
            let store = SqliteReplicationStore::open(OpenReplicationParams {
                durable_root: &durable,
                runtime_root: &runtime,
                org_id: org_id.clone(),
                local_office: hq_id.clone(),
                hq_office: hq_id.clone(),
                ca,
                epoch: Epoch::new(1).unwrap(),
                incarnation: inc.clone(),
                initial_snapshot_required: false,
            })
            .unwrap();
            let codec = Codec::new("org.native.changes", 1).unwrap();
            OrgStore::transact(
                &store,
                Arc::new(move |tx| {
                    append_hq(tx, codec.clone(), Arc::from(b"persisted".as_slice()))
                }),
            )
            .unwrap();
        }
        // Delete rebuildable runtime/ completely and reopen.
        fs::remove_dir_all(&runtime).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        let reopened = SqliteReplicationStore::open(OpenReplicationParams {
            durable_root: &durable,
            runtime_root: &runtime,
            org_id,
            local_office: hq_id.clone(),
            hq_office: hq_id,
            ca,
            epoch: Epoch::new(1).unwrap(),
            incarnation: inc,
            initial_snapshot_required: false,
        })
        .unwrap();
        assert_eq!(reopened.sync_status().unwrap().seq, 1);
        assert_eq!(reopened.org_events_after(1, 0).unwrap().len(), 1);
    }
}
