use super::*;
use org_sync_proto::Epoch;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Durable {
    context: LocalContext,
    queue: LocalOutbox,
    counter: u64,
    native_events: Vec<Change>,
    clock: u64,
}
struct Office {
    durable: Durable,
    fault: Option<(&'static str, usize)>,
    commit_failure: bool,
    codec_supported: bool,
}
struct Writer<'a> {
    data: &'a mut Durable,
    fault: Option<(&'static str, usize)>,
    calls: BTreeMap<&'static str, usize>,
    codec_supported: bool,
}
impl Writer<'_> {
    fn port(&mut self, name: &'static str) -> Result<(), StoreError> {
        let count = self.calls.entry(name).or_default();
        *count += 1;
        if self.fault == Some((name, *count)) {
            return Err(StoreError::unavailable("injected port failure"));
        }
        Ok(())
    }
}
impl LocalWorkTxn for Writer<'_> {
    fn context(&mut self) -> Result<LocalContext, StoreError> {
        self.port("context")?;
        Ok(self.data.context.clone())
    }
    fn local_outbox(&mut self) -> Result<LocalOutbox, StoreError> {
        self.port("load")?;
        Ok(self.data.queue.clone())
    }
    fn supports_codec(&mut self, _: &Codec) -> Result<bool, StoreError> {
        self.port("codec")?;
        Ok(self.codec_supported)
    }
    fn save_local(&mut self, queue: &LocalOutbox) -> Result<(), StoreError> {
        self.port("save")?;
        self.data.queue = queue.clone();
        Ok(())
    }
}
impl Office {
    fn new(name: &str) -> Self {
        let origin = Origin {
            office: OfficeId::new(name).unwrap(),
            incarnation: OriginIncarnation::new("INC1").unwrap(),
        };
        Self {
            durable: Durable {
                context: LocalContext {
                    org_id: OrgId::new("ORG").unwrap(),
                    origin: origin.clone(),
                    applied_org: Position {
                        epoch: Epoch::new(1).unwrap(),
                        seq: 4,
                    },
                    protocol_level: 1,
                    restore_pending: false,
                },
                queue: LocalOutbox::new(origin),
                counter: 0,
                native_events: Vec::new(),
                clock: 0,
            },
            fault: None,
            commit_failure: false,
            codec_supported: true,
        }
    }
    fn change(&mut self) -> Change {
        let mut change = Change {
            org_id: self.durable.context.org_id.clone(),
            key: SourceKey {
                origin: self.durable.context.origin.clone(),
                seq: SourceSeq::new(self.durable.counter + 1).unwrap(),
            },
            record_id: "TASK1".into(),
            version: RecordVersion {
                generation: 1,
                version: self.durable.counter + 1,
            },
            digest: Digest::from_bytes([1; 32]),
            record_digest: Digest::from_bytes([2; 32]),
            codec: Codec::new("org.native.changes", 1).unwrap(),
            payload: Arc::from(&b"PRIVATE_NATIVE_EVENT"[..]),
            required_org: self.durable.context.applied_org,
            created_ms: 100,
        };
        seal(&mut change);
        change
    }
    fn append(&mut self, change: Change) -> Result<SourceKey, SyncError> {
        // Mimic the existing native savepoint: domain event/counter precede queue policy.
        // ANY policy/storage/commit error discards all three, including clock state.
        let mut draft = self.durable.clone();
        draft.counter += 1;
        draft.native_events.push(change.clone());
        draft.clock += 1;
        let key = queue_local(
            &mut Writer {
                data: &mut draft,
                fault: self.fault,
                calls: BTreeMap::new(),
                codec_supported: self.codec_supported,
            },
            change,
        )?;
        if self.commit_failure {
            return Err(StoreError::unavailable("COMMIT failed").into());
        }
        self.durable = draft;
        Ok(key)
    }
    fn reopen(&self) -> Self {
        Self {
            durable: self.durable.clone(),
            fault: None,
            commit_failure: false,
            codec_supported: self.codec_supported,
        }
    }
    fn refused(&mut self, change: Change, expected: Refusal) {
        let before = self.durable.clone();
        assert!(
            matches!(self.append(change), Err(SyncError::Refused(actual)) if actual == expected),
            "expected {expected:?}"
        );
        assert_eq!(self.durable, before);
        assert_eq!(self.reopen().durable, before);
    }
}

