//! Engine-independent durable replication store bridging `org-sync` (`organization`, `work`,
//! `messages`) onto `org-office` (`TASK-52` / `FC-145` / `ADR-0018` `ENG-3`).
//!
//! Zero SQLite types or SQL appear anywhere in this module (`ENG-1`); the SQLite projection
//! lives exclusively in `crate::store::sqlite::replication`; `durable` keeps the durable
//! state the same way for every engine (`TASK-172`).

#[cfg(any(test, feature = "testkit"))]
pub mod conformance;
pub(crate) mod durable;
pub mod memory;
pub(crate) mod state_files;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use org_sync::{
    error::SyncError,
    messages::{Message, MessageStore, Queue, Receipt},
    organization::{OrgCheckpoint, OrgEvent, OrgStore},
    work::{Change as WorkChange, LocalOutbox, Origin},
};
use org_sync_proto::{Digest, Epoch, OfficeId, OrgId, Position, SourceSeq};
use serde::{Deserialize, Serialize};

use crate::error::OfficeError;

/// Serializable durable replication snapshot stored by both memory and SQLite adapters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationStateFile {
    pub version: u32,
    pub org_id: String,
    pub local_office: String,
    pub hq_office: String,
    pub ca_hex: String,
    pub epoch: u64,
    pub seq: u64,
    pub digest_hex: String,
    pub highest_epoch: u64,
    pub restore_pending: bool,
    pub pending_restore_base_hex: Option<String>,
    pub protocol_level: u32,
    pub initial_snapshot_required: bool,
    pub events: Vec<StoredOrgEvent>,
    pub work_incarnation: String,
    pub work_last_seq: u64,
    pub work_acked_seq: u64,
    pub work_pending: Vec<StoredWorkChange>,
    pub remote_work_high_water: BTreeMap<String, u64>,
    pub message_clock_high_ms: u64,
    pub message_pending: Vec<StoredMessage>,
    pub message_outgoing_receipts: Vec<StoredReceipt>,
    pub message_received_receipts: Vec<StoredReceipt>,
    pub message_expired: Vec<StoredMessage>,
    #[serde(default)]
    pub pending_relays: Vec<StoredRelayItem>,
    /// Last custody-confirmed HQ journal `(incarnation, ordinal)` published into `OrgEvent`s
    /// so a crash before advancing `org-sync-hq` never republishes the same entry (`TASK-52` P2-9).
    #[serde(default)]
    pub last_published_journal: Option<(String, u64)>,
    /// Last satellite journal `(incarnation, ordinal)` atomically published into the durable
    /// `org-sync` work outbox (`TASK-52` #6).
    #[serde(default)]
    pub last_published_work_journal: Option<(String, u64)>,
    /// Last native messaging journal `(incarnation, ordinal)` atomically published into the
    /// durable message/relay outbox (`TASK-52` #10).
    #[serde(default)]
    pub last_published_message_journal: Option<(String, u64)>,
    /// Per-destination native messaging journal `(incarnation, ordinal)` watermarks so one
    /// backpressured destination never stalls healthy destinations (`TASK-52` P8-#7).
    #[serde(default)]
    pub last_published_message_journal_by_dest: BTreeMap<String, (String, u64)>,
    /// Bounded ring of `org-store` operation IDs produced by inbound replication applies so a
    /// satellite's local work outbox consumer never re-exports replicated HQ entries (`TASK-52` H-6).
    #[serde(default)]
    pub replicated_op_ids: VecDeque<String>,
}

