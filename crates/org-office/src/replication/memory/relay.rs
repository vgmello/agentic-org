//! Message relay on the in-memory replication state: pending messages, relay queues, receipts
//! and journal relay publication.

use std::collections::VecDeque;

use org_sync::messages::{Message, Receipt};
use org_sync_proto::OfficeId;

use crate::error::OfficeError;
use crate::replication::StoredRelayItem;

use super::core::{relay_item_expired, ReplicationCoreData};

impl ReplicationCoreData {
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
}
