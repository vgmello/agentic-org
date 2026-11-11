use super::*;
use org_sync_proto::WorkRejection;
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Data {
    context: LocalContext,
    queue: LocalOutbox,
    outcomes: BTreeMap<SourceKey, RetainedOutcome>,
    proof: Option<AckProof>,
    native_events: Vec<Change>,
}
struct Memory {
    data: Mutex<Data>,
    authority: StreamAuthority,
    fault: Option<&'static str>,
    codec: bool,
    rows: Option<Vec<RetainedOutcome>>,
    point: Option<Option<RetainedOutcome>>,
}
struct Tx<'a> {
    data: &'a mut Data,
    store: &'a Memory,
}
impl Tx<'_> {
    fn port(&self, name: &str) -> Result<(), StoreError> {
        if self.store.fault == Some(name) {
            Err(StoreError::unavailable("injected native failure"))
        } else {
            Ok(())
        }
    }
}
impl LocalWorkTxn for Tx<'_> {
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
        Ok(self.store.codec)
    }
    fn save_local(&mut self, _: &LocalOutbox) -> Result<(), StoreError> {
        panic!("receipt cannot invoke the native append writer")
    }
}
impl LocalStreamTxn for Tx<'_> {
    fn authority(&mut self) -> Result<StreamAuthority, StoreError> {
        self.port("authority")?;
        Ok(self.store.authority.clone())
    }
    fn outcome(&mut self, key: &SourceKey) -> Result<Option<RetainedOutcome>, StoreError> {
        self.port("outcome")?;
        Ok(self
            .store
            .point
            .clone()
            .unwrap_or_else(|| self.data.outcomes.get(key).cloned()))
    }
    fn retained_rejections(
        &mut self,
        origin: &Origin,
        from: u64,
        through: u64,
    ) -> Result<Vec<RetainedOutcome>, StoreError> {
        self.port("retained")?;
        Ok(self.store.rows.clone().unwrap_or_else(|| {
            self.data
                .outcomes
                .values()
                .filter(|r| {
                    r.key.origin == *origin
                        && r.key.seq.get() > from
                        && r.key.seq.get() <= through
                        && matches!(r.outcome, WorkOutcome::Rejected { .. })
                })
                .take(WORK_CREDIT_WINDOW + 1)
                .cloned()
                .collect()
        }))
    }
    fn save_ack(
        &mut self,
        queue: &LocalOutbox,
        outcomes: &[RetainedOutcome],
        proof: &AckProof,
    ) -> Result<(), StoreError> {
        self.port("save")?;
        self.data.queue = queue.clone();
        self.port("after_queue")?;
        for row in outcomes {
            assert!(
                self.data
                    .outcomes
                    .insert(row.key.clone(), row.clone())
                    .is_none(),
                "outcomes are immutable"
            );
        }
        self.port("after_outcomes")?;
        self.data.proof = Some(proof.clone());
        self.port("after_proof")
    }
}
impl LocalStreamStore for Memory {
    fn transact(&self, attempt: Arc<LocalStreamAttempt>) -> Result<LocalStreamOutput, SyncError> {
        let mut committed = self.data.lock().unwrap();
        let mut draft = committed.clone();
        let output = attempt(&mut Tx {
            data: &mut draft,
            store: self,
        })?;
        if self.fault == Some("commit") {
            return Err(StoreError::unavailable("COMMIT failed").into());
        }
        *committed = draft;
        Ok(output)
    }
}
#[derive(Clone)]
struct Peer(PeerIdentity, u64);
impl VerifiedPeer for Peer {
    fn identity(&self) -> PeerIdentity {
        self.0.clone()
    }
}
impl WorkStreamPeer for Peer {
    fn connection_generation(&self) -> u64 {
        self.1
    }
}
impl Memory {
    fn new(office: &str, count: u64) -> Self {
        let origin = Origin {
            office: OfficeId::new(office).unwrap(),
            incarnation: OriginIncarnation::new("INC1").unwrap(),
        };
        let context = LocalContext {
            org_id: OrgId::new("ORG").unwrap(),
            origin: origin.clone(),
            applied_org: Position {
                epoch: Epoch::new(2).unwrap(),
                seq: 10,
            },
            protocol_level: 1,
            restore_pending: false,
        };
        let changes: Vec<_> = (1..=count)
            .map(|seq| {
                let codec = Codec::new("org.native.changes", 1).unwrap();
                let payload: Arc<[u8]> = format!("immutable-{seq}").into_bytes().into();
                let record_digest =
                    work_record_digest_v1("TASK1", 1, seq, &codec, &payload).unwrap();
                Change {
                    org_id: context.org_id.clone(),
                    key: SourceKey {
                        origin: origin.clone(),
                        seq: SourceSeq::new(seq).unwrap(),
                    },
                    record_id: "TASK1".into(),
                    version: RecordVersion {
                        generation: 1,
                        version: seq,
                    },
                    digest: work_source_digest_v1(
                        &origin.office,
                        &origin.incarnation,
                        SourceSeq::new(seq).unwrap(),
                        record_digest,
                        context.applied_org,
                        100,
                    ),
                    record_digest,
                    codec,
                    payload,
                    required_org: context.applied_org,
                    created_ms: 100,
                }
            })
            .collect();
        let queue = LocalOutbox::restore(origin, count, 0, changes.clone().into()).unwrap();
        Self {
            data: Mutex::new(Data {
                context,
                queue,
                outcomes: BTreeMap::new(),
                proof: None,
                native_events: changes,
            }),
            authority: StreamAuthority {
                hq: OfficeId::new("HQ").unwrap(),
                highest_epoch: Epoch::new(2).unwrap(),
                connection_generation: Some(7),
                pruned_through: 0,
            },
            fault: None,
            codec: true,
            rows: None,
            point: None,
        }
    }
    fn snapshot(&self) -> Data {
        self.data.lock().unwrap().clone()
    }
    fn peer(&self) -> Peer {
        Peer(
            PeerIdentity {
                org_id: self.snapshot().context.org_id,
                office_id: self.authority.hq.clone(),
                role: PeerRole::Hq,
                authority: self.authority.highest_epoch,
            },
            self.authority.connection_generation.unwrap_or(7),
        )
    }
    fn ack(&self, from: u64, through: u64, rejected: &[u64]) -> WorkAck {
        let origin = self.snapshot().context.origin;
        WorkAck {
            origin_office_id: origin.office,
            incarnation: origin.incarnation,
            from_exclusive: from,
            through_seq: through,
            rejected: rejected
                .iter()
                .map(|&seq| WorkRejection {
                    seq,
                    code: "not_owner".into(),
                })
                .collect(),
        }
    }
    fn apply(&self, ack: WorkAck) -> Result<LocalStreamOutput, SyncError> {
        acknowledge_local(self, &self.peer(), ack)
    }
    fn refused(&self, peer: &Peer, ack: WorkAck, expected: Refusal) {
        let before = self.snapshot();
        assert!(
            matches!(acknowledge_local(self, peer, ack), Err(SyncError::Refused(actual)) if actual == expected),
            "expected {expected:?}"
        );
        assert_eq!(self.snapshot(), before);
        assert_eq!(self.reopen().snapshot(), before);
    }
    fn reopen(&self) -> Self {
        Self {
            data: Mutex::new(self.snapshot()),
            authority: StreamAuthority {
                connection_generation: None,
                ..self.authority.clone()
            },
            fault: None,
            codec: self.codec,
            rows: None,
            point: None,
        }
    }
    fn prune(&mut self, through: u64) {
        self.authority.pruned_through = through;
        self.data
            .lock()
            .unwrap()
            .outcomes
            .retain(|key, _| key.seq.get() > through);
    }
}

