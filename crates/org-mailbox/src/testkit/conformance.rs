//! The store conformance suite: cases generic over any `MessageStore` + `MemberDirectory`
//! backend. Run every case against a backend with [`crate::mailbox_conformance_tests!`].
//!
//! Each case opens a fresh, empty backend through [`OpenBackend`] seeded with
//! [`members`] at [`POSITION`]. The `store_*` and `directory_*` cases check
//! the ports' contracts (ADR-0003 §4.3, as amended by the store split); the `mailbox_*` cases
//! run the slice-1 Mailbox (send, inbox, read, wait) over the backend, including the §2.2
//! register-before-commit race and cancellation.
//!
//! **Run it against the production shape.** For a SQL store that means a file-backed WAL
//! database, the real batching writer, and a read pool of two or more connections separate
//! from the writer (thread-bound or small, so a thread's next read reuses its connection),
//! with the production statement cache: prepared statements used and reset exactly as
//! production does, since an unreset statement holds a stale read transaction open. With one
//! shared connection the visibility and snapshot claim cases pass vacuously: every read then
//! sees the writer's latest state.

use std::sync::{Arc, Barrier, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use org_proto::msg::{
    is_urgent, json_string_len, HeaderPage, Message, MessageHeader, MessageLifecycleState as State,
    MessagePriority, Meta, MsgInboxParams, MsgReadParams, MsgSendParams, MsgWaitParams, SendResult,
    KIND_DELIVERY_FAILED, KIND_MSG, KIND_REQUEST, MAX_BODY_BYTES, MAX_HANDLE_BYTES, MAX_ID_BYTES,
    MAX_META_BYTES, MAX_SUBJECT_BYTES, MAX_WAIT_MS, OFFICE_SENDER,
};
use org_proto::{Member, MemberKind, Position};

use super::fixture::*;
use crate::{
    cancellation_pair, content_hash, format_utc, CommitMessageOutcome, HeaderQuery, HeaderScope,
    Mailbox, MailboxError, MemberDirectory, MessageStore, NewStoredMessage, OfficeIdentity,
    PeekUnreadOutcome, StoreError, StoredMessage, MAX_EXPIRED_IDS,
};

/// Opens a fresh, empty backend whose directory holds `members` at `position`. Any
/// `Fn(&[Member], Position) -> (S, D)` is one.
pub trait OpenBackend {
    type Store: MessageStore + Send + Sync + 'static;
    type Directory: MemberDirectory + Send + Sync + 'static;
    fn open(&self, members: &[Member], position: Position) -> (Self::Store, Self::Directory);
}

impl<F, S, D> OpenBackend for F
where
    F: Fn(&[Member], Position) -> (S, D),
    S: MessageStore + Send + Sync + 'static,
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

fn open_store(backend: &impl OpenBackend) -> impl MessageStore + Send + Sync {
    backend.open(&members(), POSITION).0
}

/// A local message from `from` to `to`, as the Mailbox would commit it.
pub(crate) fn new_message(
    n: u32,
    from: &str,
    to: &str,
    priority: MessagePriority,
) -> NewStoredMessage {
    let id = msg_id(n);
    let body = format!("body {n}");
    let meta = Meta::new();
    let hash = content_hash(to, KIND_MSG, priority, None, None, &body, &meta).unwrap();
    NewStoredMessage {
        message: Message {
            header: MessageHeader {
                id: id.clone(),
                thread_id: id,
                reply_to: None,
                from_member_id: from.into(),
                from_display_name: "Sender".into(),
                to_member_id: to.into(),
                to_display_name: "Recipient".into(),
                kind: KIND_MSG.into(),
                priority,
                urgent: is_urgent(KIND_MSG, priority),
                subject: None,
                created_at: NOW_UTC.into(),
                expires_at: None,
                ttl_ms: None,
                body_bytes: json_string_len(&body) as u64,
                meta_bytes: 0,
                local_seq: 0,
                state: State::Unread,
                triage_gen: 0,
                snoozed_until: None,
                snooze_after_task: false,
                snooze_note: None,
                read_at: None,
                expired_waiting_person: false,
            },
            body,
            meta,
        },
        origin_office_id: OFFICE_ID.into(),
        origin_incarnation: INCARNATION.into(),
        origin_seq: None,
        org_position: POSITION,
        content_hash: hash,
        initial_state: State::Unread,
    }
}

/// An expired request to `to`, its deadline an hour before [`NOW_UTC`].
pub(crate) fn expired_request(n: u32, from: &str, to: &str) -> NewStoredMessage {
    let mut request = new_message(n, from, to, MessagePriority::Urgent);
    let header = &mut request.message.header;
    header.kind = KIND_REQUEST.into();
    header.urgent = true;
    header.ttl_ms = Some(60_000);
    header.expires_at = format_utc(NOW_MS - 3_600_000);
    request
}

pub(crate) fn commit(store: &impl MessageStore, msg: NewStoredMessage) -> CommitMessageOutcome {
    store.commit_message(msg).expect("commit_message")
}

pub(crate) fn inbox_query(member: &str, unread_only: bool, limit: u32) -> HeaderQuery {
    HeaderQuery {
        scope: HeaderScope::Inbox(member.into()),
        from_member_id: None,
        thread_id: None,
        unread_only,
        include_snoozed: !unread_only,
        cursor: None,
        limit,
    }
}

pub(crate) fn ids(page: &HeaderPage) -> Vec<String> {
    page.headers
        .iter()
        .map(|header| header.id.clone())
        .collect()
}

pub(crate) fn state_of(store: &impl MessageStore, id: &str) -> State {
    store
        .get_message(id)
        .expect("get_message")
        .expect("message exists")
        .message
        .header
        .state
}

pub(crate) fn peek(store: &impl MessageStore, member: &str) -> PeekUnreadOutcome {
    store
        .peek_next_unread(member, None, false, NOW_UTC)
        .expect("peek_next_unread")
}

type Hook = Mutex<Option<Box<dyn FnOnce() + Send>>>;

fn run_once(hook: &Hook) {
    let hook = hook
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    if let Some(hook) = hook {
        hook();
    }
}

/// A store wrapper that runs a hook once, after a call returns and before its caller sees
/// the result: after `peek_next_unread`, a commit landing between the wait's check and its
/// block; after `get_message`, an identical send committing between a send's duplicate
/// check and its commit.
struct HookedStore<S> {
    inner: S,
    after_peek: Hook,
    after_get: Hook,
}

impl<S: MessageStore> MessageStore for HookedStore<S> {
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
        let outcome =
            self.inner
                .peek_next_unread(to_member_id, from_member_id, urgent_only, now_utc);
        run_once(&self.after_peek);
        outcome
    }
    fn get_message(&self, id: &str) -> Result<Option<StoredMessage>, StoreError> {
        let found = self.inner.get_message(id);
        run_once(&self.after_get);
        found
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

/// A Mailbox over the backend, at HQ, with a manual clock and recorded events.
struct Office<S> {
    mailbox: Arc<Mailbox>,
    store: Arc<HookedStore<S>>,
    events: Arc<RecordingEvents>,
    clock: Arc<ManualClock>,
}

fn office<B: OpenBackend>(backend: &B) -> Office<B::Store> {
    let (store, directory) = backend.open(&members(), POSITION);
    let store = Arc::new(HookedStore {
        inner: store,
        after_peek: Mutex::new(None),
        after_get: Mutex::new(None),
    });
    let events = Arc::new(RecordingEvents::default());
    let clock = Arc::new(ManualClock::new(NOW_MS));
    let mailbox = Arc::new(Mailbox::new(
        store.clone(),
        Arc::new(directory),
        OfficeIdentity {
            office_id: OFFICE_ID.into(),
            incarnation: INCARNATION.into(),
            is_hq: true,
        },
        clock.clone(),
        events.clone(),
    ));
    Office {
        mailbox,
        store,
        events,
        clock,
    }
}

pub(crate) fn send_params(n: u32, to: &str) -> MsgSendParams {
    MsgSendParams {
        id: msg_id(n),
        to: to.into(),
        kind: KIND_MSG.into(),
        priority: None,
        subject: None,
        body: format!("body {n}"),
        meta: Meta::new(),
        reply_to: None,
    }
}

pub(crate) fn send(
    mailbox: &Mailbox,
    from: &str,
    n: u32,
    to: &str,
) -> Result<SendResult, MailboxError> {
    mailbox.send(from, send_params(n, to))
}

pub(crate) fn wait_params(timeout_ms: u64) -> MsgWaitParams {
    MsgWaitParams {
        timeout_ms,
        from: None,
        urgent_only: false,
    }
}

pub(crate) fn far_deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

pub(crate) fn unread_inbox(mailbox: &Mailbox, member: &str) -> Vec<String> {
    let page = mailbox
        .inbox(member, MsgInboxParams::default())
        .expect("inbox");
    ids(&page)
}

/// Polls until `condition` holds, failing after a generous bound (the machine may be loaded).
pub(crate) fn eventually(what: &str, condition: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(1));
    }
}

// ---------------------------------------------------------------------------------------
// MessageStore
// ---------------------------------------------------------------------------------------