fn seal(change: &mut Change) {
    change.record_digest = work_record_digest_v1(
        &change.record_id,
        change.version.generation,
        change.version.version,
        &change.codec,
        &change.payload,
    )
    .unwrap();
    change.digest = work_source_digest_v1(
        &change.key.origin.office,
        &change.key.origin.incarnation,
        change.key.seq,
        change.record_digest,
        change.required_org,
        change.created_ms,
    );
}

#[test]
fn frame_derived_payload_boundary_rejects_old_one_mib_limit() {
    assert_eq!(MAX_PAYLOAD_BYTES, MAX_ENCODED_FRAME_BYTES / 2);
    let mut office = Office::new("A");
    let mut change = office.change();
    change.payload = vec![0; MAX_ENCODED_FRAME_BYTES / 2 + 1].into();
    seal(&mut change);
    office.refused(change, Refusal::InvalidInput);
    let mut change = office.change();
    change.payload = vec![0; MAX_ENCODED_FRAME_BYTES / 2].into();
    seal(&mut change);
    let key = office.append(change.clone()).unwrap();
    assert_eq!(key, change.key);
    assert_eq!(office.reopen().durable.queue.pending.front(), Some(&change));
}

#[test]
fn local_quota_refusal_rolls_back_native_write_and_sequence() {
    for byte_quota in [false, true] {
        let mut office = Office::new("A");
        let size = if byte_quota { MAX_PAYLOAD_BYTES } else { 1 };
        let count = if byte_quota {
            MAX_PENDING_BYTES / size
        } else {
            MAX_PENDING
        };
        let mut first = office.change();
        first.payload = vec![0; size].into();
        for seq in 1..=count as u64 {
            let mut change = first.clone();
            change.key.seq = SourceSeq::new(seq).unwrap();
            change.version.version = seq;
            seal(&mut change);
            office.durable.queue.pending.push_back(change);
        }
        office.durable.queue.last_seq = count as u64;
        office.durable.counter = count as u64;
        let next = office.change();
        office.refused(next, Refusal::QuotaExceeded);
        // A second office has its own quota and gap-free counter.
        let mut other = Office::new("B");
        let first = other.change();
        assert_eq!(other.append(first).unwrap().seq.get(), 1);
    }
}

#[test]
fn committed_event_is_immutable_and_next_sequence_survives_reopen() {
    let mut office = Office::new("A");
    let first = office.change();
    let mut live = first.payload.to_vec();
    office.append(first.clone()).unwrap();
    live.fill(b'!');
    let mut office = office.reopen();
    let second = office.change();
    assert_eq!(office.append(second.clone()).unwrap().seq.get(), 2);
    let queue = &office.reopen().durable.queue;
    assert_eq!(queue.origin(), &first.key.origin);
    assert_eq!(queue.last_seq(), 2);
    assert_eq!(queue.acknowledged_seq(), 0);
    assert_eq!(queue.pending(), &VecDeque::from([first.clone(), second]));
    assert_ne!(queue.pending()[0].payload.as_ref(), live);
    assert_eq!(office.durable.native_events[0], first);
    let restored = LocalOutbox::restore(queue.origin.clone(), 2, 0, queue.pending.clone()).unwrap();
    assert_eq!(&restored, queue);
}

