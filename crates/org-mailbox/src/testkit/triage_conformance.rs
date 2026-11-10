//! The triage conformance suite: cases generic over any `MessageStore` + `TriageStore` +
//! `MemberDirectory` backend. Run every case against a backend with
//! [`crate::triage_conformance_tests!`], separately from [`crate::mailbox_conformance_tests!`]:
//! a backend opts in once it implements [`TriageStore`].
//!
//! The `triage_*` cases check the [`TriageStore`] contract (its doc comments are the spec);
//! the `mailbox_*` cases run the Mailbox's triage (`defer`, `done`, snooze timers, expiry and
//! `delivery_failed`) over the backend, with [`crate::Mailbox::with_triage`].
//! Run them against the production shape, as the module docs of
//! [`super::conformance`] say.

use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use org_proto::msg::{
    DeliveryFailedReason, HeaderPage, MessageHeader, MessageLifecycleState as State,
    MessagePriority, Meta, MsgDeferParams, MsgDoneParams, MsgInboxParams, MsgReadParams,
    MsgSendParams, MsgWaitResult, NotifyReason, SnoozeTarget, KIND_DELIVERY_FAILED, KIND_MSG,
    MAX_ID_BYTES, MAX_NOTE_BYTES, OFFICE_SENDER,
};
use org_proto::{ulid_time_ms, Member, Position};

