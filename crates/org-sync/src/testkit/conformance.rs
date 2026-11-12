//! Adapter contract checks, reusable by B1-14's SQLite fixtures. Fixtures supply native
//! codec payloads and observations; this module knows no database or native domain schema.
use super::*;

pub trait OrganizationHarness: OrgStore + Sized {
    /// Create one valid native change and append it through the same writer transaction.
    fn publish_fixture(&self, tag: u8) -> OrgEvent;
    fn inspect_checkpoint(&self) -> OrgCheckpoint;
    /// Observe domain application, not merely the cursor or retained log row count.
    fn domain_application_count(&self) -> usize;
    fn fail_next_commit(&self);
    fn reopen(&self) -> Self;
    /// Persist the enrollment/bootstrap flag on a fresh, unmaterialized replica.
    fn require_initial_snapshot(&self);
    fn initial_snapshot_required(&self) -> bool;
    /// Export source domain state and verify/stage it using the native adapter's real path.
    fn stage_snapshot(&self, source: &Self) -> ValidatedSnapshot;
    /// Inject support in the test binary/codec only, not a shipped native capability.
    fn support_protocol_fixture(&self, level: u32);
    /// Apply a native fixture and an injected policy transition to level 2 in one writer
    /// attempt, forwarding production record_event unchanged. Not a native upgrade command.
    fn publish_protocol_raise_fixture(&self) -> OrgEvent;
}
/// Fresh HQ + satellite of one org, same epoch/base and one supported codec. Panics on a
/// violated store contract, so native adapter tests can call it from their normal #[test].
pub fn organization_adapter<H: OrganizationHarness>(hq: &H, satellite: &H) {
    let c = satellite.inspect_checkpoint();
    let peer = MemoryPeer(PeerIdentity {
        authority: c.position.epoch,
        org_id: c.org_id,
        office_id: c.hq_office,
        role: PeerRole::Hq,
    });
    satellite.require_initial_snapshot();
    let bootstrap = hq.publish_fixture(0);
    let snapshot = satellite.stage_snapshot(hq);
    let one = hq.publish_fixture(1);
    let two = hq.publish_fixture(2);
    let satellite = satellite.reopen();
    assert!(satellite.initial_snapshot_required());
    let empty = satellite.inspect_checkpoint();
    assert_eq!(
        apply(&satellite, &peer, one.clone()).unwrap(),
        OrgOutput::SnapshotRequired
    );
    assert_eq!(satellite.inspect_checkpoint(), empty);
    assert_eq!(satellite.domain_application_count(), 0);
    let mut missing = snapshot.clone();
    missing.staging_id = "AcceptedMissingStagingForConformance".into();
    let error = install_snapshot(&satellite, missing).unwrap_err();
    assert!(matches!(
        error,
        SyncError::Refused(Refusal::IntegrityFailure)
    ));
    assert!(error.aborts_batch());
    let satellite = satellite.reopen();
    assert!(satellite.initial_snapshot_required());
    assert_eq!(satellite.inspect_checkpoint(), empty);
    assert_eq!(satellite.domain_application_count(), 0);
    // The existing accepted stage must survive the refused install and reopen.
    satellite.fail_next_commit();
    assert!(install_snapshot(&satellite, snapshot.clone()).is_err());
    let satellite = satellite.reopen();
    assert!(satellite.initial_snapshot_required());
    assert_eq!(satellite.inspect_checkpoint(), empty);
    assert_eq!(satellite.domain_application_count(), 0);
    install_snapshot(&satellite, snapshot).unwrap();
    let satellite = satellite.reopen();
    assert!(!satellite.initial_snapshot_required());
    assert_eq!(satellite.inspect_checkpoint().position, bootstrap.position);
    assert_eq!(satellite.inspect_checkpoint().digest, bootstrap.digest);
    assert_eq!(satellite.domain_application_count(), 1);

    assert!(matches!(
        apply(&satellite, &peer, two.clone()).unwrap(),
        OrgOutput::Gap { .. }
    ));
    assert_eq!(satellite.domain_application_count(), 1);
    let before = satellite.inspect_checkpoint();
    satellite.fail_next_commit();
    assert!(apply(&satellite, &peer, one.clone()).is_err());
    assert_eq!(satellite.domain_application_count(), 1);
    assert_eq!(satellite.inspect_checkpoint(), before);
    apply(&satellite, &peer, one.clone()).unwrap();
    let satellite = satellite.reopen();
    assert_eq!(
        apply(&satellite, &peer, one.clone()).unwrap(),
        OrgOutput::Ack {
            position: one.position,
            digest: one.digest,
            duplicate: true,
        }
    );
    assert_eq!(satellite.domain_application_count(), 2);
    apply(&satellite, &peer, two.clone()).unwrap();
    assert_eq!(satellite.domain_application_count(), 3);
    assert_eq!(satellite.inspect_checkpoint().position, two.position);
    assert_eq!(
        apply(&satellite, &peer, one).unwrap(),
        OrgOutput::Ack {
            position: two.position,
            digest: two.digest,
            duplicate: true,
        }
    );

    hq.support_protocol_fixture(2);
    satellite.support_protocol_fixture(2);
    let raise = hq.publish_protocol_raise_fixture();
    apply(&satellite, &peer, raise).unwrap();
    let satellite = satellite.reopen();
    let before_restore = satellite.inspect_checkpoint();
    assert_eq!(
        before_restore.protocol_level, 2,
        "record_event must preserve codec policy writes"
    );

    let authority = before_restore.highest_epoch.checked_next().unwrap();
    let record = AuthorityRecord {
        org_id: before_restore.org_id.clone(),
        hq_office: before_restore.hq_office.clone(),
        ca: before_restore.ca,
        epoch: authority,
        restored_position: bootstrap.position,
        restored_digest: bootstrap.digest,
        signature: Arc::from([]),
    };
    assert_eq!(
        accept_authority(&satellite, record, Arc::new(FixtureSignature)).unwrap(),
        OrgOutput::AuthorityAccepted {
            snapshot_required: true
        }
    );
    let satellite = satellite.reopen();
    let fenced = satellite.inspect_checkpoint();
    assert_eq!(fenced.highest_epoch, authority);
    assert!(fenced.restore_pending);
    assert_eq!(fenced.pending_restore_base, Some(bootstrap.digest));
    assert_eq!(fenced.position, before_restore.position);
    assert_eq!(fenced.digest, before_restore.digest);
    assert!(matches!(
        apply(&satellite, &peer, two.clone()),
        Err(SyncError::Refused(Refusal::NotAuthorized))
    ));
    let current_peer = MemoryPeer(PeerIdentity {
        authority,
        ..peer.identity()
    });
    let current_frame = OrgEvent::new(
        two.org_id,
        Position {
            epoch: authority,
            seq: 1,
        },
        bootstrap.digest,
        two.codec,
        two.payload,
    )
    .unwrap();
    assert!(matches!(
        apply(&satellite, &current_peer, current_frame),
        Err(SyncError::Refused(Refusal::RestorePending))
    ));
    assert_eq!(satellite.inspect_checkpoint(), fenced);
}

