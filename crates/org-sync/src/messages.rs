//! Reliable message delivery progresses independently of organization command holds.
//! Native message IDs, payload validation, read state and recipient authorization belong
//! to the mailbox adapter. A transport receipt never acknowledges an uncommitted delivery.
use crate::{
    error::{Refusal, StoreError, SyncError},
    organization::{PeerIdentity, VerifiedPeer},
};
use org_sync_proto::{Digest, Epoch, OfficeId, OrgId};
use std::{collections::VecDeque, sync::Arc};

pub const MAX_PENDING: usize = 1024;
pub const MAX_PENDING_BYTES: usize = 16 * 1024 * 1024;
/// Reserve encoded-frame headroom for base64 and its bounded metadata envelope.
pub const MAX_PAYLOAD_BYTES: usize = org_sync_proto::MAX_ENCODED_FRAME_BYTES / 2;
const _: () = assert!(
    MAX_PAYLOAD_BYTES.div_ceil(3) * 4 + org_sync_proto::MAX_ENCODED_FRAME_BYTES / 4
        <= org_sync_proto::MAX_ENCODED_FRAME_BYTES
);

#[derive(Clone, PartialEq, Eq)]
pub struct Message {
    pub org_id: OrgId,
    pub origin: OfficeId,
    pub destination: OfficeId,
    /// Native message identity, never a connection-local frame ID.
    pub id: String,
    pub fingerprint: Digest,
    pub payload: Arc<[u8]>,
    pub deadline_ms: u64,
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Message")
            .field("org_id", &self.org_id)
            .field("origin", &self.origin)
            .field("destination", &self.destination)
            .field("id", &self.id)
            .field("payload_bytes", &self.payload.len())
            .field("deadline_ms", &self.deadline_ms)
            .finish()
    }
}
impl Message {
    fn validate(&self) -> Result<(), Refusal> {
        if self.id.is_empty()
            || self.id.len() > 128
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || self.payload.is_empty()
            || self.payload.len() > MAX_PAYLOAD_BYTES
            || self.deadline_ms == 0
        {
            return Err(Refusal::InvalidInput);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub message: Message,
    /// Retained at least through the native mailbox's dedup deadline. The adapter must
    /// preserve this identity until that deadline, including on snapshot installation.
    pub delivered: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Queue {
    pub org_id: OrgId,
    pub office_id: OfficeId,
    pub pending: VecDeque<Message>,
    pub clock_high_ms: u64,
}
impl Queue {
    pub fn new(org_id: OrgId, office_id: OfficeId) -> Self {
        Self {
            org_id,
            office_id,
            pending: VecDeque::new(),
            clock_high_ms: 0,
        }
    }
    fn validate(&self) -> Result<(), Refusal> {
        let mut ids = std::collections::BTreeSet::new();
        if self.pending.len() > MAX_PENDING || self.bytes() > MAX_PENDING_BYTES {
            return Err(Refusal::InvariantFailure);
        }
        for m in &self.pending {
            m.validate().map_err(|_| Refusal::InvariantFailure)?;
            if m.org_id != self.org_id || m.origin != self.office_id || !ids.insert(&m.id) {
                return Err(Refusal::InvariantFailure);
            }
        }
        Ok(())
    }
    fn bytes(&self) -> usize {
        self.pending
            .iter()
            .fold(0usize, |n, m| n.saturating_add(m.payload.len()))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageOutput {
    Queued { id: String },
    Deliver(Message),
    Receipt(Receipt),
    Idle,
    Refused(Refusal),
}
/// Same replayable immutable-facts/no-I/O/bounded-retry and durable-output contract as
/// OrgStore. All queue/receipt/native delivery writes share one atomic writer attempt.
pub type MessageAttempt =
    dyn Fn(&mut dyn MessageTxn) -> Result<MessageOutput, SyncError> + Send + Sync;
pub trait MessageStore {
    fn transact(&self, attempt: Arc<MessageAttempt>) -> Result<MessageOutput, SyncError>;
}
/// Pinned local endpoint and highest authenticated authority in the writer snapshot.
#[derive(Clone, Debug)]
pub struct MessageAuthority {
    pub org_id: OrgId,
    pub office_id: OfficeId,
    pub epoch: Epoch,
}

pub trait MessageTxn {
    /// Durable organization restore fence from the same writer snapshot as queue changes.
    fn restore_pending(&mut self) -> Result<bool, StoreError>;
    fn authority(&mut self) -> Result<MessageAuthority, StoreError>;
    fn queue(&mut self) -> Result<Queue, StoreError>;
    /// Check the replicated organization-wide protocol level against this binary. Pairwise
    /// transport capabilities never bypass it. This read has no I/O or clock effects.
    fn supports_current_protocol(&mut self) -> Result<bool, StoreError>;
    fn outgoing_receipt(&mut self, id: &str) -> Result<Option<Receipt>, StoreError>;
    /// Retained native delivery proof. An undelivered row is invalid here; false receipts
    /// are local outgoing expiry notices, never destination ACKs.
    fn received(&mut self, origin: &OfficeId, id: &str) -> Result<Option<Receipt>, StoreError>;
    /// Validate native fingerprint and authorize the local principal's immutable intent.
    fn authorize_send(&mut self, message: &Message) -> Result<(), SyncError>;
    /// Read-only check of pinned peer role, route and native recipient/codec rules,
    /// before exposing restore state or reading receipts. A verified HQ may forward
    /// an authenticated origin.
    fn authorize_receive(
        &mut self,
        peer: &PeerIdentity,
        message: &Message,
    ) -> Result<(), SyncError>;
    /// Read-only check of pinned peer role and authenticated ACK route before restore state.
    fn authorize_receipt(
        &mut self,
        peer: &PeerIdentity,
        receipt: &Receipt,
    ) -> Result<(), SyncError>;
    fn apply_message(&mut self, message: &Message) -> Result<(), SyncError>;
    /// Rejected identity shares the origin/ID uniqueness and native retention horizon
    /// with received proofs, but never establishes delivery.
    fn expired_identity(
        &mut self,
        origin: &OfficeId,
        id: &str,
    ) -> Result<Option<Message>, StoreError>;
    fn record_expired(&mut self, message: &Message) -> Result<(), StoreError>;
    fn record_received(&mut self, receipt: &Receipt) -> Result<(), StoreError>;
    fn save_queue(&mut self, queue: &Queue, receipt: Option<&Receipt>) -> Result<(), StoreError>;
}

fn validate_queue(tx: &mut dyn MessageTxn, queue: &Queue) -> Result<(), SyncError> {
    queue.validate()?;
    let pinned = tx.authority()?;
    if queue.org_id != pinned.org_id || queue.office_id != pinned.office_id {
        return Err(Refusal::InvariantFailure.into());
    }
    Ok(())
}
fn load_queue(tx: &mut dyn MessageTxn) -> Result<Queue, SyncError> {
    let queue = tx.queue()?;
    validate_queue(tx, &queue)?;
    Ok(queue)
}

/// Internal writer boundary, also exercised by the transactional adapter conformance.
pub(crate) fn persist(
    tx: &mut dyn MessageTxn,
    queue: &Queue,
    receipt: Option<&Receipt>,
) -> Result<(), SyncError> {
    validate_queue(tx, queue)?;
    tx.save_queue(queue, receipt)?;
    Ok(())
}

fn ready(tx: &mut dyn MessageTxn) -> Result<(), SyncError> {
    if tx.restore_pending()? {
        return Err(Refusal::RestorePending.into());
    }
    if !tx.supports_current_protocol()? {
        return Err(Refusal::UnsupportedVersion.into());
    }
    Ok(())
}

pub fn enqueue(
    store: &dyn MessageStore,
    message: Message,
    now_ms: u64,
) -> Result<MessageOutput, SyncError> {
    store.transact(Arc::new(move |tx| {
        let mut queue = load_queue(tx)?;
        ready(tx)?;
        message.validate()?;
        if queue.org_id != message.org_id || queue.office_id != message.origin {
            return Err(Refusal::NotAuthorized.into());
        }
        tx.authorize_send(&message)?;
        if let Some(receipt) = tx.outgoing_receipt(&message.id)? {
            if receipt.message != message {
                return Err(Refusal::OperationReused.into());
            }
            return Ok(MessageOutput::Receipt(receipt));
        }
        if let Some(old) = queue.pending.iter().find(|m| m.id == message.id) {
            if old != &message {
                return Err(Refusal::OperationReused.into());
            }
            return Ok(MessageOutput::Queued {
                id: message.id.clone(),
            });
        }
        queue.clock_high_ms = queue.clock_high_ms.max(now_ms);
        if message.deadline_ms <= queue.clock_high_ms {
            let receipt = Receipt {
                message: message.clone(),
                delivered: false,
            };
            persist(tx, &queue, Some(&receipt))?;
            return Ok(MessageOutput::Refused(Refusal::Expired));
        }
        if queue.pending.len() >= MAX_PENDING
            || queue.bytes().saturating_add(message.payload.len()) > MAX_PENDING_BYTES
        {
            return Err(Refusal::QuotaExceeded.into());
        }
        queue.pending.push_back(message.clone());
        persist(tx, &queue, None)?;
        Ok(MessageOutput::Queued {
            id: message.id.clone(),
        })
    }))
}

/// Repeated delivery is safe only because the destination commits payload + identity before
/// acknowledging. No command queue state is read here.
pub fn next(store: &dyn MessageStore, now_ms: u64) -> Result<MessageOutput, SyncError> {
    store.transact(Arc::new(move |tx| {
        let mut queue = load_queue(tx)?;
        ready(tx)?;
        queue.clock_high_ms = queue.clock_high_ms.max(now_ms);
        let Some(message) = queue.pending.front().cloned() else {
            persist(tx, &queue, None)?;
            return Ok(MessageOutput::Idle);
        };
        if message.deadline_ms <= queue.clock_high_ms {
            queue.pending.pop_front();
            let receipt = Receipt {
                message,
                delivered: false,
            };
            persist(tx, &queue, Some(&receipt))?;
            return Ok(MessageOutput::Receipt(receipt));
        }
        persist(tx, &queue, None)?;
        Ok(MessageOutput::Deliver(message))
    }))
}

fn check_remote(
    tx: &mut dyn MessageTxn,
    peer: &PeerIdentity,
    org_id: &OrgId,
    local_endpoint: &OfficeId,
) -> Result<(), SyncError> {
    let pinned = tx.authority()?;
    if &pinned.org_id != org_id
        || peer.org_id != pinned.org_id
        || peer.authority != pinned.epoch
        || &pinned.office_id != local_endpoint
    {
        return Err(Refusal::NotAuthorized.into());
    }
    Ok(())
}

pub fn receive(
    store: &dyn MessageStore,
    peer: &dyn VerifiedPeer,
    message: Message,
    now_ms: u64,
) -> Result<MessageOutput, SyncError> {
    let peer = peer.identity();
    store.transact(Arc::new(move |tx| {
        message.validate()?;
        check_remote(tx, &peer, &message.org_id, &message.destination)?;
        tx.authorize_receive(&peer, &message)?;
        ready(tx)?;
        let mut queue = load_queue(tx)?;
        if let Some(old) = tx.received(&message.origin, &message.id)? {
            if !old.delivered {
                return Err(Refusal::InvariantFailure.into());
            }
            if old.message != message {
                return Err(Refusal::Divergence.into());
            }
            return Ok(MessageOutput::Receipt(old));
        }
        if let Some(old) = tx.expired_identity(&message.origin, &message.id)? {
            if old != message {
                return Err(Refusal::Divergence.into());
            }
            return Ok(MessageOutput::Refused(Refusal::Expired));
        }
        queue.clock_high_ms = queue.clock_high_ms.max(now_ms);
        if message.deadline_ms <= queue.clock_high_ms {
            tx.record_expired(&message)?;
            persist(tx, &queue, None)?;
            return Ok(MessageOutput::Refused(Refusal::Expired));
        }
        tx.apply_message(&message)?;
        let receipt = Receipt {
            message: message.clone(),
            delivered: true,
        };
        tx.record_received(&receipt)?;
        persist(tx, &queue, None)?;
        Ok(MessageOutput::Receipt(receipt))
    }))
}

pub fn acknowledge(
    store: &dyn MessageStore,
    peer: &dyn VerifiedPeer,
    receipt: Receipt,
) -> Result<MessageOutput, SyncError> {
    let peer = peer.identity();
    store.transact(Arc::new(move |tx| {
        receipt.message.validate()?;
        if !receipt.delivered {
            return Err(Refusal::NotAuthorized.into());
        }
        check_remote(tx, &peer, &receipt.message.org_id, &receipt.message.origin)?;
        tx.authorize_receipt(&peer, &receipt)?;
        ready(tx)?;
        let mut queue = load_queue(tx)?;
        if let Some(old) = tx.outgoing_receipt(&receipt.message.id)? {
            if old.message != receipt.message {
                return Err(Refusal::Divergence.into());
            }
            if !old.delivered {
                persist(tx, &queue, Some(&receipt))?;
                return Ok(MessageOutput::Receipt(receipt.clone()));
            }
            return Ok(MessageOutput::Receipt(old));
        }
        let old = queue
            .pending
            .iter()
            .find(|m| m.id == receipt.message.id)
            .ok_or(Refusal::InvalidInput)?;
        if old != &receipt.message {
            return Err(Refusal::Divergence.into());
        }
        queue.pending.retain(|m| m.id != receipt.message.id);
        persist(tx, &queue, Some(&receipt))?;
        Ok(MessageOutput::Receipt(receipt.clone()))
    }))
}
