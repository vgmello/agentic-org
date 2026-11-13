//! Append immutable per-version Work to the local outbox inside the native writer.
//! Source allocation, event encoding, clocks and transaction commit remain native-owned.
use crate::{
    error::{Refusal, StoreError, SyncError},
    versions,
};
use org_sync_proto::{
    work_record_digest_v1, work_source_digest_v1, Codec, Digest, OfficeId, OrgId,
    OriginIncarnation, Position, SourceSeq, MAX_ENCODED_FRAME_BYTES,
};
use std::{collections::VecDeque, fmt, sync::Arc};

pub const MAX_PENDING: usize = 1024;
pub const MAX_PENDING_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PAYLOAD_BYTES: usize = MAX_ENCODED_FRAME_BYTES / 2;
// Reserve a quarter frame for bounded identifiers, metadata and the transport envelope.
const _: () = assert!(
    MAX_PAYLOAD_BYTES.div_ceil(3) * 4 + MAX_ENCODED_FRAME_BYTES / 4 <= MAX_ENCODED_FRAME_BYTES
);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Origin {
    pub office: OfficeId,
    pub incarnation: OriginIncarnation,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceKey {
    pub origin: Origin,
    pub seq: SourceSeq,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecordVersion {
    pub generation: u64,
    pub version: u64,
}
/// Native codec output. The writer verifies both digests before persistence; wire input
/// must never bypass queue_local. Payload is a frozen per-version event, not a live snapshot.
#[derive(Clone, PartialEq, Eq)]
pub struct Change {
    pub org_id: OrgId,
    pub key: SourceKey,
    pub record_id: String,
    pub version: RecordVersion,
    pub digest: Digest,
    /// Canonical content identity independent of source office/incarnation/sequence.
    pub record_digest: Digest,
    pub codec: Codec,
    pub payload: Arc<[u8]>,
    pub required_org: Position,
    pub created_ms: u64,
}
impl fmt::Debug for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Change")
            .field("key", &self.key)
            .field("version", &self.version)
            .field("codec", &self.codec)
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}
impl Change {
    fn validate(&self) -> Result<(), Refusal> {
        if self.record_id.is_empty()
            || self.record_id.len() > 128
            || !self
                .record_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || self.version.generation == 0
            || self.version.version == 0
            || self.payload.is_empty()
            || self.payload.len() > MAX_PAYLOAD_BYTES
        {
            return Err(Refusal::InvalidInput);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalContext {
    pub org_id: OrgId,
    pub origin: Origin,
    pub applied_org: Position,
    pub protocol_level: u32,
    pub restore_pending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalOutbox {
    origin: Origin,
    last_seq: u64,
    acknowledged_seq: u64,
    pending: VecDeque<Change>,
}
impl LocalOutbox {
    pub fn new(origin: Origin) -> Self {
        Self {
            origin,
            last_seq: 0,
            acknowledged_seq: 0,
            pending: VecDeque::new(),
        }
    }
    /// Reconstitute persisted state. Context and integrity are rechecked inside the writer.
    pub fn restore(
        origin: Origin,
        last_seq: u64,
        acknowledged_seq: u64,
        pending: VecDeque<Change>,
    ) -> Result<Self, Refusal> {
        let queue = Self {
            origin,
            last_seq,
            acknowledged_seq,
            pending,
        };
        queue.validate_shape()?;
        Ok(queue)
    }
    pub fn origin(&self) -> &Origin {
        &self.origin
    }
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }
    pub fn acknowledged_seq(&self) -> u64 {
        self.acknowledged_seq
    }
    pub fn pending(&self) -> &VecDeque<Change> {
        &self.pending
    }
    fn bytes(&self) -> usize {
        self.pending
            .iter()
            .fold(0usize, |n, c| n.saturating_add(c.payload.len()))
    }
    fn validate_shape(&self) -> Result<(), Refusal> {
        if self.pending.len() > MAX_PENDING || self.bytes() > MAX_PENDING_BYTES {
            return Err(Refusal::InvariantFailure);
        }
        let mut after = self.acknowledged_seq;
        for change in &self.pending {
            change.validate().map_err(|_| Refusal::InvariantFailure)?;
            if change.key.origin != self.origin
                || after.checked_add(1) != Some(change.key.seq.get())
            {
                return Err(Refusal::InvariantFailure);
            }
            after = change.key.seq.get();
        }
        if after != self.last_seq {
            return Err(Refusal::InvariantFailure);
        }
        Ok(())
    }
}

/// Implement on the SAME native savepoint that mutates Work and allocates its source key.
/// Abort that entire savepoint on ANY Err, including Refused. Never expose the returned key
/// before durable COMMIT. A retry repeats the whole native operation, not just queue_local.
pub trait LocalWorkTxn {
    fn context(&mut self) -> Result<LocalContext, StoreError>;
    fn local_outbox(&mut self) -> Result<LocalOutbox, StoreError>;
    fn supports_codec(&mut self, codec: &Codec) -> Result<bool, StoreError>;
    fn save_local(&mut self, outbox: &LocalOutbox) -> Result<(), StoreError>;
}

fn verify_change_integrity(change: &Change) -> Result<(), Refusal> {
    let record = work_record_digest_v1(
        &change.record_id,
        change.version.generation,
        change.version.version,
        &change.codec,
        &change.payload,
    )
    .map_err(|_| Refusal::IntegrityFailure)?;
    let source = work_source_digest_v1(
        &change.key.origin.office,
        &change.key.origin.incarnation,
        change.key.seq,
        record,
        change.required_org,
        change.created_ms,
    );
    if record != change.record_digest || source != change.digest {
        return Err(Refusal::IntegrityFailure);
    }
    Ok(())
}

fn validate_queue(c: &LocalContext, queue: &LocalOutbox) -> Result<(), SyncError> {
    queue.validate_shape()?;
    if queue.origin != c.origin {
        return Err(Refusal::RecoveryRequired.into());
    }
    for change in &queue.pending {
        if change.org_id != c.org_id {
            return Err(Refusal::InvariantFailure.into());
        }
        verify_change_integrity(change)?;
    }
    Ok(())
}

/// Append native-supplied immutable bytes without allocating a second sequence or clock.
pub fn queue_local(tx: &mut dyn LocalWorkTxn, change: Change) -> Result<SourceKey, SyncError> {
    let c = tx.context()?;
    if versions::protocol_profile(c.protocol_level).is_none() {
        return Err(Refusal::UnsupportedVersion.into());
    }
    if c.restore_pending {
        return Err(Refusal::RestorePending.into());
    }
    let mut queue = tx.local_outbox()?;
    validate_queue(&c, &queue)?;
    change.validate()?;
    if !versions::codec_supported(c.protocol_level, &change.codec)
        || !tx.supports_codec(&change.codec)?
    {
        return Err(Refusal::UnsupportedVersion.into());
    }
    if change.org_id != c.org_id
        || change.key.origin != c.origin
        || queue.last_seq.checked_add(1) != Some(change.key.seq.get())
        || change.required_org.epoch != c.applied_org.epoch
        || change.required_org.seq > c.applied_org.seq
    {
        return Err(Refusal::InvalidInput.into());
    }
    if queue.pending.len() >= MAX_PENDING
        || queue.bytes().saturating_add(change.payload.len()) > MAX_PENDING_BYTES
    {
        return Err(Refusal::QuotaExceeded.into());
    }
    let key = change.key.clone();
    queue.last_seq = key.seq.get();
    queue.pending.push_back(change);
    validate_queue(&c, &queue)?;
    tx.save_local(&queue)?;
    Ok(key)
}

mod forward;
pub use forward::{
    acknowledge_local, forward_local, receipt_action, AckProof, ForwardTrigger, LocalStreamAttempt,
    LocalStreamOutput, LocalStreamStore, LocalStreamTxn, ReceiptAction, RetainedOutcome,
    StreamAuthority, WorkOutcome, WorkStreamPeer,
};

#[cfg(test)]
mod tests;
