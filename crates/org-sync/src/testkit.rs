//! In-memory transactional adapter for conformance. No filesystem, network or real clock.
use crate::{
    error::{Refusal, StoreError, SyncError, MAX_STORE_RETRIES},
    organization::*,
};
use org_sync_proto::{Codec, Digest, Epoch, OfficeId, OrgId, Position};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

#[derive(Clone, Copy, Debug)]
pub enum Fault {
    Busy,
    Locked,
    CommitUnavailable,
    DomainRefusal,
    OrgApplyFatal(Refusal),
}
#[derive(Clone, Debug)]
struct Data {
    checkpoint: OrgCheckpoint,
    initial_snapshot_required: bool,
    events: BTreeMap<(u64, u64), OrgEvent>,
    applied: Vec<Arc<[u8]>>,
    staging: BTreeMap<String, Vec<Arc<[u8]>>>,
    supported_max: u32,
}

/// A committed copy and its independently local office state. Every attempt mutates a clone;
/// commit failure discards the entire clone, including domain writes and cursor bookkeeping.
pub struct MemoryOffice {
    data: RefCell<Data>,
    faults: RefCell<VecDeque<Fault>>,
    attempts: RefCell<usize>,
}
impl MemoryOffice {
    pub fn new(
        org_id: OrgId,
        local_office: OfficeId,
        hq_office: OfficeId,
        ca: Digest,
        epoch: Epoch,
    ) -> Self {
        Self {
            data: RefCell::new(Data {
                checkpoint: OrgCheckpoint {
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
                },
                initial_snapshot_required: false,
                events: BTreeMap::new(),
                applied: Vec::new(),
                staging: BTreeMap::new(),
                supported_max: 1,
            }),
            faults: RefCell::default(),
            attempts: RefCell::new(0),
        }
    }
    /// Reopen only the committed durable state; no injected faults or transient state survives.
    pub fn restart(&self) -> Self {
        Self {
            data: RefCell::new(self.data.borrow().clone()),
            faults: RefCell::default(),
            attempts: RefCell::new(0),
        }
    }
    pub fn checkpoint(&self) -> OrgCheckpoint {
        self.data.borrow().checkpoint.clone()
    }
    pub fn applied(&self) -> Vec<Arc<[u8]>> {
        self.data.borrow().applied.clone()
    }
    pub fn inject(&self, fault: Fault) {
        self.faults.borrow_mut().push_back(fault);
    }
    pub fn attempts(&self) -> usize {
        *self.attempts.borrow()
    }
    pub fn stage(&self, id: impl Into<String>, payloads: Vec<Arc<[u8]>>) {
        self.data.borrow_mut().staging.insert(id.into(), payloads);
    }
    pub fn publish(&self, payload: &[u8]) -> Result<OrgEvent, SyncError> {
        let payload: Arc<[u8]> = Arc::from(payload);
        let codec = Codec::new("conformance", 1).unwrap();
        match OrgStore::transact(
            self,
            Arc::new(move |tx| append_hq(tx, codec.clone(), payload.clone())),
        )? {
            OrgOutput::Published(event) => Ok(event),
            _ => Err(Refusal::InvariantFailure.into()),
        }
    }
}
impl MemoryOffice {
    fn atomic<T>(
        &self,
        mut attempt: impl FnMut(&mut Tx<'_>) -> Result<T, SyncError>,
    ) -> Result<T, SyncError> {
        for retry in 0..=MAX_STORE_RETRIES {
            *self.attempts.borrow_mut() += 1;
            let fault = self.faults.borrow_mut().pop_front();
            let mut candidate = self.data.borrow().clone();
            let output = match attempt(&mut Tx {
                data: &mut candidate,
                domain_refusal: matches!(fault, Some(Fault::DomainRefusal)),
                org_apply_failure: match fault {
                    Some(Fault::OrgApplyFatal(reason)) => Some(reason),
                    _ => None,
                },
            }) {
                Err(SyncError::Storage(error))
                    if error.is_retryable() && retry < MAX_STORE_RETRIES =>
                {
                    continue
                }
                result => result?,
            };
            if matches!(fault, Some(Fault::Busy | Fault::Locked)) {
                if retry < MAX_STORE_RETRIES {
                    continue;
                }
                return Err(StoreError::busy_or_locked("injected private storage cause").into());
            }
            if matches!(fault, Some(Fault::CommitUnavailable)) {
                return Err(StoreError::unavailable("injected private commit cause").into());
            }
            *self.data.borrow_mut() = candidate;
            return Ok(output);
        }
        unreachable!("bounded retry loop returns on final attempt")
    }
}
impl OrgStore for MemoryOffice {
    fn transact(&self, attempt: Arc<OrgAttempt>) -> Result<OrgOutput, SyncError> {
        self.atomic(|tx| attempt(tx))
    }
}
struct Tx<'a> {
    data: &'a mut Data,
    domain_refusal: bool,
    org_apply_failure: Option<Refusal>,
}
impl OrgTxn for Tx<'_> {
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
        Ok(codec.id() == "conformance" && codec.version() == 1)
    }
    fn supports_protocol(&mut self, level: u32) -> Result<bool, StoreError> {
        Ok(level > 0 && level <= self.data.supported_max)
    }
    fn apply_event(&mut self, event: &OrgEvent) -> Result<(), SyncError> {
        self.data.applied.push(event.payload.clone());
        if &*event.payload == b"protocol:2" {
            self.data.checkpoint.protocol_level = 2;
        }
        if let Some(reason) = self.org_apply_failure {
            return Err(reason.into());
        }
        if self.domain_refusal {
            return Err(Refusal::InvalidInput.into());
        }
        Ok(())
    }
    fn record_event(&mut self, event: &OrgEvent, next: &OrgCheckpoint) -> Result<(), StoreError> {
        self.data.events.insert(
            (event.position.epoch.get(), event.position.seq),
            event.clone(),
        );
        // record_event owns the log/cursor, not native domain policy such as protocol level.
        self.data.checkpoint.position = next.position;
        self.data.checkpoint.digest = next.digest;
        Ok(())
    }
    fn install_snapshot(
        &mut self,
        snapshot: &ValidatedSnapshot,
        next: &OrgCheckpoint,
    ) -> Result<(), SyncError> {
        let data = self
            .data
            .staging
            .get(&snapshot.staging_id)
            .cloned()
            .ok_or(Refusal::InvalidInput)?;
        self.data.applied = data;
        self.data.events.clear();
        self.data.checkpoint = next.clone();
        self.data.initial_snapshot_required = false;
        if let Some(reason) = self.org_apply_failure {
            return Err(reason.into());
        }
        Ok(())
    }
    fn set_authority(&mut self, next: &OrgCheckpoint) -> Result<(), StoreError> {
        self.data.checkpoint = next.clone();
        Ok(())
    }
}