#[test]
fn invalid_change_shape_is_refused_before_persistence() {
    for case in 0..7 {
        let mut office = Office::new("A");
        let mut change = office.change();
        match case {
            0 => change.record_id.clear(),
            1 => change.record_id = "a".repeat(129),
            2 => change.record_id = "bad/record".into(),
            3 => change.record_id = "é".into(),
            4 => change.version.generation = 0,
            5 => change.version.version = 0,
            _ => change.payload = Arc::from([]),
        }
        seal(&mut change);
        office.refused(change, Refusal::InvalidInput);
    }
    let mut office = Office::new("A");
    let mut change = office.change();
    change.record_id = format!("{}._-", "a".repeat(125));
    seal(&mut change);
    office.append(change).unwrap();
}

#[test]
fn exact_origin_org_checkpoint_and_contiguous_sequence_are_required() {
    for case in 0..6 {
        let mut office = Office::new("A");
        let mut change = office.change();
        match case {
            0 => change.org_id = OrgId::new("OTHER").unwrap(),
            1 => change.key.origin.office = OfficeId::new("OTHER").unwrap(),
            2 => change.key.origin.incarnation = OriginIncarnation::new("INC2").unwrap(),
            3 => change.key.seq = SourceSeq::new(2).unwrap(),
            4 => change.required_org.epoch = Epoch::new(2).unwrap(),
            _ => change.required_org.seq += 1,
        }
        seal(&mut change);
        office.refused(change, Refusal::InvalidInput);
    }
    let mut office = Office::new("A");
    let first = office.change();
    office.append(first.clone()).unwrap();
    office.refused(first, Refusal::InvalidInput);
}

#[test]
fn applied_org_may_be_ahead_of_a_change_at_the_same_epoch() {
    for required in [0, 3, 4] {
        let mut office = Office::new("A");
        let mut change = office.change();
        change.required_org.seq = required;
        seal(&mut change);
        assert_eq!(office.append(change.clone()).unwrap(), change.key);
        assert_eq!(office.reopen().durable.queue.pending.front(), Some(&change));
    }
}

#[test]
fn stale_required_org_epoch_is_refused_after_restore_with_state_unchanged() {
    let mut office = Office::new("A");
    office.durable.context.applied_org.epoch = Epoch::new(2).unwrap();
    let mut change = office.change();
    change.required_org = Position {
        epoch: Epoch::new(1).unwrap(),
        seq: 0,
    };
    seal(&mut change);
    office.refused(change, Refusal::InvalidInput);
}

#[test]
fn native_allocation_rolls_back_on_failure_and_retries_with_the_same_source_key() {
    let mut office = Office::new("A");
    let first = office.change();
    office.append(first).unwrap();
    let mut second = office.change();
    second.payload = Arc::from(&b"tampered native event"[..]);
    office.refused(second, Refusal::IntegrityFailure);
    let second = office.change();
    assert_eq!(second.key.seq.get(), 2);
    assert_eq!(office.append(second.clone()).unwrap(), second.key);
    assert_eq!(office.reopen().durable.counter, 2);
}

#[test]
fn restore_and_protocol_guards_leave_native_state_unchanged() {
    for case in 0..6 {
        let mut office = Office::new("A");
        let mut change = office.change();
        let expected = match case {
            0 => {
                office.durable.context.restore_pending = true;
                Refusal::RestorePending
            }
            1 => {
                office.durable.context.protocol_level = 0;
                Refusal::UnsupportedVersion
            }
            2 => {
                office.durable.context.protocol_level = 99;
                Refusal::UnsupportedVersion
            }
            3 => {
                change.codec = Codec::new("foreign", 1).unwrap();
                Refusal::UnsupportedVersion
            }
            4 => {
                office.durable.context.protocol_level = 2;
                change.codec = Codec::new("org.native.changes", 2).unwrap();
                Refusal::UnsupportedVersion
            }
            _ => {
                office.codec_supported = false;
                Refusal::UnsupportedVersion
            }
        };
        seal(&mut change);
        office.refused(change, expected);
    }
    let mut office = Office::new("A");
    office.durable.context.protocol_level = 2;
    let change = office.change();
    office.append(change).unwrap();
}

