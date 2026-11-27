use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier, Mutex};
use std::thread;
use std::time::Duration;

use org_proto::msg::{
    HeaderPage, Message, MessageHeader, MessageLifecycleState, MessagePriority, Meta,
    MsgDeferParams, MsgInboxParams, MsgReadParams, MsgSendParams, MsgUnsnoozeParams, SnoozeTarget,
    KIND_DELIVERY_FAILED, KIND_MSG,
};

use super::*;
use crate::ids::delivery_failed_id;
use crate::testkit::fixture::*;
use crate::testkit::{InMemoryDirectory, InMemoryStore};
use crate::{
    format_utc, CommitMessageOutcome, HeaderQuery, HeaderScope, LocalStateTransition, MailboxError,
    MessageStore, NewStoredMessage, NoMessageEvents, NotifyCandidate, SnoozeDeadline, StoreError,
    StoredMessage, TriageStore,
};

/// A triage store whose first `update_state_local` applies, then holds before returning
/// until the test lets it go: the window in which another defer could overtake it.
struct GatedTriage {
    inner: InMemoryStore,
    gated: AtomicBool,
    applied: Mutex<mpsc::Sender<()>>,
    release: Barrier,
}

impl TriageStore for GatedTriage {
    fn update_state_local(
        &self,
        change: LocalStateTransition,
    ) -> Result<MessageHeader, StoreError> {
        let result = self.inner.update_state_local(change);
        if self.gated.swap(false, Ordering::SeqCst) {
            lock(&self.applied).send(()).unwrap();
            self.release.wait();
        }
        result
    }
    fn defer_local(&self, change: LocalStateTransition) -> Result<MessageHeader, StoreError> {
        let result = self.inner.defer_local(change);
        if self.gated.swap(false, Ordering::SeqCst) {
            lock(&self.applied).send(()).unwrap();
            self.release.wait();
        }
        result
    }
    fn unsnooze_local(&self, change: LocalStateTransition) -> Result<MessageHeader, StoreError> {
        self.inner.unsnooze_local(change)
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

/// A message store over an [`InMemoryStore`] that races the Mailbox's notice commits. Its
/// `fail_commit`th `commit_message` call (from 1) fails with the given error after landing
/// another expiry's notice first (the same notice under [`RACING_NOTICE`]), and its next
/// `blind_finds` `find_reply_for_ask` calls answer `None`, as if no notice had landed yet.
#[derive(Default)]
struct RacingStore {
    inner: InMemoryStore,
    commits: AtomicUsize,
    fail_commit: Mutex<Option<(usize, StoreError)>>,
    blind_finds: Mutex<usize>,
}

/// The id another expiry's notice lands under in [`RacingStore`].
const RACING_NOTICE: &str = "01JCRACINGNOTICE0000000000";

impl MessageStore for RacingStore {
    fn commit_message(&self, msg: NewStoredMessage) -> Result<CommitMessageOutcome, StoreError> {
        let call = self.commits.fetch_add(1, Ordering::SeqCst) + 1;
        let failure = {
            let mut fail = lock(&self.fail_commit);
            match fail.take() {
                Some((at, error)) if at == call => Some(error),
                other => {
                    *fail = other;
                    None
                }
            }
        };
        let Some(error) = failure else {
            return self.inner.commit_message(msg);
        };
        let racing = NewStoredMessage {
            message: Message {
                header: MessageHeader {
                    id: RACING_NOTICE.into(),
                    ..msg.message.header
                },
                ..msg.message
            },
            ..msg
        };
        assert!(
            self.inner
                .commit_message(racing)
                .expect("the racing notice")
                .inserted
        );
        Err(error)
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
    ) -> Result<crate::PeekUnreadOutcome, StoreError> {
        self.inner
            .peek_next_unread(to_member_id, from_member_id, urgent_only, now_utc)
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
        {
            let mut blind = lock(&self.blind_finds);
            if *blind > 0 {
                *blind -= 1;
                return Ok(None);
            }
        }
        self.inner.find_reply_for_ask(ask_id, responder_id)
    }
}

/// A Mailbox (no triage) over `store`, recording its events.
fn racing_mailbox(store: &Arc<RacingStore>, events: &Arc<RecordingEvents>) -> Mailbox {
    Mailbox::new(
        store.clone(),
        Arc::new(InMemoryDirectory::new(members(), POSITION)),
        identity(),
        Arc::new(ManualClock::new(NOW_MS)),
        events.clone(),
    )
}

/// Commits `msg_id(1)`, an expired request from Pat to Ada, and returns its header.
fn expired_ask(store: &impl MessageStore) -> MessageHeader {
    use crate::testkit::conformance::{commit, expired_request};

    let mut ask = expired_request(1, PAT_ID, ADA_ID);
    ask.message.header.to_display_name = ADA_NAME.into();
    commit(store, ask);
    store
        .get_message(&msg_id(1))
        .unwrap()
        .unwrap()
        .message
        .header
}

/// The ids of the `delivery_failed` notices in Pat's inbox replying to `msg_id(1)`.
fn notices(store: &impl MessageStore) -> Vec<String> {
    store
        .list_headers(HeaderQuery {
            scope: HeaderScope::Inbox(PAT_ID.into()),
            from_member_id: None,
            thread_id: None,
            unread_only: false,
            include_snoozed: true,
            cursor: None,
            limit: 100,
        })
        .unwrap()
        .headers
        .into_iter()
        .filter(|header| {
            header.kind == KIND_DELIVERY_FAILED
                && header.reply_to.as_deref() == Some(msg_id(1).as_str())
        })
        .map(|header| header.id)
        .collect()
}

/// A notice commit that fails after another expiry's notice landed leaves the asker told:
/// `Ok`, with exactly that one notice, and nothing published by the loser. That is two
/// expiries racing to the fallback (SQL's unique index fails the second with a storage
/// error, the memory store with `AlreadyExists`), or a first-id commit losing that way.
#[test]
fn a_notice_commit_failing_after_another_notice_landed_is_told() {
    use crate::testkit::conformance::{commit, new_message};

    let errors: [fn() -> StoreError; 2] = [
        || StoreError::new("UNIQUE constraint failed: msg_headers.reply_to"),
        || StoreError::AlreadyExists,
    ];
    // (the failing commit, whether the first id is squatted so the fallback is tried)
    for (failing, squatted) in [(2, true), (1, false)] {
        for error in errors {
            let store = Arc::new(RacingStore::default());
            let events = Arc::new(RecordingEvents::default());
            let mailbox = racing_mailbox(&store, &events);
            let request = expired_ask(&store.inner);
            if squatted {
                let mut squat = new_message(2, REMY_ID, PAT_ID, MessagePriority::Normal);
                squat.message.header.id = delivery_failed_id(&request, INCARNATION);
                commit(&store.inner, squat);
            }
            *lock(&store.fail_commit) = Some((failing, error()));

            assert_eq!(
                mailbox.notify_expired(&request, NOW_UTC),
                Ok(()),
                "commit {failing} failed after the racing notice landed"
            );
            assert_eq!(store.commits.load(Ordering::SeqCst), failing);
            assert_eq!(notices(&store.inner), [RACING_NOTICE]);
            assert!(events.delivery_failed().is_empty());
        }
    }
}

/// Without another notice, a failed commit is still an error.
#[test]
fn a_failed_notice_commit_without_another_notice_fails() {
    let store = Arc::new(RacingStore::default());
    let mailbox = racing_mailbox(&store, &Arc::new(RecordingEvents::default()));
    let request = expired_ask(&store.inner);
    // The commit fails (landing its racing notice first), but the asker never shows up as
    // told: every check misses it.
    *lock(&store.fail_commit) = Some((1, StoreError::new("disk I/O error")));
    *lock(&store.blind_finds) = usize::MAX;
    assert_eq!(
        mailbox.notify_expired(&request, NOW_UTC),
        Err(MailboxError::StorageError)
    );
}

fn identity() -> OfficeIdentity {
    OfficeIdentity {
        office_id: OFFICE_ID.into(),
        incarnation: INCARNATION.into(),
        is_hq: true,
    }
}

fn defer_at(offset_ms: u64) -> MsgDeferParams {
    MsgDeferParams {
        id: msg_id(1),
        until: SnoozeTarget::Until {
            at: format_utc(NOW_MS + offset_ms).unwrap(),
        },
        note: None,
    }
}

/// Two defers of one message: the second one starts while the first has written the
/// store but not yet its timer. It must wait for the first to finish, so the timers end
/// at the deadline the store holds (the second's).
#[test]
fn concurrent_defers_leave_the_timer_at_the_stores_deadline() {
    let store = InMemoryStore::new();
    let (applied, first_applied) = mpsc::channel();
    let triage = Arc::new(GatedTriage {
        inner: store.clone(),
        gated: AtomicBool::new(false),
        applied: Mutex::new(applied),
        release: Barrier::new(2),
    });
    let mailbox = Arc::new(
        Mailbox::with_triage(
            Arc::new(store.clone()),
            triage.clone(),
            Arc::new(InMemoryDirectory::new(members(), POSITION)),
            identity(),
            Arc::new(ManualClock::new(NOW_MS)),
            Arc::new(NoMessageEvents),
        )
        .unwrap(),
    );
    let sent = MsgSendParams {
        id: msg_id(1),
        to: ADA_ID.into(),
        kind: KIND_MSG.into(),
        priority: None,
        subject: None,
        body: "hello".into(),
        meta: Meta::new(),
        reply_to: None,
        attachment_ids: Vec::new(),
    };
    mailbox.send(PAT_ID, sent).unwrap();

    triage.gated.store(true, Ordering::SeqCst);
    let first = {
        let mailbox = mailbox.clone();
        thread::spawn(move || mailbox.defer(ADA_ID, defer_at(1_000)))
    };
    // Bounded, so a first defer that fails before its store write fails the test rather
    // than hanging it.
    first_applied
        .recv_timeout(Duration::from_secs(10))
        .expect("the first defer reached its store write");
    let (finished, second_done) = mpsc::channel();
    let second = {
        let mailbox = mailbox.clone();
        thread::spawn(move || {
            let result = mailbox.defer(ADA_ID, defer_at(2_000));
            finished.send(()).unwrap();
            result
        })
    };
    // Without the ordering the second defer writes the store and its timer now, and the
    // first then overwrites the timer with its own (older) deadline.
    assert!(
        second_done
            .recv_timeout(Duration::from_millis(300))
            .is_err(),
        "the second defer finished while the first's timer update was pending"
    );
    triage.release.wait();
    assert_eq!(first.join().unwrap().unwrap().triage_gen, 1);
    let second = second.join().unwrap().unwrap();
    assert_eq!(second.triage_gen, 2);

    let stored = store
        .get_message(&msg_id(1))
        .unwrap()
        .unwrap()
        .message
        .header;
    let deadline = format_utc(NOW_MS + 2_000).unwrap();
    assert_eq!(stored.snoozed_until.as_deref(), Some(deadline.as_str()));
    let timers = lock(&mailbox.snoozes);
    assert_eq!(timers.next_for(ADA_ID), Some(epoch_ms(NOW_MS + 2_000)));
}

/// Expiring a request more than once commits one notice and announces it once:
///
/// - a later expiry finds the notice through the store (`find_reply_for_ask`) and makes
///   no commit at all;
/// - one racing the first, whose check doesn't see the notice yet, commits under the same
///   first id and gets the replay (`inserted: false`), so it publishes nothing;
/// - one racing it with other content (the recipient renamed since) is refused
///   (`AlreadyExists`), asks the store again, finds the notice and tries no fallback.
///
/// Each counts as told.
#[test]
fn expiring_twice_commits_exactly_one_notice() {
    let store = Arc::new(RacingStore::default());
    let events = Arc::new(RecordingEvents::default());
    let mailbox = racing_mailbox(&store, &events);
    let request = expired_ask(&store.inner);
    let commits = || store.commits.load(Ordering::SeqCst);
    let notice = delivery_failed_id(&request, INCARNATION);

    assert_eq!(mailbox.notify_expired(&request, NOW_UTC), Ok(()));
    assert_eq!(commits(), 1);
    assert_eq!(notices(&store.inner), std::slice::from_ref(&notice));

    // Found by the store: no commit.
    assert_eq!(mailbox.notify_expired(&request, NOW_UTC), Ok(()));
    assert_eq!(commits(), 1);

    // Racing, same content: the replay of the first id.
    *lock(&store.blind_finds) = 1;
    assert_eq!(mailbox.notify_expired(&request, NOW_UTC), Ok(()));
    assert_eq!(*lock(&store.blind_finds), 0, "the check missed the notice");
    assert_eq!(commits(), 2, "committed again under the first id");

    // Racing, other content: refused, then found on the second check.
    let renamed = MessageHeader {
        to_display_name: "Ada L.".into(),
        ..request.clone()
    };
    *lock(&store.blind_finds) = 1;
    assert_eq!(mailbox.notify_expired(&renamed, NOW_UTC), Ok(()));
    assert_eq!(
        *lock(&store.blind_finds),
        0,
        "the first check missed the notice"
    );
    assert_eq!(commits(), 3, "no fallback after the refusal");

    assert_eq!(notices(&store.inner), std::slice::from_ref(&notice));
    let stored = store.inner.get_message(&notice).unwrap().unwrap();
    assert!(is_notice_for(&stored.message.header, &request));
    assert_eq!(stored.message.header.to_display_name, PAT_HANDLE);
    assert_eq!(events.received(), [notice]);
    assert_eq!(events.delivery_failed().len(), 1);
}

fn test_triage_mailbox(clock: Arc<ManualClock>, events: Arc<RecordingEvents>) -> Mailbox {
    let store = Arc::new(InMemoryStore::new());
    let dir = Arc::new(InMemoryDirectory::new(members(), POSITION));
    Mailbox::with_triage(
        store.clone(),
        store,
        dir,
        OfficeIdentity {
            office_id: OFFICE_ID.into(),
            incarnation: INCARNATION.into(),
            is_hq: true,
        },
        clock,
        events,
    )
    .unwrap()
}

#[test]
fn unsnooze_and_release_all_due_snoozes_resurface_and_emit_events() {
    let clock = Arc::new(ManualClock::new(NOW_MS));
    let events = Arc::new(RecordingEvents::default());
    let mb = test_triage_mailbox(Arc::clone(&clock), Arc::clone(&events));
    let sent = mb
        .send(
            PAT_ID,
            MsgSendParams {
                id: msg_id(71),
                to: ADA_ID.into(),
                kind: KIND_MSG.into(),
                priority: None,
                subject: Some("Snooze test".into()),
                body: "hello".into(),
                meta: Meta::new(),
                reply_to: None,
                attachment_ids: Vec::new(),
            },
        )
        .unwrap();
    let target = format_utc(NOW_MS + 5_000).unwrap();
    let deferred = mb
        .defer(
            ADA_ID,
            MsgDeferParams {
                id: sent.id.clone(),
                until: SnoozeTarget::Until { at: target },
                note: Some("after standup".into()),
            },
        )
        .unwrap();
    assert_eq!(deferred.triage_gen, 1);
    assert!(mb.next_snooze_deadline_ms().is_some());

    // Early un-snooze returns to Unread, clears timer, increments triage_gen.
    let unsnoozed = mb
        .unsnooze(
            ADA_ID,
            MsgUnsnoozeParams {
                id: sent.id.clone(),
            },
        )
        .unwrap();
    assert_eq!(unsnoozed.state, MessageLifecycleState::Unread);
    assert_eq!(unsnoozed.triage_gen, 2);
    assert_eq!(mb.next_snooze_deadline_ms(), None);

    // Re-snooze and advance clock past deadline: release_all_due_snoozes resurfaces it.
    let due_at = format_utc(NOW_MS + 2_000).unwrap();
    mb.defer(
        ADA_ID,
        MsgDeferParams {
            id: sent.id.clone(),
            until: SnoozeTarget::Until { at: due_at },
            note: None,
        },
    )
    .unwrap();
    assert_eq!(mb.release_all_due_snoozes().unwrap(), 0);
    clock.set(NOW_MS + 2_500);
    assert_eq!(mb.release_all_due_snoozes().unwrap(), 1);
    let inbox = mb
        .inbox(
            ADA_ID,
            MsgInboxParams {
                from: None,
                thread_id: None,
                include_all: false,
                cursor: None,
                limit: None,
            },
        )
        .unwrap();
    assert_eq!(inbox.headers.len(), 1);
    assert_eq!(inbox.headers[0].state, MessageLifecycleState::Unread);
    assert_eq!(events.state_changed().len(), 4);
}

#[test]
fn defer_rejects_past_expiry_and_does_not_leak_to_non_recipients() {
    let clock = Arc::new(ManualClock::new(NOW_MS));
    let events = Arc::new(RecordingEvents::default());
    let store = Arc::new(InMemoryStore::new());
    let dir = Arc::new(InMemoryDirectory::new(members(), POSITION));
    let mb = Mailbox::with_triage(
        store.clone(),
        store.clone(),
        dir,
        OfficeIdentity {
            office_id: OFFICE_ID.into(),
            incarnation: INCARNATION.into(),
            is_hq: true,
        },
        clock,
        events,
    )
    .unwrap();
    let mut ask = crate::testkit::conformance::expired_request(72, PAT_ID, ADA_ID);
    ask.message.header.expires_at = Some(format_utc(NOW_MS + 120_000).unwrap());
    ask.message.header.ttl_ms = Some(120_000);
    let ask_id = ask.message.header.id.clone();
    store.commit_message(ask).unwrap();

    // Non-recipient gets NotFound even when snoozing past expiry (no existence leak).
    let past_exp = format_utc(NOW_MS + 180_000).unwrap();
    assert_eq!(
        mb.defer(
            PAT_ID,
            MsgDeferParams {
                id: ask_id.clone(),
                until: SnoozeTarget::Until {
                    at: past_exp.clone()
                },
                note: None,
            },
        ),
        Err(MailboxError::NotFound)
    );
    // Recipient cannot snooze at/past ask expires_at, nor in the past.
    assert_eq!(
        mb.defer(
            ADA_ID,
            MsgDeferParams {
                id: ask_id.clone(),
                until: SnoozeTarget::Until { at: past_exp },
                note: None,
            },
        ),
        Err(MailboxError::InvalidRequest)
    );
    let in_past = format_utc(NOW_MS - 1_000).unwrap();
    assert_eq!(
        mb.defer(
            ADA_ID,
            MsgDeferParams {
                id: ask_id.clone(),
                until: SnoozeTarget::Until { at: in_past },
                note: None,
            },
        ),
        Err(MailboxError::InvalidRequest)
    );
    // Strictly before expires_at succeeds.
    let before_exp = format_utc(NOW_MS + 30_000).unwrap();
    assert!(mb
        .defer(
            ADA_ID,
            MsgDeferParams {
                id: ask_id,
                until: SnoozeTarget::Until { at: before_exp },
                note: None,
            },
        )
        .is_ok());
}

#[test]
fn unsnooze_refuses_non_snoozed_and_read_racing_defer_keeps_timer() {
    let clock = Arc::new(ManualClock::new(NOW_MS));
    let events = Arc::new(RecordingEvents::default());
    let mb = test_triage_mailbox(Arc::clone(&clock), Arc::clone(&events));
    let sent = mb
        .send(
            PAT_ID,
            MsgSendParams {
                id: msg_id(73),
                to: ADA_ID.into(),
                kind: KIND_MSG.into(),
                priority: None,
                subject: Some("Race".into()),
                body: "hello".into(),
                meta: Meta::new(),
                reply_to: None,
                attachment_ids: Vec::new(),
            },
        )
        .unwrap();
    // Unread -> unsnooze refused.
    assert_eq!(
        mb.unsnooze(
            ADA_ID,
            MsgUnsnoozeParams {
                id: sent.id.clone()
            }
        ),
        Err(MailboxError::InvalidRequest)
    );
    // Read -> unsnooze refused; defer after read installs timer cleanly.
    let read = mb
        .read(
            ADA_ID,
            MsgReadParams {
                id: Some(sent.id.clone()),
            },
        )
        .unwrap();
    assert!(read.message.is_some());
    assert_eq!(
        mb.unsnooze(
            ADA_ID,
            MsgUnsnoozeParams {
                id: sent.id.clone()
            }
        ),
        Err(MailboxError::InvalidRequest)
    );
    let until = format_utc(NOW_MS + 3_000).unwrap();
    mb.defer(
        ADA_ID,
        MsgDeferParams {
            id: sent.id.clone(),
            until: SnoozeTarget::Until { at: until },
            note: None,
        },
    )
    .unwrap();
    assert_eq!(mb.next_snooze_deadline_ms(), Some(epoch_ms(NOW_MS + 3_000)));
    // Done -> unsnooze refused and timer removed.
    mb.done(
        ADA_ID,
        org_proto::msg::MsgDoneParams {
            id: sent.id.clone(),
            note: None,
        },
    )
    .unwrap();
    assert_eq!(mb.next_snooze_deadline_ms(), None);
    assert_eq!(
        mb.unsnooze(ADA_ID, MsgUnsnoozeParams { id: sent.id }),
        Err(MailboxError::InvalidRequest)
    );
}

struct FailingNotifyTriage(Arc<InMemoryStore>);

impl TriageStore for FailingNotifyTriage {
    fn update_state_local(
        &self,
        change: LocalStateTransition,
    ) -> Result<MessageHeader, StoreError> {
        self.0.update_state_local(change)
    }
    fn defer_local(&self, change: LocalStateTransition) -> Result<MessageHeader, StoreError> {
        self.0.defer_local(change)
    }
    fn unsnooze_local(&self, change: LocalStateTransition) -> Result<MessageHeader, StoreError> {
        self.0.unsnooze_local(change)
    }
    fn check_notify(
        &self,
        _to_member_id: &str,
        _after_task: bool,
        _urgent_only: bool,
        _sweep_before_utc: &str,
        _now_utc: &str,
    ) -> Result<Vec<NotifyCandidate>, StoreError> {
        Err(StoreError::new("simulated db outage"))
    }
    fn list_active_snooze_deadlines(&self) -> Result<Vec<SnoozeDeadline>, StoreError> {
        self.0.list_active_snooze_deadlines()
    }
}

#[test]
fn release_all_due_snoozes_restores_timers_and_propagates_store_error() {
    let clock = Arc::new(ManualClock::new(NOW_MS));
    let store = Arc::new(InMemoryStore::new());
    let triage = Arc::new(FailingNotifyTriage(Arc::clone(&store)));
    let mb = Mailbox::with_triage(
        store,
        triage,
        Arc::new(InMemoryDirectory::new(members(), POSITION)),
        OfficeIdentity {
            office_id: OFFICE_ID.into(),
            incarnation: INCARNATION.into(),
            is_hq: true,
        },
        clock.clone(),
        Arc::new(NoMessageEvents),
    )
    .unwrap();
    let sent = mb
        .send(
            PAT_ID,
            MsgSendParams {
                id: msg_id(74),
                to: ADA_ID.into(),
                kind: KIND_MSG.into(),
                priority: None,
                subject: None,
                body: "hi".into(),
                meta: Meta::new(),
                reply_to: None,
                attachment_ids: Vec::new(),
            },
        )
        .unwrap();
    mb.defer(
        ADA_ID,
        MsgDeferParams {
            id: sent.id,
            until: SnoozeTarget::Until {
                at: format_utc(NOW_MS + 1_000).unwrap(),
            },
            note: None,
        },
    )
    .unwrap();
    clock.set(NOW_MS + 2_000);
    assert_eq!(
        mb.release_all_due_snoozes(),
        Err(MailboxError::StorageError)
    );
    assert_eq!(
        mb.next_snooze_deadline_ms(),
        Some(epoch_ms(NOW_MS + 1_000)),
        "failed check_notify must restore timers for retry"
    );
}

#[test]
fn the_bare_mailbox_binds_no_file_and_refuses_a_message_that_says_nothing() {
    let store = Arc::new(InMemoryStore::new());
    let mailbox = Mailbox::new(
        store.clone(),
        Arc::new(InMemoryDirectory::new(members(), POSITION)),
        identity(),
        Arc::new(ManualClock::new(NOW_MS)),
        Arc::new(NoMessageEvents),
    );
    let params = |n: u32, body: &str, files: Vec<String>| MsgSendParams {
        id: msg_id(n),
        to: ADA_ID.into(),
        kind: KIND_MSG.into(),
        priority: None,
        subject: None,
        body: body.into(),
        meta: Meta::new(),
        reply_to: None,
        attachment_ids: files,
    };
    for (n, body, files) in [
        (1, "", Vec::new()),
        (2, " \n\t", Vec::new()),
        (3, "with a file", vec![msg_id(90)]),
        (4, "", vec![msg_id(91)]),
    ] {
        assert_eq!(
            mailbox.send(PAT_ID, params(n, body, files)).unwrap_err(),
            MailboxError::InvalidRequest,
            "message {n}"
        );
        assert!(store.get_message(&msg_id(n)).unwrap().is_none());
    }
    let sent = mailbox
        .send(PAT_ID, params(5, "hello", Vec::new()))
        .unwrap();
    // A reply passes its files on, so the bare mailbox refuses it the same way.
    let reply = org_proto::msg::MsgReplyParams {
        id: msg_id(6),
        to_id: sent.id,
        priority: None,
        subject: None,
        body: "thanks".into(),
        meta: Meta::new(),
        attachment_ids: vec![msg_id(92)],
    };
    assert_eq!(
        mailbox.reply(ADA_ID, reply).unwrap_err(),
        MailboxError::InvalidRequest
    );
}