#[derive(Clone)]
pub struct MemoryPeer(pub PeerIdentity);
impl VerifiedPeer for MemoryPeer {
    fn identity(&self) -> PeerIdentity {
        self.0.clone()
    }
}

#[cfg(test)]
impl MemoryPeer {
    fn at_epoch(peer: &dyn VerifiedPeer, authority: Epoch) -> Self {
        Self(PeerIdentity {
            authority,
            ..peer.identity()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pair() -> (MemoryOffice, MemoryOffice, MemoryPeer) {
        let org = OrgId::new("ORG1").unwrap();
        let hq = OfficeId::new("HQ1").unwrap();
        let ca = Digest::from_bytes([1; 32]);
        let epoch = Epoch::new(1).unwrap();
        (
            MemoryOffice::new(org.clone(), hq.clone(), hq.clone(), ca, epoch),
            MemoryOffice::new(
                org.clone(),
                OfficeId::new("SAT1").unwrap(),
                hq.clone(),
                ca,
                epoch,
            ),
            MemoryPeer(PeerIdentity {
                authority: Epoch::new(1).unwrap(),
                org_id: org,
                office_id: hq,
                role: PeerRole::Hq,
            }),
        )
    }
    #[test]
    fn unsupported_codec_and_equal_position_snapshot_divergence_never_apply() {
        let (hq, sat, peer) = pair();
        let one = hq.publish(b"one").unwrap();
        apply(&sat, &peer, one.clone()).unwrap();
        let future = OrgEvent::new(
            one.org_id.clone(),
            Position {
                epoch: one.position.epoch,
                seq: 2,
            },
            one.digest,
            Codec::new("future", 1).unwrap(),
            b"two".as_slice(),
        )
        .unwrap();
        assert!(matches!(
            apply(&sat, &peer, future),
            Err(SyncError::Refused(Refusal::UnsupportedVersion))
        ));
        sat.stage("conflicting", vec![Arc::from(b"different".as_slice())]);
        let snapshot = ValidatedSnapshot {
            org_id: one.org_id,
            authority: one.position.epoch,
            position: one.position,
            digest: Digest::ZERO,
            codec: one.codec,
            protocol_level: 1,
            staging_id: "conflicting".into(),
        };
        assert!(matches!(
            install_snapshot(&sat, snapshot),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        assert_eq!(sat.applied(), hq.applied());
        assert_eq!(sat.checkpoint().digest, one.digest);
    }

    #[test]
    fn authoritative_apply_and_snapshot_preserve_batch_fatal_adapter_errors() {
        let (hq, sat, peer) = pair();
        let event = hq.publish(b"first").unwrap();
        sat.stage("staged", hq.applied());
        let checkpoint = sat.checkpoint();
        let snapshot = ValidatedSnapshot {
            org_id: event.org_id.clone(),
            authority: event.position.epoch,
            position: event.position,
            digest: event.digest,
            codec: event.codec.clone(),
            staging_id: "staged".into(),
            protocol_level: 1,
        };
        for reason in [Refusal::IntegrityFailure, Refusal::InvariantFailure] {
            sat.inject(Fault::OrgApplyFatal(reason));
            let event_error = apply(&sat, &peer, event.clone()).unwrap_err();
            sat.inject(Fault::OrgApplyFatal(reason));
            let snapshot_error = install_snapshot(&sat, snapshot.clone()).unwrap_err();
            for error in [event_error, snapshot_error] {
                assert!(error.aborts_batch());
                assert!(matches!(error, SyncError::Refused(actual) if actual == reason));
            }
            assert_eq!(sat.checkpoint(), checkpoint);
            assert!(sat.applied().is_empty());
        }
    }

    #[test]
    fn two_offices_reorder_repeat_and_recover_from_a_lost_ack_without_reapply() {
        let (hq, sat, peer) = pair();
        let one = hq.publish(b"one").unwrap();
        let two = hq.publish(b"two").unwrap();
        assert!(matches!(
            apply(&sat, &peer, two.clone()).unwrap(),
            OrgOutput::Gap { .. }
        ));
        assert!(sat.applied().is_empty());
        sat.inject(Fault::CommitUnavailable);
        assert!(matches!(
            apply(&sat, &peer, one.clone()),
            Err(SyncError::Storage(_))
        ));
        assert!(sat.applied().is_empty());
        assert_eq!(sat.checkpoint().position.seq, 0);
        assert!(matches!(
            apply(&sat, &peer, one.clone()).unwrap(),
            OrgOutput::Ack {
                duplicate: false,
                ..
            }
        ));
        // Ignore the first ACK, simulating a disconnect after durable commit.
        assert!(matches!(
            apply(&sat, &peer, one).unwrap(),
            OrgOutput::Ack {
                duplicate: true,
                ..
            }
        ));
        apply(&sat, &peer, two).unwrap();
        assert_eq!(sat.applied(), hq.applied());
        assert_eq!(sat.checkpoint().digest, hq.checkpoint().digest);
    }
    #[test]
    fn tampered_payload_wrong_peer_and_domain_refusal_never_advance_the_cursor() {
        let (hq, sat, peer) = pair();
        let event = hq.publish(b"one").unwrap();
        let mut bad = event.clone();
        bad.payload = Arc::from(b"changed".as_slice());
        assert!(matches!(
            apply(&sat, &peer, bad),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        let mut wrong = peer.clone();
        wrong.0.role = PeerRole::Satellite;
        assert!(matches!(
            apply(&sat, &wrong, event.clone()),
            Err(SyncError::Refused(Refusal::NotAuthorized))
        ));
        sat.inject(Fault::DomainRefusal);
        let error = apply(&sat, &peer, event.clone()).unwrap_err();
        assert!(error.aborts_batch());
        assert!(sat.applied().is_empty());
        assert_eq!(sat.checkpoint().position.seq, 0);
        apply(&sat, &peer, event).unwrap();
        assert_eq!(sat.applied().len(), 1);
    }
    #[test]
    fn busy_locked_retries_are_bounded_and_storage_causes_are_server_only() {
        let (hq, sat, peer) = pair();
        let event = hq.publish(b"one").unwrap();
        for fault in [Fault::Busy, Fault::Locked, Fault::Busy] {
            sat.inject(fault);
        }
        apply(&sat, &peer, event.clone()).unwrap();
        assert_eq!(sat.attempts(), 4);
        let (_, sat, _) = pair();
        for _ in 0..4 {
            sat.inject(Fault::Busy);
        }
        let error = apply(&sat, &peer, event).unwrap_err();
        assert_eq!(error.to_string(), "storage unavailable");
        assert!(std::error::Error::source(&error).is_some());
        assert_eq!(sat.attempts(), 4);
        assert!(sat.applied().is_empty());
    }
    struct Signature;
    impl AuthorityVerifier for Signature {
        fn verify(&self, record: &AuthorityRecord) -> Result<(), Refusal> {
            if &*record.signature == b"valid-test-proof" {
                Ok(())
            } else {
                Err(Refusal::NotAuthorized)
            }
        }
    }
    fn record(hq: &MemoryOffice, position: Position, digest: Digest) -> AuthorityRecord {
        let meta = hq.checkpoint();
        AuthorityRecord {
            org_id: meta.org_id,
            hq_office: meta.hq_office,
            ca: meta.ca,
            epoch: Epoch::new(2).unwrap(),
            restored_position: position,
            restored_digest: digest,
            signature: Arc::from(b"valid-test-proof".as_slice()),
        }
    }
    #[test]
    fn fatal_authority_verification_refusals_are_preserved() {
        struct FatalVerifier(Refusal);
        impl AuthorityVerifier for FatalVerifier {
            fn verify(&self, _: &AuthorityRecord) -> Result<(), Refusal> {
                Err(self.0)
            }
        }
        let (hq, sat, _) = pair();
        let before = sat.checkpoint();
        let authority = record(&hq, before.position, before.digest);
        for reason in [
            Refusal::IntegrityFailure,
            Refusal::Divergence,
            Refusal::InvariantFailure,
        ] {
            let error = accept_authority(&sat, authority.clone(), Arc::new(FatalVerifier(reason)))
                .unwrap_err();
            assert!(error.aborts_batch());
            assert!(matches!(error, SyncError::Refused(actual) if actual == reason));
            assert_eq!(sat.checkpoint(), before);
        }
    }
    #[test]
    fn epoch_fence_survives_a_reconstructed_office_and_an_ahead_peer_needs_snapshot() {
        let (hq, sat, peer) = pair();
        let one = hq.publish(b"one").unwrap();
        let two = hq.publish(b"two").unwrap();
        apply(&sat, &peer, one.clone()).unwrap();
        apply(&sat, &peer, two).unwrap();
        let restored = record(&hq, one.position, one.digest);
        assert_eq!(
            accept_authority(&sat, restored.clone(), Arc::new(Signature)).unwrap(),
            OrgOutput::AuthorityAccepted {
                snapshot_required: true
            }
        );
        assert!(sat.checkpoint().restore_pending);
        assert_eq!(sat.checkpoint().highest_epoch.get(), 2);
        let mut wrong_ca = restored.clone();
        wrong_ca.epoch = Epoch::new(3).unwrap();
        wrong_ca.ca = Digest::ZERO;
        assert!(matches!(
            accept_authority(&sat, wrong_ca, Arc::new(Signature)),
            Err(SyncError::Refused(Refusal::NotAuthorized))
        ));
        let mut unsigned = restored.clone();
        unsigned.epoch = Epoch::new(3).unwrap();
        unsigned.signature = Arc::from([]);
        assert!(accept_authority(&sat, unsigned, Arc::new(Signature)).is_err());
        let restarted = MemoryOffice {
            data: RefCell::new(sat.data.borrow().clone()),
            faults: RefCell::default(),
            attempts: RefCell::new(0),
        };
        let mut stale = restored;
        stale.epoch = Epoch::new(1).unwrap();
        stale.restored_position.seq = 0;
        assert!(accept_authority(&restarted, stale, Arc::new(Signature)).is_err());
        assert!(apply(&restarted, &peer, one).is_err());
        restarted.stage("restored", vec![Arc::from(b"one".as_slice())]);
        let snapshot = ValidatedSnapshot {
            org_id: OrgId::new("ORG1").unwrap(),
            authority: Epoch::new(2).unwrap(),
            position: Position {
                epoch: Epoch::new(2).unwrap(),
                seq: 0,
            },
            digest: hq.data.borrow().events[&(1, 1)].digest,
            codec: Codec::new("conformance", 1).unwrap(),
            protocol_level: 1,
            staging_id: "restored".into(),
        };
        let before = restarted.checkpoint();
        let mut conflicting = snapshot.clone();
        conflicting.digest = Digest::ZERO;
        assert!(matches!(
            install_snapshot(&restarted, conflicting),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        assert_eq!(restarted.checkpoint(), before);
        install_snapshot(&restarted, snapshot).unwrap();
        assert!(!restarted.checkpoint().restore_pending);
        assert_eq!(restarted.applied().len(), 1);
    }
    #[test]
    fn future_org_event_on_current_hq_session_cannot_observe_restore_fence() {
        let (hq, sat, peer) = pair();
        let meta = sat.checkpoint();
        let future = OrgEvent::new(
            meta.org_id.clone(),
            Position {
                epoch: Epoch::new(2).unwrap(),
                seq: 1,
            },
            Digest::ZERO,
            Codec::new("conformance", 1).unwrap(),
            b"future".as_slice(),
        )
        .unwrap();
        assert!(matches!(
            apply(&sat, &peer, future),
            Err(SyncError::Refused(Refusal::NotAuthorized))
        ));
        let old = hq.publish(b"old").unwrap();
        let signed = record(&hq, old.position, old.digest);
        let current = MemoryPeer::at_epoch(&peer, signed.epoch);
        accept_authority(&sat, signed, Arc::new(Signature)).unwrap();
        let future = OrgEvent::new(
            meta.org_id,
            Position {
                epoch: Epoch::new(3).unwrap(),
                seq: 1,
            },
            Digest::ZERO,
            Codec::new("conformance", 1).unwrap(),
            b"future".as_slice(),
        )
        .unwrap();
        assert!(matches!(
            apply(&sat, &current, future),
            Err(SyncError::Refused(Refusal::NotAuthorized))
        ));
        assert!(sat.applied().is_empty());
    }
    #[test]
    fn repeated_signed_authority_must_match_the_retained_pending_base() {
        let (hq, sat, _) = pair();
        let one = hq.publish(b"one").unwrap();
        let signed = record(&hq, one.position, one.digest);
        accept_authority(&sat, signed.clone(), Arc::new(Signature)).unwrap();
        let before = sat.checkpoint();
        let mut conflict = signed.clone();
        conflict.restored_digest = Digest::ZERO;
        assert!(matches!(
            accept_authority(&sat, conflict, Arc::new(Signature)),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        assert_eq!(sat.checkpoint(), before);
        assert_eq!(
            accept_authority(&sat, signed, Arc::new(Signature)).unwrap(),
            OrgOutput::AuthorityAccepted {
                snapshot_required: true
            }
        );
    }
    #[test]
    fn stale_org_event_on_current_hq_session_cannot_observe_restore_fence() {
        let (hq, sat, peer) = pair();
        let old = hq.publish(b"old").unwrap();
        let new = record(&hq, old.position, old.digest);
        let current = MemoryPeer::at_epoch(&peer, new.epoch);
        assert!(matches!(
            accept_authority(&sat, new, Arc::new(Signature)).unwrap(),
            OrgOutput::AuthorityAccepted {
                snapshot_required: true
            }
        ));
        assert!(matches!(
            apply(&sat, &current, old),
            Err(SyncError::Refused(Refusal::NotAuthorized))
        ));
        assert!(sat.applied().is_empty());
    }
    #[test]
    fn fresh_satellite_requires_initial_snapshot_before_ordering_deltas() {
        let (hq, sat, peer) = pair();
        sat.data.borrow_mut().initial_snapshot_required = true;
        let one = hq.publish(b"one").unwrap();
        let two = hq.publish(b"two").unwrap();
        for event in [two.clone(), one.clone()] {
            assert_eq!(
                apply(&sat, &peer, event).unwrap(),
                OrgOutput::SnapshotRequired
            );
        }
        assert_eq!(sat.checkpoint().position.seq, 0);
        assert!(sat.applied().is_empty());
        sat.stage("initial", vec![one.payload.clone()]);
        let snapshot = ValidatedSnapshot {
            org_id: one.org_id.clone(),
            authority: one.position.epoch,
            position: one.position,
            digest: one.digest,
            codec: one.codec.clone(),
            staging_id: "initial".into(),
            protocol_level: 1,
        };
        sat.inject(Fault::CommitUnavailable);
        assert!(install_snapshot(&sat, snapshot.clone()).is_err());
        assert!(sat.data.borrow().initial_snapshot_required);
        assert!(sat.applied().is_empty());
        install_snapshot(&sat, snapshot).unwrap();
        assert!(!sat.data.borrow().initial_snapshot_required);
        assert!(matches!(
            apply(&sat, &peer, two).unwrap(),
            OrgOutput::Ack { .. }
        ));
        assert_eq!(sat.applied(), hq.applied());
    }
    #[test]
    fn synthetic_bootstrap_base_does_not_satisfy_repeated_or_higher_authority() {
        let (hq, fresh, _) = pair();
        let initial = fresh.checkpoint();
        let signed = record(&hq, initial.position, initial.digest);
        fresh.data.borrow_mut().initial_snapshot_required = true;
        assert_eq!(
            accept_authority(&fresh, signed.clone(), Arc::new(Signature)).unwrap(),
            OrgOutput::AuthorityAccepted {
                snapshot_required: true
            }
        );
        assert!(fresh.checkpoint().restore_pending);
        assert_eq!(fresh.checkpoint().position, initial.position);
        let (_, enrolled, _) = pair();
        {
            let mut data = enrolled.data.borrow_mut();
            data.initial_snapshot_required = true;
            data.checkpoint.highest_epoch = signed.epoch;
            data.checkpoint.position.epoch = signed.epoch;
        }
        assert_eq!(
            accept_authority(&enrolled, signed, Arc::new(Signature)).unwrap(),
            OrgOutput::AuthorityAccepted {
                snapshot_required: true
            }
        );
        assert!(enrolled.data.borrow().initial_snapshot_required);
    }
    #[test]
    fn local_restore_fence_requires_new_signed_authority_before_snapshot_install() {
        let (hq, sat, peer) = pair();
        let one = hq.publish(b"one").unwrap();
        apply(&sat, &peer, one.clone()).unwrap();
        sat.data.borrow_mut().checkpoint.restore_pending = true;
        sat.stage("premature", Vec::new());
        let before = sat.checkpoint();
        let snapshot = ValidatedSnapshot {
            org_id: one.org_id.clone(),
            authority: one.position.epoch,
            position: Position {
                epoch: one.position.epoch,
                seq: 0,
            },
            digest: Digest::ZERO,
            codec: one.codec,
            protocol_level: 1,
            staging_id: "premature".into(),
        };
        assert!(matches!(
            install_snapshot(&sat, snapshot),
            Err(SyncError::Refused(Refusal::RestorePending))
        ));
        assert_eq!(sat.checkpoint(), before);
        assert_eq!(sat.applied(), hq.applied());
    }
    #[test]
    fn matching_restored_base_keeps_the_chain_across_epoch_change() {
        let (hq, sat, peer) = pair();
        let one = hq.publish(b"one").unwrap();
        apply(&sat, &peer, one.clone()).unwrap();
        let authority = record(&hq, one.position, one.digest);
        accept_authority(&hq, authority.clone(), Arc::new(Signature)).unwrap();
        accept_authority(&sat, authority, Arc::new(Signature)).unwrap();
        let next = hq.publish(b"after restore").unwrap();
        assert_eq!(next.previous, one.digest);
        assert_eq!(next.position.epoch.get(), 2);
        assert_eq!(next.position.seq, 1);
        assert!(matches!(
            apply(&sat, &peer, next.clone()),
            Err(SyncError::Refused(Refusal::NotAuthorized))
        ));
        let current = MemoryPeer::at_epoch(&peer, next.position.epoch);
        apply(&sat, &current, next).unwrap();
        assert_eq!(sat.applied(), hq.applied());
    }
}

pub mod conformance;

#[cfg(test)]
mod protocol_tests {
    use super::*;
    #[test]
    fn replicated_level_survives_cursor_write_and_an_old_returning_office_must_upgrade() {
        let org = OrgId::new("ORG1").unwrap();
        let hq_id = OfficeId::new("HQ1").unwrap();
        let hq = MemoryOffice::new(
            org.clone(),
            hq_id.clone(),
            hq_id.clone(),
            Digest::ZERO,
            Epoch::new(1).unwrap(),
        );
        let sat = MemoryOffice::new(
            org.clone(),
            OfficeId::new("SAT1").unwrap(),
            hq_id.clone(),
            Digest::ZERO,
            Epoch::new(1).unwrap(),
        );
        hq.data.borrow_mut().supported_max = 2;
        let peer = MemoryPeer(PeerIdentity {
            authority: Epoch::new(1).unwrap(),
            org_id: org,
            office_id: hq_id,
            role: PeerRole::Hq,
        });
        let event = hq.publish(b"protocol:2").unwrap();
        assert_eq!(hq.checkpoint().protocol_level, 2);
        apply(&sat, &peer, event).unwrap();
        assert_eq!(sat.checkpoint().protocol_level, 2);
        let event = hq.publish(b"after raise").unwrap();
        assert!(matches!(
            apply(&sat, &peer, event.clone()),
            Err(SyncError::Refused(Refusal::UnsupportedVersion))
        ));
        sat.data.borrow_mut().supported_max = 2;
        apply(&sat, &peer, event).unwrap();
        sat.stage("LEVEL2", vec![Arc::from(&b"snapshot"[..])]);
        let current = sat.checkpoint();
        let snapshot = ValidatedSnapshot {
            org_id: current.org_id,
            authority: current.highest_epoch,
            position: current.position,
            digest: current.digest,
            codec: Codec::new("conformance", 1).unwrap(),
            staging_id: "LEVEL2".into(),
            protocol_level: 1,
        };
        assert!(matches!(
            install_snapshot(&sat, snapshot),
            Err(SyncError::Refused(Refusal::UnsupportedVersion))
        ));
        assert_eq!(sat.checkpoint().protocol_level, 2);
    }
}

#[cfg(test)]
mod attempt_busy_regression {
    use super::*;
    #[test]
    fn a_busy_error_from_inside_the_attempt_retries_the_whole_attempt() {
        let office = MemoryOffice::new(
            OrgId::new("ORG1").unwrap(),
            OfficeId::new("SAT1").unwrap(),
            OfficeId::new("HQ1").unwrap(),
            Digest::ZERO,
            Epoch::new(1).unwrap(),
        );
        let result = OrgStore::transact(
            &office,
            Arc::new(|tx| {
                let mut meta = tx.checkpoint()?;
                meta.protocol_level = 2;
                tx.set_authority(&meta)?;
                Err(StoreError::busy_or_locked("query busy").into())
            }),
        );
        assert!(matches!(result, Err(SyncError::Storage(_))));
        assert_eq!(office.attempts(), MAX_STORE_RETRIES + 1);
        assert_eq!(office.checkpoint().protocol_level, 1);
    }
    #[test]
    fn a_nonretryable_storage_error_is_not_retried() {
        let office = MemoryOffice::new(
            OrgId::new("ORG1").unwrap(),
            OfficeId::new("SAT1").unwrap(),
            OfficeId::new("HQ1").unwrap(),
            Digest::ZERO,
            Epoch::new(1).unwrap(),
        );
        assert!(OrgStore::transact(
            &office,
            Arc::new(|_| Err(StoreError::unavailable("private disk failure").into()))
        )
        .is_err());
        assert_eq!(office.attempts(), 1);
    }
}