#[test]
fn current_verified_hq_identity_epoch_and_connection_are_independent_guards() {
    for case in 0..8 {
        let mut store = Memory::new("A", 2);
        let mut peer = store.peer();
        match case {
            0 => peer.0.org_id = OrgId::new("FOREIGN").unwrap(),
            1 => peer.0.office_id = OfficeId::new("OLDHQ").unwrap(),
            2 => peer.0.role = PeerRole::Satellite,
            3 => peer.0.authority = Epoch::new(1).unwrap(),
            4 => peer.0.authority = Epoch::new(3).unwrap(),
            5 => peer.1 = 6,
            6 => peer.1 = 8,
            _ => store.authority.connection_generation = None,
        }
        store.refused(&peer, store.ack(0, 1, &[]), Refusal::NotAuthorized);
    }
    let store = Memory::new("A", 2);
    assert_eq!(
        store.apply(store.ack(0, 1, &[])).unwrap(),
        LocalStreamOutput::Advanced { from: 0, to: 1 }
    );
}
#[test]
fn wrong_office_and_incarnation_each_refuse_without_mutation() {
    for case in 0..2 {
        let store = Memory::new("A", 2);
        let mut ack = store.ack(0, 1, &[]);
        if case == 0 {
            ack.origin_office_id = OfficeId::new("B").unwrap();
        } else {
            ack.incarnation = OriginIncarnation::new("INC2").unwrap();
        }
        store.refused(&store.peer(), ack, Refusal::WrongStream);
    }
}
#[test]
fn refusal_precedence_matches_frozen_contract() {
    let store = Memory::new("A", 2);
    let mut ack = store.ack(5, 4, &[]); // invalid shape and both bounds
    ack.incarnation = OriginIncarnation::new("BAD").unwrap();
    let mut peer = store.peer();
    peer.1 += 1;
    store.refused(&peer, ack.clone(), Refusal::NotAuthorized);
    store.refused(&store.peer(), ack, Refusal::WrongStream);
    let mut store = Memory::new("A", 2);
    store.data.lock().unwrap().context.restore_pending = true;
    store.refused(&store.peer(), store.ack(5, 4, &[]), Refusal::RestorePending);
    store.data.lock().unwrap().context.restore_pending = false;
    store.data.lock().unwrap().context.protocol_level = u32::MAX;
    store.refused(
        &store.peer(),
        store.ack(5, 4, &[]),
        Refusal::UnsupportedVersion,
    );
    store.data.lock().unwrap().context.protocol_level = 1;
    store.refused(&store.peer(), store.ack(5, 4, &[]), Refusal::Malformed);
    store.refused(
        &store.peer(),
        store.ack(3, 4, &[]),
        Refusal::AheadOfForwarded,
    );
    store.refused(&store.peer(), store.ack(1, 2, &[]), Refusal::Gap);
    store.authority.pruned_through = 1;
    store.refused(
        &store.peer(),
        store.ack(0, 1, &[]),
        Refusal::InvariantFailure,
    );
}
#[test]
fn malformed_ack_never_reaches_native_save() {
    let store = Memory::new("A", 40);
    for case in 0..9 {
        let mut ack = store.ack(0, 3, &[2]);
        match case {
            0 => ack.from_exclusive = 3,
            1 => ack.from_exclusive = 4,
            2 => ack.rejected[0].seq = 0,
            3 => ack.rejected[0].seq = 4,
            4 => ack.rejected.push(ack.rejected[0].clone()),
            5 => ack.rejected[0].code.clear(),
            6 => ack.rejected[0].code = "x".repeat(65),
            7 => ack.rejected[0].code = "X".into(),
            _ => {
                ack.through_seq = 33;
                ack.rejected = (1..=33)
                    .map(|seq| WorkRejection {
                        seq,
                        code: "x".into(),
                    })
                    .collect();
            }
        }
        store.refused(&store.peer(), ack, Refusal::Malformed);
    }
}
#[test]
fn lower_equal_higher_base_floor_and_committed_bounds() {
    let store = Memory::new("A", 5);
    store.apply(store.ack(0, 2, &[])).unwrap();
    store.refused(&store.peer(), store.ack(3, 4, &[]), Refusal::Gap);
    store.refused(
        &store.peer(),
        store.ack(2, 6, &[]),
        Refusal::AheadOfForwarded,
    );
    assert_eq!(
        store.apply(store.ack(1, 2, &[])).unwrap(),
        LocalStreamOutput::Duplicate
    );
    assert_eq!(
        store.apply(store.ack(0, 1, &[])).unwrap(),
        LocalStreamOutput::Duplicate
    );
    assert_eq!(
        store.apply(store.ack(2, 3, &[])).unwrap(),
        LocalStreamOutput::Advanced { from: 2, to: 3 }
    );
    assert_eq!(
        store.apply(store.ack(2, 5, &[])).unwrap(),
        LocalStreamOutput::Advanced { from: 3, to: 5 }
    );
    assert!(store.snapshot().queue.pending().is_empty());
}
#[test]
fn advance_commits_receipt_prefix_quota_and_proof_without_changing_native_versions() {
    let store = Memory::new("A", 5);
    let before = store.snapshot();
    let ack = store.ack(0, 3, &[2]);
    assert_eq!(
        store.apply(ack.clone()).unwrap(),
        LocalStreamOutput::Advanced { from: 0, to: 3 }
    );
    let after = store.snapshot();
    assert_eq!(after.native_events, before.native_events);
    assert_eq!(after.queue.last_seq(), 5);
    assert_eq!(after.queue.acknowledged_seq(), 3);
    assert_eq!(
        after
            .queue
            .pending()
            .iter()
            .map(|c| c.key.seq.get())
            .collect::<Vec<_>>(),
        [4, 5]
    );
    assert!(after.queue.bytes() < before.queue.bytes());
    assert_eq!(after.outcomes.len(), 3);
    for (key, row) in &after.outcomes {
        assert_eq!(
            row.outcome,
            if key.seq.get() == 2 {
                WorkOutcome::Rejected {
                    code: "not_owner".into(),
                }
            } else {
                WorkOutcome::Accepted
            }
        );
    }
    let proof = after.proof.unwrap();
    assert_eq!(proof.origin, before.context.origin);
    assert_eq!((proof.from_exclusive, proof.through_seq), (0, 3));
    assert_eq!(proof.digest, work_ack_proof_digest_v1(&ack).unwrap());
    let mut reopened = store.reopen();
    assert_eq!(reopened.snapshot(), store.snapshot());
    reopened.refused(&store.peer(), ack, Refusal::NotAuthorized);
    reopened.authority.connection_generation = Some(8);
    assert_eq!(
        reopened.apply(reopened.ack(3, 5, &[])).unwrap(),
        LocalStreamOutput::Advanced { from: 3, to: 5 }
    );
}
#[test]
fn out_of_order_ack_preserves_pending_head_and_prefix() {
    let store = Memory::new("A", 8);
    let original = store.ack(0, 3, &[2]);
    store.apply(original.clone()).unwrap();
    store.apply(store.ack(3, 6, &[5])).unwrap();
    let before = store.snapshot();
    assert_eq!(store.apply(original).unwrap(), LocalStreamOutput::Duplicate);
    assert_eq!(
        store.apply(store.ack(1, 3, &[2])).unwrap(),
        LocalStreamOutput::Duplicate
    );
    assert_eq!(store.snapshot(), before);
    assert_eq!(before.queue.pending().front().unwrap().key.seq.get(), 7);
}
#[test]
fn contradictions_in_retained_overlap_are_detected_in_both_directions_and_code() {
    let store = Memory::new("A", 6);
    store.apply(store.ack(0, 3, &[2])).unwrap();
    for ack in [
        store.ack(0, 3, &[]),
        store.ack(0, 3, &[1, 2]),
        store.ack(0, 3, &[2, 3]),
        store.ack(0, 5, &[]),
    ] {
        store.refused(&store.peer(), ack, Refusal::ConflictingProof);
    }
    let mut changed = store.ack(0, 3, &[2]);
    changed.rejected[0].code = "other".into();
    store.refused(&store.peer(), changed, Refusal::ConflictingProof);
    assert_eq!(
        store.apply(store.ack(2, 5, &[])).unwrap(),
        LocalStreamOutput::Advanced { from: 3, to: 5 }
    );
}
#[test]
fn ack_base_limits_comparison_and_pruned_horizon_is_inclusive() {
    let mut store = Memory::new("A", 110);
    store.apply(store.ack(0, 90, &[80])).unwrap();
    store.apply(store.ack(90, 100, &[])).unwrap();
    assert_eq!(
        store.apply(store.ack(90, 100, &[])).unwrap(),
        LocalStreamOutput::Duplicate
    );
    store.prune(90);
    assert_eq!(
        store.apply(store.ack(0, 90, &[])).unwrap(),
        LocalStreamOutput::Duplicate
    );
    assert_eq!(
        store.apply(store.ack(0, 100, &[])).unwrap(),
        LocalStreamOutput::Duplicate
    );
    store.apply(store.ack(100, 105, &[101])).unwrap();
    store.prune(100);
    store.refused(
        &store.peer(),
        store.ack(90, 105, &[]),
        Refusal::ConflictingProof,
    );
    store.prune(101);
    assert_eq!(
        store.apply(store.ack(90, 105, &[])).unwrap(),
        LocalStreamOutput::Duplicate
    );
}
#[test]
fn maximum_pruned_replay_is_bounded_and_never_reenqueues() {
    let mut store = Memory::new("A", 0);
    let origin = store.snapshot().context.origin;
    store.data.lock().unwrap().queue =
        LocalOutbox::restore(origin, u64::MAX, u64::MAX, VecDeque::new()).unwrap();
    store.prune(u64::MAX);
    let before = store.snapshot();
    assert_eq!(
        store.apply(store.ack(0, u64::MAX, &[1])).unwrap(),
        LocalStreamOutput::Duplicate
    );
    assert_eq!(store.snapshot(), before);
}
#[test]
fn every_partial_write_and_commit_error_rolls_back_and_retry_advances_once() {
    for fault in [
        "context",
        "authority",
        "load",
        "save",
        "after_queue",
        "after_outcomes",
        "after_proof",
        "commit",
    ] {
        let mut store = Memory::new("A", 4);
        let ack = store.ack(0, 3, &[2]);
        let before = store.snapshot();
        store.fault = Some(fault);
        assert!(
            matches!(store.apply(ack.clone()), Err(SyncError::Storage(_))),
            "{fault}"
        );
        assert_eq!(store.snapshot(), before, "{fault}");
        let mut reopened = store.reopen();
        reopened.authority.connection_generation = Some(8);
        assert_eq!(
            reopened.apply(ack.clone()).unwrap(),
            LocalStreamOutput::Advanced { from: 0, to: 3 }
        );
        let after = reopened.snapshot();
        assert_eq!(reopened.apply(ack).unwrap(), LocalStreamOutput::Duplicate);
        assert_eq!(reopened.snapshot(), after);
    }
}
#[test]
fn retained_query_and_point_errors_abort_without_reclassifying_or_changes() {
    for fault in ["retained", "outcome"] {
        let mut store = Memory::new("A", 3);
        let ack = store.ack(0, 2, &[1]);
        store.apply(ack.clone()).unwrap();
        let before = store.snapshot();
        store.fault = Some(fault);
        assert!(matches!(store.apply(ack), Err(SyncError::Storage(_))));
        assert_eq!(store.snapshot(), before);
    }
}
#[test]
fn two_offices_and_two_parallel_receipts_remain_isolated_and_advance_once() {
    let a = Memory::new("A", 3);
    let b = Memory::new("B", 3);
    let before_b = b.snapshot();
    let ack = a.ack(0, 2, &[1]);
    b.refused(&b.peer(), ack.clone(), Refusal::WrongStream);
    let outputs = std::thread::scope(|scope| {
        let first = scope.spawn(|| a.apply(ack.clone()).unwrap());
        let second = scope.spawn(|| a.apply(ack.clone()).unwrap());
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(
        outputs
            .iter()
            .filter(|o| matches!(o, LocalStreamOutput::Advanced { .. }))
            .count(),
        1
    );
    assert_eq!(
        outputs
            .iter()
            .filter(|o| **o == LocalStreamOutput::Duplicate)
            .count(),
        1
    );
    assert_eq!(a.snapshot().outcomes.len(), 2);
    assert_eq!(b.snapshot(), before_b);
}
#[test]
fn malformed_native_retained_rows_fail_closed() {
    for case in 0..9 {
        let mut store = Memory::new("A", 4);
        store.apply(store.ack(0, 3, &[2])).unwrap();
        let original = store
            .snapshot()
            .outcomes
            .values()
            .find(|r| r.key.seq.get() == 2)
            .unwrap()
            .clone();
        let mut rows = vec![original.clone()];
        match case {
            0 => rows[0].outcome = WorkOutcome::Accepted,
            1 => rows[0].key.origin.office = OfficeId::new("B").unwrap(),
            2 => rows[0].key.seq = SourceSeq::new(1).unwrap(),
            3 => rows[0].key.seq = SourceSeq::new(4).unwrap(),
            4 => rows.push(original.clone()),
            5 => rows[0].outcome = WorkOutcome::Rejected { code: "BAD".into() },
            6 => rows = vec![original.clone(); 34],
            7 => store.point = Some(None),
            _ => {
                let mut wrong = original;
                wrong.key.seq = SourceSeq::new(3).unwrap();
                store.point = Some(Some(wrong));
            }
        }
        store.rows = Some(rows);
        store.refused(
            &store.peer(),
            store.ack(1, 3, &[2]),
            Refusal::InvariantFailure,
        );
    }
}

#[test]
fn entirely_pruned_replays_need_no_outcome_query_or_save() {
    let mut store = Memory::new("A", 3);
    store.apply(store.ack(0, 2, &[2])).unwrap();
    store.prune(2);
    let before = store.snapshot();
    store.fault = Some("retained");
    assert_eq!(
        store.apply(store.ack(0, 2, &[2])).unwrap(),
        LocalStreamOutput::Duplicate
    );
    assert_eq!(
        store.apply(store.ack(0, 1, &[1])).unwrap(),
        LocalStreamOutput::Duplicate
    );
    assert_eq!(store.snapshot(), before);
}
#[test]
fn overlap_filters_out_pruned_and_new_rejections_but_checks_retained_upper_boundary() {
    let mut store = Memory::new("A", 8);
    store.apply(store.ack(0, 3, &[1, 3])).unwrap();
    store.prune(2);
    assert_eq!(
        store.apply(store.ack(0, 5, &[1, 2, 3, 4])).unwrap(),
        LocalStreamOutput::Advanced { from: 3, to: 5 }
    );
    store.refused(
        &store.peer(),
        store.ack(0, 5, &[1, 3]),
        Refusal::ConflictingProof,
    );
    assert_eq!(
        store.apply(store.ack(4, 6, &[])).unwrap(),
        LocalStreamOutput::Advanced { from: 5, to: 6 }
    );
}
#[test]
fn native_rejection_limit_and_point_value_are_independent_invariants() {
    let mut store = Memory::new("A", 40);
    store.apply(store.ack(0, 40, &[])).unwrap();
    let origin = store.snapshot().context.origin;
    store.rows = Some(
        (1..=34)
            .map(|seq| RetainedOutcome {
                key: SourceKey {
                    origin: origin.clone(),
                    seq: SourceSeq::new(seq).unwrap(),
                },
                outcome: WorkOutcome::Rejected { code: "x".into() },
            })
            .collect(),
    );
    store.refused(
        &store.peer(),
        store.ack(0, 40, &[]),
        Refusal::InvariantFailure,
    );
    let mut store = Memory::new("A", 3);
    store.apply(store.ack(0, 2, &[1])).unwrap();
    store.rows = Some(vec![]);
    let key = store.snapshot().outcomes.keys().next().unwrap().clone();
    store.point = Some(Some(RetainedOutcome {
        key,
        outcome: WorkOutcome::Rejected {
            code: "other".into(),
        },
    }));
    store.refused(
        &store.peer(),
        store.ack(0, 2, &[1]),
        Refusal::ConflictingProof,
    );
}

#[test]
fn retained_query_code_must_match_receipt_even_if_point_value_matches() {
    let mut store = Memory::new("A", 3);
    store.apply(store.ack(0, 2, &[1])).unwrap();
    let mut row = store.snapshot().outcomes.values().next().unwrap().clone();
    row.outcome = WorkOutcome::Rejected {
        code: "other".into(),
    };
    store.rows = Some(vec![row]);
    store.refused(
        &store.peer(),
        store.ack(0, 2, &[1]),
        Refusal::ConflictingProof,
    );
}
#[test]
fn thirty_three_retained_rejections_are_a_conflict_not_a_query_limit_violation() {
    let mut store = Memory::new("A", 34);
    store.apply(store.ack(0, 34, &[])).unwrap();
    let origin = store.snapshot().context.origin;
    store.rows = Some(
        (1..=33)
            .map(|seq| RetainedOutcome {
                key: SourceKey {
                    origin: origin.clone(),
                    seq: SourceSeq::new(seq).unwrap(),
                },
                outcome: WorkOutcome::Rejected { code: "x".into() },
            })
            .collect(),
    );
    store.refused(
        &store.peer(),
        store.ack(0, 34, &[]),
        Refusal::ConflictingProof,
    );
}

#[test]
fn receipt_reader_reuses_queue_context_and_integrity_validation_before_any_write() {
    for case in 0..3 {
        let store = Memory::new("A", 3);
        let expected = match case {
            0 => {
                store.data.lock().unwrap().context.origin.incarnation =
                    OriginIncarnation::new("OLD").unwrap();
                Refusal::RecoveryRequired
            }
            1 => {
                store
                    .data
                    .lock()
                    .unwrap()
                    .queue
                    .pending
                    .front_mut()
                    .unwrap()
                    .payload = Arc::from(&b"tampered"[..]);
                Refusal::IntegrityFailure
            }
            _ => {
                store.data.lock().unwrap().queue.last_seq = 4;
                Refusal::InvariantFailure
            }
        };
        store.refused(&store.peer(), store.ack(0, 2, &[]), expected);
    }
}

#[test]
fn conflicting_receipt_is_batch_fatal_while_gap_is_not() {
    assert!(SyncError::from(Refusal::ConflictingProof).aborts_batch());
    assert!(!SyncError::from(Refusal::Gap).aborts_batch());
}
#[test]
fn native_rejection_sequence_strictly_below_base_or_previous_is_an_invariant_failure() {
    for descending in [false, true] {
        let mut store = Memory::new("A", 4);
        store.apply(store.ack(0, 3, &[2, 3])).unwrap();
        let data = store.snapshot();
        let mut rows = vec![data
            .outcomes
            .values()
            .find(|r| r.key.seq.get() == 3)
            .unwrap()
            .clone()];
        if descending {
            rows.push(
                data.outcomes
                    .values()
                    .find(|r| r.key.seq.get() == 2)
                    .unwrap()
                    .clone(),
            );
            store.rows = Some(rows);
            store.refused(
                &store.peer(),
                store.ack(1, 3, &[2, 3]),
                Refusal::InvariantFailure,
            );
        } else {
            rows[0].key.seq = SourceSeq::new(1).unwrap();
            store.rows = Some(rows);
            store.refused(
                &store.peer(),
                store.ack(2, 3, &[3]),
                Refusal::InvariantFailure,
            );
        }
    }
}