#[test]
fn unsupported_protocol_and_restore_fence_precede_queue_interpretation() {
    for level in [0, 99, 1] {
        let mut office = Office::new("A");
        let change = office.change();
        office.durable.context.protocol_level = level;
        office.durable.context.restore_pending = level == 1;
        office.fault = Some(("load", 1));
        office.refused(
            change,
            if level == 1 {
                Refusal::RestorePending
            } else {
                Refusal::UnsupportedVersion
            },
        );
    }
}

#[test]
fn exact_count_and_byte_quota_can_be_committed_and_reopened() {
    for byte_quota in [false, true] {
        let mut office = Office::new("A");
        let count = if byte_quota {
            MAX_PENDING_BYTES / MAX_PAYLOAD_BYTES
        } else {
            MAX_PENDING - 1
        };
        for seq in 1..=count as u64 {
            let mut change = office.change();
            change.key.seq = SourceSeq::new(seq).unwrap();
            change.version.version = seq;
            let size = if byte_quota {
                MAX_PAYLOAD_BYTES - usize::from(seq == count as u64)
            } else {
                1
            };
            change.payload = vec![0; size].into();
            seal(&mut change);
            office.durable.counter = seq;
            office.durable.queue.last_seq = seq;
            office.durable.queue.pending.push_back(change);
        }
        let mut last = office.change();
        last.payload = Arc::from([1]);
        seal(&mut last);
        office.append(last).unwrap();
        let mut office = office.reopen();
        let queue = &office.durable.queue;
        if byte_quota {
            assert_eq!(queue.bytes(), MAX_PENDING_BYTES);
        } else {
            assert_eq!(queue.pending.len(), MAX_PENDING);
        }
        assert_eq!(
            LocalOutbox::restore(
                queue.origin.clone(),
                queue.last_seq,
                0,
                queue.pending.clone()
            )
            .unwrap(),
            *queue
        );
        let next = office.change();
        office.refused(next, Refusal::QuotaExceeded);
    }
}

#[test]
fn source_and_record_integrity_are_verified_before_save() {
    for case in 0..8 {
        let mut office = Office::new("A");
        let mut change = office.change();
        match case {
            0 => change.digest = Digest::ZERO,
            1 => change.record_digest = Digest::ZERO,
            2 => change.payload = Arc::from(&b"tampered"[..]),
            3 => change.record_id = "other".into(),
            4 => change.version.generation += 1,
            5 => change.version.version += 1,
            6 => change.codec = Codec::new("org.native.snapshot", 1).unwrap(),
            _ => change.created_ms += 1,
        }
        office.refused(change, Refusal::IntegrityFailure);
    }
}

#[test]
fn malformed_retained_queue_cannot_be_loaded_or_written() {
    for case in 0..13 {
        let mut office = Office::new("A");
        let first = office.change();
        office.append(first).unwrap();
        let next = office.change();
        let queue = &mut office.durable.queue;
        let expected = match case {
            0 => {
                queue.origin.office = OfficeId::new("OTHER").unwrap();
                Refusal::RecoveryRequired
            }
            1 => {
                queue.origin.incarnation = OriginIncarnation::new("INC2").unwrap();
                Refusal::RecoveryRequired
            }
            2 => {
                queue.acknowledged_seq = 2;
                Refusal::InvariantFailure
            }
            3 => {
                queue.last_seq = 2;
                Refusal::InvariantFailure
            }
            4 => {
                queue.pending[0].key.seq = SourceSeq::new(2).unwrap();
                Refusal::InvariantFailure
            }
            5 => {
                queue.pending[0].key.origin.office = OfficeId::new("OTHER").unwrap();
                Refusal::InvariantFailure
            }
            6 => {
                queue.pending[0].payload = Arc::from([]);
                Refusal::InvariantFailure
            }
            7 => {
                queue.pending[0].org_id = OrgId::new("OTHER").unwrap();
                Refusal::InvariantFailure
            }
            8 => {
                queue.pending[0].payload = Arc::from(&b"changed"[..]);
                Refusal::IntegrityFailure
            }
            9 => {
                queue.pending.clear();
                Refusal::InvariantFailure
            }
            10 => {
                queue.pending[0].record_digest = Digest::ZERO;
                Refusal::IntegrityFailure
            }
            11 => {
                queue.pending[0].digest = Digest::ZERO;
                Refusal::IntegrityFailure
            }
            _ => {
                queue.pending[0].payload = vec![0; MAX_PAYLOAD_BYTES + 1].into();
                Refusal::InvariantFailure
            }
        };
        // Keep the queue bound to the altered origin in cases 0/1 so shape validation
        // succeeds and only the context binding can reject it.
        if case < 2 {
            queue.pending[0].key.origin = queue.origin.clone();
        }
        office.refused(next, expected);
    }
}

