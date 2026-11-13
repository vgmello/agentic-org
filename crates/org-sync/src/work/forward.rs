//! Current-HQ receipt ledger; native storage owns the transaction and retention.
use super::*;
use crate::organization::{PeerIdentity, PeerRole, VerifiedPeer};
use org_sync_proto::{
    work_ack_proof_digest_v1, work_rejection_code_valid, Epoch, WorkAck, WORK_CREDIT_WINDOW,
};

#[derive(Clone, Debug)]
pub struct StreamAuthority {
    pub hq: OfficeId,
    pub highest_epoch: Epoch,
    /// Volatile native connection generation; None after restart until HQ is verified.
    pub connection_generation: Option<u64>,
    pub pruned_through: u64,
}
pub trait WorkStreamPeer: VerifiedPeer {
    fn connection_generation(&self) -> u64;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkOutcome {
    /// HQ durably copied/deduped the source item; not an assertion of business success.
    Accepted,
    Rejected {
        code: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedOutcome {
    pub key: SourceKey,
    pub outcome: WorkOutcome,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AckProof {
    pub origin: Origin,
    pub from_exclusive: u64,
    pub through_seq: u64,
    pub digest: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalStreamOutput {
    /// Unframed candidates only: the transport must still enforce frame and credit limits.
    ForwardCandidates {
        changes: Vec<Change>,
    },
    Duplicate,
    /// from is the previous floor, exclusive; to is the committed inclusive floor.
    Advanced {
        from: u64,
        to: u64,
    },
}
pub type LocalStreamAttempt =
    dyn Fn(&mut dyn LocalStreamTxn) -> Result<LocalStreamOutput, SyncError> + Send + Sync;
/// Same whole-attempt, bounded busy/locked retry and durable-output contract as OrgStore.
/// Authority is checked at effect start; callbacks do no network I/O.
pub trait LocalStreamStore {
    fn transact(&self, attempt: Arc<LocalStreamAttempt>) -> Result<LocalStreamOutput, SyncError>;
}
pub trait LocalStreamTxn: LocalWorkTxn {
    fn authority(&mut self) -> Result<StreamAuthority, StoreError>;
    fn outcome(&mut self, key: &SourceKey) -> Result<Option<RetainedOutcome>, StoreError>;
    /// Complete, ordered rejected outcomes in this range, stopping after 33 rows.
    /// This bounded query detects an omitted rejection without scanning accepted history.
    fn retained_rejections(
        &mut self,
        origin: &Origin,
        from_exclusive: u64,
        through: u64,
    ) -> Result<Vec<RetainedOutcome>, StoreError>;
    /// Outcomes, prefix/quota release and proof commit together; never overwrite outcomes.
    fn save_ack(
        &mut self,
        queue: &LocalOutbox,
        outcomes: &[RetainedOutcome],
        proof: &AckProof,
    ) -> Result<(), StoreError>;
}

fn authorize(
    tx: &mut dyn LocalStreamTxn,
    peer: &PeerIdentity,
    generation: u64,
) -> Result<(LocalContext, StreamAuthority), SyncError> {
    let c = tx.context()?;
    let a = tx.authority()?;
    if peer.org_id != c.org_id
        || peer.office_id != a.hq
        || peer.role != PeerRole::Hq
        || peer.authority != a.highest_epoch
        || a.connection_generation != Some(generation)
    {
        return Err(Refusal::NotAuthorized.into());
    }
    Ok((c, a))
}
fn ready(c: &LocalContext) -> Result<(), Refusal> {
    if versions::protocol_profile(c.protocol_level).is_none() {
        return Err(Refusal::UnsupportedVersion);
    }
    if c.restore_pending {
        return Err(Refusal::RestorePending);
    }
    Ok(())
}
fn retained_match(
    tx: &mut dyn LocalStreamTxn,
    origin: &Origin,
    ack: &WorkAck,
    from: u64,
    through: u64,
) -> Result<(), SyncError> {
    if from >= through {
        return Ok(());
    }
    let rows = tx.retained_rejections(origin, from, through)?;
    if rows.len() > WORK_CREDIT_WINDOW + 1 {
        return Err(Refusal::InvariantFailure.into());
    }
    let mut after = from;
    for row in rows {
        let WorkOutcome::Rejected { code } = row.outcome else {
            return Err(Refusal::InvariantFailure.into());
        };
        let seq = row.key.seq.get();
        if row.key.origin != *origin
            || seq <= after
            || seq > through
            || !work_rejection_code_valid(&code)
        {
            return Err(Refusal::InvariantFailure.into());
        }
        after = seq;
        if !ack.rejected.iter().any(|r| r.seq == seq && r.code == code) {
            return Err(Refusal::ConflictingProof.into());
        }
    }
    for rejected in ack
        .rejected
        .iter()
        .filter(|r| r.seq > from && r.seq <= through)
    {
        let key = SourceKey {
            origin: origin.clone(),
            seq: SourceSeq::new(rejected.seq).map_err(|_| Refusal::InvariantFailure)?,
        };
        let row = tx.outcome(&key)?.ok_or(Refusal::InvariantFailure)?;
        if row.key != key {
            return Err(Refusal::InvariantFailure.into());
        }
        if row.outcome
            != (WorkOutcome::Rejected {
                code: rejected.code.clone(),
            })
        {
            return Err(Refusal::ConflictingProof.into());
        }
    }
    Ok(())
}

pub fn acknowledge_local(
    store: &dyn LocalStreamStore,
    peer: &dyn WorkStreamPeer,
    ack: WorkAck,
) -> Result<LocalStreamOutput, SyncError> {
    let identity = peer.identity();
    let generation = peer.connection_generation();
    store.transact(Arc::new(move |tx| {
        let (c, a) = authorize(tx, &identity, generation)?;
        if ack.origin_office_id != c.origin.office || ack.incarnation != c.origin.incarnation {
            return Err(Refusal::WrongStream.into());
        }
        ready(&c)?;
        ack.validate().map_err(|_| Refusal::Malformed)?;
        let mut queue = tx.local_outbox()?;
        validate_queue(&c, &queue)?;
        let floor = queue.acknowledged_seq;
        if a.pruned_through > floor {
            return Err(Refusal::InvariantFailure.into());
        }
        // Deliberately weaker than "actually sent": the bound is highest committed
        // for this incarnation. There is no send-intent or extra persisted counter.
        if ack.through_seq > queue.last_seq {
            return Err(Refusal::AheadOfForwarded.into());
        }
        if ack.from_exclusive > floor {
            return Err(Refusal::Gap.into());
        }
        retained_match(
            tx,
            &c.origin,
            &ack,
            ack.from_exclusive.max(a.pruned_through),
            ack.through_seq.min(floor),
        )?;
        if ack.through_seq <= floor {
            return Ok(LocalStreamOutput::Duplicate);
        }
        let outcomes: Vec<_> = queue
            .pending
            .iter()
            .take_while(|change| change.key.seq.get() <= ack.through_seq)
            .map(|change| {
                let outcome = ack
                    .rejected
                    .iter()
                    .find(|r| r.seq == change.key.seq.get())
                    .map_or(WorkOutcome::Accepted, |r| WorkOutcome::Rejected {
                        code: r.code.clone(),
                    });
                RetainedOutcome {
                    key: change.key.clone(),
                    outcome,
                }
            })
            .collect();
        queue.acknowledged_seq = ack.through_seq;
        queue.pending.drain(..outcomes.len());
        validate_queue(&c, &queue)?;
        let proof = AckProof {
            origin: c.origin.clone(),
            from_exclusive: ack.from_exclusive,
            through_seq: ack.through_seq,
            digest: work_ack_proof_digest_v1(&ack).map_err(|_| Refusal::Malformed)?,
        };
        tx.save_ack(&queue, &outcomes, &proof)?;
        Ok(LocalStreamOutput::Advanced {
            from: floor,
            to: ack.through_seq,
        })
    }))
}
#[cfg(test)]
mod tests;

mod planning;
pub use planning::{forward_local, receipt_action, ForwardTrigger, ReceiptAction};
