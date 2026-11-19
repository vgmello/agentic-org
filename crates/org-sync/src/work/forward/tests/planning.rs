use super::*;

fn plan(store: &Memory, trigger: ForwardTrigger) -> Vec<Change> {
    match forward_local(store, &store.peer(), trigger).unwrap() {
        LocalStreamOutput::ForwardCandidates { changes } => changes,
        other => panic!("expected candidates, got {other:?}"),
    }
}
#[test]
fn forwarding_selects_only_the_first_32_pending_sequences() {
    for count in [0, 1, 31, 32, 33] {
        let store = Memory::new("A", count);
        let before = store.snapshot();
        let changes = plan(&store, ForwardTrigger::Active);
        assert_eq!(changes.len(), (count as usize).min(32));
        assert_eq!(
            changes,
            before
                .queue
                .pending
                .iter()
                .take(32)
                .cloned()
                .collect::<Vec<_>>()
        );
        assert_eq!(store.snapshot(), before);
    }
}

fn refused_plan(store: &Memory, peer: &Peer, trigger: ForwardTrigger, expected: Refusal) {
    let before = store.snapshot();
    assert!(
        matches!(forward_local(store, peer, trigger), Err(SyncError::Refused(actual)) if actual == expected),
        "expected {expected:?}"
    );
    assert_eq!(store.snapshot(), before);
    assert_eq!(store.reopen().snapshot(), before);
}
#[test]
fn each_of_the_three_triggers_replays_the_same_immutable_prefix() {
    let store = Memory::new("A", 40);
    let before = store.snapshot();
    let wanted = before
        .queue
        .pending
        .iter()
        .take(32)
        .cloned()
        .collect::<Vec<_>>();
    for trigger in [
        ForwardTrigger::Active,
        ForwardTrigger::Gap,
        ForwardTrigger::Advanced,
    ] {
        assert_eq!(plan(&store, trigger), wanted);
        assert_eq!(store.snapshot(), before);
    }
}
#[test]
fn credit_return_alone_never_selects_or_reads_the_queue() {
    let mut store = Memory::new("A", 33);
    let before = store.snapshot();
    store.fault = Some("load");
    assert!(plan(&store, ForwardTrigger::CreditReturned).is_empty());
    assert_eq!(store.snapshot(), before);
    store.fault = None;
    assert_eq!(plan(&store, ForwardTrigger::Active).len(), 32);
}
#[test]
fn a_nonzero_floor_starts_at_the_first_unreceipted_item_and_does_not_dequeue() {
    let store = Memory::new("A", 60);
    store.apply(store.ack(0, 10, &[])).unwrap();
    let before = store.snapshot();
    let changes = plan(&store, ForwardTrigger::Advanced);
    assert_eq!(
        changes.iter().map(|c| c.key.seq.get()).collect::<Vec<_>>(),
        (11..=42).collect::<Vec<_>>()
    );
    assert_eq!(
        changes,
        before
            .queue
            .pending
            .iter()
            .take(32)
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(store.snapshot(), before);
}
#[test]
fn window_selection_never_adds_32_to_a_near_maximum_floor() {
    for (floor, count) in [(u64::MAX - 33, 33), (u64::MAX - 3, 3), (u64::MAX, 0)] {
        let store = Memory::new("A", count);
        {
            let mut data = store.data.lock().unwrap();
            let mut changes = data.queue.pending.clone();
            for (i, change) in changes.iter_mut().enumerate() {
                change.key.seq = SourceSeq::new(floor + i as u64 + 1).unwrap();
                change.digest = work_source_digest_v1(
                    &change.key.origin.office,
                    &change.key.origin.incarnation,
                    change.key.seq,
                    change.record_digest,
                    change.required_org,
                    change.created_ms,
                );
            }
            data.queue =
                LocalOutbox::restore(data.context.origin.clone(), floor + count, floor, changes)
                    .unwrap();
        }
        let before = store.snapshot();
        assert_eq!(
            plan(&store, ForwardTrigger::Active),
            before
                .queue
                .pending
                .iter()
                .take(32)
                .cloned()
                .collect::<Vec<_>>()
        );
        assert_eq!(store.snapshot(), before);
    }
}
#[test]
fn forward_rechecks_every_hq_identity_epoch_and_generation_disjunct() {
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
        for trigger in [ForwardTrigger::Active, ForwardTrigger::CreditReturned] {
            refused_plan(&store, &peer, trigger, Refusal::NotAuthorized);
        }
    }
}
#[test]
fn forward_preserves_authority_profile_restore_and_load_error_order() {
    for trigger in [ForwardTrigger::Active, ForwardTrigger::CreditReturned] {
        let mut store = Memory::new("A", 2);
        store.data.lock().unwrap().context.protocol_level = u32::MAX;
        store.data.lock().unwrap().context.restore_pending = true;
        store.fault = Some("load");
        let mut bad = store.peer();
        bad.1 += 1;
        refused_plan(&store, &bad, trigger, Refusal::NotAuthorized);
        refused_plan(&store, &store.peer(), trigger, Refusal::UnsupportedVersion);
        store.data.lock().unwrap().context.protocol_level = 1;
        refused_plan(&store, &store.peer(), trigger, Refusal::RestorePending);
    }
}
#[test]
fn forward_refuses_corrupt_queue_and_even_corruption_beyond_the_window() {
    for case in 0..5 {
        let store = Memory::new("A", 33);
        let expected = {
            let mut data = store.data.lock().unwrap();
            match case {
                0 => {
                    data.queue.last_seq += 1;
                    Refusal::InvariantFailure
                }
                1 => {
                    data.queue = Memory::new("B", 33).snapshot().queue;
                    Refusal::RecoveryRequired
                }
                2 => {
                    data.queue.pending[32].org_id = OrgId::new("FOREIGN").unwrap();
                    Refusal::InvariantFailure
                }
                3 => {
                    data.queue.pending[32].payload = Arc::from(&b"tampered"[..]);
                    Refusal::IntegrityFailure
                }
                _ => {
                    data.queue.pending[32].digest = Digest::from_bytes([0; 32]);
                    Refusal::IntegrityFailure
                }
            }
        };
        refused_plan(&store, &store.peer(), ForwardTrigger::Active, expected);
    }
}
#[test]
fn forward_checks_pruned_floor_invariant_at_equality_and_one_past() {
    let mut store = Memory::new("A", 40);
    store.apply(store.ack(0, 10, &[])).unwrap();
    store.authority.pruned_through = 10;
    assert_eq!(plan(&store, ForwardTrigger::Active).len(), 30);
    store.authority.pruned_through = 11;
    refused_plan(
        &store,
        &store.peer(),
        ForwardTrigger::Active,
        Refusal::InvariantFailure,
    );
}
#[test]
fn store_and_commit_failures_publish_no_candidates_or_state() {
    for fault in ["context", "authority", "load", "commit"] {
        let mut store = Memory::new("A", 2);
        store.fault = Some(fault);
        let before = store.snapshot();
        assert!(
            matches!(
                forward_local(&store, &store.peer(), ForwardTrigger::Active),
                Err(SyncError::Storage(_))
            ),
            "{fault}"
        );
        assert_eq!(store.snapshot(), before);
        assert_eq!(store.reopen().snapshot(), before);
    }
}
#[test]
fn reopen_requires_a_new_verified_connection_and_keeps_the_same_prefix() {
    let store = Memory::new("A", 40);
    store.apply(store.ack(0, 10, &[])).unwrap();
    let wanted = plan(&store, ForwardTrigger::Active);
    let mut reopened = store.reopen();
    refused_plan(
        &reopened,
        &store.peer(),
        ForwardTrigger::Active,
        Refusal::NotAuthorized,
    );
    reopened.authority.connection_generation = Some(8);
    assert_eq!(plan(&reopened, ForwardTrigger::Active), wanted);
    assert_eq!(reopened.snapshot(), store.snapshot());
}
#[test]
fn two_offices_with_the_same_verified_hq_select_only_their_local_stream() {
    let a = Memory::new("A", 33);
    let b = Memory::new("B", 2);
    let before_a = a.snapshot();
    let before_b = b.snapshot();
    // HQ identity is shared; the transaction, rather than the peer, owns the origin.
    let LocalStreamOutput::ForwardCandidates {
        changes: selected_b,
    } = forward_local(&b, &a.peer(), ForwardTrigger::Active).unwrap()
    else {
        panic!("expected candidates");
    };
    assert_eq!(
        selected_b,
        before_b.queue.pending.iter().cloned().collect::<Vec<_>>()
    );
    assert!(selected_b
        .iter()
        .all(|c| c.key.origin.office == OfficeId::new("B").unwrap()));
    let selected_a = plan(&a, ForwardTrigger::Active);
    assert_eq!(selected_a.len(), 32);
    assert!(selected_a
        .iter()
        .all(|c| c.key.origin.office == OfficeId::new("A").unwrap()));
    assert_eq!(a.snapshot(), before_a);
    assert_eq!(b.snapshot(), before_b);
}
struct Retry {
    store: Mutex<Memory>,
    replace_connection: bool,
}
impl LocalStreamStore for Retry {
    fn transact(&self, attempt: Arc<LocalStreamAttempt>) -> Result<LocalStreamOutput, SyncError> {
        let mut store = self.store.lock().unwrap();
        let mut uncommitted = store.snapshot();
        let first = attempt(&mut Tx {
            data: &mut uncommitted,
            store: &store,
        })?;
        assert!(
            matches!(first, LocalStreamOutput::ForwardCandidates { ref changes } if changes[0].key.seq.get() == 1)
        );
        // Discard the first attempt; these are independent committed changes before a retry.
        if self.replace_connection {
            store.authority.connection_generation = Some(8);
        } else {
            store.apply(store.ack(0, 1, &[]))?;
        }
        store.transact(attempt)
    }
}
#[test]
fn whole_attempt_retry_reloads_the_floor_and_rechecks_current_authority() {
    for replace_connection in [false, true] {
        let retry = Retry {
            store: Mutex::new(Memory::new("A", 40)),
            replace_connection,
        };
        let peer = retry.store.lock().unwrap().peer();
        let result = forward_local(&retry, &peer, ForwardTrigger::Active);
        if replace_connection {
            assert!(matches!(
                result,
                Err(SyncError::Refused(Refusal::NotAuthorized))
            ));
            assert_eq!(
                retry
                    .store
                    .lock()
                    .unwrap()
                    .snapshot()
                    .queue
                    .acknowledged_seq,
                0
            );
        } else {
            let LocalStreamOutput::ForwardCandidates { changes } = result.unwrap() else {
                panic!("expected candidates")
            };
            assert_eq!(
                changes.iter().map(|c| c.key.seq.get()).collect::<Vec<_>>(),
                (2..=33).collect::<Vec<_>>()
            );
            assert_eq!(
                retry
                    .store
                    .lock()
                    .unwrap()
                    .snapshot()
                    .queue
                    .acknowledged_seq,
                1
            );
        }
    }
}
#[test]
fn receipt_actions_keep_reforward_or_close_without_leaking_local_codes() {
    let cases = [
        (Ok(LocalStreamOutput::Duplicate), ReceiptAction::Keep),
        (
            Ok(LocalStreamOutput::ForwardCandidates { changes: vec![] }),
            ReceiptAction::Keep,
        ),
        (
            Ok(LocalStreamOutput::Advanced { from: 0, to: 1 }),
            ReceiptAction::Reforward,
        ),
        (
            Err(SyncError::Storage(StoreError::unavailable(
                "private native cause",
            ))),
            ReceiptAction::Keep,
        ),
        (Err(Refusal::Gap.into()), ReceiptAction::Reforward),
        (Err(Refusal::RestorePending.into()), ReceiptAction::Keep),
        (
            Err(Refusal::ConflictingProof.into()),
            ReceiptAction::DivergenceThenClose,
        ),
    ];
    for (result, expected) in cases {
        let before = format!("{result:?}");
        let action = receipt_action(&result);
        assert_eq!(action, expected);
        assert_eq!(format!("{result:?}"), before);
        assert_eq!(
            action.wire_code(),
            if action == ReceiptAction::DivergenceThenClose {
                Some("divergence")
            } else {
                None
            }
        );
    }
    for refusal in [
        Refusal::NotAuthorized,
        Refusal::UnsupportedVersion,
        Refusal::InvalidInput,
        Refusal::OperationReused,
        Refusal::Expired,
        Refusal::QuotaExceeded,
        Refusal::RecoveryRequired,
        Refusal::IntegrityFailure,
        Refusal::Divergence,
        Refusal::InvariantFailure,
        Refusal::WrongStream,
        Refusal::Malformed,
        Refusal::AheadOfForwarded,
    ] {
        let action = receipt_action(&Err(refusal.into()));
        assert_eq!(action, ReceiptAction::Close, "{refusal:?}");
        assert_eq!(action.wire_code(), None, "{refusal:?}");
    }
}
