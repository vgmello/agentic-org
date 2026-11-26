//! [`OrgStore`], [`MessageStore`] and [`ReplicationStore`] for [`SqliteReplicationStore`].

use std::sync::Arc;

use org_sync::{
    error::{StoreError, SyncError},
    messages::{MessageAttempt, MessageOutput, MessageStore, Queue},
    organization::{OrgAttempt, OrgCheckpoint, OrgEvent, OrgOutput, OrgStore},
    work::{Change as WorkChange, LocalOutbox, Origin},
};
use org_sync_proto::{Codec, Digest, Epoch, OfficeId, SourceSeq};

use super::SqliteReplicationStore;
use crate::error::OfficeError;
use crate::replication::{
    memory::{CoreMessageTxn, CoreOrgTxn},
    ReplicationStore, StoredRelayItem, SyncStatusSnapshot,
};

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