/// Test-only signature capability. Cryptographic PKI integration is a separate adapter test.
struct FixtureSignature;
impl AuthorityVerifier for FixtureSignature {
    fn verify(&self, _: &AuthorityRecord) -> Result<(), Refusal> {
        Ok(())
    }
}

impl OrganizationHarness for MemoryOffice {
    fn publish_fixture(&self, tag: u8) -> OrgEvent {
        self.publish(&[tag]).unwrap()
    }
    fn inspect_checkpoint(&self) -> OrgCheckpoint {
        self.checkpoint()
    }
    fn domain_application_count(&self) -> usize {
        self.applied().len()
    }
    fn fail_next_commit(&self) {
        self.inject(Fault::CommitUnavailable);
    }
    fn reopen(&self) -> Self {
        self.restart()
    }
    fn require_initial_snapshot(&self) {
        self.data.borrow_mut().initial_snapshot_required = true;
    }
    fn initial_snapshot_required(&self) -> bool {
        self.data.borrow().initial_snapshot_required
    }
    fn stage_snapshot(&self, source: &Self) -> ValidatedSnapshot {
        let c = source.checkpoint();
        let id = format!("SNAP{}", c.position.seq);
        self.stage(&id, source.applied());
        ValidatedSnapshot {
            org_id: c.org_id,
            authority: c.highest_epoch,
            position: c.position,
            digest: c.digest,
            codec: Codec::new("conformance", 1).unwrap(),
            staging_id: id,
            protocol_level: c.protocol_level,
        }
    }
    fn support_protocol_fixture(&self, level: u32) {
        self.data.borrow_mut().supported_max = level;
    }
    fn publish_protocol_raise_fixture(&self) -> OrgEvent {
        self.publish(b"protocol:2").unwrap()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_adapter_implements_the_organization_conformance_contract() {
        for epoch in [Epoch::new(1).unwrap(), Epoch::new(2).unwrap()] {
            let org = OrgId::new("ORG1").unwrap();
            let hq_id = OfficeId::new("HQ1").unwrap();
            let hq = MemoryOffice::new(
                org.clone(),
                hq_id.clone(),
                hq_id.clone(),
                Digest::ZERO,
                epoch,
            );
            let sat = MemoryOffice::new(
                org,
                OfficeId::new("SAT1").unwrap(),
                hq_id,
                Digest::ZERO,
                epoch,
            );
            organization_adapter(&hq, &sat);
        }
    }
}
