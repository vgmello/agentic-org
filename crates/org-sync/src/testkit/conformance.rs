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
    let one = hq.publish_fixture(1);
    let two = hq.publish_fixture(2);
    assert!(matches!(
        apply(satellite, &peer, two.clone()).unwrap(),
        OrgOutput::Gap { .. }
    ));
    assert_eq!(satellite.domain_application_count(), 0);
    satellite.fail_next_commit();
    assert!(apply(satellite, &peer, one.clone()).is_err());
    assert_eq!(satellite.domain_application_count(), 0);
    assert_eq!(satellite.inspect_checkpoint().position.seq, 0);
    apply(satellite, &peer, one.clone()).unwrap();
    let satellite = satellite.reopen();
    assert!(matches!(
        apply(&satellite, &peer, one).unwrap(),
        OrgOutput::Ack {
            duplicate: true,
            ..
        }
    ));
    assert_eq!(satellite.domain_application_count(), 1);
    apply(&satellite, &peer, two).unwrap();
    assert_eq!(satellite.domain_application_count(), 2);
    assert_eq!(satellite.inspect_checkpoint().position.seq, 2);
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
