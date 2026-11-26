//! The org and message transactions over the in-memory replication state.

use std::sync::Arc;

use org_sync::{
    error::{Refusal, StoreError, SyncError},
    messages::{Message, MessageAuthority, MessageTxn, Queue, Receipt},
    organization::{OrgCheckpoint, OrgEvent, OrgTxn, PeerIdentity, ValidatedSnapshot},
};
use org_sync_proto::{Codec, Digest, OfficeId, Position};

use super::core::ReplicationCoreData;
use crate::error::OfficeError;

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

impl ReplicationCoreData {
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
}