use super::conformance::{
    commit, eventually, expired_request, far_deadline, ids, inbox_query, new_message, peek, send,
    state_of, unread_inbox, wait_params,
};
use super::fixture::*;
use crate::{
    cancellation_pair, format_utc, parse_utc, CommitMessageOutcome, HeaderQuery,
    LocalStateTransition, Mailbox, MailboxError, MemberDirectory, MessageStore, NewStoredMessage,
    NotifyCandidate, OfficeIdentity, PeekUnreadOutcome, SnoozeDeadline, StoreError, StoredMessage,
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
/// and `snooze_after_task` must be set. Snoozing a read message clears its `read_at`.
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
    assert_eq!(claimed.message.header.read_at.as_deref(), Some(NOW_UTC));
    let header = apply(&store, snooze_until(2, &until, None));
    assert_eq!((header.state, header.triage_gen), (State::Snoozed, 1));
    assert_eq!(header.read_at, None, "snoozing from read clears read_at");
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
    assert_eq!(
        header_of(&store, 2).read_at,
        None,
        "released unread, not read"
    );

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

/// Done (from unread, read or snoozed) leaves the unread index for good, clearing the
/// snooze and its note and keeping `read_at`; done again is idempotent; nothing leaves done.
pub fn triage_done_transitions(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    for n in 1..=4 {
        unread(&store, n);
    }
    store
        .claim_read(ADA_ID, Some(&msg_id(2)), NOW_UTC)
        .unwrap()
        .unwrap();
    apply(&store, snooze_until(3, &at(60_000), Some("later")));
    apply(
        &store,
        LocalStateTransition {
            reason: Some("after this".into()),
            ..snooze_after_task(4)
        },
    );
    for n in 1..=4 {
        let done = LocalStateTransition {
            reason: Some("handled".into()),
            ..change(n, State::Done)
        };
        let header = apply(&store, done);
        assert_eq!(header.state, State::Done, "message {n}");
        assert_eq!(header.snoozed_until, None, "message {n}");
        assert!(!header.snooze_after_task, "message {n}");
        assert_eq!(
            header.snooze_note, None,
            "message {n}: done cleared the note"
        );
        let read_at = (n == 2).then_some(NOW_UTC);
        assert_eq!(
            header.read_at.as_deref(),
            read_at,
            "message {n}: read_at kept"
        );
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

/// Read (from unread or snoozed) leaves the unread index, clearing the snooze and its note,
/// and stamps `read_at`; read again is idempotent, keeping the first `read_at`.
pub fn triage_read_transitions(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    for n in 1..=3 {
        unread(&store, n);
    }
    let header = apply(&store, change(1, State::Read));
    assert_eq!(header.state, State::Read);
    assert_eq!(header.read_at.as_deref(), Some(NOW_UTC));

    apply(
        &store,
        LocalStateTransition {
            reason: Some("after this".into()),
            ..snooze_after_task(2)
        },
    );
    apply(&store, snooze_until(3, &at(60_000), Some("later")));
    let later = at(5_000);
    for n in 2..=3 {
        let header = apply(
            &store,
            LocalStateTransition {
                now_utc: later.clone(),
                ..change(n, State::Read)
            },
        );
        assert_eq!(header.state, State::Read, "message {n}");
        assert_eq!(header.read_at.as_deref(), Some(later.as_str()));
        assert!(
            !header.snooze_after_task,
            "message {n}: reading cleared the snooze"
        );
        assert_eq!(header.snoozed_until, None, "message {n}");
        assert_eq!(
            header.snooze_note, None,
            "message {n}: reading cleared the note"
        );
    }
    assert!(unread_ids(&store).is_empty());

    let before = header_of(&store, 1);
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
    assert_eq!(again, before, "read again leaves the header unchanged");
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
    // Only an unread message is flagged: a read one is never reopened for it.
    unread(&store, 4);
    apply(&store, change(4, State::Read));
    let reopen = LocalStateTransition {
        expired_waiting_person: true,
        ..change(4, State::Unread)
    };
    assert!(matches!(
        refused(&store, reopen),
        StoreError::InvalidTransition
    ));
    assert_eq!(state_of(&store, &msg_id(4)), State::Read);

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

/// Expired (from unread or snoozed) leaves the unread index, clearing the snooze and its
/// note; expired again is idempotent; a read message can't expire (and keeps its `read_at`)
/// and nothing leaves expired.
pub fn triage_expired_transitions(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    commit(&store, expired_request(1, PAT_ID, ADA_ID));
    commit(&store, expired_request(2, PAT_ID, ADA_ID));
    unread(&store, 3);
    commit(&store, expired_request(4, PAT_ID, ADA_ID));
    apply(
        &store,
        LocalStateTransition {
            reason: Some("after this".into()),
            ..snooze_after_task(4)
        },
    );
    assert_eq!(peek(&store, ADA_ID).expired_ids, [msg_id(1), msg_id(2)]);

    let header = apply(&store, change(1, State::Expired));
    assert_eq!(header.state, State::Expired);
    assert_eq!(header.read_at, None);
    apply(&store, snooze_until(2, &at(60_000), Some("later")));
    for n in [2, 4] {
        let header = apply(&store, change(n, State::Expired));
        assert_eq!(header.state, State::Expired, "message {n}");
        assert_eq!(header.snoozed_until, None, "message {n}");
        assert!(!header.snooze_after_task, "message {n}");
        assert_eq!(
            header.snooze_note, None,
            "message {n}: expiring cleared the note"
        );
        assert_eq!(header.read_at, None, "message {n}");
    }
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
    let header = header_of(&store, 3);
    assert_eq!(header.state, State::Read);
    assert_eq!(header.read_at.as_deref(), Some(NOW_UTC), "read_at kept");
}

/// `triage_gen` goes up by exactly 1 on every snooze (re-snoozes included) and on each
/// release by check_notify; read, done, expired and unread leave it unchanged.
pub fn triage_gen_moves_only_on_snooze_and_release(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    unread(&store, 1);
    let gen_of = |n: u32| header_of(&store, n).triage_gen;
    apply(&store, change(1, State::Read));
    assert_eq!(gen_of(1), 0, "read");
    apply(&store, change(1, State::Unread));
    assert_eq!(gen_of(1), 0, "unread from read");
    apply(&store, snooze_until(1, &at(1_000), None));
    assert_eq!(gen_of(1), 1, "snooze");
    apply(&store, snooze_until(1, &at(2_000), None));
    assert_eq!(gen_of(1), 2, "re-snooze");
    apply(&store, snooze_after_task(1));
    assert_eq!(gen_of(1), 3, "re-snooze to after-task");
    assert_eq!(
        notify(&store, true, false, NOW_UTC, NOW_UTC),
        [(msg_id(1), NotifyReason::AfterTask)]
    );
    assert_eq!(gen_of(1), 4, "after-task release");
    apply(&store, snooze_until(1, &at(1_000), None));
    assert_eq!(gen_of(1), 5);
    assert_eq!(
        notify(&store, false, false, NOW_UTC, &at(1_000)),
        [(msg_id(1), NotifyReason::Due)]
    );
    assert_eq!(gen_of(1), 6, "due release");
    apply(&store, change(1, State::Read));
    assert_eq!(gen_of(1), 6, "read");
    apply(&store, change(1, State::Done));
    apply(&store, change(1, State::Done));
    assert_eq!(gen_of(1), 6, "done, and done again");

    commit(&store, expired_request(2, PAT_ID, ADA_ID));
    apply(&store, snooze_after_task(2));
    apply(&store, change(2, State::Expired));
    apply(&store, change(2, State::Expired));
    assert_eq!(gen_of(2), 1, "expired, and expired again");
    let mut ask = expired_request(3, ADA_ID, PAT_ID);
    ask.message.header.to_display_name = PAT_HANDLE.into();
    commit(&store, ask);
    let flag = LocalStateTransition {
        to_member_id: PAT_ID.into(),
        expired_waiting_person: true,
        ..change(3, State::Unread)
    };
    apply(&store, flag);
    assert_eq!(gen_of(3), 0, "unread, flagging");
}

/// `expired_waiting_person`, once set, survives every later transition, release and claim:
/// nothing clears it.
pub fn triage_expired_waiting_person_is_never_cleared(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    let for_pat = |change: LocalStateTransition| LocalStateTransition {
        to_member_id: PAT_ID.into(),
        ..change
    };
    for n in [1, 2] {
        let mut ask = expired_request(n, ADA_ID, PAT_ID);
        ask.message.header.to_display_name = PAT_HANDLE.into();
        commit(&store, ask);
        let flag = LocalStateTransition {
            expired_waiting_person: true,
            ..for_pat(change(n, State::Unread))
        };
        assert!(apply(&store, flag).expired_waiting_person);
    }
    let flagged = |n: u32| header_of(&store, n).expired_waiting_person;
    let steps = [
        for_pat(snooze_until(1, &at(1_000), Some("later"))),
        for_pat(snooze_after_task(1)),
    ];
    for step in steps {
        let state = step.state;
        assert!(apply(&store, step).expired_waiting_person, "{state:?}");
    }
    let released = store
        .check_notify(PAT_ID, true, false, NOW_UTC, NOW_UTC)
        .unwrap();
    assert!(released.iter().any(
        |candidate| candidate.header.id == msg_id(1) && candidate.header.expired_waiting_person
    ));
    assert!(flagged(1), "released");
    let claimed = store
        .claim_read(PAT_ID, Some(&msg_id(1)), NOW_UTC)
        .unwrap()
        .unwrap();
    assert!(claimed.message.header.expired_waiting_person, "claimed");
    let unflagged_unread = for_pat(change(1, State::Unread));
    assert!(
        apply(&store, unflagged_unread).expired_waiting_person,
        "unread from read"
    );
    assert!(apply(&store, for_pat(change(1, State::Read))).expired_waiting_person);
    assert!(apply(&store, for_pat(change(1, State::Done))).expired_waiting_person);
    assert!(flagged(1), "done");
    assert!(apply(&store, for_pat(change(2, State::Expired))).expired_waiting_person);
    assert!(flagged(2), "expired");
}

/// `reason` is the snooze note for `Snoozed` (and the done note for `Done`, which no port
/// reads back) and is ignored for every other target: read, unread and expired with a reason
/// leave the same header as without one.
pub fn triage_reason_is_only_the_snooze_or_done_note(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    let with_note = |change: LocalStateTransition| LocalStateTransition {
        reason: Some("a note".into()),
        ..change
    };
    unread(&store, 1);
    unread(&store, 2);
    let header = apply(&store, with_note(change(1, State::Read)));
    assert_eq!(header.snooze_note, None, "read");
    let plain = apply(&store, change(2, State::Read));
    assert_eq!(
        (header.state, header.read_at, header.triage_gen),
        (plain.state, plain.read_at, plain.triage_gen)
    );
    let header = apply(&store, with_note(change(1, State::Unread)));
    assert_eq!(
        (header.state, header.snooze_note, header.read_at),
        (State::Unread, None, None),
        "unread"
    );
    let header = apply(&store, with_note(snooze_after_task(1)));
    assert_eq!(header.snooze_note.as_deref(), Some("a note"), "snoozed");
    let header = apply(&store, with_note(change(1, State::Done)));
    assert_eq!(
        (header.state, header.snooze_note),
        (State::Done, None),
        "done: the note isn't the snooze note"
    );
    commit(&store, expired_request(3, PAT_ID, ADA_ID));
    let header = apply(&store, with_note(change(3, State::Expired)));
    assert_eq!((header.state, header.snooze_note), (State::Expired, None));
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

/// check_notify returns each message at most once: a snooze released in the call carries
/// its release reason even when the message is urgent or `after_task` is set, and is never
/// also returned as `New`.
pub fn triage_check_notify_returns_a_release_once_with_its_reason(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    for n in 1..=3 {
        commit(
            &store,
            new_message(n, PAT_ID, ADA_ID, MessagePriority::Urgent),
        );
    }
    let now = at(120_000);
    apply(&store, snooze_until(1, &at(60_000), None));
    assert_eq!(
        notify(&store, false, false, NOW_UTC, &now),
        [
            (msg_id(1), NotifyReason::Due),
            (msg_id(2), NotifyReason::New),
            (msg_id(3), NotifyReason::New),
        ],
        "a due urgent snooze comes back once, as Due"
    );

    apply(&store, snooze_until(2, &at(60_000), None));
    apply(&store, snooze_after_task(3));
    assert_eq!(
        notify(&store, true, false, NOW_UTC, &now),
        [
            (msg_id(1), NotifyReason::New),
            (msg_id(2), NotifyReason::Due),
            (msg_id(3), NotifyReason::AfterTask),
        ],
        "at a break, a due urgent snooze comes back once as Due, an after-task one once as \
         AfterTask"
    );
    for n in 1..=3 {
        assert_eq!(header_of(&store, n).state, State::Unread, "message {n}");
    }
}

/// A time snooze whose `snoozed_until` can't be read is released as due by the next
/// check_notify, whatever `now` is, rather than stranded. A store may instead refuse to write
/// such a snooze, but only with `StoreError::InvalidTransition` and writing nothing; then it
/// has nothing to release. Any other error fails the case.
pub fn triage_check_notify_releases_an_unreadable_snooze_as_due(backend: impl OpenTriageBackend) {
    let store = open_store(&backend);
    unread(&store, 1);
    let before = header_of(&store, 1);
    match store.update_state_local(snooze_until(1, "not a time", None)) {
        Ok(_) => {}
        Err(StoreError::InvalidTransition) => {
            assert_eq!(header_of(&store, 1), before, "a refusal writes nothing");
            return;
        }
        Err(error) => panic!("an unreadable snoozed_until refused with {error:?}"),
    }
    assert_eq!(header_of(&store, 1).state, State::Snoozed);
    assert_eq!(
        notify(&store, false, false, NOW_UTC, NOW_UTC),
        [(msg_id(1), NotifyReason::Due)]
    );
    let header = header_of(&store, 1);
    assert_eq!((header.state, header.triage_gen), (State::Unread, 2));
    assert_eq!(header.snoozed_until, None);
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

// ---------------------------------------------------------------------------------------
// Mailbox triage over the backend
// ---------------------------------------------------------------------------------------

/// A Mailbox with triage over the backend, at HQ, with a manual clock and recorded events.
struct TriageOffice<S> {
    mailbox: Arc<Mailbox>,
    store: Arc<S>,
    events: Arc<RecordingEvents>,
    clock: Arc<ManualClock>,
}

fn identity() -> OfficeIdentity {
    OfficeIdentity {
        office_id: OFFICE_ID.into(),
        incarnation: INCARNATION.into(),
        is_hq: true,
    }
}

fn office_over<S, D>(store: S, directory: D) -> TriageOffice<S>
where
    S: MessageStore + TriageStore + Send + Sync + 'static,
    D: MemberDirectory + Send + Sync + 'static,
{
    let store = Arc::new(store);
    let events = Arc::new(RecordingEvents::default());
    let clock = Arc::new(ManualClock::new(NOW_MS));
    let mailbox = Mailbox::with_triage(
        store.clone(),
        store.clone(),
        Arc::new(directory),
        identity(),
        clock.clone(),
        events.clone(),
    )
    .expect("with_triage");
    TriageOffice {
        mailbox: Arc::new(mailbox),
        store,
        events,
        clock,
    }
}

fn triage_office<B: OpenTriageBackend>(backend: &B) -> TriageOffice<B::Store> {
    let (store, directory) = backend.open(&members(), POSITION);
    office_over(store, directory)
}

fn defer_until(n: u32, until: &str, note: Option<&str>) -> MsgDeferParams {
    MsgDeferParams {
        id: msg_id(n),
        until: SnoozeTarget::Until { at: until.into() },
        note: note.map(Into::into),
    }
}

fn done_params(n: u32, note: Option<&str>) -> MsgDoneParams {
    MsgDoneParams {
        id: msg_id(n),
        note: note.map(Into::into),
    }
}

fn wait_now(mailbox: &Mailbox, member: &str) -> Option<String> {
    let (_handle, cancel) = cancellation_pair();
    mailbox
        .wait(member, wait_params(0), far_deadline(), &cancel)
        .expect("wait")
        .message
        .map(|message| message.header.id)
}

fn read_next(mailbox: &Mailbox, member: &str) -> Option<String> {
    mailbox
        .read(member, MsgReadParams::default())
        .expect("read")
        .message
        .map(|message| message.header.id)
}

/// Holds a wait for `member` (5 s) on another thread, until it is registered and blocked.
fn held_wait(
    office: &TriageOffice<impl Send + Sync + 'static>,
    member: &'static str,
) -> thread::JoinHandle<(Result<MsgWaitResult, MailboxError>, Duration)> {
    let mailbox = office.mailbox.clone();
    let worker = thread::spawn(move || {
        let (_handle, cancel) = cancellation_pair();
        let started = Instant::now();
        let result = mailbox.wait(member, wait_params(5_000), far_deadline(), &cancel);
        (result, started.elapsed())
    });
    eventually("the wait to register", || office.mailbox.held_waits() == 1);
    // Let it reach its block (a wait still checking would pass anyway, by its first peek).
    thread::sleep(Duration::from_millis(50));
    worker
}

/// `msg.defer` with a time hides the message from the default inbox, from wait and from
/// read until it's due, then a held wait returns it with no new commit: the snooze timer,
/// not a commit, wakes it. A timer that fires before the clock reaches the deadline loops
/// round; the wait still ends only at its own timeout.
pub fn mailbox_defer_hides_until_due_then_a_held_wait_gets_it(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    let mailbox = &office.mailbox;
    send(mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    let until = at(250);
    let deferred = mailbox
        .defer(ADA_ID, defer_until(1, &until, Some("after the build")))
        .unwrap();
    assert_eq!(deferred.id, msg_id(1));
    assert_eq!(deferred.triage_gen, 1);
    assert_eq!(deferred.snoozed_until.as_deref(), Some(until.as_str()));
    assert!(!deferred.snooze_after_task);
    assert_eq!(deferred.snooze_note.as_deref(), Some("after the build"));
    assert!(unread_inbox(mailbox, ADA_ID).is_empty());
    let all = mailbox
        .inbox(
            ADA_ID,
            MsgInboxParams {
                include_all: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(all.headers[0].state, State::Snoozed);
    assert_eq!(wait_now(mailbox, ADA_ID), None);
    assert_eq!(read_next(mailbox, ADA_ID), None);
    let changed = office.events.state_changes();
    assert_eq!(changed.len(), 1);
    assert_eq!(
        (changed[0].state, changed[0].reason.as_deref()),
        (State::Snoozed, Some("after the build"))
    );

    // The wall clock hasn't moved: the timer fires (after ~250 ms) but nothing is due, so the
    // wait goes on to its own timeout rather than returning early.
    let (_handle, cancel) = cancellation_pair();
    let started = Instant::now();
    let waited = mailbox
        .wait(ADA_ID, wait_params(600), far_deadline(), &cancel)
        .unwrap();
    assert_eq!(waited.message, None);
    assert!(started.elapsed() >= Duration::from_millis(600));

    let worker = held_wait(&office, ADA_ID);
    office.clock.set(NOW_MS + 250);
    let (result, elapsed) = worker.join().unwrap();
    let message = result.unwrap().message.expect("the due snooze is returned");
    assert_eq!(message.header.id, msg_id(1));
    assert_eq!(message.header.state, State::Unread);
    assert_eq!(message.header.triage_gen, 2);
    assert!(
        elapsed < Duration::from_secs(4),
        "woken by the timer: {elapsed:?}"
    );
    assert_eq!(office.events.received(), [msg_id(1)], "no new commit");
    let released = office.events.state_changes();
    assert_eq!(released.len(), 2);
    assert_eq!(
        (released[1].state, released[1].triage_gen),
        (State::Unread, 2)
    );
    assert_eq!(unread_inbox(mailbox, ADA_ID), [msg_id(1)]);
    assert_eq!(office.mailbox.held_waits(), 0);
}

/// A defer while a wait is held wakes it to take up the new timer: a message read and then
/// deferred comes back to the held wait when due, long before the wait's own timeout.
pub fn mailbox_defer_wakes_a_held_wait_to_take_its_timer(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    send(&office.mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    assert_eq!(read_next(&office.mailbox, ADA_ID), Some(msg_id(1)));
    let worker = held_wait(&office, ADA_ID);
    // From read: deferring puts it back, snoozed (§2.3).
    office
        .mailbox
        .defer(ADA_ID, defer_until(1, &at(250), None))
        .unwrap();
    office.clock.set(NOW_MS + 250);
    let (result, elapsed) = worker.join().unwrap();
    assert_eq!(
        result.unwrap().message.map(|message| message.header.id),
        Some(msg_id(1))
    );
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}");
}

/// The snooze timers are seeded from the store: a time snooze made before this Mailbox
/// existed still comes back to a held wait.
pub fn mailbox_with_triage_seeds_snooze_timers(backend: impl OpenTriageBackend) {
    let (store, directory) = backend.open(&members(), POSITION);
    unread(&store, 1);
    apply(&store, snooze_until(1, &at(250), None));
    let office = office_over(store, directory);
    let worker = held_wait(&office, ADA_ID);
    office.clock.set(NOW_MS + 250);
    let (result, elapsed) = worker.join().unwrap();
    assert_eq!(
        result.unwrap().message.map(|message| message.header.id),
        Some(msg_id(1))
    );
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}");
}

/// An after-task snooze isn't a timer: it never comes back on time, to wait or read.
pub fn mailbox_after_task_snooze_never_fires_on_time(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    send(&office.mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    let deferred = office
        .mailbox
        .defer(
            ADA_ID,
            MsgDeferParams {
                id: msg_id(1),
                until: SnoozeTarget::AfterTask,
                note: None,
            },
        )
        .unwrap();
    assert!(deferred.snooze_after_task);
    assert_eq!(deferred.snoozed_until, None);
    office.clock.set(NOW_MS + 86_400_000);
    let (_handle, cancel) = cancellation_pair();
    let waited = office
        .mailbox
        .wait(ADA_ID, wait_params(100), far_deadline(), &cancel)
        .unwrap();
    assert_eq!(waited.message, None);
    assert_eq!(read_next(&office.mailbox, ADA_ID), None);
    assert!(unread_inbox(&office.mailbox, ADA_ID).is_empty());
    assert_eq!(state_of(&*office.store, &msg_id(1)), State::Snoozed);
}

/// `msg.done` closes a message (unread, read or snoozed) for good: it never resurfaces, even
/// when its snooze would have come due; done again is accepted; a done message can't be
/// deferred.
pub fn mailbox_done_never_resurfaces(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    let mailbox = &office.mailbox;
    send(mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    send(mailbox, PAT_ID, 2, ADA_HANDLE).unwrap();
    mailbox
        .defer(ADA_ID, defer_until(2, &at(250), None))
        .unwrap();
    let done = mailbox
        .done(ADA_ID, done_params(1, Some("answered in chat")))
        .unwrap();
    assert_eq!(
        (done.id.as_str(), done.resolved_at.as_str()),
        (msg_id(1).as_str(), NOW_UTC)
    );
    mailbox.done(ADA_ID, done_params(2, None)).unwrap();
    let changed = office.events.state_changes();
    let closed = changed.iter().find(|data| data.id == msg_id(1)).unwrap();
    assert_eq!(
        (closed.state, closed.reason.as_deref()),
        (State::Done, Some("answered in chat"))
    );

    office.clock.set(NOW_MS + 3_600_000);
    let (_handle, cancel) = cancellation_pair();
    let waited = mailbox
        .wait(ADA_ID, wait_params(300), far_deadline(), &cancel)
        .unwrap();
    assert_eq!(waited.message, None, "a done snooze never fires");
    assert_eq!(read_next(mailbox, ADA_ID), None);
    assert!(unread_inbox(mailbox, ADA_ID).is_empty());
    for n in [1, 2] {
        assert_eq!(state_of(&*office.store, &msg_id(n)), State::Done);
    }
    assert!(
        mailbox.done(ADA_ID, done_params(1, None)).is_ok(),
        "done again"
    );
    assert_eq!(
        mailbox.defer(ADA_ID, defer_until(1, &at(7_200_000), None)),
        Err(MailboxError::InvalidRequest)
    );
    assert_eq!(state_of(&*office.store, &msg_id(1)), State::Done);
}

/// defer and done refuse what msg.rs refuses: an oversized id or note is `too_large`; a past,
/// present, unreadable or unknown target is `invalid_request`; another member's message or an
/// unknown id is `not_found`; an unbound member is `not_bound`. A Mailbox without triage
/// answers `unavailable`. Nothing refused changes the message.
pub fn mailbox_defer_and_done_validation(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    let mailbox = &office.mailbox;
    send(mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    send(mailbox, ADA_ID, 2, REMY_HANDLE).unwrap();
    // json_string_len counts the quotes: a string of len - 2 bytes fits exactly.
    let over = |limit: usize| "x".repeat(limit - 1);
    let later = at(60_000);
    use MailboxError::{InvalidRequest, NotBound, NotFound, TooLarge};
    let defers = [
        (
            MsgDeferParams {
                id: over(MAX_ID_BYTES),
                ..defer_until(1, &later, None)
            },
            TooLarge,
        ),
        (
            defer_until(1, &later, Some(&over(MAX_NOTE_BYTES))),
            TooLarge,
        ),
        (defer_until(1, NOW_UTC, None), InvalidRequest),
        (defer_until(1, &at(-1), None), InvalidRequest),
        (defer_until(1, "tomorrow", None), InvalidRequest),
        (defer_until(1, "2026-11-08T13:00:00Z", None), InvalidRequest),
        (
            MsgDeferParams {
                id: msg_id(1),
                until: SnoozeTarget::Unknown,
                note: None,
            },
            InvalidRequest,
        ),
        (defer_until(2, &later, None), NotFound),
        (defer_until(9, &later, None), NotFound),
    ];
    for (params, error) in defers {
        assert_eq!(
            mailbox.defer(ADA_ID, params.clone()),
            Err(error),
            "{params:?}"
        );
    }
    assert_eq!(
        mailbox.defer(GONE_ID, defer_until(1, &later, None)),
        Err(NotBound)
    );
    assert_eq!(
        mailbox.done(ADA_ID, done_params(1, Some(&over(MAX_NOTE_BYTES)))),
        Err(TooLarge)
    );
    assert_eq!(
        mailbox.done(
            ADA_ID,
            MsgDoneParams {
                id: over(MAX_ID_BYTES),
                note: None
            }
        ),
        Err(TooLarge)
    );
    assert_eq!(mailbox.done(ADA_ID, done_params(2, None)), Err(NotFound));
    assert_eq!(mailbox.done(ADA_ID, done_params(9, None)), Err(NotFound));
    assert_eq!(mailbox.done(GONE_ID, done_params(1, None)), Err(NotBound));
    let header = header_of(&*office.store, 1);
    assert_eq!((header.state, header.triage_gen), (State::Unread, 0));
    assert!(office.events.state_changes().is_empty());

    // At the note limit it applies.
    let at_limit = "n".repeat(MAX_NOTE_BYTES - 2);
    mailbox
        .defer(ADA_ID, defer_until(1, &later, Some(&at_limit)))
        .unwrap();

    let (store, directory) = backend.open(&members(), POSITION);
    let plain = Mailbox::new(
        Arc::new(store),
        Arc::new(directory),
        identity(),
        Arc::new(ManualClock::new(NOW_MS)),
        Arc::new(RecordingEvents::default()),
    );
    send(&plain, PAT_ID, 1, ADA_HANDLE).unwrap();
    assert_eq!(
        plain.defer(ADA_ID, defer_until(1, &later, None)),
        Err(MailboxError::Unavailable)
    );
    assert_eq!(
        plain.done(ADA_ID, done_params(1, None)),
        Err(MailboxError::Unavailable)
    );
    // Unavailable comes before the input checks.
    assert_eq!(
        plain.defer(ADA_ID, defer_until(1, &at(-1), None)),
        Err(MailboxError::Unavailable)
    );
    assert_eq!(
        plain.done(ADA_ID, done_params(1, Some(&over(MAX_NOTE_BYTES)))),
        Err(MailboxError::Unavailable)
    );
    assert_eq!(
        plain.defer(
            ADA_ID,
            MsgDeferParams {
                id: over(MAX_ID_BYTES),
                ..defer_until(1, &later, None)
            }
        ),
        Err(MailboxError::Unavailable)
    );
}

/// A store whose peek keeps reporting every id it once reported as expired, as a second
/// worker that raced the first, or a peek from an older snapshot, would: the Mailbox's
/// expiry must stay idempotent.
struct StalePeek<S> {
    inner: S,
    reported: Mutex<Vec<String>>,
}

impl<S: MessageStore> MessageStore for StalePeek<S> {
    fn commit_message(&self, msg: NewStoredMessage) -> Result<CommitMessageOutcome, StoreError> {
        self.inner.commit_message(msg)
    }
    fn claim_read(
        &self,
        to_member_id: &str,
        id: Option<&str>,
        now_utc: &str,
    ) -> Result<Option<StoredMessage>, StoreError> {
        self.inner.claim_read(to_member_id, id, now_utc)
    }
    fn peek_next_unread(
        &self,
        to_member_id: &str,
        from_member_id: Option<&str>,
        urgent_only: bool,
        now_utc: &str,
    ) -> Result<PeekUnreadOutcome, StoreError> {
        let mut outcome =
            self.inner
                .peek_next_unread(to_member_id, from_member_id, urgent_only, now_utc)?;
        let mut reported = self
            .reported
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for id in &outcome.expired_ids {
            if !reported.contains(id) {
                reported.push(id.clone());
            }
        }
        outcome.expired_ids = reported.clone();
        Ok(outcome)
    }
    fn get_message(&self, id: &str) -> Result<Option<StoredMessage>, StoreError> {
        self.inner.get_message(id)
    }
    fn list_headers(&self, query: HeaderQuery) -> Result<HeaderPage, StoreError> {
        self.inner.list_headers(query)
    }
    fn find_reply_for_ask(
        &self,
        ask_id: &str,
        responder_id: &str,
    ) -> Result<Option<StoredMessage>, StoreError> {
        self.inner.find_reply_for_ask(ask_id, responder_id)
    }
}

impl<S: TriageStore> TriageStore for StalePeek<S> {
    fn update_state_local(
        &self,
        change: LocalStateTransition,
    ) -> Result<MessageHeader, StoreError> {
        self.inner.update_state_local(change)
    }
    fn check_notify(
        &self,
        to_member_id: &str,
        after_task: bool,
        urgent_only: bool,
        sweep_before_utc: &str,
        now_utc: &str,
    ) -> Result<Vec<NotifyCandidate>, StoreError> {
        self.inner.check_notify(
            to_member_id,
            after_task,
            urgent_only,
            sweep_before_utc,
            now_utc,
        )
    }
    fn list_active_snooze_deadlines(&self) -> Result<Vec<SnoozeDeadline>, StoreError> {
        self.inner.list_active_snooze_deadlines()
    }
}

/// The asker's `delivery_failed` for `ask`, checked against ADR-0003 §3.2 item 3.
fn expect_notice(store: &impl MessageStore, ask: u32, asker: &str, asker_name: &str) -> String {
    let notice = store
        .find_reply_for_ask(&msg_id(ask), ADA_ID)
        .unwrap()
        .expect("the asker got a delivery_failed");
    let request = store
        .get_message(&msg_id(ask))
        .unwrap()
        .unwrap()
        .message
        .header;
    let header = &notice.message.header;
    assert_eq!(header.from_member_id, OFFICE_SENDER);
    assert_eq!(header.to_member_id, asker);
    assert_eq!(header.to_display_name, asker_name);
    assert_eq!(header.kind, KIND_DELIVERY_FAILED);
    assert_eq!(header.priority, MessagePriority::High);
    assert!(header.urgent);
    assert_eq!(header.reply_to.as_deref(), Some(msg_id(ask).as_str()));
    assert_eq!(header.thread_id, request.thread_id);
    assert_eq!(header.state, State::Unread);
    assert_eq!(header.expires_at, None);
    // A ULID timed at the request's deadline: the same request always gets the same id.
    assert_eq!(
        ulid_time_ms(&header.id),
        request.expires_at.as_deref().and_then(parse_utc)
    );
    let meta = &notice.message.meta;
    assert_eq!(meta["recipient_id"], ADA_ID);
    assert_eq!(meta["recipient_name"], ADA_NAME);
    assert_eq!(meta["reason"], "expired");
    let original_body = store
        .get_message(&msg_id(ask))
        .unwrap()
        .unwrap()
        .message
        .body;
    assert!(
        !notice.message.body.contains(&original_body),
        "the notice holds no message content"
    );
    header.id.clone()
}

/// An agent's request past its deadline, passed over by wait, is expired, and its asker gets
/// exactly one `delivery_failed` (message, `message.received` and `message.delivery_failed`)
/// even when the expiry runs again; the notice wakes the asker's wait.
pub fn mailbox_wait_expires_an_agent_request_and_tells_the_asker_once(
    backend: impl OpenTriageBackend,
) {
    let (store, directory) = backend.open(&members(), POSITION);
    let office = office_over(
        StalePeek {
            inner: store,
            reported: Mutex::new(Vec::new()),
        },
        directory,
    );
    let mut ask = expired_request(1, PAT_ID, ADA_ID);
    ask.message.header.to_display_name = ADA_NAME.into();
    commit(&*office.store, ask);
    send(&office.mailbox, PAT_ID, 2, ADA_HANDLE).unwrap();
    // The asker is waiting: the notice must wake it.
    let asker = held_wait(&office, PAT_ID);

    assert_eq!(wait_now(&office.mailbox, ADA_ID), Some(msg_id(2)));
    let (woken, elapsed) = asker.join().unwrap();
    assert!(
        elapsed < Duration::from_secs(4),
        "woken by the notice: {elapsed:?}"
    );
    let woken = woken
        .unwrap()
        .message
        .expect("the asker's wait gets the notice");
    assert_eq!(woken.header.kind, KIND_DELIVERY_FAILED);
    assert_eq!(state_of(&*office.store, &msg_id(1)), State::Expired);
    let notice = expect_notice(&*office.store, 1, PAT_ID, PAT_HANDLE);
    let failed = office.events.delivery_failed();
    assert_eq!(failed.len(), 1);
    let data = &failed[0];
    assert_eq!(data.id, notice);
    assert_eq!(data.original_id, msg_id(1));
    assert_eq!(data.from_member_id, PAT_ID);
    assert_eq!(
        (data.recipient_id.as_str(), data.recipient_name.as_str()),
        (ADA_ID, ADA_NAME)
    );
    assert_eq!(data.reason, DeliveryFailedReason::Expired);
    assert!(office
        .events
        .state_changes()
        .iter()
        .any(|data| data.id == msg_id(1) && data.state == State::Expired));

    // Again (the stale peek reports the request again): no second notice or event.
    assert_eq!(wait_now(&office.mailbox, ADA_ID), Some(msg_id(2)));
    assert_eq!(read_next(&office.mailbox, ADA_ID), Some(msg_id(2)));
    assert_eq!(office.events.delivery_failed().len(), 1);
    assert_eq!(
        office
            .events
            .received()
            .iter()
            .filter(|id| **id == notice)
            .count(),
        1
    );
    assert_eq!(
        unread_inbox(&office.mailbox, PAT_ID),
        std::slice::from_ref(&notice)
    );
    assert_eq!(wait_now(&office.mailbox, PAT_ID), Some(notice));
}

/// An ask to a person never expires: wait flags it `expired_waiting_person` and returns it,
/// and no one is told. A request from a sender who can no longer receive (offboarded) is
/// expired without a notice.
pub fn mailbox_expiry_spares_persons_and_unmessageable_senders(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    let mut to_person = expired_request(1, ADA_ID, PAT_ID);
    to_person.message.header.to_display_name = PAT_HANDLE.into();
    commit(&*office.store, to_person);
    let waited = {
        let (_handle, cancel) = cancellation_pair();
        office
            .mailbox
            .wait(PAT_ID, wait_params(0), far_deadline(), &cancel)
            .unwrap()
            .message
            .expect("a person's overdue ask is still theirs to answer")
    };
    assert_eq!(waited.header.id, msg_id(1));
    assert_eq!(waited.header.state, State::Unread);
    assert!(waited.header.expired_waiting_person);
    assert!(office
        .events
        .state_changes()
        .iter()
        .any(|data| data.id == msg_id(1) && data.expired_waiting_person));
    assert_eq!(read_next(&office.mailbox, PAT_ID), Some(msg_id(1)));
    // read with no id flags one too, then claims it.
    commit(&*office.store, expired_request(3, ADA_ID, PAT_ID));
    assert_eq!(read_next(&office.mailbox, PAT_ID), Some(msg_id(3)));
    assert!(header_of(&*office.store, 3).expired_waiting_person);

    // An overdue ask older than a new message still comes first.
    commit(&*office.store, expired_request(4, ADA_ID, PAT_ID));
    send(&office.mailbox, OWNER_ID, 5, PAT_HANDLE).unwrap();
    assert_eq!(wait_now(&office.mailbox, PAT_ID), Some(msg_id(4)));
    // Read by id, a person's overdue ask is read, not expired.
    let read = office
        .mailbox
        .read(
            PAT_ID,
            MsgReadParams {
                id: Some(msg_id(4)),
            },
        )
        .unwrap()
        .message
        .unwrap();
    assert_eq!(read.header.state, State::Read);

    commit(&*office.store, expired_request(2, GONE_ID, ADA_ID));
    assert_eq!(wait_now(&office.mailbox, ADA_ID), None);
    assert_eq!(state_of(&*office.store, &msg_id(2)), State::Expired);
    assert!(office.events.delivery_failed().is_empty());
    assert_eq!(office.events.received(), [msg_id(5)]);
}

/// A directory that fails to resolve one reference while `failing` is set.
struct FlakyDirectory<D> {
    inner: D,
    reference: &'static str,
    failing: std::sync::atomic::AtomicBool,
}

impl<D: MemberDirectory> MemberDirectory for FlakyDirectory<D> {
    fn resolve_member(&self, reference: &str) -> Result<Option<(Member, Position)>, StoreError> {
        if reference == self.reference && self.failing.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(StoreError::new("directory unavailable"));
        }
        self.inner.resolve_member(reference)
    }
}

/// A notice that fails to commit doesn't fail the recipient's wait (the request is still
/// expired); a read of the request by id retries the notice, and fails while it can't be sent.
pub fn mailbox_a_failed_notice_does_not_fail_the_recipient(backend: impl OpenTriageBackend) {
    let (store, directory) = backend.open(&members(), POSITION);
    let directory = Arc::new(FlakyDirectory {
        inner: directory,
        reference: PAT_ID,
        failing: std::sync::atomic::AtomicBool::new(true),
    });
    let store = Arc::new(store);
    let events = Arc::new(RecordingEvents::default());
    let mailbox = Mailbox::with_triage(
        store.clone(),
        store.clone(),
        directory.clone(),
        identity(),
        Arc::new(ManualClock::new(NOW_MS)),
        events.clone(),
    )
    .unwrap();
    let mut ask = expired_request(1, PAT_ID, ADA_ID);
    ask.message.header.to_display_name = ADA_NAME.into();
    commit(&*store, ask);
    commit(
        &*store,
        new_message(2, OWNER_ID, ADA_ID, MessagePriority::Normal),
    );

    assert_eq!(wait_now(&mailbox, ADA_ID), Some(msg_id(2)));
    assert_eq!(state_of(&*store, &msg_id(1)), State::Expired);
    assert_eq!(store.find_reply_for_ask(&msg_id(1), ADA_ID).unwrap(), None);
    let by_id = MsgReadParams {
        id: Some(msg_id(1)),
    };
    assert_eq!(
        mailbox.read(ADA_ID, by_id.clone()),
        Err(MailboxError::StorageError)
    );
    directory
        .failing
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let read = mailbox
        .read(ADA_ID, by_id.clone())
        .unwrap()
        .message
        .unwrap();
    assert_eq!(read.header.state, State::Expired);
    expect_notice(&*store, 1, PAT_ID, PAT_HANDLE);
    assert_eq!(events.delivery_failed().len(), 1);
    // Once the notice exists, a read by id only looks it up: no directory, no commit.
    directory
        .failing
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let again = mailbox.read(ADA_ID, by_id).unwrap().message.unwrap();
    assert_eq!(again.header.state, State::Expired);
}

/// The ids of the `delivery_failed` notices replying to `msg_id(ask)` in Pat's inbox.
fn notices_for(store: &impl MessageStore, ask: u32) -> Vec<String> {
    store
        .list_headers(inbox_query(PAT_ID, false, 100))
        .unwrap()
        .headers
        .into_iter()
        .filter(|header| {
            header.kind == KIND_DELIVERY_FAILED
                && header.reply_to.as_deref() == Some(msg_id(ask).as_str())
        })
        .map(|header| header.id)
        .collect()
}

/// Commits `msg_id(n)`, an expired request from Pat to Ada, then has Ada send a message of
/// her own under its `delivery_failed` id (as if the id leaked).
fn squatted_request(office: &TriageOffice<impl MessageStore>, n: u32) {
    let mut ask = expired_request(n, PAT_ID, ADA_ID);
    ask.message.header.to_display_name = ADA_NAME.into();
    commit(&*office.store, ask);
    let notice_id = crate::ids::delivery_failed_id(&header_of(&*office.store, n), INCARNATION);
    let squat = MsgSendParams {
        id: notice_id,
        to: PAT_HANDLE.into(),
        kind: KIND_MSG.into(),
        priority: None,
        subject: None,
        body: "not a notice".into(),
        meta: Meta::new(),
        reply_to: None,
    };
    office.mailbox.send(ADA_ID, squat).unwrap();
}

/// A member who sends a message under a request's `delivery_failed` id first doesn't stop
/// the asker being told: the notice is committed under the fallback id, by a read of the
/// request by id (which succeeds) as by a wait, and the squatted row isn't taken for it.
pub fn mailbox_a_squatted_notice_id_is_told_via_the_fallback(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    let mailbox = &office.mailbox;
    for n in [1, 2] {
        squatted_request(&office, n);
    }
    let by_id = |n: u32| {
        mailbox
            .read(
                ADA_ID,
                MsgReadParams {
                    id: Some(msg_id(n)),
                },
            )
            .unwrap()
            .message
            .expect("addressed to Ada")
            .header
    };
    let fallback =
        |n: u32| crate::ids::delivery_failed_fallback_id(&msg_id(n), INCARNATION, NOW_UTC);
    assert_eq!(by_id(1).state, State::Expired);
    assert_eq!(notices_for(&*office.store, 1), [fallback(1)]);
    assert_eq!(
        by_id(1).state,
        State::Expired,
        "a later read commits nothing"
    );
    assert_eq!(notices_for(&*office.store, 1), [fallback(1)]);
    assert_eq!(wait_now(mailbox, ADA_ID), None);
    assert_eq!(state_of(&*office.store, &msg_id(2)), State::Expired);
    assert_eq!(notices_for(&*office.store, 2), [fallback(2)]);
    for n in [1, 2] {
        let told = office
            .store
            .find_reply_for_ask(&msg_id(n), ADA_ID)
            .unwrap()
            .expect("the asker was told");
        assert_eq!(told.message.header.id, fallback(n));
        assert_eq!(told.message.header.from_member_id, OFFICE_SENDER);
    }
    let failed: Vec<_> = office
        .events
        .delivery_failed()
        .into_iter()
        .map(|data| (data.id, data.original_id))
        .collect();
    assert_eq!(failed, [(fallback(1), msg_id(1)), (fallback(2), msg_id(2))]);
}

/// The squatted case through wait: the asker gets exactly one `delivery_failed`, under the
/// fallback id, with its `message.received` and `message.delivery_failed` published once,
/// and reading the request by id afterwards commits no other.
pub fn mailbox_squatted_notice_tells_the_asker_exactly_once(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    squatted_request(&office, 1);
    let fallback = crate::ids::delivery_failed_fallback_id(&msg_id(1), INCARNATION, NOW_UTC);

    assert_eq!(wait_now(&office.mailbox, ADA_ID), None);
    assert_eq!(state_of(&*office.store, &msg_id(1)), State::Expired);
    for _ in 0..2 {
        office.clock.set(NOW_MS + 1_000);
        let read = office
            .mailbox
            .read(
                ADA_ID,
                MsgReadParams {
                    id: Some(msg_id(1)),
                },
            )
            .unwrap()
            .message
            .unwrap();
        assert_eq!(read.header.state, State::Expired);
    }
    assert_eq!(
        notices_for(&*office.store, 1),
        std::slice::from_ref(&fallback)
    );
    let failed = office.events.delivery_failed();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].id, fallback);
    assert_eq!(
        office
            .events
            .received()
            .iter()
            .filter(|id| **id == fallback)
            .count(),
        1
    );
    assert!(unread_inbox(&office.mailbox, PAT_ID).contains(&fallback));
}

/// office.db restored: the office's incarnation, and with it the notice's first id, changes.
/// A request that already got its notice isn't notified again: reading it by id under the
/// new incarnation commits nothing and doesn't fail.
pub fn mailbox_a_restored_office_does_not_notify_again(backend: impl OpenTriageBackend) {
    let (store, directory) = backend.open(&members(), POSITION);
    let store = Arc::new(store);
    let directory = Arc::new(directory);
    let clock = Arc::new(ManualClock::new(NOW_MS));
    let mailbox = |incarnation: &str, events: &Arc<RecordingEvents>| {
        Mailbox::with_triage(
            store.clone(),
            store.clone(),
            directory.clone(),
            OfficeIdentity {
                incarnation: incarnation.into(),
                ..identity()
            },
            clock.clone(),
            events.clone(),
        )
        .unwrap()
    };
    let before_events = Arc::new(RecordingEvents::default());
    let before = mailbox(INCARNATION, &before_events);
    let mut ask = expired_request(1, PAT_ID, ADA_ID);
    ask.message.header.to_display_name = ADA_NAME.into();
    commit(&*store, ask);
    assert_eq!(wait_now(&before, ADA_ID), None);
    let notice = expect_notice(&*store, 1, PAT_ID, PAT_HANDLE);
    assert_eq!(before_events.delivery_failed().len(), 1);

    let after_events = Arc::new(RecordingEvents::default());
    let after = mailbox("01JCRESTORED00000000000000", &after_events);
    clock.set(NOW_MS + 60_000);
    for _ in 0..2 {
        let read = after
            .read(
                ADA_ID,
                MsgReadParams {
                    id: Some(msg_id(1)),
                },
            )
            .expect("no storage_error after a restore")
            .message
            .unwrap();
        assert_eq!(read.header.state, State::Expired);
    }
    assert_eq!(notices_for(&*store, 1), [notice]);
    assert!(after_events.delivery_failed().is_empty());
    assert!(after_events.received().is_empty());
}

/// A request the responder already answered isn't reported when it then expires: the asker
/// has the answer, so no `delivery_failed` is committed, by the expiry or by a later read.
pub fn mailbox_an_answered_request_expires_without_a_notice(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    commit(&*office.store, expired_request(1, PAT_ID, ADA_ID));
    let mut reply = new_message(2, ADA_ID, PAT_ID, MessagePriority::Normal);
    reply.message.header.reply_to = Some(msg_id(1));
    reply.message.header.thread_id = msg_id(1);
    commit(&*office.store, reply);

    assert_eq!(wait_now(&office.mailbox, ADA_ID), None);
    assert_eq!(state_of(&*office.store, &msg_id(1)), State::Expired);
    let read = office
        .mailbox
        .read(
            ADA_ID,
            MsgReadParams {
                id: Some(msg_id(1)),
            },
        )
        .unwrap()
        .message
        .unwrap();
    assert_eq!(read.header.state, State::Expired);
    assert!(notices_for(&*office.store, 1).is_empty());
    assert!(office.events.delivery_failed().is_empty());
    assert_eq!(
        office
            .store
            .find_reply_for_ask(&msg_id(1), ADA_ID)
            .unwrap()
            .map(|reply| reply.message.header.id),
        Some(msg_id(2))
    );
}

/// read with no id releases due snoozes first, as wait does.
pub fn mailbox_read_releases_due_snoozes(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    send(&office.mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    office
        .mailbox
        .defer(ADA_ID, defer_until(1, &at(250), None))
        .unwrap();
    assert_eq!(read_next(&office.mailbox, ADA_ID), None);
    office.clock.set(NOW_MS + 250);
    assert_eq!(read_next(&office.mailbox, ADA_ID), Some(msg_id(1)));
    let header = header_of(&*office.store, 1);
    assert_eq!((header.state, header.triage_gen), (State::Read, 2));
}

/// Reading an agent's overdue request by id expires it (and tells the asker) instead of
/// reading it; one still inside the skew grace is read. Reading the next message expires
/// what it passes over first.
pub fn mailbox_read_expires_an_overdue_request(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    let mailbox = &office.mailbox;
    let mut overdue = expired_request(1, PAT_ID, ADA_ID);
    overdue.message.header.to_display_name = ADA_NAME.into();
    commit(&*office.store, overdue);
    let read_id = |n: u32| {
        mailbox
            .read(
                ADA_ID,
                MsgReadParams {
                    id: Some(msg_id(n)),
                },
            )
            .unwrap()
            .message
            .expect("addressed to Ada")
            .header
    };
    let header = read_id(1);
    assert_eq!(header.state, State::Expired);
    assert_eq!(header.read_at, None);
    expect_notice(&*office.store, 1, PAT_ID, PAT_HANDLE);
    assert_eq!(read_id(1).state, State::Expired, "again: still expired");
    assert_eq!(office.events.delivery_failed().len(), 1);

    // 1 s past a 60 s ask's deadline: inside the 2 s skew grace, so still live.
    let mut live = expired_request(2, PAT_ID, ADA_ID);
    live.message.header.expires_at = Some(at(-1_000));
    commit(&*office.store, live);
    let header = read_id(2);
    assert_eq!(header.state, State::Read);
    assert_eq!(header.read_at.as_deref(), Some(NOW_UTC));

    let mut passed = expired_request(3, PAT_ID, ADA_ID);
    passed.message.header.to_display_name = ADA_NAME.into();
    commit(&*office.store, passed);
    send(mailbox, PAT_ID, 4, ADA_HANDLE).unwrap();
    assert_eq!(read_next(mailbox, ADA_ID), Some(msg_id(4)));
    assert_eq!(state_of(&*office.store, &msg_id(3)), State::Expired);
    expect_notice(&*office.store, 3, PAT_ID, PAT_HANDLE);
    assert_eq!(office.events.delivery_failed().len(), 2);

    // Expired without its notice (an earlier expiry failed after the transition): reading
    // it commits the notice, once.
    let mut orphan = expired_request(5, PAT_ID, ADA_ID);
    orphan.message.header.to_display_name = ADA_NAME.into();
    commit(&*office.store, orphan);
    apply(&*office.store, change(5, State::Expired));
    assert_eq!(read_id(5).state, State::Expired);
    expect_notice(&*office.store, 5, PAT_ID, PAT_HANDLE);
    assert_eq!(read_id(5).state, State::Expired);
    assert_eq!(office.events.delivery_failed().len(), 3);
}

/// More expired requests than one peek reports ([`crate::MAX_EXPIRED_IDS`]): one wait expires
/// them all before it blocks, and each asker is told.
pub fn mailbox_wait_expires_more_than_one_batch(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    let count = crate::MAX_EXPIRED_IDS as u32 + 5;
    for n in 1..=count {
        commit(&*office.store, expired_request(n, PAT_ID, ADA_ID));
    }
    assert_eq!(wait_now(&office.mailbox, ADA_ID), None);
    for n in 1..=count {
        assert_eq!(
            state_of(&*office.store, &msg_id(n)),
            State::Expired,
            "request {n}"
        );
    }
    assert_eq!(office.events.delivery_failed().len(), count as usize);
    assert!(peek(&*office.store, ADA_ID).expired_ids.is_empty());
}

/// A read-next cleans up expired requests behind the first batch and tells their askers.
pub fn mailbox_read_skips_more_than_one_expired_batch(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    let count = crate::MAX_EXPIRED_IDS as u32 + 5;
    for n in 1..=count {
        commit(&*office.store, expired_request(n, PAT_ID, ADA_ID));
    }
    send(&office.mailbox, PAT_ID, count + 1, ADA_HANDLE).unwrap();
    assert_eq!(read_next(&office.mailbox, ADA_ID), Some(msg_id(count + 1)));
    for n in 1..=count {
        assert_eq!(
            state_of(&*office.store, &msg_id(n)),
            State::Expired,
            "request {n}"
        );
    }
    assert_eq!(office.events.delivery_failed().len(), count as usize);
}

/// More than twice the interactive cleanup budget: after one read, maintenance
/// pages alone finish every expiry and notice. No recipient read drives those ticks.
pub fn mailbox_maintenance_finishes_a_large_backlog(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    let read_budget = (crate::mailbox::MAX_RECHECKS + 1) * crate::MAX_EXPIRED_IDS as u32;
    let count = 2 * read_budget + 5;
    for n in 1..=count {
        commit(&*office.store, expired_request(n, PAT_ID, ADA_ID));
    }
    send(&office.mailbox, PAT_ID, count + 1, ADA_HANDLE).unwrap();
    assert_eq!(read_next(&office.mailbox, ADA_ID), Some(msg_id(count + 1)));
    assert_eq!(state_of(&*office.store, &msg_id(count)), State::Unread);

    let limit = org_proto::msg::MAX_PAGE_LIMIT;
    let mut cursor = None;
    let mut inspected = 0;
    let mut finished = false;
    for _ in 0..=(count + 1).div_ceil(limit) {
        let progress = office.mailbox.expire_due(ADA_ID, cursor, limit).unwrap();
        assert!(progress.inspected <= limit as usize);
        inspected += progress.inspected;
        if let (Some(previous), Some(next)) = (cursor, progress.next_cursor) {
            assert!(next < previous, "maintenance cursor must advance");
        }
        cursor = progress.next_cursor;
        if cursor.is_none() {
            finished = true;
            break;
        }
    }
    assert!(finished, "the bounded pages must finish the pass");
    assert_eq!(inspected, count as usize + 1);
    for n in 1..=count {
        assert_eq!(
            state_of(&*office.store, &msg_id(n)),
            State::Expired,
            "request {n}"
        );
    }
    assert_eq!(office.events.delivery_failed().len(), count as usize);
    office.mailbox.expire_due(ADA_ID, None, limit).unwrap();
    assert_eq!(
        office.events.delivery_failed().len(),
        count as usize,
        "no repeat notices"
    );
}

/// A failed page retains its input cursor: retrying it sends an already-expired
/// request's missing notice, without a read and without a second transition.
pub fn mailbox_maintenance_retries_a_failed_notice(backend: impl OpenTriageBackend) {
    let (store, directory) = backend.open(&members(), POSITION);
    let directory = Arc::new(FlakyDirectory {
        inner: directory,
        reference: PAT_ID,
        failing: std::sync::atomic::AtomicBool::new(true),
    });
    let store = Arc::new(store);
    let events = Arc::new(RecordingEvents::default());
    let mailbox = Mailbox::with_triage(
        store.clone(),
        store.clone(),
        directory.clone(),
        identity(),
        Arc::new(ManualClock::new(NOW_MS)),
        events.clone(),
    )
    .unwrap();
    let mut first = expired_request(1, PAT_ID, ADA_ID);
    first.message.header.to_display_name = ADA_NAME.into();
    commit(&*store, first);
    let mut second = expired_request(2, OWNER_ID, ADA_ID);
    second.message.header.to_display_name = ADA_NAME.into();
    commit(&*store, second);
    assert_eq!(
        mailbox.expire_due(ADA_ID, None, 2),
        Err(MailboxError::StorageError)
    );
    assert_eq!(state_of(&*store, &msg_id(1)), State::Expired);
    assert_eq!(
        events.delivery_failed().len(),
        1,
        "the newer request's notice committed"
    );
    directory
        .failing
        .store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(mailbox.expire_due(ADA_ID, None, 2).unwrap().inspected, 2);
    expect_notice(&*store, 1, PAT_ID, PAT_HANDLE);
    assert_eq!(events.delivery_failed().len(), 2);
    expect_notice(&*store, 2, OWNER_ID, OWNER_NAME);
    assert_eq!(
        events.state_changes().len(),
        2,
        "no second expiry transition"
    );
    // An answered/notified request does not need its sender directory again.
    directory
        .failing
        .store(true, std::sync::atomic::Ordering::SeqCst);
    mailbox.expire_due(ADA_ID, None, 2).unwrap();
    assert_eq!(events.delivery_failed().len(), 2);
}

/// Inserts with out-of-order IDs do not move a stable sequence cursor. An arrival
/// above the current pass is picked up by the next pass, without skips or repeats.
pub fn mailbox_maintenance_cursor_survives_concurrent_arrivals(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    for n in [50, 10, 70, 30] {
        commit(&*office.store, expired_request(n, PAT_ID, ADA_ID));
    }
    let first = office.mailbox.expire_due(ADA_ID, None, 2).unwrap();
    assert_eq!(first.inspected, 2);
    let cursor = first.next_cursor.expect("two older rows remain");
    commit(&*office.store, expired_request(20, PAT_ID, ADA_ID));
    let second = office.mailbox.expire_due(ADA_ID, Some(cursor), 2).unwrap();
    assert_eq!(second.inspected, 2);
    assert_eq!(second.next_cursor, None);
    for n in [50, 10, 70, 30] {
        assert_eq!(state_of(&*office.store, &msg_id(n)), State::Expired);
    }
    assert_eq!(state_of(&*office.store, &msg_id(20)), State::Unread);
    assert_eq!(office.events.delivery_failed().len(), 4);

    let mut cursor = None;
    let mut finished = false;
    for _ in 0..3 {
        let progress = office.mailbox.expire_due(ADA_ID, cursor, 2).unwrap();
        assert!(progress.inspected <= 2);
        cursor = progress.next_cursor;
        if cursor.is_none() {
            finished = true;
            break;
        }
    }
    assert!(finished, "the next pass covers the concurrent arrival");
    assert_eq!(state_of(&*office.store, &msg_id(20)), State::Expired);
    assert_eq!(office.events.delivery_failed().len(), 5);
}

/// Maintenance is recipient-scoped, bounded, and preserves actionable human asks
/// and a human's explicit snooze.
pub fn mailbox_maintenance_validation_and_human_requests(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    for limit in [0, org_proto::msg::MAX_PAGE_LIMIT + 1] {
        assert_eq!(
            office.mailbox.expire_due(ADA_ID, None, limit),
            Err(MailboxError::InvalidRequest)
        );
    }
    assert_eq!(
        office.mailbox.expire_due(ADA_HANDLE, None, 1),
        Err(MailboxError::NotBound)
    );
    commit(&*office.store, expired_request(1, PAT_ID, REMY_ID));
    commit(&*office.store, expired_request(2, ADA_ID, PAT_ID));
    commit(&*office.store, expired_request(3, ADA_ID, PAT_ID));
    let mut snooze = snooze_until(3, &at(60_000), None);
    snooze.to_member_id = PAT_ID.into();
    apply(&*office.store, snooze);
    office.mailbox.expire_due(ADA_ID, None, 10).unwrap();
    assert_eq!(state_of(&*office.store, &msg_id(1)), State::Unread);
    office.mailbox.expire_due(PAT_ID, None, 10).unwrap();
    assert_eq!(state_of(&*office.store, &msg_id(2)), State::Unread);
    assert!(header_of(&*office.store, 2).expired_waiting_person);
    assert_eq!(state_of(&*office.store, &msg_id(3)), State::Snoozed);
    assert!(!header_of(&*office.store, 3).expired_waiting_person);
    assert!(office.events.delivery_failed().is_empty());
}

/// Maintenance applies the deadline plus bounded skew, and an already flagged
/// human ask does not emit another state-change event on each pass.
pub fn mailbox_maintenance_deadline_and_flag_boundaries(backend: impl OpenTriageBackend) {
    let office = triage_office(&backend);
    // A 60-second TTL has a capped 2-second skew allowance: future, 1 ms before,
    // and exactly at that allowance's end.
    for (n, offset) in [(1, 60_000), (2, -1_999), (3, -2_000)] {
        let mut request = expired_request(n, PAT_ID, ADA_ID);
        request.message.header.to_display_name = ADA_NAME.into();
        request.message.header.created_at = at(offset - 60_000);
        request.message.header.expires_at = Some(at(offset));
        request.message.header.ttl_ms = Some(60_000);
        commit(&*office.store, request);
    }
    commit(&*office.store, expired_request(4, ADA_ID, PAT_ID));
    let mut flagged = change(4, State::Unread);
    flagged.to_member_id = PAT_ID.into();
    flagged.expired_waiting_person = true;
    apply(&*office.store, flagged);

    office.mailbox.expire_due(ADA_ID, None, 10).unwrap();
    for n in [1, 2] {
        assert_eq!(
            state_of(&*office.store, &msg_id(n)),
            State::Unread,
            "request {n}"
        );
        assert!(office
            .store
            .find_reply_for_ask(&msg_id(n), ADA_ID)
            .unwrap()
            .is_none());
    }
    assert_eq!(state_of(&*office.store, &msg_id(3)), State::Expired);
    expect_notice(&*office.store, 3, PAT_ID, PAT_HANDLE);
    assert_eq!(office.events.delivery_failed().len(), 1);
    let changes = office.events.state_changes().len();
    assert_eq!(changes, 1);
    for _ in 0..2 {
        office.mailbox.expire_due(PAT_ID, None, 10).unwrap();
        assert_eq!(
            office.events.state_changes().len(),
            changes,
            "flagged asks emit no event"
        );
    }
    assert_eq!(state_of(&*office.store, &msg_id(4)), State::Unread);
    assert!(header_of(&*office.store, 4).expired_waiting_person);
    assert_eq!(office.events.delivery_failed().len(), 1);
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
            triage_gen_moves_only_on_snooze_and_release,
            triage_expired_waiting_person_is_never_cleared,
            triage_reason_is_only_the_snooze_or_done_note,
            triage_check_notify_reasons_filters_and_releases,
            triage_check_notify_exclusions_and_boundaries,
            triage_check_notify_returns_a_release_once_with_its_reason,
            triage_check_notify_releases_an_unreadable_snooze_as_due,
            triage_check_notify_releases_once_under_concurrency,
            triage_list_active_snooze_deadlines,
            mailbox_defer_hides_until_due_then_a_held_wait_gets_it,
            mailbox_defer_wakes_a_held_wait_to_take_its_timer,
            mailbox_with_triage_seeds_snooze_timers,
            mailbox_after_task_snooze_never_fires_on_time,
            mailbox_done_never_resurfaces,
            mailbox_defer_and_done_validation,
            mailbox_wait_expires_an_agent_request_and_tells_the_asker_once,
            mailbox_expiry_spares_persons_and_unmessageable_senders,
            mailbox_read_expires_an_overdue_request,
            mailbox_wait_expires_more_than_one_batch,
            mailbox_read_skips_more_than_one_expired_batch,
            mailbox_maintenance_finishes_a_large_backlog,
            mailbox_maintenance_retries_a_failed_notice,
            mailbox_maintenance_cursor_survives_concurrent_arrivals,
            mailbox_maintenance_validation_and_human_requests,
            mailbox_maintenance_deadline_and_flag_boundaries,
            mailbox_a_failed_notice_does_not_fail_the_recipient,
            mailbox_a_squatted_notice_id_is_told_via_the_fallback,
            mailbox_squatted_notice_tells_the_asker_exactly_once,
            mailbox_a_restored_office_does_not_notify_again,
            mailbox_an_answered_request_expires_without_a_notice,
            mailbox_read_releases_due_snoozes,
        }
    };
}