/// Commits allocate increasing `local_seq` and a local `origin_seq` (>= 1, increasing),
/// keep the origin and content hash, and start Unread.
pub fn store_commit_allocates_sequence_and_origin(backend: impl OpenBackend) {
    let store = open_store(&backend);
    let mut last = (0, 0);
    for n in 1..=3 {
        let input = new_message(n, PAT_ID, ADA_ID, MessagePriority::Normal);
        let hash = input.content_hash;
        let outcome = commit(&store, input);
        assert!(outcome.inserted);
        let stored = &outcome.message;
        let header = &stored.message.header;
        assert!(header.local_seq > last.0, "local_seq increases");
        assert!(stored.origin_seq >= 1 && stored.origin_seq > last.1);
        last = (header.local_seq, stored.origin_seq);
        assert_eq!(stored.origin_office_id, OFFICE_ID);
        assert_eq!(stored.origin_incarnation, INCARNATION);
        assert_eq!(stored.org_position, POSITION);
        assert_eq!(stored.content_hash, hash);
        assert_eq!(header.state, State::Unread);
        assert_eq!(header.read_at, None);
        assert_eq!(
            store.get_message(&header.id).unwrap().as_ref(),
            Some(stored)
        );
    }
    assert_eq!(store.get_message(&msg_id(99)).unwrap(), None);
}

