//! In-memory engine-independent [`ReplicationStore`] adapter (`ADR-0018` `ENG-2`/`ENG-3`).
use std::sync::{Arc, Mutex};

use org_sync::{
    error::SyncError,
    messages::{Message, MessageAttempt, MessageOutput, MessageStore, Queue, Receipt},
    organization::{OrgAttempt, OrgCheckpoint, OrgEvent, OrgOutput, OrgStore},
    work::{Change as WorkChange, LocalOutbox, Origin},
};
use org_sync_proto::{Codec, Digest, Epoch, OfficeId, OrgId, OriginIncarnation, SourceSeq};

use super::{ReplicationStore, StoredRelayItem, SyncStatusSnapshot};
use crate::error::OfficeError;

mod core;
mod relay;
mod txn;

use core::current_unix_ms;
pub(crate) use core::ReplicationCoreData;
pub(crate) use txn::{CoreMessageTxn, CoreOrgTxn};

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