#[test]
fn corrupt_retained_state_is_rejected_before_fresh_input_is_interpreted() {
    for case in 0..3 {
        let mut office = Office::new("A");
        let first = office.change();
        office.append(first).unwrap();
        let mut next = office.change();
        next.payload = Arc::from([]);
        seal(&mut next);
        let queued = &mut office.durable.queue.pending[0];
        let expected = match case {
            0 => {
                queued.version.generation = 0;
                seal(queued);
                Refusal::InvariantFailure
            }
            1 => {
                queued.org_id = OrgId::new("OTHER").unwrap();
                Refusal::InvariantFailure
            }
            _ => {
                queued.digest = Digest::ZERO;
                Refusal::IntegrityFailure
            }
        };
        office.refused(next, expected);
    }
}

#[test]
fn authentic_retained_gap_is_refused_but_acknowledged_prefix_is_valid() {
    for acknowledged in [0, 1] {
        let mut office = Office::new("A");
        let mut retained = office.change();
        retained.key.seq = SourceSeq::new(2).unwrap();
        retained.version.version = 2;
        seal(&mut retained);
        office.durable.counter = 2;
        office.durable.native_events.push(retained.clone());
        office.durable.queue.last_seq = 2;
        office.durable.queue.acknowledged_seq = acknowledged;
        office.durable.queue.pending.push_back(retained);
        let next = office.change();
        if acknowledged == 0 {
            office.refused(next, Refusal::InvariantFailure);
        } else {
            assert_eq!(office.append(next).unwrap().seq.get(), 3);
            assert_eq!(office.reopen().durable.queue.acknowledged_seq(), 1);
            assert_eq!(office.durable.queue.pending.len(), 2);
        }
    }
}

#[test]
fn store_and_commit_failures_roll_back_and_retry_without_a_gap() {
    for (port, nth) in [
        ("context", 1),
        ("load", 1),
        ("codec", 1),
        ("save", 1),
        ("commit", 1),
    ] {
        let mut office = Office::new("A");
        let first = office.change();
        office.append(first).unwrap();
        let second = office.change();
        let before = office.durable.clone();
        office.fault = Some((port, nth));
        office.commit_failure = port == "commit";
        let error = office.append(second.clone()).unwrap_err();
        assert!(matches!(error, SyncError::Storage(_)));
        assert_eq!(office.durable, before);
        let mut office = office.reopen();
        assert_eq!(office.append(second).unwrap().seq.get(), 2);
        assert_eq!(office.durable.counter, 2);
        assert_eq!(office.durable.clock, 2);
        assert_eq!(office.durable.native_events.len(), 2);
        assert_eq!(office.reopen().durable.queue.pending.len(), 2);
    }
}

