use super::*;

fn offices() -> (MemoryOffice, MemoryOffice, MemoryPeer) {
    let org = OrgId::new("ORG1").unwrap();
    let hq = OfficeId::new("HQ1").unwrap();
    let epoch = Epoch::new(1).unwrap();
    let ca = Digest::from_bytes([1; 32]);
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
            org_id: org,
            office_id: hq,
            role: PeerRole::Hq,
            authority: epoch,
        }),
    )
}

fn refused(result: Result<OrgOutput, SyncError>, expected: Refusal) {
    let error = result.unwrap_err();
    assert!(
        matches!(&error, SyncError::Refused(actual) if *actual == expected),
        "expected {expected:?}, got {error}"
    );
    assert!(error.aborts_batch());
}

#[test]
fn event_and_published_debug_show_length_without_payload_bytes() {
    let (hq, _, _) = offices();
    for payload in [b"private event body".as_slice(), b"".as_slice()] {
        let event = hq.publish(payload).unwrap();
        let output = OrgOutput::Published(event.clone());
        for debug in [
            format!("{event:?}"),
            format!("{event:#?}"),
            format!("{output:?}"),
            format!("{output:#?}"),
        ] {
            assert!(debug.contains(&format!("payload_bytes: {}", payload.len())));
            assert!(!debug.contains("payload:"));
            assert!(!debug.contains("private event body"));
            assert!(debug.contains("org_id:"));
            assert!(debug.contains("position:"));
            assert!(debug.contains("codec:"));
        }
    }
}

#[test]
fn corrupt_event_bytes_are_integrity_failure_and_valid_history_conflicts_diverge() {
    let (hq, satellite, peer) = offices();
    let one = hq.publish(b"one").unwrap();
    let before = satellite.checkpoint();
    for field in ["payload", "digest", "previous"] {
        let mut corrupt = one.clone();
        match field {
            "payload" => corrupt.payload = Arc::from(b"changed".as_slice()),
            "digest" => corrupt.digest = Digest::from_bytes([8; 32]),
            _ => corrupt.previous = Digest::from_bytes([9; 32]),
        }
        refused(apply(&satellite, &peer, corrupt), Refusal::IntegrityFailure);
        assert_eq!(satellite.checkpoint(), before);
        assert!(satellite.applied().is_empty());
        assert!(satellite.data.borrow().events.is_empty());
    }
    apply(&satellite, &peer, one.clone()).unwrap();
    let committed = satellite.checkpoint();
    let conflicting = OrgEvent::new(
        one.org_id.clone(),
        one.position,
        one.previous,
        one.codec.clone(),
        b"other".as_slice(),
    )
    .unwrap();
    refused(apply(&satellite, &peer, conflicting), Refusal::Divergence);
    assert_eq!(satellite.checkpoint(), committed);
    assert_eq!(satellite.applied(), vec![one.payload]);
    assert_eq!(satellite.data.borrow().events.len(), 1);
}

#[test]
fn missing_accepted_snapshot_stage_is_integrity_failure_without_state_changes() {
    let (hq, satellite, peer) = offices();
    let one = hq.publish(b"committed").unwrap();
    apply(&satellite, &peer, one.clone()).unwrap();
    satellite.stage("retained", vec![one.payload.clone()]);
    let before = satellite.checkpoint();
    let events = satellite.data.borrow().events.clone();
    let stages = satellite.data.borrow().staging.clone();
    let snapshot = ValidatedSnapshot {
        org_id: one.org_id,
        authority: one.position.epoch,
        position: one.position,
        digest: one.digest,
        codec: one.codec,
        staging_id: "acceptedMissing".into(),
        protocol_level: 1,
    };
    refused(
        install_snapshot(&satellite, snapshot.clone()),
        Refusal::IntegrityFailure,
    );
    let reopened = satellite.restart();
    assert_eq!(reopened.checkpoint(), before);
    assert_eq!(reopened.applied(), vec![one.payload.clone()]);
    assert_eq!(reopened.data.borrow().events, events);
    assert_eq!(reopened.data.borrow().staging, stages);
    reopened.stage("acceptedMissing", vec![one.payload]);
    assert_eq!(
        install_snapshot(&reopened, snapshot).unwrap(),
        OrgOutput::SnapshotInstalled
    );
    assert_eq!(reopened.applied(), satellite.applied());
}

#[test]
fn malformed_snapshot_reference_remains_invalid_input_before_adapter_lookup() {
    let (_, satellite, _) = offices();
    let before = satellite.checkpoint();
    let stages = satellite.data.borrow().staging.clone();
    let events = satellite.data.borrow().events.clone();
    let mut snapshot = ValidatedSnapshot {
        org_id: before.org_id.clone(),
        authority: before.highest_epoch,
        position: before.position,
        digest: before.digest,
        codec: Codec::new("conformance", 1).unwrap(),
        staging_id: String::new(),
        protocol_level: 1,
    };
    for staging_id in [String::new(), "A".repeat(65), "invalid-stage".into()] {
        snapshot.staging_id = staging_id;
        let error = install_snapshot(&satellite, snapshot.clone()).unwrap_err();
        assert!(
            matches!(error, SyncError::Refused(Refusal::InvalidInput)),
            "staging_id={:?}: {error}",
            snapshot.staging_id
        );
        assert!(!error.aborts_batch());
        let reopened = satellite.restart();
        assert_eq!(reopened.checkpoint(), before);
        assert!(reopened.applied().is_empty());
        assert_eq!(reopened.data.borrow().staging, stages);
        assert_eq!(reopened.data.borrow().events, events);
    }
    // The maximum accepted length must reach the adapter and install real staged data.
    snapshot.staging_id = "A".repeat(64);
    let payload = Arc::from(b"boundary snapshot contents".as_slice());
    satellite.stage(snapshot.staging_id.clone(), vec![Arc::clone(&payload)]);
    assert_eq!(
        install_snapshot(&satellite, snapshot).unwrap(),
        OrgOutput::SnapshotInstalled
    );
    assert_eq!(satellite.restart().applied(), vec![payload]);
}
