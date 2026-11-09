//! The triage conformance suite: cases generic over any `MessageStore` + `TriageStore` +
//! `MemberDirectory` backend. Run every case against a backend with
//! [`crate::triage_conformance_tests!`], separately from [`crate::mailbox_conformance_tests!`]:
//! a backend opts in once it implements [`TriageStore`].
//!
//! The `triage_*` cases check the [`TriageStore`] contract (its doc comments are the spec).
//! Run them against the production shape, as the module docs of
//! [`super::conformance`] say.

use std::sync::Barrier;
use std::thread;

use org_proto::msg::{
    MessageHeader, MessageLifecycleState as State, MessagePriority, NotifyReason,
};
use org_proto::{Member, Position};

use super::conformance::{commit, expired_request, ids, inbox_query, new_message, peek, state_of};
use super::fixture::*;
use crate::{
    format_utc, LocalStateTransition, MemberDirectory, MessageStore, SnoozeDeadline, StoreError,
    TriageStore,
};

/// Opens a fresh, empty backend whose store also implements [`TriageStore`] and whose
/// directory holds `members` at `position`. Any `Fn(&[Member], Position) -> (S, D)` is one.
pub trait OpenTriageBackend {
    type Store: MessageStore + TriageStore + Send + Sync + 'static;
    type Directory: MemberDirectory + Send + Sync + 'static;
    fn open(&self, members: &[Member], position: Position) -> (Self::Store, Self::Directory);
}

impl<F, S, D> OpenTriageBackend for F
where
    F: Fn(&[Member], Position) -> (S, D),
    S: MessageStore + TriageStore + Send + Sync + 'static,
    D: MemberDirectory + Send + Sync + 'static,
{
    type Store = S;
    type Directory = D;
    fn open(&self, members: &[Member], position: Position) -> (S, D) {
        self(members, position)
    }
}

// ---------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------

fn open_store<B: OpenTriageBackend>(backend: &B) -> B::Store {
    backend.open(&members(), POSITION).0
}

fn at(offset_ms: i64) -> String {
    format_utc(NOW_MS.checked_add_signed(offset_ms).unwrap()).unwrap()
}

/// A transition of `msg_id(n)` for Ada, at [`NOW_UTC`].
fn change(n: u32, state: State) -> LocalStateTransition {
    LocalStateTransition {
        id: msg_id(n),
        to_member_id: ADA_ID.into(),
        state,
        snoozed_until: None,
        snooze_after_task: false,
        reason: None,
        now_utc: NOW_UTC.into(),
        owner_office_id: OFFICE_ID.into(),
        origin_incarnation: INCARNATION.into(),
        org_position: POSITION,
        expired_waiting_person: false,
    }
}

fn snooze_until(n: u32, until: &str, note: Option<&str>) -> LocalStateTransition {
    LocalStateTransition {
        snoozed_until: Some(until.into()),
        reason: note.map(Into::into),
        ..change(n, State::Snoozed)
    }
}

fn snooze_after_task(n: u32) -> LocalStateTransition {
    LocalStateTransition {
        snooze_after_task: true,
        ..change(n, State::Snoozed)
    }
}

fn apply(store: &impl TriageStore, change: LocalStateTransition) -> MessageHeader {
    store
        .update_state_local(change)
        .expect("update_state_local")
}

fn refused(store: &impl TriageStore, change: LocalStateTransition) -> StoreError {
    match store.update_state_local(change) {
        Ok(header) => panic!("transition applied: {header:?}"),
        Err(error) => error,
    }
}

fn header_of(store: &impl MessageStore, n: u32) -> MessageHeader {
    store
        .get_message(&msg_id(n))
        .expect("get_message")
        .expect("message exists")
        .message
        .header
}

