use super::*;

const TESS: &str = "tess-1";

#[test]
fn every_caller_has_a_partition_of_their_own() {
    let owner = PartitionKey::tess_ask("owner-1");
    let member = PartitionKey::tess_ask("member-2");
    assert_eq!(owner.as_str(), "tess:ask:owner-1");
    assert_eq!(owner.caller_id(), Some("owner-1"));
    assert_ne!(owner, member);
    assert_eq!(PartitionKey::for_caller(None), PartitionKey::tess_wake());
    assert_eq!(PartitionKey::tess_wake().caller_id(), None);
    assert_eq!(PartitionKey::for_caller(Some("member-2")), member);

    let ledger = PartitionLedger::new();
    ledger.begin_turn(TESS, &owner);
    ledger.record_runtime_session(TESS, &owner, "conv-owner");
    // The member's first turn starts fresh: the owner's conversation never reaches it.
    let first = ledger.begin_turn(TESS, &member);
    assert_eq!(first.resume_runtime_session_id, None);
    ledger.record_runtime_session(TESS, &member, "conv-member");
    // Back to the owner: their own conversation resumes.
    let again = ledger.begin_turn(TESS, &owner);
    assert_eq!(
        again.resume_runtime_session_id.as_deref(),
        Some("conv-owner")
    );
    assert_eq!(again.turn, 2);
    // Another Chief of Staff (another office's) shares nothing either.
    assert_eq!(
        ledger
            .begin_turn("tess-2", &owner)
            .resume_runtime_session_id,
        None
    );
}

#[test]
fn a_partition_rotates_after_twenty_turns() {
    let ledger = PartitionLedger::new();
    let key = PartitionKey::tess_ask("owner-1");
    for n in 1..=ROTATE_AFTER_TURNS {
        let turn = ledger.begin_turn(TESS, &key);
        assert_eq!((turn.turn, turn.rotated), (n, false));
        ledger.record_runtime_session(TESS, &key, "conv-1");
    }
    assert!(ledger.needs_rotation(TESS, &key));
    let fresh = ledger.begin_turn(TESS, &key);
    assert!(fresh.rotated);
    assert_eq!(fresh.turn, 1);
    assert_eq!(
        fresh.resume_runtime_session_id, None,
        "a fresh conversation"
    );
    let state = ledger.state(TESS, &key).expect("state");
    assert_eq!(state.rotations, 1);
    assert!(!state.needs_rotation());
}

#[test]
fn a_partition_rotates_at_sixty_percent_of_its_context_window() {
    let ledger = PartitionLedger::new();
    let key = PartitionKey::tess_ask("member-2");
    ledger.begin_turn(TESS, &key);
    ledger.record_runtime_session(TESS, &key, "conv-1");
    ledger.observe_context(TESS, &key, 119_999, Some(200_000));
    assert!(!ledger.needs_rotation(TESS, &key), "59.99% stays");
    ledger.observe_context(TESS, &key, 120_000, None);
    assert!(
        ledger.needs_rotation(TESS, &key),
        "60% rotates; the window is remembered"
    );
    let next = ledger.begin_turn(TESS, &key);
    assert!(next.rotated);
    assert_eq!(next.resume_runtime_session_id, None);
    // An unknown window never rotates on context alone, and a zero window is ignored.
    let other = PartitionKey::tess_ask("member-3");
    ledger.begin_turn(TESS, &other);
    ledger.observe_context(TESS, &other, u64::MAX, None);
    assert!(!ledger.needs_rotation(TESS, &other));
    ledger.observe_context(TESS, &other, u64::MAX, Some(0));
    assert!(!ledger.needs_rotation(TESS, &other));
}

#[test]
fn records_for_an_unknown_partition_are_ignored() {
    let ledger = PartitionLedger::new();
    let key = PartitionKey::tess_ask("nobody");
    ledger.record_runtime_session(TESS, &key, "conv");
    ledger.observe_context(TESS, &key, 1, Some(2));
    assert_eq!(ledger.state(TESS, &key), None);
}

#[test]
fn concurrent_turns_count_every_turn_once() {
    let ledger = PartitionLedger::new();
    let key = PartitionKey::tess_ask("owner-1");
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let ledger = ledger.clone();
            let key = key.clone();
            scope.spawn(move || {
                ledger.begin_turn(TESS, &key);
            });
        }
    });
    assert_eq!(ledger.state(TESS, &key).expect("state").turns, 8);
}