/// A matching replay returns the first commit (`inserted: false`) and adds no row; the
/// same id with other content or another sender is `AlreadyExists`.
pub fn store_commit_replay_and_conflict(backend: impl OpenBackend) {
    let store = open_store(&backend);
    let first = commit(
        &store,
        new_message(1, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    let replay = commit(
        &store,
        new_message(1, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    assert!(!replay.inserted);
    assert_eq!(replay.message, first.message);
    assert_eq!(
        ids(&store.list_headers(inbox_query(ADA_ID, false, 100)).unwrap()),
        [msg_id(1)]
    );

    let mut other_content = new_message(1, PAT_ID, ADA_ID, MessagePriority::Normal);
    other_content.content_hash[0] ^= 1;
    assert!(matches!(
        store.commit_message(other_content),
        Err(StoreError::AlreadyExists)
    ));
    let other_sender = new_message(1, OWNER_ID, ADA_ID, MessagePriority::Normal);
    assert!(matches!(
        store.commit_message(other_sender),
        Err(StoreError::AlreadyExists)
    ));
    assert_eq!(store.get_message(&msg_id(1)).unwrap(), Some(first.message));
}

/// A request gets at most one `delivery_failed` notice: a second one replying to the same
/// request, under another id, is refused and writes nothing. Any error passes (a SQL store
/// reports its unique index as a storage error), as long as the first notice is the only one.
pub fn store_commit_refuses_a_second_delivery_failed(backend: impl OpenBackend) {
    let store = open_store(&backend);
    commit(&store, expired_request(1, PAT_ID, ADA_ID));
    let notice = |n: u32| {
        let mut notice = new_message(n, OFFICE_SENDER, PAT_ID, MessagePriority::High);
        let header = &mut notice.message.header;
        header.kind = KIND_DELIVERY_FAILED.into();
        header.urgent = is_urgent(KIND_DELIVERY_FAILED, MessagePriority::High);
        header.reply_to = Some(msg_id(1));
        header.thread_id = msg_id(1);
        notice
    };
    let first = commit(&store, notice(2));
    assert!(first.inserted);
    assert!(store.commit_message(notice(3)).is_err());
    assert_eq!(store.get_message(&msg_id(3)).unwrap(), None);
    let notices: Vec<_> = store
        .list_headers(inbox_query(PAT_ID, false, 100))
        .unwrap()
        .headers
        .into_iter()
        .filter(|header| {
            header.kind == KIND_DELIVERY_FAILED && header.reply_to.as_deref() == Some(&msg_id(1))
        })
        .map(|header| header.id)
        .collect();
    assert_eq!(notices, [msg_id(2)]);
    // The first notice still replays.
    assert!(!commit(&store, notice(2)).inserted);
    // A notice for another request is its own.
    commit(&store, expired_request(4, PAT_ID, ADA_ID));
    let mut other = notice(5);
    other.message.header.reply_to = Some(msg_id(4));
    other.message.header.thread_id = msg_id(4);
    assert!(commit(&store, other).inserted);
}

/// `find_reply_for_ask(ask, responder)` finds the earliest reply to `ask` addressed to its
/// asker, from `responder` or the office's own `delivery_failed` notice. A third party's
/// reply to the asker and the responder's reply to someone else don't count; an unknown ask
/// is `None`.
pub fn store_find_reply_for_ask(backend: impl OpenBackend) {
    let store = open_store(&backend);
    let reply = |n: u32, from: &str, to: &str, ask: u32| {
        let mut reply = new_message(n, from, to, MessagePriority::Normal);
        let header = &mut reply.message.header;
        header.reply_to = Some(msg_id(ask));
        header.thread_id = msg_id(ask);
        reply
    };
    let found = |ask: u32, responder: &str| {
        store
            .find_reply_for_ask(&msg_id(ask), responder)
            .expect("find_reply_for_ask")
            .map(|stored| stored.message.header.id)
    };
    // Pat asks Ada.
    commit(&store, expired_request(1, PAT_ID, ADA_ID));
    assert_eq!(found(1, ADA_ID), None, "no reply yet");
    // Remy answers Pat, and Ada answers Remy: neither tells Pat what Ada did.
    commit(&store, reply(2, REMY_ID, PAT_ID, 1));
    commit(&store, reply(3, ADA_ID, REMY_ID, 1));
    assert_eq!(
        found(1, ADA_ID),
        None,
        "a third party's reply, or one to another member"
    );
    // Ada answers Pat, twice: the earliest is the answer.
    commit(&store, reply(4, ADA_ID, PAT_ID, 1));
    commit(&store, reply(5, ADA_ID, PAT_ID, 1));
    assert_eq!(found(1, ADA_ID), Some(msg_id(4)));
    // Asked about another responder, Remy's reply to Pat is the answer.
    assert_eq!(found(1, REMY_ID), Some(msg_id(2)));

    // Pat asks Ada again; the office tells Pat the ask expired.
    commit(&store, expired_request(6, PAT_ID, ADA_ID));
    let mut notice = reply(7, OFFICE_SENDER, PAT_ID, 6);
    let header = &mut notice.message.header;
    header.kind = KIND_DELIVERY_FAILED.into();
    header.priority = MessagePriority::High;
    header.urgent = is_urgent(KIND_DELIVERY_FAILED, MessagePriority::High);
    commit(&store, notice);
    assert_eq!(found(6, ADA_ID), Some(msg_id(7)), "the office's notice");

    // An ask that doesn't exist, even with a reply naming it.
    commit(&store, reply(8, ADA_ID, PAT_ID, 99));
    assert_eq!(found(99, ADA_ID), None, "an unknown ask");
}

/// A message committed Read (an ask reply claimed before commit) is never unread.
pub fn store_initial_read_is_never_unread(backend: impl OpenBackend) {
    let store = open_store(&backend);
    let mut input = new_message(1, PAT_ID, ADA_ID, MessagePriority::Normal);
    input.initial_state = State::Read;
    let outcome = commit(&store, input);
    assert_eq!(outcome.message.message.header.state, State::Read);
    assert_eq!(peek(&store, ADA_ID).message, None);
    assert_eq!(store.claim_read(ADA_ID, None, NOW_UTC).unwrap(), None);
    assert!(ids(&store.list_headers(inbox_query(ADA_ID, true, 100)).unwrap()).is_empty());
    assert_eq!(
        ids(&store.list_headers(inbox_query(ADA_ID, false, 100)).unwrap()),
        [msg_id(1)]
    );
}

/// Peek returns the oldest matching unread message and changes nothing.
pub fn store_peek_is_read_only_and_oldest_first(backend: impl OpenBackend) {
    let store = open_store(&backend);
    commit(
        &store,
        new_message(1, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    commit(
        &store,
        new_message(2, OWNER_ID, ADA_ID, MessagePriority::Urgent),
    );
    commit(
        &store,
        new_message(3, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    for _ in 0..2 {
        let outcome = peek(&store, ADA_ID);
        assert_eq!(outcome.message.unwrap().message.header.id, msg_id(1));
        assert!(outcome.expired_ids.is_empty());
    }
    let from_owner = store
        .peek_next_unread(ADA_ID, Some(OWNER_ID), false, NOW_UTC)
        .unwrap();
    assert_eq!(from_owner.message.unwrap().message.header.id, msg_id(2));
    let urgent = store.peek_next_unread(ADA_ID, None, true, NOW_UTC).unwrap();
    assert_eq!(urgent.message.unwrap().message.header.id, msg_id(2));
    assert_eq!(peek(&store, REMY_ID).message, None);
    for n in 1..=3 {
        assert_eq!(state_of(&store, &msg_id(n)), State::Unread);
    }
}

/// An expired unread message is passed over and reported, never deleted or changed.
pub fn store_peek_reports_expired_without_deleting(backend: impl OpenBackend) {
    let store = open_store(&backend);
    commit(&store, expired_request(1, PAT_ID, ADA_ID));
    commit(
        &store,
        new_message(2, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    for _ in 0..2 {
        let outcome = peek(&store, ADA_ID);
        assert_eq!(outcome.message.unwrap().message.header.id, msg_id(2));
        assert_eq!(outcome.expired_ids, [msg_id(1)]);
    }
    assert_eq!(state_of(&store, &msg_id(1)), State::Unread);
}

/// `msg_id(n)`, a normal (not urgent) message from `from` to Ada whose `expires_at` is
/// `offset_ms` after [`NOW_UTC`] (an hour back is past its deadline and grace).
fn with_deadline(n: u32, from: &str, offset_ms: i64) -> NewStoredMessage {
    let mut message = new_message(n, from, ADA_ID, MessagePriority::Normal);
    let header = &mut message.message.header;
    header.ttl_ms = Some(60_000);
    header.expires_at = format_utc(NOW_MS.checked_add_signed(offset_ms).unwrap());
    message
}

/// `from_member_id` and `urgent_only` filter `expired_ids` as well as the message.
pub fn store_peek_filters_expired_ids(backend: impl OpenBackend) {
    let store = open_store(&backend);
    commit(&store, expired_request(1, PAT_ID, ADA_ID));
    commit(&store, expired_request(2, OWNER_ID, ADA_ID));
    commit(&store, with_deadline(3, PAT_ID, -3_600_000));
    commit(
        &store,
        new_message(4, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    let peek_with = |from: Option<&str>, urgent_only: bool| {
        let outcome = store
            .peek_next_unread(ADA_ID, from, urgent_only, NOW_UTC)
            .expect("peek_next_unread");
        (
            outcome.message.map(|stored| stored.message.header.id),
            outcome.expired_ids,
        )
    };
    assert_eq!(
        peek_with(None, false),
        (Some(msg_id(4)), vec![msg_id(1), msg_id(2), msg_id(3)])
    );
    assert_eq!(
        peek_with(Some(PAT_ID), false),
        (Some(msg_id(4)), vec![msg_id(1), msg_id(3)]),
        "only the sender's expired messages"
    );
    assert_eq!(peek_with(Some(OWNER_ID), false), (None, vec![msg_id(2)]));
    assert_eq!(
        peek_with(None, true),
        (None, vec![msg_id(1), msg_id(2)]),
        "only urgent expired messages"
    );
    assert_eq!(peek_with(Some(PAT_ID), true), (None, vec![msg_id(1)]));
    assert_eq!(peek_with(Some(REMY_ID), false), (None, vec![]));
}

/// An unreadable `now_utc` counts every deadline as passed: each message with an
/// `expires_at`, however far off, is expired, and only one without can be the message.
pub fn store_peek_unreadable_now_passes_every_deadline(backend: impl OpenBackend) {
    let store = open_store(&backend);
    commit(&store, with_deadline(1, PAT_ID, 3_600_000));
    commit(
        &store,
        new_message(2, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    let outcome = peek(&store, ADA_ID);
    assert_eq!(
        outcome.message.map(|stored| stored.message.header.id),
        Some(msg_id(1)),
        "not yet expired at a readable now"
    );
    assert!(outcome.expired_ids.is_empty());
    let outcome = store
        .peek_next_unread(ADA_ID, None, false, "not a time")
        .expect("peek_next_unread");
    assert_eq!(
        outcome.message.map(|stored| stored.message.header.id),
        Some(msg_id(2))
    );
    assert_eq!(outcome.expired_ids, [msg_id(1)]);
    for n in 1..=2 {
        assert_eq!(state_of(&store, &msg_id(n)), State::Unread);
    }
}

/// Peek reports at most [`MAX_EXPIRED_IDS`] expired ids, the oldest first in commit order
/// (not id order: they are committed in falling id order), and still returns the oldest
/// unexpired message behind them.
pub fn store_peek_caps_expired_ids_oldest_first(backend: impl OpenBackend) {
    let store = open_store(&backend);
    let expired = MAX_EXPIRED_IDS as u32 + 5;
    for n in (1..=expired).rev() {
        commit(&store, expired_request(n, PAT_ID, ADA_ID));
    }
    commit(
        &store,
        new_message(expired + 1, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    let outcome = peek(&store, ADA_ID);
    assert_eq!(
        outcome.message.unwrap().message.header.id,
        msg_id(expired + 1)
    );
    assert_eq!(outcome.expired_ids.len(), MAX_EXPIRED_IDS);
    assert_eq!(
        outcome.expired_ids,
        (6..=expired).rev().map(msg_id).collect::<Vec<_>>(),
        "the first {MAX_EXPIRED_IDS} committed, ids {expired} down to 6"
    );
    assert_eq!(state_of(&store, &msg_id(expired)), State::Unread);
}

/// Claims go oldest first, only to the recipient, and an already read id comes back
/// unchanged.
pub fn store_claim_read_order_and_ownership(backend: impl OpenBackend) {
    let store = open_store(&backend);
    commit(
        &store,
        new_message(1, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    commit(
        &store,
        new_message(2, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    commit(
        &store,
        new_message(3, PAT_ID, REMY_ID, MessagePriority::Normal),
    );
    let first = store.claim_read(ADA_ID, None, NOW_UTC).unwrap().unwrap();
    assert_eq!(first.message.header.id, msg_id(1));
    assert_eq!(first.message.header.state, State::Read);
    assert_eq!(first.message.header.read_at.as_deref(), Some(NOW_UTC));
    let later = "2026-11-08T12:00:05.000Z";
    let second = store.claim_read(ADA_ID, None, later).unwrap().unwrap();
    assert_eq!(second.message.header.id, msg_id(2));
    assert_eq!(store.claim_read(ADA_ID, None, later).unwrap(), None);
    assert_eq!(
        store.claim_read(ADA_ID, Some(&msg_id(3)), later).unwrap(),
        None
    );
    assert_eq!(state_of(&store, &msg_id(3)), State::Unread);
    assert_eq!(
        store.claim_read(ADA_ID, Some(&msg_id(9)), later).unwrap(),
        None
    );
    let again = store
        .claim_read(ADA_ID, Some(&msg_id(1)), later)
        .unwrap()
        .unwrap();
    assert_eq!(again.message.header.state, State::Read);
    assert_eq!(again.message.header.read_at.as_deref(), Some(NOW_UTC));
    let named = store
        .claim_read(REMY_ID, Some(&msg_id(3)), later)
        .unwrap()
        .unwrap();
    assert_eq!(named.message.header.read_at.as_deref(), Some(later));
}

/// Concurrent claims of one message succeed exactly once.
pub fn store_claim_read_is_once_under_concurrency(backend: impl OpenBackend) {
    let store = open_store(&backend);
    for n in 1..=5 {
        commit(
            &store,
            new_message(n, PAT_ID, ADA_ID, MessagePriority::Normal),
        );
    }
    let barrier = Barrier::new(8);
    let claimed: Vec<String> = thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    let mut mine = Vec::new();
                    // Bounded: a backend that keeps returning a message fails, never spins.
                    for _ in 0..=5 {
                        let Some(stored) = store.claim_read(ADA_ID, None, NOW_UTC).unwrap() else {
                            break;
                        };
                        mine.push(stored.message.header.id);
                    }
                    mine
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect()
    });
    let mut sorted = claimed.clone();
    sorted.sort();
    assert_eq!(sorted, (1..=5).map(msg_id).collect::<Vec<_>>());
}

/// K barrier-synced claimers each claim once from at least K unread messages: every claim
/// gets a message (`Some`), the ids are pairwise distinct, and each is now read. A store
/// whose concurrent claims can lose (answer `None` while messages remain) or double-claim
/// fails here.
pub fn store_claim_read_k_barrier_claimers_all_get_distinct(backend: impl OpenBackend) {
    const K: usize = 8;
    let store = open_store(&backend);
    for n in 1..=K as u32 + 2 {
        commit(
            &store,
            new_message(n, PAT_ID, ADA_ID, MessagePriority::Normal),
        );
    }
    let barrier = Barrier::new(K);
    let claimed: Vec<Option<String>> = thread::scope(|scope| {
        let workers: Vec<_> = (0..K)
            .map(|_| {
                scope.spawn(|| {
                    // Read first, so a backend that claims from a reader's snapshot has one.
                    // Fail only after the barrier, so neither an error nor a panic in the
                    // backend can strand the other threads waiting at it. One claim each:
                    // bounded, never a spin.
                    let warmed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        store.peek_next_unread(ADA_ID, None, false, NOW_UTC)
                    }));
                    barrier.wait();
                    match warmed {
                        Ok(result) => drop(result.expect("peek_next_unread")),
                        Err(panic) => std::panic::resume_unwind(panic),
                    }
                    let stored = store.claim_read(ADA_ID, None, NOW_UTC).unwrap()?;
                    assert_eq!(stored.message.header.state, State::Read);
                    Some(stored.message.header.id)
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect()
    });
    let claimed: Vec<String> = claimed
        .into_iter()
        .map(|id| id.expect("every claimer gets a message while enough are unread"))
        .collect();
    let mut distinct = claimed.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        K,
        "claims are pairwise distinct: {claimed:?}"
    );
    for id in &claimed {
        assert_eq!(state_of(&store, id), State::Read);
    }
    let mut everything = distinct;
    everything.extend(ids(&store
        .list_headers(inbox_query(ADA_ID, true, 100))
        .unwrap()));
    everything.sort();
    assert_eq!(
        everything,
        (1..=K as u32 + 2).map(msg_id).collect::<Vec<_>>(),
        "the claimed and the still unread messages are disjoint and are every message"
    );
}

/// A reader that peeked, then lost the oldest message to another claimer, must claim the
/// next one: `claim_read(None)` never answers `None` while an unread message remains. A
/// store that selects on the reader's stale snapshot and then loses its guarded update
/// (`... WHERE state = 'unread'` touching 0 rows) fails here.
pub fn store_claim_read_after_a_lost_race_takes_the_next(backend: impl OpenBackend) {
    let store = open_store(&backend);
    commit(
        &store,
        new_message(1, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    commit(
        &store,
        new_message(2, PAT_ID, ADA_ID, MessagePriority::Normal),
    );
    enum Step {
        Peek,
        Claim,
    }
    thread::scope(|scope| {
        // Owned inside the scope, so a failed assertion ends reader A instead of stranding it.
        let (ask, asked) = std::sync::mpsc::channel::<Step>();
        let (answer, answers) = std::sync::mpsc::channel::<Option<String>>();
        let store = &store;
        // Reader A: one long-lived thread, as a pooled or thread-bound connection would be.
        // The case bites only if A's claim reuses the connection A's peek used.
        scope.spawn(move || {
            while let Ok(step) = asked.recv() {
                let seen = match step {
                    Step::Peek => peek(store, ADA_ID).message,
                    Step::Claim => store.claim_read(ADA_ID, None, NOW_UTC).unwrap(),
                };
                if answer.send(seen.map(|m| m.message.header.id)).is_err() {
                    break;
                }
            }
        });
        let timeout = Duration::from_secs(10);
        ask.send(Step::Peek).unwrap();
        assert_eq!(answers.recv_timeout(timeout).unwrap(), Some(msg_id(1)));
        // B claims message 1 between A's peek and A's claim.
        let taken = store.claim_read(ADA_ID, None, NOW_UTC).unwrap().unwrap();
        assert_eq!(taken.message.header.id, msg_id(1));
        ask.send(Step::Claim).unwrap();
        assert_eq!(
            answers.recv_timeout(timeout).unwrap(),
            Some(msg_id(2)),
            "claim_read(None) answered without message 2 while it was unread"
        );
    });
    assert_eq!(store.claim_read(ADA_ID, None, NOW_UTC).unwrap(), None);
}

/// A commit that returned `Ok` is visible at once to a reader on another, long-lived
/// thread (as a pooled or thread-bound connection would be), and so is a claim: reads use a
/// fresh snapshot, never a long-lived read transaction (§2.2).
pub fn store_commits_and_claims_are_visible_to_other_readers(backend: impl OpenBackend) {
    let store = open_store(&backend);
    type Seen = (Option<String>, bool, Vec<String>);
    thread::scope(|scope| {
        // Owned inside the scope: if an assertion fails, `ask` drops while unwinding and
        // the reader thread ends, instead of the scope waiting on it forever.
        let (ask, asked) = std::sync::mpsc::channel::<Option<u32>>();
        let (answer, answers) = std::sync::mpsc::channel::<Seen>();
        let store = &store;
        scope.spawn(move || {
            // One reader thread for the whole case; it reads before every commit too.
            while let Ok(Some(round)) = asked.recv() {
                let peeked = peek(store, ADA_ID).message.map(|m| m.message.header.id);
                let got = store.get_message(&msg_id(round)).unwrap().is_some();
                let unread = ids(&store.list_headers(inbox_query(ADA_ID, true, 100)).unwrap());
                answer.send((peeked, got, unread)).unwrap();
            }
        });
        for round in 1..=5 {
            ask.send(Some(round)).unwrap();
            assert_eq!(
                answers.recv().unwrap(),
                (None, false, vec![]),
                "round {round}"
            );
            commit(
                store,
                new_message(round, PAT_ID, ADA_ID, MessagePriority::Normal),
            );
            ask.send(Some(round)).unwrap();
            assert_eq!(
                answers.recv().unwrap(),
                (Some(msg_id(round)), true, vec![msg_id(round)]),
                "the reader must see the commit that just returned (round {round})"
            );
            let claimed = store.claim_read(ADA_ID, None, NOW_UTC).unwrap().unwrap();
            assert_eq!(claimed.message.header.id, msg_id(round));
        }
        ask.send(Some(5)).unwrap();
        assert_eq!(answers.recv().unwrap(), (None, true, vec![]));
        ask.send(None).unwrap();
    });
}

/// `claim_read(None)` selects and marks in one writer transaction: claimers that have been
/// reading (and could hold stale snapshots) race each other and a writer still committing,
/// and every message is claimed exactly once.
pub fn store_claim_read_selects_and_marks_atomically(backend: impl OpenBackend) {
    let store = open_store(&backend);
    for n in 1..=30 {
        commit(
            &store,
            new_message(n, PAT_ID, ADA_ID, MessagePriority::Normal),
        );
    }
    let first = store.claim_read(ADA_ID, None, NOW_UTC).unwrap().unwrap();
    assert_eq!(first.message.header.id, msg_id(1));
    let barrier = Barrier::new(7);
    let mut claimed: Vec<String> = thread::scope(|scope| {
        let store = &store;
        let barrier = &barrier;
        let producer = scope.spawn(move || {
            barrier.wait();
            for n in 31..=60 {
                commit(
                    store,
                    new_message(n, PAT_ID, ADA_ID, MessagePriority::Normal),
                );
            }
        });
        let claimers: Vec<_> = (0..6)
            .map(|_| {
                scope.spawn(move || {
                    // Read first, so a backend that claims from a reader's snapshot has one.
                    // Fail only after the barrier, so neither an error nor a panic in the
                    // backend can strand the other threads waiting at it.
                    let warmed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        store.peek_next_unread(ADA_ID, None, false, NOW_UTC)
                    }));
                    barrier.wait();
                    let warmed = match warmed {
                        Ok(result) => result.expect("peek_next_unread"),
                        Err(panic) => std::panic::resume_unwind(panic),
                    };
                    assert!(warmed.message.is_some(), "peek sees the unread messages");
                    let mut mine = Vec::new();
                    for _ in 0..200 {
                        match store.claim_read(ADA_ID, None, NOW_UTC).unwrap() {
                            Some(stored) => {
                                assert_eq!(stored.message.header.state, State::Read);
                                mine.push(stored.message.header.id);
                            }
                            None => thread::yield_now(),
                        }
                        let _ = peek(store, ADA_ID);
                    }
                    mine
                })
            })
            .collect();
        producer.join().unwrap();
        claimers
            .into_iter()
            .flat_map(|c| c.join().unwrap())
            .collect()
    });
    // Bounded: a backend that keeps returning a message fails below, never spins.
    for _ in 0..=60 {
        let Some(stored) = store.claim_read(ADA_ID, None, NOW_UTC).unwrap() else {
            break;
        };
        claimed.push(stored.message.header.id);
    }
    claimed.push(first.message.header.id);
    claimed.sort();
    assert_eq!(
        claimed,
        (1..=60).map(msg_id).collect::<Vec<_>>(),
        "every message is claimed exactly once"
    );
    assert_eq!(peek(&store, ADA_ID).message, None);
}

/// Headers come newest first by `local_seq`, page by cursor without gaps or repeats, and
/// filter by scope, sender, thread and read state.
pub fn store_list_headers_order_paging_and_filters(backend: impl OpenBackend) {
    let store = open_store(&backend);
    for n in 1..=5 {
        let from = if n % 2 == 0 { OWNER_ID } else { PAT_ID };
        let mut input = new_message(n, from, ADA_ID, MessagePriority::Normal);
        if n == 4 {
            input.message.header.thread_id = msg_id(1);
            input.message.header.reply_to = Some(msg_id(1));
        }
        commit(&store, input);
    }
    commit(
        &store,
        new_message(6, ADA_ID, PAT_ID, MessagePriority::Normal),
    );

    let mut query = inbox_query(ADA_ID, true, 2);
    let mut pages = Vec::new();
    loop {
        let page = store.list_headers(query.clone()).unwrap();
        pages.push(ids(&page));
        let Some(cursor) = page.next_cursor else {
            break;
        };
        query.cursor = Some(cursor);
    }
    assert_eq!(
        pages,
        [
            vec![msg_id(5), msg_id(4)],
            vec![msg_id(3), msg_id(2)],
            vec![msg_id(1)]
        ]
    );

    let mut from_owner = inbox_query(ADA_ID, true, 100);
    from_owner.from_member_id = Some(OWNER_ID.into());
    assert_eq!(
        ids(&store.list_headers(from_owner).unwrap()),
        [msg_id(4), msg_id(2)]
    );
    let mut thread = inbox_query(ADA_ID, true, 100);
    thread.thread_id = Some(msg_id(1));
    assert_eq!(
        ids(&store.list_headers(thread).unwrap()),
        [msg_id(4), msg_id(1)]
    );
    let participant = HeaderQuery {
        scope: HeaderScope::Participant(PAT_ID.into()),
        ..inbox_query(PAT_ID, false, 100)
    };
    assert_eq!(
        ids(&store.list_headers(participant).unwrap()),
        [msg_id(6), msg_id(5), msg_id(3), msg_id(1)]
    );

    store
        .claim_read(ADA_ID, Some(&msg_id(2)), NOW_UTC)
        .unwrap()
        .unwrap();
    assert_eq!(
        ids(&store.list_headers(inbox_query(ADA_ID, true, 100)).unwrap()),
        [msg_id(5), msg_id(4), msg_id(3), msg_id(1)]
    );
    let all = store.list_headers(inbox_query(ADA_ID, false, 100)).unwrap();
    assert_eq!(all.headers.len(), 5);
    assert_eq!(all.next_cursor, None);
    assert_eq!(all.headers[3].state, State::Read);
}

// ---------------------------------------------------------------------------------------
// MemberDirectory
// ---------------------------------------------------------------------------------------

fn resolve(directory: &impl MemberDirectory, reference: &str) -> Option<String> {
    directory
        .resolve_member(reference)
        .expect("resolve_member")
        .map(|(member, position)| {
            assert_eq!(position, POSITION);
            member.id
        })
}

/// "owner" first, then an exact id (which beats a handle), then a handle ignoring case;
/// display names and "office" never resolve.
pub fn directory_resolution_order(backend: impl OpenBackend) {
    let (_, directory) = backend.open(&members(), POSITION);
    for owner in ["owner", "OWNER", "Owner", OWNER_ID] {
        assert_eq!(resolve(&directory, owner).as_deref(), Some(OWNER_ID));
    }
    assert_eq!(resolve(&directory, OWNER_NAME), None);
    for handle in ["ada", "ADA", "Ada", ADA_ID] {
        assert_eq!(resolve(&directory, handle).as_deref(), Some(ADA_ID));
    }
    assert_eq!(resolve(&directory, ADA_NAME), None);
    for handle in ["pat", "PAT", PAT_ID] {
        assert_eq!(resolve(&directory, handle).as_deref(), Some(PAT_ID));
    }
    assert_eq!(
        resolve(&directory, &PAT_ID.to_ascii_lowercase()).as_deref(),
        Some(SHADOW_ID),
        "a lower-case spelling is not the exact id, so it is the shadow's handle"
    );
    for nobody in ["office", "OFFICE", "nobody", ""] {
        assert_eq!(resolve(&directory, nobody), None);
    }
}

/// Handles fold case by Unicode lower case (`str::to_lowercase`), as org-office keys them:
/// "Émile" is found as "émile" and "ÉMILE", by the directory and through send.
pub fn directory_handles_fold_unicode_case(backend: impl OpenBackend) {
    let (_, directory) = backend.open(&members(), POSITION);
    for spelling in ["\u{c9}mile", "\u{e9}mile", "\u{c9}MILE", "\u{e9}MILE"] {
        assert_eq!(
            resolve(&directory, spelling).as_deref(),
            Some(EMILE_ID),
            "{spelling}"
        );
    }
    assert_eq!(resolve(&directory, "emile"), None, "an accent is not case");

    let office = office(&backend);
    assert_eq!(
        recipient(&office, 1, "  \u{e9}MILE\t").as_deref(),
        Ok(EMILE_ID)
    );
}

/// The directory returns members with their stored status, whatever it is.
pub fn directory_returns_status_unfiltered(backend: impl OpenBackend) {
    let (_, directory) = backend.open(&members(), POSITION);
    let status = |reference: &str| {
        let (member, _) = directory.resolve_member(reference).unwrap().unwrap();
        assert_eq!(member.kind, MemberKind::Agent);
        (
            member.id.clone(),
            member.agent.unwrap().status,
            member.office_id,
        )
    };
    use org_proto::AgentStatus::*;
    assert_eq!(
        status(REMY_HANDLE),
        (REMY_ID.into(), Paused, Some(OFFICE_ID.into()))
    );
    assert_eq!(
        status(GONE_HANDLE),
        (GONE_ID.into(), Offboarded, Some(OFFICE_ID.into()))
    );
    assert_eq!(
        status(GONE_ID),
        (GONE_ID.into(), Offboarded, Some(OFFICE_ID.into()))
    );
    assert_eq!(
        status(ODD_HANDLE),
        (ODD_ID.into(), Unknown, Some(OFFICE_ID.into()))
    );
    assert_eq!(
        status(FAR_HANDLE),
        (FAR_ID.into(), Active, Some(OTHER_OFFICE_ID.into()))
    );
}

// ---------------------------------------------------------------------------------------
// Mailbox over the backend
// ---------------------------------------------------------------------------------------

/// Send commits with this office's origin and a local origin_seq, hashes the resolved
/// recipient id, and publishes one `message.received`.
pub fn mailbox_send_commits_with_origin(backend: impl OpenBackend) {
    let office = office(&backend);
    let result = send(&office.mailbox, ADA_ID, 1, PAT_HANDLE).unwrap();
    assert_eq!(result.id, msg_id(1));
    assert_eq!(result.thread_id, msg_id(1));
    assert!(result.local_seq >= 1 && result.origin_seq >= 1);
    let stored = office.store.get_message(&msg_id(1)).unwrap().unwrap();
    let header = &stored.message.header;
    assert_eq!(stored.origin_office_id, OFFICE_ID);
    assert_eq!(stored.origin_incarnation, INCARNATION);
    assert_eq!(stored.origin_seq, result.origin_seq);
    assert_eq!(stored.org_position, POSITION);
    assert_eq!(
        stored.content_hash,
        content_hash(
            PAT_ID,
            KIND_MSG,
            MessagePriority::Normal,
            None,
            None,
            "body 1",
            &Meta::new()
        )
        .unwrap()
    );
    assert_eq!(
        (
            header.from_member_id.as_str(),
            header.from_display_name.as_str()
        ),
        (ADA_ID, ADA_NAME)
    );
    assert_eq!(
        (
            header.to_member_id.as_str(),
            header.to_display_name.as_str()
        ),
        (PAT_ID, PAT_HANDLE)
    );
    assert_eq!(header.created_at, NOW_UTC);
    assert_eq!(
        header.expires_at, None,
        "send never creates an expiring message"
    );
    assert_eq!(header.local_seq, result.local_seq);
    assert_eq!(office.events.received(), [msg_id(1)]);

    let second = send(&office.mailbox, ADA_ID, 2, PAT_HANDLE).unwrap();
    assert!(second.local_seq > result.local_seq && second.origin_seq > result.origin_seq);
}

/// A retry with the same id and content returns the first result and publishes nothing;
/// other content or another sender under that id is `already_exists`.
pub fn mailbox_send_replay_and_conflict(backend: impl OpenBackend) {
    let office = office(&backend);
    let first = send(&office.mailbox, ADA_ID, 1, PAT_HANDLE).unwrap();
    office.clock.set(NOW_MS + 5_000);
    assert_eq!(send(&office.mailbox, ADA_ID, 1, " pat ").unwrap(), first);
    assert_eq!(send(&office.mailbox, ADA_ID, 1, PAT_ID).unwrap(), first);
    let mut changed = send_params(1, PAT_HANDLE);
    changed.body = "other".into();
    assert_eq!(
        office.mailbox.send(ADA_ID, changed),
        Err(MailboxError::AlreadyExists)
    );
    assert_eq!(
        send(&office.mailbox, REMY_ID, 1, PAT_HANDLE),
        Err(MailboxError::AlreadyExists)
    );
    // A retry can't be re-routed: the same id and body to another member is other content.
    assert_eq!(
        send(&office.mailbox, ADA_ID, 1, "owner"),
        Err(MailboxError::AlreadyExists)
    );
    assert_eq!(office.events.received(), [msg_id(1)]);
}

fn recipient(office: &Office<impl MessageStore>, n: u32, to: &str) -> Result<String, MailboxError> {
    send(&office.mailbox, PAT_ID, n, to)?;
    Ok(office
        .store
        .get_message(&msg_id(n))
        .unwrap()
        .unwrap()
        .message
        .header
        .to_member_id)
}

/// Two identical sends racing past the duplicate check commit once: one inserts, the other
/// replays (`inserted: false`), both return the same result, and only one
/// `message.received` is published.
pub fn mailbox_send_concurrent_replay_publishes_once(backend: impl OpenBackend) {
    let office = office(&backend);
    let weak: Weak<Mailbox> = Arc::downgrade(&office.mailbox);
    let (sent, received) = std::sync::mpsc::channel();
    *office.store.after_get.lock().unwrap() = Some(Box::new(move || {
        let mailbox = weak.upgrade().expect("mailbox alive");
        let result = thread::spawn(move || send(&mailbox, PAT_ID, 1, ADA_HANDLE))
            .join()
            .unwrap();
        sent.send(result).unwrap();
    }));
    let outer = send(&office.mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    let inner = received.recv().unwrap().unwrap();
    assert_eq!(outer, inner);
    assert_eq!(office.events.received(), [msg_id(1)]);
    assert_eq!(unread_inbox(&office.mailbox, ADA_ID), [msg_id(1)]);
}

/// Send refuses what msg.rs refuses: an oversized id, kind, `to`, subject, body or meta is
/// `too_large`; a non-ULID id, an empty, `request` or `delivery_failed` kind and an
/// `Unknown` priority are `invalid_request`. Nothing refused is committed or published.
pub fn mailbox_send_validation(backend: impl OpenBackend) {
    let office = office(&backend);
    let refused = |n: u32, error: MailboxError, change: &dyn Fn(&mut MsgSendParams)| {
        let mut params = send_params(n, ADA_HANDLE);
        change(&mut params);
        let id = params.id.clone();
        assert_eq!(office.mailbox.send(PAT_ID, params), Err(error), "case {n}");
        assert_eq!(office.store.get_message(&id).unwrap(), None, "case {n}");
    };
    use MailboxError::{InvalidRequest, TooLarge};
    // json_string_len counts the quotes: a string of len - 2 bytes fits exactly.
    let over = |limit: usize| "x".repeat(limit - 1);
    refused(1, TooLarge, &|p| p.id = over(MAX_ID_BYTES));
    refused(2, InvalidRequest, &|p| p.id = "not-a-ulid".into());
    refused(3, InvalidRequest, &|p| {
        p.id = msg_id(3).to_ascii_lowercase()
    });
    refused(4, InvalidRequest, &|p| p.kind = String::new());
    refused(5, InvalidRequest, &|p| p.kind = KIND_REQUEST.into());
    refused(6, InvalidRequest, &|p| p.kind = KIND_DELIVERY_FAILED.into());
    refused(7, TooLarge, &|p| p.kind = over(MAX_ID_BYTES));
    refused(8, InvalidRequest, &|p| {
        p.priority = Some(MessagePriority::Unknown)
    });
    refused(9, TooLarge, &|p| p.subject = Some(over(MAX_SUBJECT_BYTES)));
    refused(10, TooLarge, &|p| p.body = over(MAX_BODY_BYTES));
    refused(11, TooLarge, &|p| p.to = over(MAX_HANDLE_BYTES));
    refused(12, TooLarge, &|p| p.reply_to = Some(over(MAX_ID_BYTES)));
    refused(13, TooLarge, &|p| {
        p.meta
            .insert("k".into(), serde_json::Value::String(over(MAX_META_BYTES)));
    });
    assert!(office.events.received().is_empty());

    // At the limits, and with each allowed priority, it commits.
    let mut at_limit = send_params(20, ADA_HANDLE);
    at_limit.body = "x".repeat(MAX_BODY_BYTES - 2);
    at_limit.subject = Some("s".repeat(MAX_SUBJECT_BYTES - 2));
    at_limit.priority = Some(MessagePriority::High);
    office.mailbox.send(PAT_ID, at_limit).unwrap();
    let header = office
        .store
        .get_message(&msg_id(20))
        .unwrap()
        .unwrap()
        .message
        .header;
    assert!(header.urgent);
    assert_eq!(header.priority, MessagePriority::High);
    assert_eq!(office.events.received(), [msg_id(20)]);
}

/// With `reply_to`, the parent must be one the sender sent or received (else `not_found`)
/// and not a `delivery_failed` notice (`invalid_request`); the reply joins its thread.
pub fn mailbox_send_reply_to(backend: impl OpenBackend) {
    let office = office(&backend);
    let mailbox = &office.mailbox;
    let reply = |from: &str, n: u32, to: &str, parent: u32| {
        let mut params = send_params(n, to);
        params.reply_to = Some(msg_id(parent));
        mailbox.send(from, params)
    };
    send(mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    let answer = reply(ADA_ID, 2, PAT_HANDLE, 1).unwrap();
    assert_eq!(answer.thread_id, msg_id(1));
    let again = reply(PAT_ID, 3, ADA_HANDLE, 2).unwrap();
    assert_eq!(
        again.thread_id,
        msg_id(1),
        "a reply to a reply stays in the thread"
    );
    let stored = office.store.get_message(&msg_id(3)).unwrap().unwrap();
    assert_eq!(stored.message.header.reply_to, Some(msg_id(2)));
    assert_eq!(stored.message.header.thread_id, msg_id(1));

    // Remy neither sent nor received message 1: it can't reply into that thread.
    assert_eq!(
        reply(REMY_ID, 4, ADA_HANDLE, 1),
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        reply(ADA_ID, 5, PAT_HANDLE, 99),
        Err(MailboxError::NotFound)
    );

    let mut notice = new_message(6, OFFICE_SENDER, ADA_ID, MessagePriority::Normal);
    notice.message.header.kind = KIND_DELIVERY_FAILED.into();
    commit(&*office.store, notice);
    assert_eq!(
        reply(ADA_ID, 7, PAT_HANDLE, 6),
        Err(MailboxError::InvalidRequest)
    );
    for n in [4, 5, 7] {
        assert_eq!(office.store.get_message(&msg_id(n)).unwrap(), None);
    }
}

/// Addresses are trimmed of leading and trailing ASCII whitespace only; nothing left names
/// no member; `office` is refused.
pub fn mailbox_addressing_trims_ascii_whitespace(backend: impl OpenBackend) {
    let office = office(&backend);
    assert_eq!(recipient(&office, 1, "  ada\t\r\n").as_deref(), Ok(ADA_ID));
    assert_eq!(recipient(&office, 2, "\x0cADA ").as_deref(), Ok(ADA_ID));
    assert_eq!(
        recipient(&office, 3, "\u{a0}ada"),
        Err(MailboxError::UnknownRecipient)
    );
    assert_eq!(
        recipient(&office, 4, "ada\u{2003}"),
        Err(MailboxError::UnknownRecipient)
    );
    assert_eq!(
        recipient(&office, 5, " \t "),
        Err(MailboxError::UnknownRecipient)
    );
    assert_eq!(
        recipient(&office, 6, ""),
        Err(MailboxError::UnknownRecipient)
    );
    assert_eq!(
        recipient(&office, 7, " office "),
        Err(MailboxError::InvalidRequest)
    );
    assert_eq!(
        recipient(&office, 8, "Office"),
        Err(MailboxError::InvalidRequest)
    );
    for n in 3..=8 {
        assert_eq!(office.store.get_message(&msg_id(n)).unwrap(), None);
    }
}

/// `owner` comes first, an exact id beats a handle, and display names never resolve.
pub fn mailbox_addressing_owner_and_exact_id(backend: impl OpenBackend) {
    let office = office(&backend);
    assert_eq!(recipient(&office, 1, "OWNER").as_deref(), Ok(OWNER_ID));
    assert_eq!(recipient(&office, 2, " owner ").as_deref(), Ok(OWNER_ID));
    assert_eq!(recipient(&office, 3, PAT_ID).as_deref(), Ok(PAT_ID));
    assert_eq!(recipient(&office, 4, ADA_ID).as_deref(), Ok(ADA_ID));
    assert_eq!(
        recipient(&office, 5, OWNER_NAME),
        Err(MailboxError::UnknownRecipient)
    );
    assert_eq!(
        recipient(&office, 6, ADA_NAME),
        Err(MailboxError::UnknownRecipient)
    );
}

/// A paused agent binds and receives; an offboarded or Unknown-status agent is refused as a
/// recipient (`unknown_recipient`) and as the bound member (`not_bound`).
pub fn mailbox_paused_receives_offboarded_refused(backend: impl OpenBackend) {
    let office = office(&backend);
    let mailbox = &office.mailbox;
    send(mailbox, PAT_ID, 1, REMY_HANDLE).unwrap();
    assert_eq!(unread_inbox(mailbox, REMY_ID), [msg_id(1)]);
    let (_handle, cancel) = cancellation_pair();
    let waited = mailbox
        .wait(REMY_ID, wait_params(0), far_deadline(), &cancel)
        .unwrap();
    assert_eq!(waited.message.unwrap().header.id, msg_id(1));
    send(mailbox, REMY_ID, 2, PAT_HANDLE).unwrap();
    let read = mailbox.read(REMY_ID, MsgReadParams::default()).unwrap();
    assert_eq!(read.message.unwrap().header.id, msg_id(1));

    for (n, to) in [(3, GONE_HANDLE), (4, GONE_ID), (5, ODD_HANDLE), (6, ODD_ID)] {
        assert_eq!(
            send(mailbox, PAT_ID, n, to),
            Err(MailboxError::UnknownRecipient),
            "{to}"
        );
        assert_eq!(office.store.get_message(&msg_id(n)).unwrap(), None);
    }
    // The bound id must be the member's exact id: a lower-case spelling of Pat's id is the
    // shadow agent's handle, not a binding.
    let not_canonical = PAT_ID.to_ascii_lowercase();
    for bound in [GONE_ID, ODD_ID, FAR_ID, "nobody", not_canonical.as_str()] {
        assert_eq!(
            send(mailbox, bound, 7, ADA_HANDLE),
            Err(MailboxError::NotBound)
        );
        assert_eq!(
            mailbox.inbox(bound, MsgInboxParams::default()),
            Err(MailboxError::NotBound)
        );
        assert_eq!(
            mailbox.read(bound, MsgReadParams::default()),
            Err(MailboxError::NotBound)
        );
        assert_eq!(
            mailbox.wait(bound, wait_params(0), far_deadline(), &cancel),
            Err(MailboxError::NotBound)
        );
    }
    // Cross-office routing is not in slice 1.
    assert_eq!(
        send(mailbox, PAT_ID, 8, FAR_HANDLE),
        Err(MailboxError::Unavailable)
    );
    assert_eq!(office.mailbox.held_waits(), 0);
}

/// The inbox lists newest first, pages by cursor, filters by a trimmed sender and hides
/// read messages unless `include_all`.
pub fn mailbox_inbox_order_paging_and_read_state(backend: impl OpenBackend) {
    let office = office(&backend);
    let mailbox = &office.mailbox;
    for n in 1..=5 {
        let from = if n % 2 == 0 { OWNER_ID } else { PAT_ID };
        send(mailbox, from, n, ADA_HANDLE).unwrap();
    }
    let page = |params: MsgInboxParams| ids(&mailbox.inbox(ADA_ID, params).unwrap());
    let first = mailbox
        .inbox(
            ADA_ID,
            MsgInboxParams {
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(ids(&first), [msg_id(5), msg_id(4)]);
    assert_eq!(
        page(MsgInboxParams {
            limit: Some(2),
            cursor: first.next_cursor,
            ..Default::default()
        }),
        [msg_id(3), msg_id(2)]
    );
    assert_eq!(
        page(MsgInboxParams {
            from: Some("  owner ".into()),
            ..Default::default()
        }),
        [msg_id(4), msg_id(2)]
    );
    assert_eq!(
        mailbox.inbox(
            ADA_ID,
            MsgInboxParams {
                from: Some("nobody".into()),
                ..Default::default()
            }
        ),
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        mailbox.inbox(
            ADA_ID,
            MsgInboxParams {
                from: Some(" ".into()),
                ..Default::default()
            }
        ),
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        mailbox.inbox(
            ADA_ID,
            MsgInboxParams {
                from: Some("office".into()),
                ..Default::default()
            }
        ),
        Err(MailboxError::InvalidRequest)
    );
    assert_eq!(
        mailbox.inbox(
            ADA_ID,
            MsgInboxParams {
                limit: Some(101),
                ..Default::default()
            }
        ),
        Err(MailboxError::InvalidRequest)
    );

    mailbox
        .read(
            ADA_ID,
            MsgReadParams {
                id: Some(msg_id(3)),
            },
        )
        .unwrap();
    assert_eq!(
        unread_inbox(mailbox, ADA_ID),
        [msg_id(5), msg_id(4), msg_id(2), msg_id(1)]
    );
    let all = mailbox
        .inbox(
            ADA_ID,
            MsgInboxParams {
                include_all: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        ids(&all),
        [msg_id(5), msg_id(4), msg_id(3), msg_id(2), msg_id(1)]
    );
    assert_eq!(all.headers[2].state, State::Read);
    assert!(unread_inbox(mailbox, PAT_ID).is_empty());
}

/// Read claims the oldest unread (or the named) message once, stamps it, and publishes
/// one `message.state_changed`.
pub fn mailbox_read_claims_and_marks_read(backend: impl OpenBackend) {
    let office = office(&backend);
    let mailbox = &office.mailbox;
    send(mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    send(mailbox, PAT_ID, 2, ADA_HANDLE).unwrap();
    send(mailbox, PAT_ID, 3, REMY_HANDLE).unwrap();
    let read = mailbox.read(ADA_ID, MsgReadParams::default()).unwrap();
    let header = read.message.unwrap().header;
    assert_eq!(header.id, msg_id(1));
    assert_eq!(header.state, State::Read);
    assert_eq!(header.read_at.as_deref(), Some(NOW_UTC));
    assert_eq!(office.events.state_changed(), [msg_id(1)]);

    office.clock.set(NOW_MS + 1_000);
    let again = mailbox
        .read(
            ADA_ID,
            MsgReadParams {
                id: Some(msg_id(1)),
            },
        )
        .unwrap();
    assert_eq!(
        again.message.unwrap().header.read_at.as_deref(),
        Some(NOW_UTC)
    );
    assert_eq!(
        office.events.state_changed(),
        [msg_id(1)],
        "a re-read publishes nothing"
    );
    assert_eq!(
        mailbox.read(
            ADA_ID,
            MsgReadParams {
                id: Some(msg_id(3))
            }
        ),
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        mailbox.read(
            ADA_ID,
            MsgReadParams {
                id: Some(msg_id(9))
            }
        ),
        Err(MailboxError::NotFound)
    );
    let next = mailbox.read(ADA_ID, MsgReadParams::default()).unwrap();
    assert_eq!(next.message.unwrap().header.id, msg_id(2));
    assert_eq!(
        mailbox
            .read(ADA_ID, MsgReadParams::default())
            .unwrap()
            .message,
        None
    );
    assert_eq!(office.events.state_changed(), [msg_id(1), msg_id(2)]);
}

/// Concurrent reads of one inbox claim each message exactly once.
pub fn mailbox_concurrent_reads_claim_once(backend: impl OpenBackend) {
    let office = office(&backend);
    for n in 1..=4 {
        send(&office.mailbox, PAT_ID, n, ADA_HANDLE).unwrap();
    }
    let barrier = Barrier::new(6);
    let mut claimed: Vec<String> = thread::scope(|scope| {
        let workers: Vec<_> = (0..6)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    let mut mine = Vec::new();
                    // Bounded: an inbox that keeps returning a message fails, never spins.
                    for _ in 0..=4 {
                        let read = office.mailbox.read(ADA_ID, MsgReadParams::default());
                        let Some(message) = read.unwrap().message else {
                            break;
                        };
                        mine.push(message.header.id);
                    }
                    mine
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });
    claimed.sort();
    assert_eq!(claimed, (1..=4).map(msg_id).collect::<Vec<_>>());
    let mut events = office.events.state_changed();
    events.sort();
    assert_eq!(events, claimed);
}

/// Wait returns the oldest matching unread message without marking it read.
pub fn mailbox_wait_returns_unread_without_claiming(backend: impl OpenBackend) {
    let office = office(&backend);
    let mailbox = &office.mailbox;
    send(mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    send(mailbox, OWNER_ID, 2, ADA_HANDLE).unwrap();
    let (_handle, cancel) = cancellation_pair();
    for _ in 0..2 {
        let waited = mailbox
            .wait(ADA_ID, wait_params(0), far_deadline(), &cancel)
            .unwrap();
        assert_eq!(waited.message.unwrap().header.state, State::Unread);
    }
    let from_owner = MsgWaitParams {
        from: Some(" OWNER".into()),
        ..wait_params(0)
    };
    let waited = mailbox
        .wait(ADA_ID, from_owner, far_deadline(), &cancel)
        .unwrap();
    assert_eq!(waited.message.unwrap().header.id, msg_id(2));
    let urgent = MsgWaitParams {
        urgent_only: true,
        ..wait_params(0)
    };
    assert_eq!(
        mailbox
            .wait(ADA_ID, urgent, far_deadline(), &cancel)
            .unwrap()
            .message,
        None
    );
    let office_sender = MsgWaitParams {
        from: Some("office".into()),
        ..wait_params(0)
    };
    assert_eq!(
        mailbox.wait(ADA_ID, office_sender, far_deadline(), &cancel),
        Err(MailboxError::InvalidRequest)
    );
    assert_eq!(unread_inbox(mailbox, ADA_ID), [msg_id(2), msg_id(1)]);
    assert_eq!(mailbox.held_waits(), 0);
}

/// §2.2: a waiter registered before a commit is woken by it and gets the message once;
/// the message is delivered once (one event, one inbox row, one successful claim).
pub fn mailbox_wait_registered_before_commit_delivers_once(backend: impl OpenBackend) {
    let office = office(&backend);
    // Send only after the waiter's first check found nothing, so it must be woken.
    let (checked, first_check) = std::sync::mpsc::channel();
    *office.store.after_peek.lock().unwrap() = Some(Box::new(move || {
        let _ = checked.send(());
    }));
    let mailbox = office.mailbox.clone();
    let (_handle, cancel) = cancellation_pair();
    let waiter =
        thread::spawn(move || mailbox.wait(ADA_ID, wait_params(5_000), far_deadline(), &cancel));
    first_check
        .recv_timeout(Duration::from_secs(10))
        .expect("the waiter checks the inbox");
    assert_eq!(office.mailbox.held_waits(), 1);
    send(&office.mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    let waited = waiter.join().unwrap().unwrap();
    // Woken by the commit: a lost wake-up returns None at the 5 s timeout and fails here.
    assert_eq!(waited.message.unwrap().header.id, msg_id(1));
    assert_eq!(office.mailbox.held_waits(), 0);
    assert_eq!(office.events.received(), [msg_id(1)]);
    assert_eq!(unread_inbox(&office.mailbox, ADA_ID), [msg_id(1)]);
    let read = office
        .mailbox
        .read(ADA_ID, MsgReadParams::default())
        .unwrap();
    assert_eq!(read.message.unwrap().header.id, msg_id(1));
    assert_eq!(
        office
            .mailbox
            .read(ADA_ID, MsgReadParams::default())
            .unwrap()
            .message,
        None
    );
}

/// One commit wakes ALL of the member's held waits, and each returns the message; a wait
/// for another member stays held.
pub fn mailbox_wait_one_commit_wakes_every_waiter(backend: impl OpenBackend) {
    let office = office(&backend);
    let (_handle, cancel) = cancellation_pair();
    let (other_handle, other_cancel) = cancellation_pair();
    let other = {
        let mailbox = office.mailbox.clone();
        thread::spawn(move || {
            mailbox.wait(REMY_ID, wait_params(30_000), far_deadline(), &other_cancel)
        })
    };
    let waiters: Vec<_> = (0..3)
        .map(|_| {
            let mailbox = office.mailbox.clone();
            let cancel = cancel.clone();
            thread::spawn(move || mailbox.wait(ADA_ID, wait_params(5_000), far_deadline(), &cancel))
        })
        .collect();
    eventually("all four waiters to register", || {
        office.mailbox.held_waits() == 4
    });
    send(&office.mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    for waiter in waiters {
        let waited = waiter.join().unwrap().unwrap();
        assert_eq!(
            waited.message.map(|message| message.header.id),
            Some(msg_id(1)),
            "every waiter of the recipient is woken by the one commit"
        );
    }
    assert_eq!(
        office.mailbox.held_waits(),
        1,
        "the other member's wait stays held"
    );
    drop(other_handle);
    assert_eq!(other.join().unwrap(), Err(MailboxError::NotBound));
    assert_eq!(office.mailbox.held_waits(), 0);
    assert_eq!(office.events.received(), [msg_id(1)]);
}

/// §2.2: a commit that lands after the wait's check found nothing, but before it blocks,
/// still wakes it (the waiter was registered before the check).
pub fn mailbox_wait_commit_between_check_and_block(backend: impl OpenBackend) {
    let office = office(&backend);
    let weak: Weak<Mailbox> = Arc::downgrade(&office.mailbox);
    *office.store.after_peek.lock().unwrap() = Some(Box::new(move || {
        let mailbox = weak.upgrade().expect("mailbox alive");
        thread::spawn(move || send(&mailbox, PAT_ID, 1, ADA_HANDLE).unwrap())
            .join()
            .unwrap();
    }));
    let (_handle, cancel) = cancellation_pair();
    // A lost wake-up returns None at the 3 s deadline, which the assertion below catches.
    let waited = office
        .mailbox
        .wait(ADA_ID, wait_params(3_000), far_deadline(), &cancel)
        .unwrap();
    assert_eq!(
        waited.message.map(|message| message.header.id),
        Some(msg_id(1)),
        "the commit between check and block was lost"
    );
    assert_eq!(office.mailbox.held_waits(), 0);
}

/// Wait never blocks past the earlier of `timeout_ms` and the caller's deadline, and
/// checks once even when the deadline has passed.
pub fn mailbox_wait_deadline(backend: impl OpenBackend) {
    let office = office(&backend);
    let mailbox = &office.mailbox;
    let (_handle, cancel) = cancellation_pair();

    let started = Instant::now();
    let waited = mailbox
        .wait(ADA_ID, wait_params(50), far_deadline(), &cancel)
        .unwrap();
    assert_eq!(waited.message, None);
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(50), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");

    // The caller's deadline beats a longer timeout_ms. A wait that ignored it would block
    // the full FALLBACK_MS and fail the bound below, so a broken backend fails in seconds.
    const FALLBACK_MS: u64 = 8_000;
    let bound = Duration::from_millis(FALLBACK_MS / 2);
    let started = Instant::now();
    let deadline = Instant::now() + Duration::from_millis(50);
    let waited = mailbox
        .wait(ADA_ID, wait_params(FALLBACK_MS), deadline, &cancel)
        .unwrap();
    assert_eq!(waited.message, None);
    assert!(started.elapsed() >= Duration::from_millis(50));
    assert!(
        started.elapsed() < bound,
        "the caller's deadline was ignored"
    );

    let passed = Instant::now();
    let started = Instant::now();
    assert_eq!(
        mailbox
            .wait(ADA_ID, wait_params(FALLBACK_MS), passed, &cancel)
            .unwrap()
            .message,
        None
    );
    assert!(
        started.elapsed() < bound,
        "a passed deadline must not block"
    );
    send(mailbox, PAT_ID, 1, ADA_HANDLE).unwrap();
    let started = Instant::now();
    let waited = mailbox
        .wait(ADA_ID, wait_params(FALLBACK_MS), passed, &cancel)
        .unwrap();
    assert_eq!(waited.message.unwrap().header.id, msg_id(1));
    assert!(started.elapsed() < bound);

    assert_eq!(
        mailbox.wait(
            ADA_ID,
            wait_params(MAX_WAIT_MS + 1),
            Instant::now(),
            &cancel
        ),
        Err(MailboxError::InvalidRequest)
    );
    assert_eq!(mailbox.held_waits(), 0);
}

/// Dropping or cancelling the handle ends blocked waits at once with `not_bound` and
/// removes their registrations; a cancelled handle refuses new waits.
pub fn mailbox_wait_cancel_removes_waiter(backend: impl OpenBackend) {
    let office = office(&backend);
    let (handle, cancel) = cancellation_pair();
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let mailbox = office.mailbox.clone();
            let cancel = cancel.clone();
            thread::spawn(move || {
                let started = Instant::now();
                let result = mailbox.wait(ADA_ID, wait_params(5_000), far_deadline(), &cancel);
                (result, started.elapsed())
            })
        })
        .collect();
    eventually("both waiters to register", || {
        office.mailbox.held_waits() == 2
    });
    drop(handle);
    // A worker caught between registering and attaching removes its own registration when
    // it sees the cancel, so allow it a moment (waiters' unit tests pin the drop's removal).
    eventually("the cancel to remove the registrations", || {
        office.mailbox.held_waits() == 0
    });
    for worker in workers {
        let (result, elapsed) = worker.join().unwrap();
        assert_eq!(result, Err(MailboxError::NotBound));
        assert!(
            elapsed < Duration::from_secs(4),
            "woken by the cancel, not the deadline"
        );
    }
    assert_eq!(
        office
            .mailbox
            .wait(ADA_ID, wait_params(5_000), far_deadline(), &cancel),
        Err(MailboxError::NotBound)
    );

    let (handle, cancel) = cancellation_pair();
    let mailbox = office.mailbox.clone();
    let worker =
        thread::spawn(move || mailbox.wait(ADA_ID, wait_params(5_000), far_deadline(), &cancel));
    eventually("the waiter to register", || {
        office.mailbox.held_waits() == 1
    });
    handle.cancel();
    assert_eq!(worker.join().unwrap(), Err(MailboxError::NotBound));
    assert_eq!(office.mailbox.held_waits(), 0);
}

/// A commit racing a cancel: the wait either returns the message or ends `not_bound`,
/// never leaves a registration, and the message is in the inbox exactly once, unread.
pub fn mailbox_wait_commit_racing_cancel(backend: impl OpenBackend) {
    let office = office(&backend);
    for n in 1..=100u32 {
        let (handle, cancel) = cancellation_pair();
        let mailbox = office.mailbox.clone();
        let waiter = thread::spawn(move || {
            mailbox.wait(ADA_ID, wait_params(5_000), far_deadline(), &cancel)
        });
        let mailbox = office.mailbox.clone();
        let sender = thread::spawn(move || send(&mailbox, PAT_ID, n, ADA_HANDLE));
        for _ in 0..n % 4 {
            thread::yield_now();
        }
        drop(handle);
        match waiter.join().unwrap() {
            Ok(result) => {
                assert_eq!(
                    result.message.map(|message| message.header.id),
                    Some(msg_id(n))
                )
            }
            Err(error) => assert_eq!(error, MailboxError::NotBound),
        }
        sender.join().unwrap().unwrap();
        assert_eq!(office.mailbox.held_waits(), 0);
        assert_eq!(unread_inbox(&office.mailbox, ADA_ID), [msg_id(n)]);
        let read = office
            .mailbox
            .read(ADA_ID, MsgReadParams::default())
            .unwrap();
        assert_eq!(read.message.unwrap().header.id, msg_id(n));
    }
    assert_eq!(office.events.received().len(), 100);
}

/// Every case, by name: what [`crate::mailbox_conformance_tests!`] runs.
#[doc(hidden)]
#[macro_export]
macro_rules! __mailbox_conformance_cases {
    ($open:expr; $($case:ident),* $(,)?) => {
        $(
            #[test]
            fn $case() {
                $crate::testkit::conformance::$case($open)
            }
        )*
    };
}

/// Generates one `#[test]` per conformance case, each opening a fresh backend with
/// `$open`, any `Fn(&[Member], Position) -> (impl MessageStore, impl MemberDirectory)`
/// (both `Send + Sync + 'static`). Needs the `testkit` feature. The backend seeds
/// `testkit::fixture::members()` raw: the fixture holds a handle shaped like an id, which
/// HQ's validated commands would refuse.
///
/// ```ignore
/// mod conformance {
///     org_mailbox::mailbox_conformance_tests!(super::open_sqlite);
/// }
/// ```
#[macro_export]
macro_rules! mailbox_conformance_tests {
    ($open:expr) => {
        $crate::__mailbox_conformance_cases! {
            $open;
            store_commit_allocates_sequence_and_origin,
            store_commit_replay_and_conflict,
            store_commit_refuses_a_second_delivery_failed,
            store_find_reply_for_ask,
            store_initial_read_is_never_unread,
            store_peek_is_read_only_and_oldest_first,
            store_peek_reports_expired_without_deleting,
            store_peek_filters_expired_ids,
            store_peek_unreadable_now_passes_every_deadline,
            store_peek_caps_expired_ids_oldest_first,
            store_claim_read_order_and_ownership,
            store_claim_read_is_once_under_concurrency,
            store_claim_read_k_barrier_claimers_all_get_distinct,
            store_commits_and_claims_are_visible_to_other_readers,
            store_claim_read_after_a_lost_race_takes_the_next,
            store_claim_read_selects_and_marks_atomically,
            store_list_headers_order_paging_and_filters,
            directory_resolution_order,
            directory_returns_status_unfiltered,
            directory_handles_fold_unicode_case,
            mailbox_send_commits_with_origin,
            mailbox_send_replay_and_conflict,
            mailbox_send_concurrent_replay_publishes_once,
            mailbox_send_validation,
            mailbox_send_reply_to,
            mailbox_addressing_trims_ascii_whitespace,
            mailbox_addressing_owner_and_exact_id,
            mailbox_paused_receives_offboarded_refused,
            mailbox_inbox_order_paging_and_read_state,
            mailbox_read_claims_and_marks_read,
            mailbox_concurrent_reads_claim_once,
            mailbox_wait_returns_unread_without_claiming,
            mailbox_wait_registered_before_commit_delivers_once,
            mailbox_wait_one_commit_wakes_every_waiter,
            mailbox_wait_commit_between_check_and_block,
            mailbox_wait_deadline,
            mailbox_wait_cancel_removes_waiter,
            mailbox_wait_commit_racing_cancel,
        }
    };
}