#[test]
fn persisted_boundaries_and_sequence_overflow_are_checked() {
    let office = Office::new("A");
    let origin = office.durable.context.origin;
    assert!(LocalOutbox::restore(origin.clone(), 0, 1, VecDeque::new()).is_err());
    assert!(LocalOutbox::restore(origin.clone(), 1, 0, VecDeque::new()).is_err());
    let queue = LocalOutbox::restore(origin, u64::MAX, u64::MAX, VecDeque::new()).unwrap();
    let mut office = Office::new("A");
    let change = office.change();
    office.durable.queue = queue;
    office.refused(change, Refusal::InvalidInput);
}

#[test]
fn retained_queue_over_count_or_byte_limit_is_an_invariant_failure() {
    for byte_quota in [false, true] {
        let mut office = Office::new("A");
        let first = office.change();
        let count = if byte_quota {
            MAX_PENDING_BYTES / MAX_PAYLOAD_BYTES + 1
        } else {
            MAX_PENDING + 1
        };
        for seq in 1..=count as u64 {
            let mut change = first.clone();
            change.key.seq = SourceSeq::new(seq).unwrap();
            if byte_quota {
                change.payload = vec![0; MAX_PAYLOAD_BYTES].into();
            }
            seal(&mut change);
            office.durable.queue.pending.push_back(change);
        }
        office.durable.queue.last_seq = count as u64;
        office.durable.counter = count as u64;
        let change = office.change();
        office.refused(change, Refusal::InvariantFailure);
        assert!(matches!(
            LocalOutbox::restore(
                office.durable.queue.origin.clone(),
                count as u64,
                0,
                office.durable.queue.pending.clone()
            ),
            Err(Refusal::InvariantFailure)
        ));
    }
}

#[test]
fn debug_never_contains_native_payload_or_record_identifier() {
    let mut office = Office::new("A");
    let change = office.change();
    assert!(!format!("{change:?}").contains("PRIVATE_NATIVE_EVENT"));
    assert!(!format!("{change:?}").contains("TASK1"));
    let body_debug = format!("{:?}", change.payload);
    assert!(!format!("{change:?}").contains(&body_debug));
    office.append(change).unwrap();
    assert!(!format!("{:?}", office.durable.queue).contains("PRIVATE_NATIVE_EVENT"));
    assert!(!format!("{:?}", office.durable.queue).contains(&body_debug));
}

impl crate::testkit::work_conformance::LocalWorkHarness for Office {
    type Snapshot = Durable;
    fn snapshot(&self) -> Durable {
        self.durable.clone()
    }
    fn queue(&self) -> LocalOutbox {
        self.durable.queue.clone()
    }
    fn source_counter(&self) -> u64 {
        self.durable.counter
    }
    fn native_version_count(&self) -> usize {
        self.durable.native_events.len()
    }
    fn publish_fixture(&mut self, tag: u8) -> Result<SourceKey, SyncError> {
        let mut change = self.change();
        change.payload = format!("PRIVATE_NATIVE_EVENT{tag}").into_bytes().into();
        seal(&mut change);
        self.append(change)
    }
    fn fail_next_commit(&mut self) {
        self.commit_failure = true;
    }
    fn set_restore_pending(&mut self, pending: bool) {
        self.durable.context.restore_pending = pending;
    }
    fn exhaust_pending_quota(&mut self) {
        while self.durable.queue.pending.len() < MAX_PENDING {
            let change = self.change();
            self.durable.counter = change.key.seq.get();
            self.durable.queue.last_seq = change.key.seq.get();
            self.durable.native_events.push(change.clone());
            self.durable.queue.pending.push_back(change);
            self.durable.clock += 1;
        }
    }
    fn reopen(&self) -> Self {
        Office::reopen(self)
    }
}

#[test]
fn local_work_adapter_conformance_covers_atomicity_reopen_quota_and_isolation() {
    crate::testkit::work_conformance::local_work_adapter(
        &mut Office::new("A"),
        &mut Office::new("B"),
    );
}
