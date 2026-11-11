//! Apply authoritative organization events through a replayable atomic storage attempt.
use crate::{
    error::{Refusal, StoreError, SyncError},
    ordering::{self, ApplyOrder, Checkpoint, EventHead},
};
use org_sync_proto::{event_digest_v1, Codec, Digest, Epoch, OfficeId, OrgId, Position};
use std::sync::Arc;

pub const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrgEvent {
    pub org_id: OrgId,
    pub position: Position,
    pub previous: Digest,
    pub digest: Digest,
    pub codec: Codec,
    pub payload: Arc<[u8]>,
}
impl OrgEvent {
    pub fn new(
        org_id: OrgId,
        position: Position,
        previous: Digest,
        codec: Codec,
        payload: impl Into<Arc<[u8]>>,
    ) -> Result<Self, Refusal> {
        let payload = payload.into();
        if position.seq == 0 || payload.len() > MAX_PAYLOAD_BYTES {
            return Err(Refusal::InvalidInput);
        }
        let digest = event_digest_v1(&org_id, position, previous, &codec, &payload)
            .map_err(|_| Refusal::InvalidInput)?;
        Ok(Self {
            org_id,
            position,
            previous,
            digest,
            codec,
            payload,
        })
    }
    fn validate(&self) -> Result<(), Refusal> {
        if self.position.seq == 0 || self.payload.len() > MAX_PAYLOAD_BYTES {
            return Err(Refusal::InvalidInput);
        }
        let expected = event_digest_v1(
            &self.org_id,
            self.position,
            self.previous,
            &self.codec,
            &self.payload,
        )
        .map_err(|_| Refusal::InvalidInput)?;
        if expected != self.digest {
            return Err(Refusal::Divergence);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrgCheckpoint {
    pub org_id: OrgId,
    pub local_office: OfficeId,
    pub hq_office: OfficeId,
    pub ca: Digest,
    pub position: Position,
    pub digest: Digest,
    pub highest_epoch: Epoch,
    /// Also set by the local recovery journal before it has accepted the next authority.
    pub restore_pending: bool,
    /// Signed restored digest at sequence zero of highest_epoch while fenced.
    /// Persist it atomically with highest_epoch; the committed cursor remains in the old epoch.
    pub pending_restore_base: Option<Digest>,
    pub protocol_level: u32,
}
impl OrgCheckpoint {
    fn ordering(&self) -> Checkpoint {
        Checkpoint {
            position: self.position,
            digest: self.digest,
            highest_epoch: self.highest_epoch,
            restore_pending: self.restore_pending,
        }
    }
    fn validate(&self) -> Result<(), Refusal> {
        let awaiting_snapshot = self.position.epoch < self.highest_epoch;
        if self.protocol_level == 0
            || self.position.epoch > self.highest_epoch
            || (awaiting_snapshot && !self.restore_pending)
            || awaiting_snapshot != self.pending_restore_base.is_some()
        {
            return Err(Refusal::InvariantFailure);
        }
        Ok(())
    }
}

/// PKI verifies the signature; core independently checks pinned org/HQ/CA and epoch.
/// A frame's claimed role or boolean is never proof of authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityRecord {
    pub org_id: OrgId,
    pub hq_office: OfficeId,
    pub ca: Digest,
    pub epoch: Epoch,
    pub restored_position: Position,
    pub restored_digest: Digest,
    pub signature: Arc<[u8]>,
}
/// Pure deterministic signature verification over immutable injected CA material.
/// Certificate/revocation I/O and wall-clock reads happen before the replayable attempt.
pub trait AuthorityVerifier: Send + Sync {
    fn verify(&self, record: &AuthorityRecord) -> Result<(), Refusal>;
}

/// Issued by the trusted staging/codec adapter after manifest, data, schema, encryption and
/// user-visible restore-warning checks. Not a peer-provided filesystem path or raw frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedSnapshot {
    pub org_id: OrgId,
    pub authority: Epoch,
    pub position: Position,
    pub digest: Digest,
    pub codec: Codec,
    pub staging_id: String,
    pub protocol_level: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrgOutput {
    Ack {
        position: Position,
        digest: Digest,
        duplicate: bool,
    },
    Published(OrgEvent),
    Gap {
        after: Position,
    },
    SnapshotRequired,
    AuthorityAccepted {
        snapshot_required: bool,
    },
    SnapshotInstalled,
}

/// Callbacks capture immutable facts only, read injected time only, and do no I/O or
/// external effects. They may be replayed after a batch abort. All reads/writes occupy one
/// writer job/savepoint and a consistent snapshot. Any Err rolls back the attempt;
/// integrity, divergence and invariant failures abort the entire batch. Return output only after durable
/// batch commit. Retry a whole busy/locked attempt at most MAX_STORE_RETRIES, never forever.
pub type OrgAttempt = dyn Fn(&mut dyn OrgTxn) -> Result<OrgOutput, SyncError> + Send + Sync;
pub trait OrgStore {
    fn transact(&self, attempt: Arc<OrgAttempt>) -> Result<OrgOutput, SyncError>;
}
pub trait OrgTxn {
    fn checkpoint(&mut self) -> Result<OrgCheckpoint, StoreError>;
    /// A bound replica with no materialized organization projection needs a
    /// trusted initial snapshot before any deltas, even at a matching epoch base.
    /// Every adapter must explicitly supply its durable state.
    fn requires_initial_snapshot(&mut self) -> Result<bool, StoreError>;
    fn digest_at(&mut self, position: Position) -> Result<Option<Digest>, StoreError>;
    /// Pure codec capability lookup, before application; unknown codecs never partially apply.
    fn supports_codec(&mut self, codec: &Codec) -> Result<bool, StoreError>;
    fn supports_protocol(&mut self, level: u32) -> Result<bool, StoreError>;
    /// Native domain application includes copied command outcomes. A refusal becomes
    /// divergence/invariant failure and aborts the entire batch, never partial state.
    fn apply_event(&mut self, event: &OrgEvent) -> Result<(), SyncError>;
    /// Persist event/cursor/digest only. Do not overwrite native domain policy modified by
    /// apply_event (including a replicated organization protocol-level transition).
    fn record_event(&mut self, event: &OrgEvent, next: &OrgCheckpoint) -> Result<(), StoreError>;
    /// Atomically install staged state, update the checkpoint and clear any initial
    /// snapshot requirement. A failed writer commit leaves the flag and state intact.
    fn install_snapshot(
        &mut self,
        snapshot: &ValidatedSnapshot,
        next: &OrgCheckpoint,
    ) -> Result<(), SyncError>;
    /// Persist epoch, pending signed restore base and cursor together.
    fn set_authority(&mut self, next: &OrgCheckpoint) -> Result<(), StoreError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerRole {
    Hq,
    Satellite,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerIdentity {
    pub org_id: OrgId,
    pub office_id: OfficeId,
    pub role: PeerRole,
    /// Authority epoch authenticated during session negotiation, never a frame claim.
    pub authority: Epoch,
}
/// Implemented by the trusted PKI/connection adapter after authentication and live
/// revocation checks. No Deserialize implementation or frame-supplied role is authority.
/// identity() is pure and returns immutable verified facts, never performs I/O.
pub trait VerifiedPeer: Send + Sync {
    fn identity(&self) -> PeerIdentity;
}

pub fn apply(
    store: &dyn OrgStore,
    peer: &dyn VerifiedPeer,
    event: OrgEvent,
) -> Result<OrgOutput, SyncError> {
    let peer = peer.identity();
    store.transact(Arc::new(move |tx| {
        let mut current = tx.checkpoint()?;
        current.validate()?;
        if event.org_id != current.org_id
            || peer.org_id != current.org_id
            || peer.office_id != current.hq_office
            || peer.role != PeerRole::Hq
            || peer.authority != current.highest_epoch
            || event.position.epoch != current.highest_epoch
        {
            return Err(Refusal::NotAuthorized.into());
        }
        event.validate()?;
        if !tx.supports_protocol(current.protocol_level)?
            || event.codec.version() > current.protocol_level
            || !tx.supports_codec(&event.codec)?
        {
            return Err(Refusal::UnsupportedVersion.into());
        }
        if !current.restore_pending && tx.requires_initial_snapshot()? {
            return Ok(OrgOutput::SnapshotRequired);
        }
        let retained = if event.position.epoch == current.position.epoch
            && event.position.seq < current.position.seq
        {
            tx.digest_at(event.position)?
        } else {
            None
        };
        let order = ordering::order_event(
            current.ordering(),
            EventHead {
                position: event.position,
                previous: event.previous,
                digest: event.digest,
            },
            retained,
        );
        match order {
            ApplyOrder::Apply(next) => {
                tx.apply_event(&event).map_err(classify_apply_error)?;
                current.position = next.position;
                current.digest = next.digest;
                tx.record_event(&event, &current)?;
                Ok(OrgOutput::Ack {
                    position: current.position,
                    digest: current.digest,
                    duplicate: false,
                })
            }
            ApplyOrder::Duplicate => Ok(OrgOutput::Ack {
                position: current.position,
                digest: current.digest,
                duplicate: true,
            }),
            ApplyOrder::Gap { after } => Ok(OrgOutput::Gap { after }),
            ApplyOrder::SnapshotRequired => Ok(OrgOutput::SnapshotRequired),
            ApplyOrder::RestorePending => Err(Refusal::RestorePending.into()),
            ApplyOrder::StaleEpoch => Err(Refusal::NotAuthorized.into()),
            ApplyOrder::Divergence => Err(Refusal::Divergence.into()),
            ApplyOrder::InvalidPosition => Err(Refusal::InvalidInput.into()),
        }
    }))
}

fn classify_apply_error(error: SyncError) -> SyncError {
    if error.aborts_batch() {
        return error;
    }
    match error {
        SyncError::Storage(_) => error,
        SyncError::Refused(_) => Refusal::Divergence.into(),
    }
}

/// Called INSIDE the native HQ command's writer attempt, after domain decision. The caller
/// returns Published only through OrgStore::transact, so nothing publishes before commit.
pub fn append_hq(
    tx: &mut dyn OrgTxn,
    codec: Codec,
    payload: Arc<[u8]>,
) -> Result<OrgOutput, SyncError> {
    let mut current = tx.checkpoint()?;
    current.validate()?;
    if current.local_office != current.hq_office {
        return Err(Refusal::NotAuthorized.into());
    }
    if current.restore_pending {
        return Err(Refusal::RestorePending.into());
    }
    if current.position.epoch != current.highest_epoch {
        return Err(Refusal::RecoveryRequired.into());
    }
    if !tx.supports_protocol(current.protocol_level)?
        || codec.version() > current.protocol_level
        || !tx.supports_codec(&codec)?
    {
        return Err(Refusal::UnsupportedVersion.into());
    }
    let seq = current
        .position
        .seq
        .checked_add(1)
        .ok_or(Refusal::InvariantFailure)?;
    let event = OrgEvent::new(
        current.org_id.clone(),
        Position {
            epoch: current.highest_epoch,
            seq,
        },
        current.digest,
        codec,
        payload,
    )?;
    tx.apply_event(&event).map_err(classify_apply_error)?;
    current.position = event.position;
    current.digest = event.digest;
    tx.record_event(&event, &current)?;
    Ok(OrgOutput::Published(event))
}

pub fn accept_authority(
    store: &dyn OrgStore,
    record: AuthorityRecord,
    verifier: Arc<dyn AuthorityVerifier>,
) -> Result<OrgOutput, SyncError> {
    store.transact(Arc::new(move |tx| {
        let mut current = tx.checkpoint()?;
        current.validate()?;
        if record.org_id != current.org_id
            || record.hq_office != current.hq_office
            || record.ca != current.ca
            || record.epoch < current.highest_epoch
        {
            return Err(Refusal::NotAuthorized.into());
        }
        if let Err(reason) = verifier.verify(&record) {
            let error = SyncError::from(reason);
            return Err(if error.aborts_batch() {
                error
            } else {
                Refusal::NotAuthorized.into()
            });
        }
        if record.restored_position.epoch >= record.epoch {
            return Err(Refusal::InvalidInput.into());
        }
        let initial_snapshot = tx.requires_initial_snapshot()?;
        if record.epoch == current.highest_epoch {
            if current
                .pending_restore_base
                .is_some_and(|digest| digest != record.restored_digest)
            {
                return Err(Refusal::Divergence.into());
            }
            // A repeated authority cannot clear a recovery fence or replace committed state.
            return Ok(OrgOutput::AuthorityAccepted {
                snapshot_required: current.restore_pending || initial_snapshot,
            });
        }
        let same_base = !initial_snapshot
            && current.position == record.restored_position
            && current.digest == record.restored_digest;
        current.highest_epoch = record.epoch;
        current.restore_pending = !same_base;
        current.pending_restore_base = (!same_base).then_some(record.restored_digest);
        if same_base {
            current.position = Position {
                epoch: record.epoch,
                seq: 0,
            };
        }
        tx.set_authority(&current)?;
        Ok(OrgOutput::AuthorityAccepted {
            snapshot_required: !same_base,
        })
    }))
}

pub fn install_snapshot(
    store: &dyn OrgStore,
    snapshot: ValidatedSnapshot,
) -> Result<OrgOutput, SyncError> {
    store.transact(Arc::new(move |tx| {
        let mut current = tx.checkpoint()?;
        current.validate()?;
        if snapshot.org_id != current.org_id
            || snapshot.authority != current.highest_epoch
            || snapshot.position.epoch != snapshot.authority
        {
            return Err(Refusal::NotAuthorized.into());
        }
        if current.restore_pending && current.pending_restore_base.is_none() {
            // A local restore journal fence is not authorization to rewind the current epoch.
            return Err(Refusal::RestorePending.into());
        }
        if snapshot.protocol_level == 0
            || !tx.supports_protocol(snapshot.protocol_level)?
            || snapshot.codec.version() > snapshot.protocol_level
            || (!current.restore_pending && snapshot.protocol_level < current.protocol_level)
            || !tx.supports_codec(&snapshot.codec)?
        {
            return Err(Refusal::UnsupportedVersion.into());
        }
        if snapshot.position.seq == 0
            && current
                .pending_restore_base
                .is_some_and(|base| snapshot.digest != base)
        {
            return Err(Refusal::Divergence.into());
        }
        if !current.restore_pending
            && (snapshot.position.seq < current.position.seq
                || (snapshot.position.seq == current.position.seq
                    && snapshot.digest != current.digest))
        {
            return Err(Refusal::Divergence.into());
        }
        if snapshot.staging_id.is_empty()
            || snapshot.staging_id.len() > 64
            || !snapshot
                .staging_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric())
        {
            return Err(Refusal::InvalidInput.into());
        }
        current.protocol_level = snapshot.protocol_level;
        current.position = snapshot.position;
        current.digest = snapshot.digest;
        current.restore_pending = false;
        current.pending_restore_base = None;
        tx.install_snapshot(&snapshot, &current)
            .map_err(classify_apply_error)?;
        Ok(OrgOutput::SnapshotInstalled)
    }))
}