/// Commits `msg_id(n)`, a normal message from Pat to Ada.
fn unread(store: &impl MessageStore, n: u32) {
    commit(
        store,
        new_message(n, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
}

fn unread_ids(store: &impl MessageStore) -> Vec<String> {
    ids(&store
        .list_headers(inbox_query(ADA_ID, true, 100))
        .expect("list_headers"))
}

fn sorted(mut candidates: Vec<(String, NotifyReason)>) -> Vec<(String, NotifyReason)> {
    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    candidates
}

fn notify(
    store: &impl TriageStore,
    after_task: bool,
    urgent_only: bool,
    sweep_before: &str,
    now: &str,
) -> Vec<(String, NotifyReason)> {
    sorted(
        store
            .check_notify(ADA_ID, after_task, urgent_only, sweep_before, now)
            .expect("check_notify")
            .into_iter()
            .map(|candidate| (candidate.header.id, candidate.reason))
            .collect(),
    )
}

// ---------------------------------------------------------------------------------------
// TriageStore::update_state_local
// ---------------------------------------------------------------------------------------

/// Snoozing (from unread, read or snoozed) puts the message back into the unread index as
/// snoozed, hidden from peek, claims and the default inbox, with its note and a bumped
/// `triage_gen`; re-snoozing always applies and bumps again; exactly one of `snoozed_until`
/// and `snooze_after_task` must be set.
pub fn triage_snooze_transitions(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    unread(&store, 1);
    unread(&store, 2);
    let until = at(3_600_000);

    let header = apply(&store, snooze_until(1, &until, Some("after lunch")));
    assert_eq!(header.id, msg_id(1));
    assert_eq!(header.state, State::Snoozed);
    assert_eq!(header.triage_gen, 1);
    assert_eq!(header.snoozed_until.as_deref(), Some(until.as_str()));
    assert!(!header.snooze_after_task);
    assert_eq!(header.snooze_note.as_deref(), Some("after lunch"));
    assert_eq!(
        header_of(&store, 1),
        header,
        "the returned header is the stored one"
    );
    assert_eq!(
        peek(&store, ADA_ID).message.map(|m| m.message.header.id),
        Some(msg_id(2))
    );
    assert_eq!(unread_ids(&store), [msg_id(2)]);
    assert_eq!(
        ids(&store.list_headers(inbox_query(ADA_ID, false, 100)).unwrap()),
        [msg_id(2), msg_id(1)],
        "include_all still lists it"
    );

    // Re-snoozing applies again, here to after-task: the time is cleared.
    let again = apply(&store, snooze_after_task(1));
    assert_eq!(again.state, State::Snoozed);
    assert_eq!(again.triage_gen, 2);
    assert_eq!(again.snoozed_until, None);
    assert!(again.snooze_after_task);
    assert_eq!(again.snooze_note, None);
    let same = apply(&store, snooze_after_task(1));
    assert_eq!(same.triage_gen, 3, "the same snooze again still bumps");

    // From read: back into the unread index, snoozed.
    let claimed = store.claim_read(ADA_ID, None, NOW_UTC).unwrap().unwrap();
    assert_eq!(claimed.message.header.id, msg_id(2));
    let header = apply(&store, snooze_until(2, &until, None));
    assert_eq!((header.state, header.triage_gen), (State::Snoozed, 1));
    assert_eq!(store.claim_read(ADA_ID, None, NOW_UTC).unwrap(), None);
    assert!(unread_ids(&store).is_empty());
    // Back in the unread index: it is a timer, and it comes back when due.
    assert!(store
        .list_active_snooze_deadlines()
        .unwrap()
        .iter()
        .any(|deadline| deadline.id == msg_id(2)));
    assert_eq!(
        notify(&store, false, false, NOW_UTC, &until),
        [(msg_id(2), NotifyReason::Due)]
    );
    assert_eq!(unread_ids(&store), [msg_id(2)]);

    // Exactly one of the two snooze targets.
    unread(&store, 3);
    let both = LocalStateTransition {
        snooze_after_task: true,
        ..snooze_until(3, &until, None)
    };
    assert!(matches!(
        refused(&store, both),
        StoreError::InvalidTransition
    ));
    assert!(matches!(
        refused(&store, change(3, State::Snoozed)),
        StoreError::InvalidTransition
    ));
    let header = header_of(&store, 3);
    assert_eq!((header.state, header.triage_gen), (State::Unread, 0));
}

/// Done (from unread, read or snoozed) leaves the unread index for good; done again is
/// idempotent; nothing leaves done.
pub fn triage_done_transitions(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    for n in 1..=3 {
        unread(&store, n);
    }
    store
        .claim_read(ADA_ID, Some(&msg_id(2)), NOW_UTC)
        .unwrap()
        .unwrap();
    apply(&store, snooze_until(3, &at(60_000), Some("later")));
    for n in 1..=3 {
        let done = LocalStateTransition {
            reason: Some("handled".into()),
            ..change(n, State::Done)
        };
        let header = apply(&store, done);
        assert_eq!(header.state, State::Done, "message {n}");
        assert_eq!(header.snoozed_until, None);
        assert!(!header.snooze_after_task);
        assert_eq!(header, header_of(&store, n));
    }
    assert_eq!(peek(&store, ADA_ID).message, None);
    assert!(unread_ids(&store).is_empty());
    assert_eq!(store.claim_read(ADA_ID, None, NOW_UTC).unwrap(), None);

    let before = header_of(&store, 1);
    let later = LocalStateTransition {
        now_utc: at(5_000),
        ..change(1, State::Done)
    };
    assert_eq!(apply(&store, later), before, "done again is idempotent");
    for state in [State::Snoozed, State::Read, State::Unread, State::Expired] {
        let mut attempt = change(1, state);
        attempt.snooze_after_task = state == State::Snoozed;
        assert!(
            matches!(refused(&store, attempt), StoreError::InvalidTransition),
            "done -> {state:?}"
        );
    }
    assert_eq!(header_of(&store, 1), before);
    assert!(
        store.list_active_snooze_deadlines().unwrap().is_empty(),
        "done cleared the snooze timer"
    );
}

/// Read (from unread or snoozed) leaves the unread index and stamps `read_at`; read again
/// is idempotent, keeping the first `read_at`.
pub fn triage_read_transitions(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    unread(&store, 1);
    unread(&store, 2);
    let header = apply(&store, change(1, State::Read));
    assert_eq!(header.state, State::Read);
    assert_eq!(header.read_at.as_deref(), Some(NOW_UTC));

    apply(&store, snooze_after_task(2));
    let later = at(5_000);
    let header = apply(
        &store,
        LocalStateTransition {
            now_utc: later.clone(),
            ..change(2, State::Read)
        },
    );
    assert_eq!(header.state, State::Read);
    assert_eq!(header.read_at.as_deref(), Some(later.as_str()));
    assert!(!header.snooze_after_task, "reading cleared the snooze");
    assert!(unread_ids(&store).is_empty());

    let again = apply(
        &store,
        LocalStateTransition {
            now_utc: at(9_000),
            ..change(1, State::Read)
        },
    );
    assert_eq!(
        again.read_at.as_deref(),
        Some(NOW_UTC),
        "read again is idempotent"
    );
}

/// Unread comes back from read (into the unread index, `read_at` cleared); unread to unread
/// sets `expired_waiting_person`, and `false` never clears it; snoozed, done and expired
/// can't become unread here.
pub fn triage_unread_transitions(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    unread(&store, 1);
    store.claim_read(ADA_ID, None, NOW_UTC).unwrap().unwrap();
    let header = apply(&store, change(1, State::Unread));
    assert_eq!(header.state, State::Unread);
    assert_eq!(header.read_at, None);
    assert!(!header.expired_waiting_person);
    assert_eq!(
        peek(&store, ADA_ID).message.map(|m| m.message.header.id),
        Some(msg_id(1))
    );

    // An ask to a person whose deadline passed: flagged, still unread, no longer expired.
    let mut ask = expired_request(2, ADA_ID, PAT_ID);
    ask.message.header.to_display_name = PAT_HANDLE.into();
    commit(&store, ask);
    assert_eq!(peek(&store, PAT_ID).expired_ids, [msg_id(2)]);
    let flag = LocalStateTransition {
        to_member_id: PAT_ID.into(),
        expired_waiting_person: true,
        ..change(2, State::Unread)
    };
    let header = apply(&store, flag);
    assert_eq!(header.state, State::Unread);
    assert!(header.expired_waiting_person);
    let after = peek(&store, PAT_ID);
    assert!(after.expired_ids.is_empty(), "a flagged ask is not expired");
    assert_eq!(after.message.map(|m| m.message.header.id), Some(msg_id(2)));
    let keep = LocalStateTransition {
        to_member_id: PAT_ID.into(),
        ..change(2, State::Unread)
    };
    assert!(
        apply(&store, keep).expired_waiting_person,
        "false never clears the flag"
    );

    unread(&store, 3);
    apply(&store, snooze_after_task(3));
    assert!(matches!(
        refused(&store, change(3, State::Unread)),
        StoreError::InvalidTransition
    ));
    apply(&store, change(3, State::Done));
    assert!(matches!(
        refused(&store, change(3, State::Unread)),
        StoreError::InvalidTransition
    ));
}

/// Expired (from unread or snoozed) leaves the unread index; expired again is idempotent; a
/// read message can't expire and nothing leaves expired.
pub fn triage_expired_transitions(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    commit(&store, expired_request(1, PAT_ID, ADA_ID));
    commit(&store, expired_request(2, PAT_ID, ADA_ID));
    unread(&store, 3);
    assert_eq!(peek(&store, ADA_ID).expired_ids, [msg_id(1), msg_id(2)]);

    let header = apply(&store, change(1, State::Expired));
    assert_eq!(header.state, State::Expired);
    apply(&store, snooze_until(2, &at(60_000), None));
    let header = apply(&store, change(2, State::Expired));
    assert_eq!(header.state, State::Expired);
    assert_eq!(header.snoozed_until, None);
    let outcome = peek(&store, ADA_ID);
    assert!(
        outcome.expired_ids.is_empty(),
        "expired rows left the index"
    );
    assert_eq!(
        outcome.message.map(|m| m.message.header.id),
        Some(msg_id(3))
    );
    assert_eq!(unread_ids(&store), [msg_id(3)]);

    let before = header_of(&store, 1);
    assert_eq!(apply(&store, change(1, State::Expired)), before);
    for state in [State::Unread, State::Read, State::Done, State::Snoozed] {
        let mut attempt = change(1, state);
        attempt.snooze_after_task = state == State::Snoozed;
        assert!(
            matches!(refused(&store, attempt), StoreError::InvalidTransition),
            "expired -> {state:?}"
        );
    }

    store.claim_read(ADA_ID, None, NOW_UTC).unwrap().unwrap();
    assert!(matches!(
        refused(&store, change(3, State::Expired)),
        StoreError::InvalidTransition
    ));
    assert_eq!(state_of(&store, &msg_id(3)), State::Read);
}

/// DeadLetter and Unknown are never targets here: `InvalidTransition`, nothing changes.
pub fn triage_dead_letter_and_unknown_are_refused(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    unread(&store, 1);
    for state in [State::DeadLetter, State::Unknown] {
        assert!(
            matches!(
                refused(&store, change(1, state)),
                StoreError::InvalidTransition
            ),
            "{state:?}"
        );
    }
    let header = header_of(&store, 1);
    assert_eq!((header.state, header.triage_gen), (State::Unread, 0));
}

/// A message addressed to another member, or an unknown id, is `NotFound` and unchanged.
pub fn triage_other_members_message_is_not_found(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    commit(
        &store,
        new_message(1, PAT_ID, REMY_ID, MessagePriority::Normal),
    );
    for state in [State::Read, State::Done, State::Expired, State::Unread] {
        assert!(
            matches!(refused(&store, change(1, state)), StoreError::NotFound),
            "{state:?}"
        );
    }
    assert!(matches!(
        refused(&store, snooze_after_task(1)),
        StoreError::NotFound
    ));
    assert!(matches!(
        refused(&store, change(9, State::Done)),
        StoreError::NotFound
    ));
    let header = header_of(&store, 1);
    assert_eq!((header.state, header.triage_gen), (State::Unread, 0));
}

// ---------------------------------------------------------------------------------------
// TriageStore::check_notify and list_active_snooze_deadlines
// ---------------------------------------------------------------------------------------

/// check_notify releases due time snoozes (`Due`) whatever `urgent_only` says, after-task
/// snoozes only with `after_task` (`AfterTask`), bumps `triage_gen` on each release, and
/// selects open messages: `New` when urgent or at a break, `Sweep` when never triaged and
/// older than `sweep_before`, otherwise nothing. Overdue requests come back as they are, and
/// another member's messages never do.
pub fn triage_check_notify_reasons_filters_and_releases(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    let old = at(-600_000);
    let sweep_before = at(-300_000);
    let now = at(120_000);
    let mut old_normal = new_message(1, PAT_ID, ADA_ID, MessagePriority::Normal);
    old_normal.message.header.created_at = old.clone();
    commit(&store, old_normal);
    commit(
        &store,
        new_message(2, PAT_ID, ADA_ID, MessagePriority::High),
    );
    unread(&store, 3);
    unread(&store, 4);
    unread(&store, 5);
    let mut old_triaged = new_message(6, PAT_ID, ADA_ID, MessagePriority::Normal);
    old_triaged.message.header.created_at = old;
    commit(&store, old_triaged);
    commit(
        &store,
        new_message(7, PAT_ID, REMY_ID, MessagePriority::Urgent),
    );
    commit(&store, expired_request(8, PAT_ID, ADA_ID));
    apply(&store, snooze_until(4, &at(60_000), None));
    apply(&store, snooze_after_task(5));
    apply(&store, snooze_until(6, &at(60_000), None));
    let not_yet = notify(&store, false, false, &sweep_before, NOW_UTC);
    assert_eq!(
        not_yet,
        sorted(vec![
            (msg_id(1), NotifyReason::Sweep),
            (msg_id(2), NotifyReason::New),
            (msg_id(8), NotifyReason::New),
        ]),
        "nothing is due yet"
    );
    assert_eq!(state_of(&store, &msg_id(4)), State::Snoozed);

    // urgent_only filters the list, not the release.
    assert_eq!(
        notify(&store, false, true, &sweep_before, &now),
        sorted(vec![
            (msg_id(2), NotifyReason::New),
            (msg_id(8), NotifyReason::New),
        ])
    );
    for n in [4, 6] {
        let header = header_of(&store, n);
        assert_eq!(header.state, State::Unread, "message {n} released");
        assert_eq!(header.triage_gen, 2, "snooze and release each bump");
        assert_eq!(header.snoozed_until, None);
    }
    assert_eq!(state_of(&store, &msg_id(5)), State::Snoozed);
    assert_eq!(
        state_of(&store, &msg_id(8)),
        State::Unread,
        "not expired here"
    );

    // Released ones are triaged, so never swept; 3 is recent and normal.
    assert_eq!(
        notify(&store, false, false, &sweep_before, &now),
        sorted(vec![
            (msg_id(1), NotifyReason::Sweep),
            (msg_id(2), NotifyReason::New),
            (msg_id(8), NotifyReason::New),
        ])
    );

    // At a break everything open is new, and the after-task snooze is released.
    let at_break: Vec<_> = notify(&store, true, false, &sweep_before, &now);
    assert_eq!(
        at_break,
        sorted(vec![
            (msg_id(1), NotifyReason::New),
            (msg_id(2), NotifyReason::New),
            (msg_id(3), NotifyReason::New),
            (msg_id(4), NotifyReason::New),
            (msg_id(5), NotifyReason::AfterTask),
            (msg_id(6), NotifyReason::New),
            (msg_id(8), NotifyReason::New),
        ])
    );
    let header = header_of(&store, 5);
    assert_eq!((header.state, header.triage_gen), (State::Unread, 2));
    assert!(!header.snooze_after_task);
    assert_eq!(
        state_of(&store, &msg_id(7)),
        State::Unread,
        "Remy's is untouched"
    );

    // A released candidate carries its header after the release.
    apply(&store, snooze_until(3, &at(60_000), None));
    let released = store
        .check_notify(ADA_ID, false, false, &sweep_before, &now)
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.header.id == msg_id(3))
        .expect("message 3 is due");
    assert_eq!(released.reason, NotifyReason::Due);
    assert_eq!(released.header, header_of(&store, 3));
    assert_eq!(released.header.state, State::Unread);
    assert_eq!(released.header.triage_gen, 2);
    assert_eq!(
        notify(&store, false, false, &sweep_before, &now)
            .iter()
            .filter(|(id, _)| id == &msg_id(3))
            .count(),
        0,
        "released once"
    );
}

/// check_notify never returns read, done or expired messages, nor a snooze that isn't due
/// (even an urgent one, even at a break); releases a snooze due exactly now (`<=`); sweeps
/// only messages created strictly before `sweep_before` (`<`); reports an old urgent message
/// as `New`, not `Sweep`; and leaves another member's due snooze alone.
pub fn triage_check_notify_exclusions_and_boundaries(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    let sweep_before = at(-300_000);
    let now = at(120_000);
    for n in 1..=3 {
        unread(&store, n);
    }
    commit(&store, expired_request(4, PAT_ID, ADA_ID));
    commit(
        &store,
        new_message(5, PAT_ID, ADA_ID, MessagePriority::Urgent),
    );
    let mut at_sweep = new_message(6, PAT_ID, ADA_ID, MessagePriority::Normal);
    at_sweep.message.header.created_at = sweep_before.clone();
    commit(&store, at_sweep);
    unread(&store, 7);
    let mut old_urgent = new_message(8, PAT_ID, ADA_ID, MessagePriority::High);
    old_urgent.message.header.created_at = at(-600_000);
    commit(&store, old_urgent);
    commit(
        &store,
        new_message(9, PAT_ID, REMY_ID, MessagePriority::Normal),
    );

    store.claim_read(ADA_ID, Some(&msg_id(1)), NOW_UTC).unwrap();
    apply(&store, change(2, State::Done));
    apply(&store, change(3, State::Read));
    apply(&store, change(4, State::Expired));
    apply(&store, snooze_until(5, &at(3_600_000), None));
    apply(&store, snooze_until(7, &now, None));
    apply(
        &store,
        LocalStateTransition {
            to_member_id: REMY_ID.into(),
            ..snooze_until(9, &at(60_000), None)
        },
    );

    assert_eq!(
        notify(&store, false, false, &sweep_before, &now),
        sorted(vec![
            (msg_id(7), NotifyReason::Due),
            (msg_id(8), NotifyReason::New),
        ]),
        "due exactly now is released; created exactly at sweep_before is not swept"
    );
    assert_eq!(
        notify(&store, true, false, &sweep_before, &now),
        sorted(vec![
            (msg_id(6), NotifyReason::New),
            (msg_id(7), NotifyReason::New),
            (msg_id(8), NotifyReason::New),
        ])
    );
    assert_eq!(
        notify(&store, true, true, &sweep_before, &now),
        [(msg_id(8), NotifyReason::New)]
    );
    let pending = header_of(&store, 5);
    assert_eq!((pending.state, pending.triage_gen), (State::Snoozed, 1));
    for (n, state) in [(1, State::Read), (2, State::Done), (3, State::Read)] {
        assert_eq!(state_of(&store, &msg_id(n)), state);
    }
    assert_eq!(state_of(&store, &msg_id(4)), State::Expired);
    assert_eq!(
        state_of(&store, &msg_id(9)),
        State::Snoozed,
        "another member's due snooze is not released by Ada's check"
    );
    assert!(store
        .list_active_snooze_deadlines()
        .unwrap()
        .iter()
        .any(|deadline| deadline.id == msg_id(9)));
}

/// check_notify is one atomic step: concurrent checks release a due snooze once (one `Due`
/// in all, `triage_gen` bumped once).
pub fn triage_check_notify_releases_once_under_concurrency(backend: impl OpenTriageBackend) {
    const K: usize = 8;
    let store = open_store(&backend);
    unread(&store, 1);
    apply(&store, snooze_until(1, &at(60_000), None));
    let now = at(120_000);
    let barrier = Barrier::new(K);
    let due: usize = thread::scope(|scope| {
        let workers: Vec<_> = (0..K)
            .map(|_| {
                scope.spawn(|| {
                    // The barrier comes before any store call, so a failing backend can't
                    // strand the other threads at it. One call each.
                    barrier.wait();
                    store
                        .check_notify(ADA_ID, false, false, NOW_UTC, &now)
                        .unwrap()
                        .into_iter()
                        .filter(|candidate| candidate.reason == NotifyReason::Due)
                        .count()
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum()
    });
    assert_eq!(due, 1, "released exactly once");
    let header = header_of(&store, 1);
    assert_eq!((header.state, header.triage_gen), (State::Unread, 2));
}

/// Every time snooze in the unread index is listed; after-task snoozes, released, read and
/// done messages aren't.
pub fn triage_list_active_snooze_deadlines(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    assert!(store.list_active_snooze_deadlines().unwrap().is_empty());
    for n in 1..=5 {
        unread(&store, n);
    }
    commit(
        &store,
        new_message(6, PAT_ID, REMY_ID, MessagePriority::Normal),
    );
    let soon = at(60_000);
    let later = at(3_600_000);
    apply(&store, snooze_until(1, &soon, None));
    apply(&store, snooze_until(2, &later, None));
    apply(&store, snooze_after_task(3));
    apply(&store, snooze_until(4, &soon, None));
    apply(&store, change(4, State::Read));
    apply(&store, snooze_until(5, &soon, None));
    apply(&store, change(5, State::Done));
    apply(
        &store,
        LocalStateTransition {
            to_member_id: REMY_ID.into(),
            ..snooze_until(6, &later, None)
        },
    );
    let mut listed = store.list_active_snooze_deadlines().unwrap();
    listed.sort_by(|a, b| a.id.cmp(&b.id));
    let deadline = |n: u32, at_utc: &str, member: &str| SnoozeDeadline {
        at_utc: at_utc.into(),
        to_member_id: member.into(),
        id: msg_id(n),
    };
    assert_eq!(
        listed,
        [
            deadline(1, &soon, ADA_ID),
            deadline(2, &later, ADA_ID),
            deadline(6, &later, REMY_ID),
        ]
    );
    store
        .check_notify(ADA_ID, false, false, NOW_UTC, &at(120_000))
        .unwrap();
    let mut listed = store.list_active_snooze_deadlines().unwrap();
    listed.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(
        listed,
        [deadline(2, &later, ADA_ID), deadline(6, &later, REMY_ID)],
        "a released snooze is no longer a timer"
    );
}

/// Every case, by name: what [`crate::triage_conformance_tests!`] runs.
#[doc(hidden)]
#[macro_export]
macro_rules! __triage_conformance_cases {
    ($open:expr; $($case:ident),* $(,)?) => {
        $(
            #[test]
            fn $case() {
                $crate::testkit::triage_conformance::$case($open)
            }
        )*
    };
}

/// Generates one `#[test]` per triage conformance case, each opening a fresh backend with
/// `$open`, any `Fn(&[Member], Position) -> (impl MessageStore + TriageStore, impl
/// MemberDirectory)` (both `Send + Sync + 'static`). Needs the `testkit` feature. Separate
/// from [`crate::mailbox_conformance_tests!`] so a backend opts in once it implements
/// [`TriageStore`].
///
/// ```ignore
/// mod triage_conformance {
///     org_mailbox::triage_conformance_tests!(super::open_sqlite);
/// }
/// ```
#[macro_export]
macro_rules! triage_conformance_tests {
    ($open:expr) => {
        $crate::__triage_conformance_cases! {
            $open;
            triage_snooze_transitions,
            triage_done_transitions,
            triage_read_transitions,
            triage_unread_transitions,
            triage_expired_transitions,
            triage_dead_letter_and_unknown_are_refused,
            triage_other_members_message_is_not_found,
            triage_check_notify_reasons_filters_and_releases,
            triage_check_notify_exclusions_and_boundaries,
            triage_check_notify_releases_once_under_concurrency,
            triage_list_active_snooze_deadlines,
        }
    };
}
