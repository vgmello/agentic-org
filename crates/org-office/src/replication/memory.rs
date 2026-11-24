//! In-memory engine-independent [`ReplicationStore`] adapter (`ADR-0018` `ENG-2`/`ENG-3`).
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use org_sync::{
    error::{Refusal, StoreError, SyncError},
    messages::{
        Message, MessageAttempt, MessageAuthority, MessageOutput, MessageStore, MessageTxn, Queue,
        Receipt,
    },
    organization::{
        OrgAttempt, OrgCheckpoint, OrgEvent, OrgOutput, OrgStore, OrgTxn, PeerIdentity,
        ValidatedSnapshot,
    },
    work::{Change as WorkChange, LocalOutbox, Origin, MAX_PENDING, MAX_PENDING_BYTES},
};
use org_sync_proto::{
    Codec, Digest, Epoch, OfficeId, OrgId, OriginIncarnation, Position, SourceSeq,
};

use super::{initial_checkpoint, ReplicationStore, StoredRelayItem, SyncStatusSnapshot};
use crate::error::OfficeError;

#[derive(Clone, Debug)]
pub(crate) struct ReplicationCoreData {
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

fn relay_item_expired(item: &StoredRelayItem, now_ms: u64) -> bool {
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

fn current_unix_ms() -> u64 {
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

    /// Atomically publishes one or more HQ `OrgEvent` payloads for `(incarnation, ordinal)` into
    /// this state (`TASK-52` M-16). Returns `Ok(0)` if already published.
    pub(crate) fn publish_hq_journal_events(
        &mut self,
        codec: Codec,
        payloads: Vec<Arc<[u8]>>,
        incarnation: &str,
        ordinal: u64,
    ) -> Result<usize, OfficeError> {
        if let Some((ref inc, ord)) = self.last_published_journal {
            if inc == incarnation && ordinal <= ord {
                return Ok(0);
            }
        }
        let mut count = 0usize;
        for payload in payloads {
            org_sync::organization::append_hq(
                &mut CoreOrgTxn { data: self },
                codec.clone(),
                payload,
            )
            .map_err(|_| OfficeError::Invalid("failed publishing org replication event"))?;
            count += 1;
        }
        self.last_published_journal = Some((incarnation.to_owned(), ordinal));
        Ok(count)
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

    /// Reclaims all expired pending outgoing messages anywhere in the queue (not only at the head)
    /// so expired traffic to an offline office cannot consume quota behind a live head (`TASK-52` P9-#7).
    /// Authoritative ask closes/withdrawals (`close-*`) are retained independently of wire deadlines
    /// (`TASK-52` P13-#1).
    fn prune_expired_pending_messages(&mut self, now_ms: u64) {
        self.message_queue.clock_high_ms = self.message_queue.clock_high_ms.max(now_ms);
        let cutoff = self.message_queue.clock_high_ms;
        let mut kept = VecDeque::with_capacity(self.message_queue.pending.len());
        while let Some(msg) = self.message_queue.pending.pop_front() {
            if !msg.id.starts_with("close-") && msg.deadline_ms <= cutoff {
                self.expired_messages
                    .insert((msg.origin.clone(), msg.id.clone()), msg.clone());
                self.outgoing_receipts
                    .entry(msg.id.clone())
                    .or_insert(Receipt {
                        message: msg,
                        delivered: false,
                    });
            } else {
                kept.push_back(msg);
            }
        }
        self.message_queue.pending = kept;
    }

    fn enqueue_message_if_eligible(
        &mut self,
        msg: Message,
        now_ms: u64,
    ) -> Result<bool, OfficeError> {
        self.prune_expired_pending_messages(now_ms);
        let is_close = msg.id.starts_with("close-");
        let valid_envelope = msg.org_id == self.checkpoint.org_id
            && msg.origin == self.checkpoint.local_office
            && (is_close || msg.deadline_ms > self.message_queue.clock_high_ms)
            && msg.payload.len() <= org_sync::messages::MAX_PAYLOAD_BYTES;
        if !valid_envelope
            || self.message_queue.pending.iter().any(|m| m.id == msg.id)
            || self.outgoing_receipts.contains_key(&msg.id)
        {
            return Ok(false);
        }
        // Enforce BOTH a per-destination fair share (half the queue so one offline destination
        // cannot starve healthy destinations) AND the aggregate `org-sync::messages::Queue` cap
        // before persisting so drain, ack, and restart never hit `InvariantFailure` (`TASK-52` P9-#7 / P10-#1).
        let total_len = self.message_queue.pending.len();
        let total_bytes: usize = self
            .message_queue
            .pending
            .iter()
            .map(|m| m.payload.len())
            .sum();
        let dest_len = self
            .message_queue
            .pending
            .iter()
            .filter(|m| m.destination == msg.destination)
            .count();
        let dest_bytes: usize = self
            .message_queue
            .pending
            .iter()
            .filter(|m| m.destination == msg.destination)
            .map(|m| m.payload.len())
            .sum();
        let per_dest_max_items = (org_sync::messages::MAX_PENDING / 2).max(1);
        let per_dest_max_bytes = (org_sync::messages::MAX_PENDING_BYTES / 2).max(1);
        if total_len >= org_sync::messages::MAX_PENDING
            || total_bytes.saturating_add(msg.payload.len()) > org_sync::messages::MAX_PENDING_BYTES
            || dest_len >= per_dest_max_items
            || dest_bytes.saturating_add(msg.payload.len()) > per_dest_max_bytes
        {
            return Err(OfficeError::Invalid("message outbox quota exceeded"));
        }
        self.message_queue.pending.push_back(msg);
        Ok(true)
    }

    pub(crate) fn has_received_message_receipt(
        &self,
        origin_office: &str,
        message_id: &str,
        fingerprint_hex: &str,
    ) -> bool {
        let Ok(origin) = OfficeId::new(origin_office) else {
            return false;
        };
        self.received_receipts
            .get(&(origin, message_id.to_owned()))
            .is_some_and(|r| r.delivered && r.message.fingerprint.to_hex() == fingerprint_hex)
    }

    pub(crate) fn enqueue_relay_checked(
        &mut self,
        item: StoredRelayItem,
        now_ms: u64,
    ) -> Result<bool, OfficeError> {
        // Reclaim expired message and wake relays across ALL destinations before checking quotas
        // (`TASK-52` P6-#8 / P7-#9).
        self.pending_relays
            .retain(|existing| !relay_item_expired(existing, now_ms));
        if self.pending_relays.iter().any(|r| r.id == item.id) {
            return Ok(false);
        }
        // Isolate receipt traffic AND per-destination quotas so one offline/removed office can
        // never exhaust HQ relay or receipt capacity for healthy offices (`TASK-52` P7-#9 / P8-#8).
        if item.kind == "receipt" {
            let receipt_count = self
                .pending_relays
                .iter()
                .filter(|r| r.kind == "receipt" && r.destination_office == item.destination_office)
                .count();
            let receipt_bytes: usize = self
                .pending_relays
                .iter()
                .filter(|r| r.kind == "receipt" && r.destination_office == item.destination_office)
                .map(|r| r.payload_json.len())
                .sum();
            if receipt_count >= org_sync::messages::MAX_PENDING * 2
                || receipt_bytes.saturating_add(item.payload_json.len())
                    > org_sync::messages::MAX_PENDING_BYTES
            {
                return Err(OfficeError::Invalid("receipt relay quota exceeded"));
            }
        } else {
            let dest_count = self
                .pending_relays
                .iter()
                .filter(|r| r.kind != "receipt" && r.destination_office == item.destination_office)
                .count();
            let dest_bytes: usize = self
                .pending_relays
                .iter()
                .filter(|r| r.kind != "receipt" && r.destination_office == item.destination_office)
                .map(|r| r.payload_json.len())
                .sum();
            if dest_count >= org_sync::messages::MAX_PENDING
                || dest_bytes.saturating_add(item.payload_json.len())
                    > org_sync::messages::MAX_PENDING_BYTES
            {
                return Err(OfficeError::Invalid("relay queue quota exceeded"));
            }
        }
        self.pending_relays.push(item);
        Ok(true)
    }

    pub(crate) fn retire_undeliverable_message_inner(
        &mut self,
        message_id: &str,
        destination_office: &str,
    ) -> bool {
        let Some(pos) = self
            .message_queue
            .pending
            .iter()
            .position(|m| m.id == message_id && m.destination.as_str() == destination_office)
        else {
            return false;
        };
        if let Some(msg) = self.message_queue.pending.remove(pos) {
            self.outgoing_receipts
                .entry(msg.id.clone())
                .or_insert(Receipt {
                    message: msg,
                    delivered: false,
                });
            true
        } else {
            false
        }
    }

    pub(crate) fn ack_message_relay_and_enqueue_receipt_inner(
        &mut self,
        destination_office: &str,
        origin_office: &str,
        message_id: &str,
        fingerprint_hex: &str,
        receipt_item: Option<StoredRelayItem>,
        now_ms: u64,
    ) -> Result<bool, OfficeError> {
        let composite_id = format!("{message_id}:{destination_office}");
        let matches = |r: &StoredRelayItem| {
            r.kind == "message"
                && (r.id == message_id || r.id == composite_id)
                && r.destination_office == destination_office
                && r.origin_office == origin_office
                && r.fingerprint_hex == fingerprint_hex
        };
        if !self.pending_relays.iter().any(matches) {
            return Ok(false);
        }
        self.pending_relays.retain(|r| !matches(r));
        if let Some(receipt) = receipt_item {
            let _ = self.enqueue_relay_checked(receipt, now_ms)?;
        }
        Ok(true)
    }

    /// Atomically publishes outbound cross-office messages and/or HQ relay items for a committed
    /// messaging journal `(incarnation, ordinal)` (`TASK-52` #10 / P4-#5 / P5-#11 / P6-#8).
    pub(crate) fn publish_message_journal_relay(
        &mut self,
        messages: Vec<Message>,
        relays: Vec<StoredRelayItem>,
        incarnation: &str,
        ordinal: u64,
        now_ms: u64,
    ) -> Result<usize, OfficeError> {
        if let Some((ref inc, ord)) = self.last_published_message_journal {
            if inc == incarnation && ordinal <= ord {
                return Ok(0);
            }
        }
        let mut count = 0usize;
        for msg in messages {
            if self.enqueue_message_if_eligible(msg, now_ms)? {
                count += 1;
            }
        }
        for item in relays {
            if self.enqueue_relay_checked(item, now_ms)? {
                count += 1;
            }
        }
        self.last_published_message_journal = Some((incarnation.to_owned(), ordinal));
        Ok(count)
    }

    /// Atomically publishes outbound messages/relays for a single destination office and advances
    /// only that destination's durable `(incarnation, ordinal)` watermark (`TASK-52` P8-#7).
    pub(crate) fn publish_message_journal_relay_for_dest(
        &mut self,
        destination_office: &str,
        messages: Vec<Message>,
        relays: Vec<StoredRelayItem>,
        incarnation: &str,
        ordinal: u64,
        now_ms: u64,
    ) -> Result<usize, OfficeError> {
        if let Some((ref inc, ord)) = self.last_published_message_journal {
            if inc == incarnation && ordinal <= ord {
                return Ok(0);
            }
        }
        if let Some((ref inc, ord)) = self
            .last_published_message_journal_by_dest
            .get(destination_office)
        {
            if inc == incarnation && ordinal <= *ord {
                return Ok(0);
            }
        }
        let mut count = 0usize;
        for msg in messages {
            if self.enqueue_message_if_eligible(msg, now_ms)? {
                count += 1;
            }
        }
        for item in relays {
            if self.enqueue_relay_checked(item, now_ms)? {
                count += 1;
            }
        }
        self.last_published_message_journal_by_dest.insert(
            destination_office.to_owned(),
            (incarnation.to_owned(), ordinal),
        );
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

pub(crate) struct CoreOrgTxn<'a> {
    pub data: &'a mut ReplicationCoreData,
}

impl OrgTxn for CoreOrgTxn<'_> {
    fn checkpoint(&mut self) -> Result<OrgCheckpoint, StoreError> {
        Ok(self.data.checkpoint.clone())
    }

    fn requires_initial_snapshot(&mut self) -> Result<bool, StoreError> {
        Ok(self.data.initial_snapshot_required)
    }

    fn digest_at(&mut self, position: Position) -> Result<Option<Digest>, StoreError> {
        Ok(self
            .data
            .events
            .get(&(position.epoch.get(), position.seq))
            .map(|e| e.digest))
    }

    fn supports_codec(&mut self, codec: &Codec) -> Result<bool, StoreError> {
        Ok(codec.version() <= self.data.supported_codec_max)
    }

    fn supports_protocol(&mut self, level: u32) -> Result<bool, StoreError> {
        Ok(level >= 1 && level <= self.data.supported_protocol_max)
    }

    fn apply_event(&mut self, event: &OrgEvent) -> Result<(), SyncError> {
        self.data.applied_org.push(Arc::clone(&event.payload));
        Ok(())
    }

    fn record_event(&mut self, event: &OrgEvent, next: &OrgCheckpoint) -> Result<(), StoreError> {
        self.data.events.insert(
            (event.position.epoch.get(), event.position.seq),
            event.clone(),
        );
        self.data.checkpoint = next.clone();
        Ok(())
    }

    fn install_snapshot(
        &mut self,
        snapshot: &ValidatedSnapshot,
        next: &OrgCheckpoint,
    ) -> Result<(), SyncError> {
        let payloads = self
            .data
            .staging
            .remove(&snapshot.staging_id)
            .ok_or(Refusal::InvalidInput)?;
        self.data.applied_org = payloads;
        self.data.events.clear();
        self.data.checkpoint = next.clone();
        self.data.initial_snapshot_required = false;
        Ok(())
    }

    fn set_authority(&mut self, next: &OrgCheckpoint) -> Result<(), StoreError> {
        self.data.checkpoint = next.clone();
        Ok(())
    }
}

pub(crate) struct CoreMessageTxn<'a> {
    pub data: &'a mut ReplicationCoreData,
}

impl MessageTxn for CoreMessageTxn<'_> {
    fn restore_pending(&mut self) -> Result<bool, StoreError> {
        Ok(self.data.checkpoint.restore_pending)
    }

    fn authority(&mut self) -> Result<MessageAuthority, StoreError> {
        Ok(MessageAuthority {
            org_id: self.data.checkpoint.org_id.clone(),
            office_id: self.data.checkpoint.local_office.clone(),
            epoch: self.data.checkpoint.highest_epoch,
            hq_office_id: self.data.checkpoint.hq_office.clone(),
        })
    }

    fn queue(&mut self) -> Result<Queue, StoreError> {
        Ok(self.data.message_queue.clone())
    }

    fn supports_current_protocol(&mut self) -> Result<bool, StoreError> {
        Ok(self.data.checkpoint.protocol_level >= 1
            && self.data.checkpoint.protocol_level <= self.data.supported_protocol_max)
    }

    fn outgoing_receipt(&mut self, id: &str) -> Result<Option<Receipt>, StoreError> {
        Ok(self.data.outgoing_receipts.get(id).cloned())
    }

    fn received(&mut self, origin: &OfficeId, id: &str) -> Result<Option<Receipt>, StoreError> {
        Ok(self
            .data
            .received_receipts
            .get(&(origin.clone(), id.to_owned()))
            .cloned())
    }

    fn authorize_send(&mut self, _message: &Message) -> Result<(), SyncError> {
        Ok(())
    }

    fn authorize_receive(
        &mut self,
        _peer: &PeerIdentity,
        _message: &Message,
    ) -> Result<(), SyncError> {
        Ok(())
    }

    fn authorize_receipt(
        &mut self,
        _peer: &PeerIdentity,
        _receipt: &Receipt,
    ) -> Result<(), SyncError> {
        Ok(())
    }

    fn apply_message(&mut self, message: &Message) -> Result<(), SyncError> {
        self.data
            .delivered_messages
            .push(Arc::clone(&message.payload));
        Ok(())
    }

    fn expired_identity(
        &mut self,
        origin: &OfficeId,
        id: &str,
    ) -> Result<Option<Message>, StoreError> {
        Ok(self
            .data
            .expired_messages
            .get(&(origin.clone(), id.to_owned()))
            .cloned())
    }

    fn record_expired(&mut self, message: &Message) -> Result<(), StoreError> {
        if message.id.starts_with("close-") {
            return Ok(());
        }
        self.data
            .expired_messages
            .entry((message.origin.clone(), message.id.clone()))
            .or_insert_with(|| message.clone());
        Ok(())
    }

    fn record_received(&mut self, receipt: &Receipt) -> Result<(), StoreError> {
        self.data.received_receipts.insert(
            (receipt.message.origin.clone(), receipt.message.id.clone()),
            receipt.clone(),
        );
        Ok(())
    }

    fn save_queue(&mut self, queue: &Queue, receipt: Option<&Receipt>) -> Result<(), StoreError> {
        self.data.message_queue = queue.clone();
        if let Some(r) = receipt {
            let entry = self
                .data
                .outgoing_receipts
                .entry(r.message.id.clone())
                .or_insert_with(|| r.clone());
            if r.delivered {
                entry.delivered = true;
            }
        }
        Ok(())
    }
}

/// In-memory transactional [`ReplicationStore`] implementing `ENG-2`/`ENG-3` atomic rollback.
pub struct MemoryReplicationStore {
    inner: Mutex<ReplicationCoreData>,
}

impl MemoryReplicationStore {
    pub fn new(
        org_id: OrgId,
        local_office: OfficeId,
        hq_office: OfficeId,
        ca: Digest,
        epoch: Epoch,
        incarnation: OriginIncarnation,
        initial_snapshot_required: bool,
    ) -> Self {
        Self {
            inner: Mutex::new(ReplicationCoreData::new(
                org_id,
                local_office,
                hq_office,
                ca,
                epoch,
                incarnation,
                initial_snapshot_required,
            )),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ReplicationCoreData> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl OrgStore for MemoryReplicationStore {
    fn transact(&self, attempt: Arc<OrgAttempt>) -> Result<OrgOutput, SyncError> {
        let mut guard = self.lock();
        let mut candidate = guard.clone();
        let output = attempt(&mut CoreOrgTxn {
            data: &mut candidate,
        })?;
        *guard = candidate;
        Ok(output)
    }
}

impl MessageStore for MemoryReplicationStore {
    fn transact(&self, attempt: Arc<MessageAttempt>) -> Result<MessageOutput, SyncError> {
        let mut guard = self.lock();
        let mut candidate = guard.clone();
        let output = attempt(&mut CoreMessageTxn {
            data: &mut candidate,
        })?;
        *guard = candidate;
        Ok(output)
    }
}

impl ReplicationStore for MemoryReplicationStore {
    fn org_checkpoint(&self) -> Result<OrgCheckpoint, OfficeError> {
        Ok(self.lock().checkpoint.clone())
    }

    fn org_events_after(&self, epoch: u64, seq: u64) -> Result<Vec<OrgEvent>, OfficeError> {
        Ok(self
            .lock()
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
        self.lock().staging.insert(staging_id.to_owned(), payloads);
        Ok(())
    }

    fn applied_org_payloads(&self) -> Result<Vec<Arc<[u8]>>, OfficeError> {
        Ok(self.lock().applied_org.clone())
    }

    fn load_work_outbox(&self) -> Result<LocalOutbox, OfficeError> {
        Ok(self.lock().work_outbox.clone())
    }

    fn enqueue_work_change(&self, change: WorkChange) -> Result<LocalOutbox, SyncError> {
        self.lock().enqueue_work(change)
    }

    fn ack_work_through(&self, acked_seq: u64) -> Result<LocalOutbox, SyncError> {
        self.lock().ack_work(acked_seq)
    }

    fn record_remote_work_seq(&self, origin: &Origin, seq: SourceSeq) -> Result<bool, OfficeError> {
        let key = format!("{}:{}", origin.office.as_str(), origin.incarnation.as_str());
        let mut guard = self.lock();
        let current = guard.remote_work_high_water.get(&key).copied().unwrap_or(0);
        if seq.get() <= current {
            return Ok(false);
        }
        guard.remote_work_high_water.insert(key, seq.get());
        Ok(true)
    }

    fn remote_work_high_water(&self, origin: &Origin) -> Result<u64, OfficeError> {
        let key = format!("{}:{}", origin.office.as_str(), origin.incarnation.as_str());
        Ok(self
            .lock()
            .remote_work_high_water
            .get(&key)
            .copied()
            .unwrap_or(0))
    }

    fn delivered_message_payloads(&self) -> Result<Vec<Arc<[u8]>>, OfficeError> {
        Ok(self.lock().delivered_messages.clone())
    }

    fn message_queue_snapshot(&self) -> Result<Queue, OfficeError> {
        Ok(self.lock().message_queue.clone())
    }

    fn outgoing_receipt_snapshot(&self, message_id: &str) -> Result<Option<Receipt>, OfficeError> {
        Ok(self.lock().outgoing_receipts.get(message_id).cloned())
    }

    fn has_received_message_receipt(
        &self,
        origin_office: &str,
        message_id: &str,
        fingerprint_hex: &str,
    ) -> Result<bool, OfficeError> {
        Ok(self
            .lock()
            .has_received_message_receipt(origin_office, message_id, fingerprint_hex))
    }

    fn reset_for_re_enrollment(
        &self,
        new_hq_office: OfficeId,
        new_ca: Digest,
        new_epoch: Epoch,
    ) -> Result<(), OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.clone();
        candidate.apply_re_enrollment(new_hq_office, new_ca, new_epoch)?;
        *guard = candidate;
        Ok(())
    }

    fn sync_status(&self) -> Result<SyncStatusSnapshot, OfficeError> {
        Ok(self.lock().status())
    }

    fn enqueue_relay_item(&self, item: StoredRelayItem) -> Result<(), OfficeError> {
        let mut guard = self.lock();
        let _ = guard.enqueue_relay_checked(item, current_unix_ms())?;
        Ok(())
    }

    fn pending_relays_for(
        &self,
        destination_office: &str,
    ) -> Result<Vec<StoredRelayItem>, OfficeError> {
        Ok(self
            .lock()
            .pending_relays
            .iter()
            .filter(|r| r.destination_office == destination_office)
            .cloned()
            .collect())
    }

    fn ack_relay_item(&self, relay_id: &str) -> Result<bool, OfficeError> {
        let mut guard = self.lock();
        let before = guard.pending_relays.len();
        guard.pending_relays.retain(|r| r.id != relay_id);
        Ok(guard.pending_relays.len() < before)
    }

    fn ack_relay_item_for_destination(
        &self,
        destination_office: &str,
        relay_id: &str,
    ) -> Result<bool, OfficeError> {
        let mut guard = self.lock();
        let before = guard.pending_relays.len();
        guard
            .pending_relays
            .retain(|r| !(r.id == relay_id && r.destination_office == destination_office));
        Ok(guard.pending_relays.len() < before)
    }

    fn ack_message_relay_checked(
        &self,
        destination_office: &str,
        origin_office: &str,
        message_id: &str,
        fingerprint_hex: &str,
    ) -> Result<bool, OfficeError> {
        let mut guard = self.lock();
        guard.ack_message_relay_and_enqueue_receipt_inner(
            destination_office,
            origin_office,
            message_id,
            fingerprint_hex,
            None,
            current_unix_ms(),
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
        let mut candidate = guard.clone();
        let removed = candidate.ack_message_relay_and_enqueue_receipt_inner(
            destination_office,
            origin_office,
            message_id,
            fingerprint_hex,
            receipt_item,
            current_unix_ms(),
        )?;
        if removed {
            *guard = candidate;
        }
        Ok(removed)
    }

    fn purge_relays_for_office(&self, office_id: &str) -> Result<usize, OfficeError> {
        let mut guard = self.lock();
        let before = guard.pending_relays.len();
        guard
            .pending_relays
            .retain(|r| r.destination_office != office_id);
        guard
            .last_published_message_journal_by_dest
            .remove(office_id);
        Ok(before.saturating_sub(guard.pending_relays.len()))
    }

    fn retire_undeliverable_message(
        &self,
        message_id: &str,
        destination_office: &str,
    ) -> Result<bool, OfficeError> {
        let mut guard = self.lock();
        Ok(guard.retire_undeliverable_message_inner(message_id, destination_office))
    }

    fn publish_message_journal_relay_for_dest(
        &self,
        destination_office: &str,
        messages: Vec<Message>,
        relays: Vec<StoredRelayItem>,
        incarnation: &str,
        ordinal: u64,
        now_ms: u64,
    ) -> Result<usize, OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.clone();
        let count = candidate.publish_message_journal_relay_for_dest(
            destination_office,
            messages,
            relays,
            incarnation,
            ordinal,
            now_ms,
        )?;
        *guard = candidate;
        Ok(count)
    }

    fn has_accepted_activity_from(
        &self,
        origin_office: &str,
        source_id: &str,
    ) -> Result<bool, OfficeError> {
        let guard = self.lock();
        let has_msg = guard
            .received_receipts
            .keys()
            .any(|(o, id)| o.as_str() == origin_office && id == source_id);
        if has_msg {
            return Ok(true);
        }
        let has_relay = guard
            .pending_relays
            .iter()
            .any(|r| r.id == source_id && r.kind == "message" && r.origin_office == origin_office);
        Ok(has_relay)
    }

    fn last_published_journal(&self) -> Result<Option<(String, u64)>, OfficeError> {
        Ok(self.lock().last_published_journal.clone())
    }

    fn record_published_journal(&self, incarnation: &str, ordinal: u64) -> Result<(), OfficeError> {
        self.lock().last_published_journal = Some((incarnation.to_owned(), ordinal));
        Ok(())
    }

    fn publish_hq_journal_events(
        &self,
        codec: Codec,
        payloads: Vec<Arc<[u8]>>,
        incarnation: &str,
        ordinal: u64,
    ) -> Result<usize, OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.clone();
        let count = candidate.publish_hq_journal_events(codec, payloads, incarnation, ordinal)?;
        *guard = candidate;
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
        let mut candidate = guard.clone();
        let count = candidate.publish_satellite_work_entries(
            codec,
            record_id,
            payloads,
            incarnation,
            ordinal,
            created_ms,
        )?;
        *guard = candidate;
        Ok(count)
    }

    fn publish_message_journal_relay(
        &self,
        messages: Vec<Message>,
        relays: Vec<StoredRelayItem>,
        incarnation: &str,
        ordinal: u64,
        now_ms: u64,
    ) -> Result<usize, OfficeError> {
        let mut guard = self.lock();
        let mut candidate = guard.clone();
        let count = candidate.publish_message_journal_relay(
            messages,
            relays,
            incarnation,
            ordinal,
            now_ms,
        )?;
        *guard = candidate;
        Ok(count)
    }

    fn record_replicated_op_id(&self, op_id: &str) -> Result<(), OfficeError> {
        self.lock().record_replicated_op(op_id);
        Ok(())
    }

    fn is_replicated_op_id(&self, op_id: &str) -> Result<bool, OfficeError> {
        Ok(self.lock().replicated_op_ids.iter().any(|s| s == op_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_replication_store_passes_eng3_conformance() {
        crate::replication::conformance::run_eng3_replication_suite(|spec| {
            Box::new(MemoryReplicationStore::new(
                spec.org_id,
                spec.local_office,
                spec.hq_office,
                spec.ca,
                spec.epoch,
                spec.incarnation,
                spec.initial_snapshot_required,
            ))
        });
    }
}
