//! The shared in-memory replication state: org checkpoint and journals, the work outbox,
//! re-enrollment and status.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use org_sync::{
    error::{Refusal, SyncError},
    messages::{Message, Queue, Receipt},
    organization::{OrgCheckpoint, OrgEvent},
    work::{Change as WorkChange, LocalOutbox, Origin, MAX_PENDING, MAX_PENDING_BYTES},
};
use org_sync_proto::{
    Codec, Digest, Epoch, OfficeId, OrgId, OriginIncarnation, Position, SourceSeq,
};

use crate::error::OfficeError;
use crate::replication::{initial_checkpoint, StoredRelayItem, SyncStatusSnapshot};

#[derive(Clone, Debug)]
pub struct ReplicationCoreData {
    pub checkpoint: OrgCheckpoint,
    pub initial_snapshot_required: bool,
    pub events: BTreeMap<(u64, u64), OrgEvent>,
    pub applied_org: Vec<Arc<[u8]>>,
    pub staging: BTreeMap<String, Vec<Arc<[u8]>>>,
    pub supported_protocol_max: u32,
    pub supported_codec_max: u32,
    pub work_outbox: LocalOutbox,
    pub remote_work_high_water: BTreeMap<String, u64>,
    pub message_queue: Queue,
    pub outgoing_receipts: BTreeMap<String, Receipt>,
    pub received_receipts: BTreeMap<(OfficeId, String), Receipt>,
    pub delivered_messages: Vec<Arc<[u8]>>,
    pub expired_messages: BTreeMap<(OfficeId, String), Message>,
    pub pending_relays: Vec<StoredRelayItem>,
    pub last_published_journal: Option<(String, u64)>,
    pub last_published_work_journal: Option<(String, u64)>,
    pub last_published_message_journal: Option<(String, u64)>,
    pub last_published_message_journal_by_dest: BTreeMap<String, (String, u64)>,
    pub replicated_op_ids: VecDeque<String>,
}