/// Durable HQ cross-office relay entry persisted inside `state.v1.json` so HQ restarts never
/// lose queued satellite-to-satellite messages or wakes (`TASK-52`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRelayItem {
    pub id: String,
    #[serde(default)]
    pub origin_office: String,
    pub destination_office: String,
    #[serde(default)]
    pub fingerprint_hex: String,
    pub kind: String,
    pub payload_json: String,
    #[serde(default)]
    pub expires_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOrgEvent {
    pub epoch: u64,
    pub seq: u64,
    pub previous_hex: String,
    pub digest_hex: String,
    pub codec_name: String,
    pub codec_version: u32,
    pub payload_b64: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredWorkChange {
    pub origin_office: String,
    pub incarnation: String,
    pub seq: u64,
    pub record_id: String,
    pub generation: u64,
    pub version: u64,
    pub digest_hex: String,
    pub record_digest_hex: String,
    pub codec_name: String,
    pub codec_version: u32,
    pub payload_b64: String,
    pub required_org_epoch: u64,
    pub required_org_seq: u64,
    pub created_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMessage {
    pub org_id: String,
    pub origin: String,
    pub destination: String,
    pub id: String,
    pub fingerprint_hex: String,
    pub payload_b64: String,
    pub deadline_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredReceipt {
    pub message: StoredMessage,
    pub delivered: bool,
}

/// Live summary of an office's replication state for status/diagnostics reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncStatusSnapshot {
    pub org_id: String,
    pub local_office: String,
    pub hq_office: String,
    pub epoch: u64,
    pub seq: u64,
    pub highest_epoch: u64,
    pub protocol_level: u32,
    pub restore_pending: bool,
    pub initial_snapshot_required: bool,
    pub pending_work_count: usize,
    pub pending_message_count: usize,
}

/// Consumer-owned engine-independent replication store port (`ADR-0018` `ENG-3`).
pub trait ReplicationStore: OrgStore + MessageStore + Send + Sync {
    /// Returns the current organization replication checkpoint.
    fn org_checkpoint(&self) -> Result<OrgCheckpoint, OfficeError>;

    /// Returns stored organization events strictly after `(epoch, seq)` in order.
    fn org_events_after(&self, epoch: u64, seq: u64) -> Result<Vec<OrgEvent>, OfficeError>;

    /// Stages a validated snapshot payload set for `org_sync::organization::install_snapshot`.
    fn stage_snapshot(&self, staging_id: &str, payloads: Vec<Arc<[u8]>>)
        -> Result<(), OfficeError>;

    /// Returns all organization payloads applied (both via events and installed snapshots).
    fn applied_org_payloads(&self) -> Result<Vec<Arc<[u8]>>, OfficeError>;

    /// Returns the local durable work outbox (`org-sync::work::LocalOutbox`).
    fn load_work_outbox(&self) -> Result<LocalOutbox, OfficeError>;

    /// Atomically appends a local work change to the durable work outbox.
    fn enqueue_work_change(&self, change: WorkChange) -> Result<LocalOutbox, SyncError>;

    /// Advances the local work outbox acknowledged sequence and prunes acknowledged entries.
    fn ack_work_through(&self, acked_seq: u64) -> Result<LocalOutbox, SyncError>;

    /// Records an applied remote work change high-water `(origin_office, incarnation) -> seq`.
    fn record_remote_work_seq(&self, origin: &Origin, seq: SourceSeq) -> Result<bool, OfficeError>;

    /// Returns the highest applied sequence for `origin`, or 0.
    fn remote_work_high_water(&self, origin: &Origin) -> Result<u64, OfficeError>;

    /// Returns all delivered cross-office message payloads committed locally.
    fn delivered_message_payloads(&self) -> Result<Vec<Arc<[u8]>>, OfficeError>;

    /// Returns a copy of the pending outgoing message queue.
    fn message_queue_snapshot(&self) -> Result<Queue, OfficeError>;

    /// Returns a durable outgoing delivery receipt for `message_id`, if already recorded
    /// (`TASK-52` P3-#12).
    fn outgoing_receipt_snapshot(&self, message_id: &str) -> Result<Option<Receipt>, OfficeError>;

    /// Returns whether `(origin_office, message_id)` with `fingerprint_hex` already has a durable
    /// `delivered: true` incoming receipt so replays never re-trigger wakes (`TASK-52` P6-#11).
    fn has_received_message_receipt(
        &self,
        origin_office: &str,
        message_id: &str,
        fingerprint_hex: &str,
    ) -> Result<bool, OfficeError>;

    /// Re-anchors a satellite's replication state to a replacement HQ after R1 import (D25),
    /// setting `initial_snapshot_required = true` and `restore_pending = false` at `(epoch, 0)`
    /// while preserving local office identity.
    fn reset_for_re_enrollment(
        &self,
        new_hq_office: OfficeId,
        new_ca: Digest,
        new_epoch: Epoch,
    ) -> Result<(), OfficeError>;

    /// Returns a compact status snapshot.
    fn sync_status(&self) -> Result<SyncStatusSnapshot, OfficeError>;

    /// Durably enqueues a cross-office relay item (message or wake) destined for another office.
    fn enqueue_relay_item(&self, item: StoredRelayItem) -> Result<(), OfficeError>;

    /// Lists all durably queued relay items destined for `destination_office`.
    fn pending_relays_for(
        &self,
        destination_office: &str,
    ) -> Result<Vec<StoredRelayItem>, OfficeError>;

    /// Durably removes an acknowledged relay item by `relay_id`.
    fn ack_relay_item(&self, relay_id: &str) -> Result<bool, OfficeError>;

    /// Durably removes an acknowledged relay item by `relay_id` only if destined for `destination_office`.
    fn ack_relay_item_for_destination(
        &self,
        destination_office: &str,
        relay_id: &str,
    ) -> Result<bool, OfficeError>;

    /// Durably removes an acknowledged message relay only when `destination_office`,
    /// `origin_office`, `message_id`, and `fingerprint_hex` all match (`TASK-52` P2-11).
    fn ack_message_relay_checked(
        &self,
        destination_office: &str,
        origin_office: &str,
        message_id: &str,
        fingerprint_hex: &str,
    ) -> Result<bool, OfficeError>;

    /// Atomically removes a matched message relay and enqueues its forwarded delivery receipt
    /// in a single durable transaction so HQ can never lose receipts across a crash (`TASK-52` P7-#11).
    fn ack_message_relay_and_enqueue_receipt(
        &self,
        destination_office: &str,
        origin_office: &str,
        message_id: &str,
        fingerprint_hex: &str,
        receipt_item: Option<StoredRelayItem>,
    ) -> Result<bool, OfficeError>;

    /// Removes all queued relay items (messages, wakes, receipts) targeting a permanently removed
    /// satellite office so revoked offices never pin HQ relay capacity (`TASK-52` P8-#8).
    fn purge_relays_for_office(&self, office_id: &str) -> Result<usize, OfficeError>;

    /// Durably retires an outgoing message addressed to a permanently removed destination office,
    /// recording a terminal `delivered: false` receipt and removing it from `pending` (`TASK-52` P15-#4).
    fn retire_undeliverable_message(
        &self,
        message_id: &str,
        destination_office: &str,
    ) -> Result<bool, OfficeError>;

    /// Atomically publishes outbound messages/relays for one destination office and advances only
    /// that destination's `(incarnation, ordinal)` watermark (`TASK-52` P8-#7).
    fn publish_message_journal_relay_for_dest(
        &self,
        destination_office: &str,
        messages: Vec<org_sync::messages::Message>,
        relays: Vec<StoredRelayItem>,
        incarnation: &str,
        ordinal: u64,
        now_ms: u64,
    ) -> Result<usize, OfficeError>;

    /// Returns true if a message or work item with `source_id` has been durably accepted from
    /// `origin_office` (used to bind wake notifications to accepted activity).
    fn has_accepted_activity_from(
        &self,
        origin_office: &str,
        source_id: &str,
    ) -> Result<bool, OfficeError>;

    /// Returns the last published `(journal_incarnation, ordinal)` recorded by HQ's journal publisher.
    fn last_published_journal(&self) -> Result<Option<(String, u64)>, OfficeError>;

    /// Durably records `(journal_incarnation, ordinal)` after publishing into `OrgEvent`s.
    fn record_published_journal(&self, incarnation: &str, ordinal: u64) -> Result<(), OfficeError>;

    /// Atomically appends one or more HQ `OrgEvent` payloads and records `(incarnation, ordinal)`
    /// in a single durable transaction (`TASK-52` M-16). Returns the number of events appended
    /// (`0` if `(incarnation, ordinal)` was already published).
    fn publish_hq_journal_events(
        &self,
        codec: org_sync_proto::Codec,
        payloads: Vec<Arc<[u8]>>,
        incarnation: &str,
        ordinal: u64,
    ) -> Result<usize, OfficeError>;

    /// Atomically appends one or more satellite work outbox entries for `(incarnation, ordinal)`
    /// in a single durable transaction (`TASK-52` #6), skipping already-published ordinals so a
    /// crash before advancing the journal consumer can never create duplicate outbox entries.
    fn publish_satellite_work_entries(
        &self,
        codec: org_sync_proto::Codec,
        record_id: &str,
        payloads: Vec<Arc<[u8]>>,
        incarnation: &str,
        ordinal: u64,
        created_ms: u64,
    ) -> Result<usize, OfficeError>;

    /// Atomically publishes outbound cross-office messages and/or HQ relay items for a committed
    /// native messaging journal `(incarnation, ordinal)` (`TASK-52` #10).
    fn publish_message_journal_relay(
        &self,
        messages: Vec<org_sync::messages::Message>,
        relays: Vec<StoredRelayItem>,
        incarnation: &str,
        ordinal: u64,
        now_ms: u64,
    ) -> Result<usize, OfficeError>;

    /// Records an `org-store` operation ID created by inbound replication apply (`TASK-52` H-6).
    fn record_replicated_op_id(&self, op_id: &str) -> Result<(), OfficeError>;

    /// Returns whether `op_id` was created by inbound replication apply (`TASK-52` H-6).
    fn is_replicated_op_id(&self, op_id: &str) -> Result<bool, OfficeError>;
}

/// Shared initial state constructor for both memory and SQLite adapters.
pub fn initial_checkpoint(
    org_id: OrgId,
    local_office: OfficeId,
    hq_office: OfficeId,
    ca: Digest,
    epoch: Epoch,
) -> OrgCheckpoint {
    OrgCheckpoint {
        org_id,
        local_office,
        hq_office,
        ca,
        position: Position { epoch, seq: 0 },
        digest: Digest::ZERO,
        highest_epoch: epoch,
        restore_pending: false,
        pending_restore_base: None,
        protocol_level: 1,
    }
}

pub(crate) fn decode_hex_digest(hex: &str) -> Result<Digest, OfficeError> {
    Digest::parse_hex(hex).map_err(|_| OfficeError::Invalid("invalid hex digest"))
}

pub(crate) fn encode_b64(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD.encode(bytes)
}

pub(crate) fn decode_b64(text: &str) -> Result<Vec<u8>, OfficeError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD
        .decode(text)
        .map_err(|_| OfficeError::Invalid("invalid base64 payload"))
}

pub(crate) fn message_to_stored(m: &Message) -> StoredMessage {
    StoredMessage {
        org_id: m.org_id.as_str().to_owned(),
        origin: m.origin.as_str().to_owned(),
        destination: m.destination.as_str().to_owned(),
        id: m.id.clone(),
        fingerprint_hex: m.fingerprint.to_hex(),
        payload_b64: encode_b64(&m.payload),
        deadline_ms: m.deadline_ms,
    }
}

pub(crate) fn message_from_stored(s: &StoredMessage) -> Result<Message, OfficeError> {
    Ok(Message {
        org_id: OrgId::new(&s.org_id).map_err(|_| OfficeError::Invalid("stored org_id"))?,
        origin: OfficeId::new(&s.origin).map_err(|_| OfficeError::Invalid("stored origin"))?,
        destination: OfficeId::new(&s.destination)
            .map_err(|_| OfficeError::Invalid("stored destination"))?,
        id: s.id.clone(),
        fingerprint: decode_hex_digest(&s.fingerprint_hex)?,
        payload: Arc::from(decode_b64(&s.payload_b64)?),
        deadline_ms: s.deadline_ms,
    })
}

pub(crate) fn receipt_to_stored(r: &Receipt) -> StoredReceipt {
    StoredReceipt {
        message: message_to_stored(&r.message),
        delivered: r.delivered,
    }
}

pub(crate) fn receipt_from_stored(s: &StoredReceipt) -> Result<Receipt, OfficeError> {
    Ok(Receipt {
        message: message_from_stored(&s.message)?,
        delivered: s.delivered,
    })
}

pub(crate) fn outbox_from_stored(
    local_office: &OfficeId,
    incarnation: &str,
    last_seq: u64,
    acked_seq: u64,
    pending: &[StoredWorkChange],
    org_id: &OrgId,
) -> Result<LocalOutbox, OfficeError> {
    let origin = Origin {
        office: local_office.clone(),
        incarnation: org_sync_proto::OriginIncarnation::new(incarnation)
            .map_err(|_| OfficeError::Invalid("stored work incarnation"))?,
    };
    let mut queue = VecDeque::with_capacity(pending.len());
    for item in pending {
        let change = WorkChange {
            org_id: org_id.clone(),
            key: org_sync::work::SourceKey {
                origin: Origin {
                    office: OfficeId::new(&item.origin_office)
                        .map_err(|_| OfficeError::Invalid("stored work origin office"))?,
                    incarnation: org_sync_proto::OriginIncarnation::new(&item.incarnation)
                        .map_err(|_| OfficeError::Invalid("stored work origin incarnation"))?,
                },
                seq: SourceSeq::new(item.seq)
                    .map_err(|_| OfficeError::Invalid("stored work seq"))?,
            },
            record_id: item.record_id.clone(),
            version: org_sync::work::RecordVersion {
                generation: item.generation,
                version: item.version,
            },
            digest: decode_hex_digest(&item.digest_hex)?,
            record_digest: decode_hex_digest(&item.record_digest_hex)?,
            codec: org_sync_proto::Codec::new(&item.codec_name, item.codec_version)
                .map_err(|_| OfficeError::Invalid("stored work codec"))?,
            payload: Arc::from(decode_b64(&item.payload_b64)?),
            required_org: Position {
                epoch: Epoch::new(item.required_org_epoch)
                    .map_err(|_| OfficeError::Invalid("stored work required_org_epoch"))?,
                seq: item.required_org_seq,
            },
            created_ms: item.created_ms,
        };
        queue.push_back(change);
    }
    LocalOutbox::restore(origin, last_seq, acked_seq, queue)
        .map_err(|_| OfficeError::Invalid("stored work outbox invariant violated"))
}
