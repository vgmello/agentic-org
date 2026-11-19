//! Local stream policy only; candidates are neither encoded frames nor send permission.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForwardTrigger {
    Active,
    Gap,
    Advanced,
    CreditReturned,
}

/// The connection adapter executes this action; the core performs no I/O.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptAction {
    Keep,
    /// Keep the connection and request planning with Gap or Advanced, respectively.
    Reforward,
    Close,
    /// Emit only the existing divergence code, then close (never acknowledge the refusal).
    DivergenceThenClose,
}
impl ReceiptAction {
    pub fn wire_code(self) -> Option<&'static str> {
        match self {
            Self::DivergenceThenClose => Some(Refusal::Divergence.code()),
            _ => None,
        }
    }
}

/// Interpret only after the receipt attempt has finished; a storage error has no receipt.
pub fn receipt_action(result: &Result<LocalStreamOutput, SyncError>) -> ReceiptAction {
    match result {
        Ok(LocalStreamOutput::Advanced { .. }) | Err(SyncError::Refused(Refusal::Gap)) => {
            ReceiptAction::Reforward
        }
        Ok(_) | Err(SyncError::Storage(_)) | Err(SyncError::Refused(Refusal::RestorePending)) => {
            ReceiptAction::Keep
        }
        Err(SyncError::Refused(Refusal::ConflictingProof)) => ReceiptAction::DivergenceThenClose,
        Err(SyncError::Refused(_)) => ReceiptAction::Close,
    }
}

/// Re-read authority and the immutable outbox in the writer snapshot on every retry.
/// No queue/outcome/floor writes. Candidates escape only after a successful transaction;
/// framing, byte/frame credit and socket writes belong to the connection adapter.
pub fn forward_local(
    store: &dyn LocalStreamStore,
    peer: &dyn WorkStreamPeer,
    trigger: ForwardTrigger,
) -> Result<LocalStreamOutput, SyncError> {
    let identity = peer.identity();
    let generation = peer.connection_generation();
    store.transact(Arc::new(move |tx| {
        let (c, a) = authorize(tx, &identity, generation)?;
        ready(&c)?;
        if trigger == ForwardTrigger::CreditReturned {
            return Ok(LocalStreamOutput::ForwardCandidates { changes: vec![] });
        }
        let queue = tx.local_outbox()?;
        validate_queue(&c, &queue)?;
        if a.pruned_through > queue.acknowledged_seq {
            return Err(Refusal::InvariantFailure.into());
        }
        let changes = queue
            .pending
            .iter()
            .take(WORK_CREDIT_WINDOW)
            .cloned()
            .collect();
        Ok(LocalStreamOutput::ForwardCandidates { changes })
    }))
}