pub(super) fn relay_item_expired(item: &StoredRelayItem, now_ms: u64) -> bool {
    if item.id.starts_with("close-") {
        return false;
    }
    if item.expires_at_ms > 0 && item.expires_at_ms <= now_ms {
        return true;
    }
    if item.kind != "message" {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(&item.payload_json)
        .ok()
        .and_then(|v| v.get("deadline_ms").and_then(serde_json::Value::as_u64))
        .is_some_and(|deadline| deadline <= now_ms)
}

pub(super) fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ReplicationCoreData {
    pub(crate) fn new(
        org_id: OrgId,
        local_office: OfficeId,
        hq_office: OfficeId,
        ca: Digest,
        epoch: Epoch,
        incarnation: OriginIncarnation,
        initial_snapshot_required: bool,
    ) -> Self {
        let origin = Origin {
            office: local_office.clone(),
            incarnation,
        };
        let message_queue = Queue::new(org_id.clone(), local_office.clone());
        Self {
            checkpoint: initial_checkpoint(org_id, local_office, hq_office, ca, epoch),
            initial_snapshot_required,
            events: BTreeMap::new(),
            applied_org: Vec::new(),
            staging: BTreeMap::new(),
            supported_protocol_max: 1,
            supported_codec_max: 1,
            work_outbox: LocalOutbox::new(origin),
            remote_work_high_water: BTreeMap::new(),
            message_queue,
            outgoing_receipts: BTreeMap::new(),
            received_receipts: BTreeMap::new(),
            delivered_messages: Vec::new(),
            expired_messages: BTreeMap::new(),
            pending_relays: Vec::new(),
            last_published_journal: None,
            last_published_work_journal: None,
            last_published_message_journal: None,
            last_published_message_journal_by_dest: BTreeMap::new(),
            replicated_op_ids: VecDeque::new(),
        }
    }

    pub(crate) fn record_replicated_op(&mut self, op_id: &str) {
        if self.replicated_op_ids.iter().any(|s| s == op_id) {
            return;
        }
        if self.replicated_op_ids.len() >= 2048 {
            self.replicated_op_ids.pop_front();
        }
        self.replicated_op_ids.push_back(op_id.to_owned());
    }

    /// Atomically publishes one or more satellite work outbox entries for `(incarnation, ordinal)`
    /// (`TASK-52` #6). Returns `Ok(0)` if already published.
    pub(crate) fn publish_satellite_work_entries(
        &mut self,
        codec: Codec,
        record_id: &str,
        payloads: Vec<Arc<[u8]>>,
        incarnation: &str,
        ordinal: u64,
        created_ms: u64,
    ) -> Result<usize, OfficeError> {
        if let Some((ref inc, ord)) = self.last_published_work_journal {
            if inc == incarnation && ordinal <= ord {
                return Ok(0);
            }
        }
        let mut count = 0usize;
        for payload in payloads {
            let next_seq = self.work_outbox.last_seq().saturating_add(1);
            let src_seq =
                SourceSeq::new(next_seq).map_err(|_| OfficeError::Invalid("work seq overflow"))?;
            let origin = self.work_outbox.origin().clone();
            let rec_digest =
                org_sync_proto::work_record_digest_v1(record_id, 1, next_seq, &codec, &payload)
                    .map_err(|_| OfficeError::Invalid("work record digest failed"))?;
            let src_digest = org_sync_proto::work_source_digest_v1(
                &origin.office,
                &origin.incarnation,
                src_seq,
                rec_digest,
                self.checkpoint.position,
                created_ms,
            );
            let change = WorkChange {
                org_id: self.checkpoint.org_id.clone(),
                key: org_sync::work::SourceKey {
                    origin,
                    seq: src_seq,
                },
                record_id: record_id.to_owned(),
                version: org_sync::work::RecordVersion {
                    generation: 1,
                    version: next_seq,
                },
                digest: src_digest,
                record_digest: rec_digest,
                codec: codec.clone(),
                payload,
                required_org: self.checkpoint.position,
                created_ms,
            };
            self.enqueue_work(change)
                .map_err(|_| OfficeError::Invalid("failed enqueuing satellite work"))?;
            count += 1;
        }
        self.last_published_work_journal = Some((incarnation.to_owned(), ordinal));
        Ok(count)
    }

    /// Re-anchors checkpoint onto a replacement HQ (D25) AND rotates the local work outbox to a
    /// fresh `OriginIncarnation` starting at `seq = 1`, preserving all unacknowledged offline work
    /// so the replacement HQ (at high-water 0) accepts pending changes cleanly (`TASK-52` H-8).
    pub(crate) fn apply_re_enrollment(
        &mut self,
        new_hq_office: OfficeId,
        new_ca: Digest,
        new_epoch: Epoch,
    ) -> Result<(), OfficeError> {
        let old_hq_office = self.checkpoint.hq_office.clone();
        self.checkpoint.hq_office = new_hq_office.clone();
        self.checkpoint.ca = new_ca;
        self.checkpoint.position = Position {
            epoch: new_epoch,
            seq: 0,
        };
        self.checkpoint.digest = Digest::ZERO;
        self.checkpoint.highest_epoch = new_epoch;
        self.checkpoint.restore_pending = false;
        self.checkpoint.pending_restore_base = None;
        self.initial_snapshot_required = true;
        self.events.clear();
        // Rebind offline pending messages addressed to the retired HQ onto the replacement HQ
        // while preserving message ID, origin, fingerprint, payload, and non-HQ satellite targets
        // (`TASK-52` P3-#8).
        for pending_msg in &mut self.message_queue.pending {
            if pending_msg.destination == old_hq_office {
                pending_msg.destination = new_hq_office.clone();
            }
        }

        let old_inc = self.work_outbox.origin().incarnation.as_str();
        let base = old_inc.split("-r").next().unwrap_or(old_inc);
        let base = &base[..base.len().min(36)];
        let prev_gen: u64 = old_inc
            .rsplit_once("-r")
            .and_then(|(_, n)| n.parse().ok())
            .unwrap_or(0);
        let next_inc_str = format!("{base}-r{}", prev_gen.saturating_add(1));
        let next_inc = OriginIncarnation::new(&next_inc_str)
            .map_err(|_| OfficeError::Invalid("failed rotating work outbox incarnation"))?;
        let new_origin = Origin {
            office: self.work_outbox.origin().office.clone(),
            incarnation: next_inc,
        };
        let mut rebased = VecDeque::with_capacity(self.work_outbox.pending().len());
        let mut seq_num = 0u64;
        for existing in self.work_outbox.pending() {
            seq_num = seq_num.saturating_add(1);
            let src_seq = SourceSeq::new(seq_num)
                .map_err(|_| OfficeError::Invalid("work outbox sequence overflow"))?;
            let required_org = Position {
                epoch: new_epoch,
                seq: 0,
            };
            let digest = org_sync_proto::work_source_digest_v1(
                &new_origin.office,
                &new_origin.incarnation,
                src_seq,
                existing.record_digest,
                required_org,
                existing.created_ms,
            );
            rebased.push_back(WorkChange {
                org_id: existing.org_id.clone(),
                key: org_sync::work::SourceKey {
                    origin: new_origin.clone(),
                    seq: src_seq,
                },
                record_id: existing.record_id.clone(),
                version: existing.version,
                digest,
                record_digest: existing.record_digest,
                codec: existing.codec.clone(),
                payload: Arc::clone(&existing.payload),
                required_org,
                created_ms: existing.created_ms,
            });
        }
        self.work_outbox = LocalOutbox::restore(new_origin, seq_num, 0, rebased)
            .map_err(|_| OfficeError::Invalid("failed rebasing work outbox for re-enrollment"))?;
        Ok(())
    }

    pub(crate) fn enqueue_work(&mut self, change: WorkChange) -> Result<LocalOutbox, SyncError> {
        let next_seq = self
            .work_outbox
            .last_seq()
            .checked_add(1)
            .ok_or(Refusal::InvariantFailure)?;
        if change.key.origin != *self.work_outbox.origin()
            || change.key.seq.get() != next_seq
            || change.org_id != self.checkpoint.org_id
        {
            return Err(Refusal::InvalidInput.into());
        }
        let mut pending = self.work_outbox.pending().clone();
        let bytes: usize = pending.iter().map(|c| c.payload.len()).sum();
        if pending.len() >= MAX_PENDING
            || bytes.saturating_add(change.payload.len()) > MAX_PENDING_BYTES
        {
            return Err(Refusal::QuotaExceeded.into());
        }
        pending.push_back(change);
        let updated = LocalOutbox::restore(
            self.work_outbox.origin().clone(),
            next_seq,
            self.work_outbox.acknowledged_seq(),
            pending,
        )?;
        self.work_outbox = updated.clone();
        Ok(updated)
    }

    pub(crate) fn ack_work(&mut self, acked_seq: u64) -> Result<LocalOutbox, SyncError> {
        let current_ack = self.work_outbox.acknowledged_seq();
        if acked_seq > self.work_outbox.last_seq() {
            return Err(Refusal::InvalidInput.into());
        }
        if acked_seq <= current_ack {
            return Ok(self.work_outbox.clone());
        }
        let mut pending: VecDeque<WorkChange> = self.work_outbox.pending().clone();
        while pending
            .front()
            .is_some_and(|c| c.key.seq.get() <= acked_seq)
        {
            pending.pop_front();
        }
        let updated = LocalOutbox::restore(
            self.work_outbox.origin().clone(),
            self.work_outbox.last_seq(),
            acked_seq,
            pending,
        )?;
        self.work_outbox = updated.clone();
        Ok(updated)
    }

    pub(crate) fn status(&self) -> SyncStatusSnapshot {
        SyncStatusSnapshot {
            org_id: self.checkpoint.org_id.as_str().to_owned(),
            local_office: self.checkpoint.local_office.as_str().to_owned(),
            hq_office: self.checkpoint.hq_office.as_str().to_owned(),
            epoch: self.checkpoint.position.epoch.get(),
            seq: self.checkpoint.position.seq,
            highest_epoch: self.checkpoint.highest_epoch.get(),
            protocol_level: self.checkpoint.protocol_level,
            restore_pending: self.checkpoint.restore_pending,
            initial_snapshot_required: self.initial_snapshot_required,
            pending_work_count: self.work_outbox.pending().len(),
            pending_message_count: self.message_queue.pending.len(),
        }
    }
}
