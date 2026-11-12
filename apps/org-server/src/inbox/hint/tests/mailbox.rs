//! Real Mailbox transitions, with its existing memory backend and a count-only state probe.
use super::*;
use org_mailbox::{
    format_utc,
    testkit::{fixture::*, InMemoryDirectory, InMemoryStore},
    Mailbox, MessageEvent, MessageEvents, MessageStore, NewStoredMessage, NoMessageEvents,
    OfficeIdentity,
};
use org_proto::msg::{
    MessageHeader, MessageLifecycleState as State, MsgDeferParams, MsgDoneParams, MsgReadParams,
    MsgSendParams, MsgWaitParams, SnoozeTarget, KIND_MSG, KIND_REQUEST,
};

struct StateProbe {
    store: Arc<InMemoryStore>,
    id: String,
    state: State,
    flagged: bool,
}
impl Source for StateProbe {
    fn name(&self) -> &'static str {
        "committed_state_probe"
    }
    fn read(&self, query: &Query<'_>) -> Result<SourceRead, SourceFailure> {
        assert_eq!(query.limit, 0);
        let matches = self
            .store
            .get_message(&self.id)
            .unwrap()
            .is_some_and(|row| {
                row.message.header.state == self.state
                    && (!self.flagged || row.message.header.expired_waiting_person)
            });
        Ok(SourceRead {
            items: vec![],
            counts: InboxCounts {
                total: u64::from(matches),
                ..Default::default()
            },
        })
    }
}
struct CommittedEvents {
    store: Arc<InMemoryStore>,
    live: Arc<LiveEvents>,
}
impl MessageEvents for CommittedEvents {
    fn publish(&self, event: MessageEvent) {
        if let MessageEvent::StateChanged(data) = &event {
            let row = self.store.get_message(&data.id).unwrap().unwrap();
            assert_eq!(
                row.message.header.state, data.state,
                "publish must follow the state write"
            );
            assert_eq!(row.message.header.triage_gen, data.triage_gen);
            assert_eq!(
                row.message.header.expired_waiting_person,
                data.expired_waiting_person
            );
        }
        self.live.publish(event);
    }
}
struct Fixture {
    mailbox: Mailbox,
    store: Arc<InMemoryStore>,
    clock: Arc<ManualClock>,
    live: Arc<LiveEvents>,
    feed: Feed,
    hq: Hq,
    hint: Hint,
    at: Instant,
    member: &'static str,
}
fn identity() -> OfficeIdentity {
    OfficeIdentity {
        office_id: OFFICE_ID.into(),
        incarnation: INCARNATION.into(),
        is_hq: true,
    }
}
fn send(id: String, to: &str, reply_to: Option<String>) -> MsgSendParams {
    MsgSendParams {
        id,
        to: to.into(),
        kind: KIND_MSG.into(),
        priority: None,
        subject: None,
        body: "mailbox hint state probe".into(),
        meta: Default::default(),
        reply_to,
    }
}
impl Fixture {
    fn new(state: State, expires: bool, person: bool, observe_reply: bool) -> Self {
        // Reuse the actual Mailbox constructor for the header, then the existing public
        // store fixture seam for a request deadline. No SQL or production source is added.
        let template = Arc::new(InMemoryStore::new());
        let directory = Arc::new(InMemoryDirectory::new(members(), POSITION));
        let clock = Arc::new(ManualClock::new(NOW_MS));
        let seed = Mailbox::new(
            template.clone(),
            directory.clone(),
            identity(),
            clock.clone(),
            Arc::new(NoMessageEvents),
        );
        seed.send(
            OWNER_ID,
            send(msg_id(1), if person { "owner" } else { ADA_HANDLE }, None),
        )
        .unwrap();
        let mut row = template.get_message(&msg_id(1)).unwrap().unwrap();
        if expires {
            row.message.header.kind = KIND_REQUEST.into();
            row.message.header.ttl_ms = Some(1000);
            row.message.header.expires_at = Some(format_utc(NOW_MS + 1000).unwrap());
        }
        let store = Arc::new(InMemoryStore::new());
        store
            .commit_message(NewStoredMessage {
                message: row.message,
                origin_office_id: row.origin_office_id,
                origin_incarnation: row.origin_incarnation,
                origin_seq: Some(row.origin_seq),
                org_position: row.org_position,
                content_hash: row.content_hash,
                initial_state: State::Unread,
            })
            .unwrap();
        let live = Arc::new(LiveEvents::new(Box::new(|| {})));
        let mailbox = Mailbox::with_triage(
            store.clone(),
            store.clone(),
            directory,
            identity(),
            clock.clone(),
            Arc::new(CommittedEvents {
                store: store.clone(),
                live: live.clone(),
            }),
        )
        .unwrap();
        let feed = Feed::default();
        let hint = Hint::new(feed.head().unwrap(), live.mailbox_head());
        let hq = Hq {
            sources: vec![Arc::new(StateProbe {
                store: store.clone(),
                id: msg_id(if observe_reply { 2 } else { 1 }),
                state,
                flagged: person && expires,
            })],
            clock: Clock::new("real-mailbox".into()),
        };
        Self {
            mailbox,
            store,
            clock,
            live,
            feed,
            hq,
            hint,
            at: Instant::now(),
            member: if person { OWNER_ID } else { ADA_ID },
        }
    }
    fn header(&self) -> MessageHeader {
        self.store
            .get_message(&msg_id(1))
            .unwrap()
            .unwrap()
            .message
            .header
    }
    fn hints(&self) -> Vec<InboxChangedData> {
        self.live
            .since(0, self.live.head().messages)
            .events
            .into_iter()
            .filter_map(|e| {
                if let Live::InboxChanged(data) = e {
                    Some(data)
                } else {
                    None
                }
            })
            .collect()
    }
    fn poll(&self, elapsed_ms: u64) {
        self.hint.poll(&self.hq, &self.feed, &self.live, || {
            self.at + Duration::from_millis(elapsed_ms)
        });
    }
    fn before(&self) {
        let sources: Vec<&dyn Source> = self
            .hq
            .sources
            .iter()
            .map(|source| source.as_ref())
            .collect();
        assert_eq!(
            recount(&sources).counts.total,
            0,
            "probe must distinguish the pre-transition state"
        );
    }
    fn changed(&self) {
        self.poll(0);
        let hints = self.hints();
        assert_eq!(hints.len(), 1, "committed transition must produce a hint");
        assert!(hints[0].as_of.seq >= 1);
        assert_eq!(hints[0].as_of.epoch, "real-mailbox");
        assert_eq!(
            hints[0].counts.total, 1,
            "recount must see the committed state"
        );
        assert!(hints[0].degraded.is_empty());
    }
    fn duplicates(&self, before: MessageHeader, mut repeat: impl FnMut()) {
        let initial = self.hints()[0].as_of.clone();
        for _ in 0..3 {
            repeat();
            assert_eq!(
                self.header(),
                before,
                "notification repeat must leave storage unchanged"
            );
            self.poll(100);
        }
        assert_eq!(
            self.hints().len(),
            1,
            "duplicates must not bypass the250ms throttle"
        );
        self.poll(250);
        let hints = self.hints();
        assert!((1..=2).contains(&hints.len()));
        for hint in &hints {
            assert_eq!(hint.as_of.epoch, initial.epoch);
            assert!(hint.as_of.seq >= initial.seq);
            assert_eq!(hint.counts.total, 1);
        }
        if hints.len() == 2 {
            assert!(hints[1].as_of.seq > hints[0].as_of.seq);
        }
        self.poll(500);
        assert_eq!(
            self.hints().len(),
            hints.len(),
            "quiet wake must not invent another hint"
        );
    }
}
#[test]
fn actual_read_is_visible_and_same_millisecond_rereads_are_monotonic_and_throttled() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let f = Fixture::new(State::Read, false, false, false);
    f.before();
    f.mailbox
        .read(
            f.member,
            MsgReadParams {
                id: Some(msg_id(1)),
            },
        )
        .unwrap();
    f.changed();
    f.duplicates(f.header(), || {
        f.mailbox
            .read(
                f.member,
                MsgReadParams {
                    id: Some(msg_id(1)),
                },
            )
            .unwrap();
    });
}
#[test]
fn actual_done_is_visible_and_terminal_repeats_are_monotonic_and_throttled() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let f = Fixture::new(State::Done, false, false, false);
    f.before();
    f.mailbox
        .done(
            f.member,
            MsgDoneParams {
                id: msg_id(1),
                note: None,
            },
        )
        .unwrap();
    f.changed();
    f.clock.set(NOW_MS + 100);
    f.duplicates(f.header(), || {
        f.mailbox
            .done(
                f.member,
                MsgDoneParams {
                    id: msg_id(1),
                    note: Some("different notification, unchanged terminal state".into()),
                },
            )
            .unwrap();
    });
}
#[test]
fn actual_expiry_hint_reads_the_expired_state_after_commit() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let f = Fixture::new(State::Expired, true, false, false);
    f.before();
    f.clock.set(NOW_MS + 2000);
    f.mailbox.expire_due(f.member, None, 10).unwrap();
    f.changed();
    f.duplicates(f.header(), || {
        f.mailbox.expire_due(f.member, None, 10).unwrap();
    });
}
#[test]
fn actual_person_expiry_hint_reads_the_persisted_waiting_flag() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let f = Fixture::new(State::Unread, true, true, false);
    f.before();
    f.clock.set(NOW_MS + 2000);
    f.mailbox.expire_due(f.member, None, 10).unwrap();
    assert!(f.header().expired_waiting_person);
    f.changed();
}
#[test]
fn actual_defer_hint_reads_the_committed_snoozed_state() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let f = Fixture::new(State::Snoozed, false, false, false);
    f.before();
    f.mailbox
        .defer(
            f.member,
            MsgDeferParams {
                id: msg_id(1),
                until: SnoozeTarget::Until {
                    at: format_utc(NOW_MS + 1000).unwrap(),
                },
                note: None,
            },
        )
        .unwrap();
    f.changed();
}
#[test]
fn actual_release_hint_reads_unread_without_claiming_it() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let mut f = Fixture::new(State::Unread, false, false, false);
    f.mailbox
        .defer(
            f.member,
            MsgDeferParams {
                id: msg_id(1),
                until: SnoozeTarget::Until {
                    at: format_utc(NOW_MS + 1000).unwrap(),
                },
                note: None,
            },
        )
        .unwrap();
    f.hint = Hint::new(f.feed.head().unwrap(), f.live.mailbox_head());
    f.before();
    f.clock.set(NOW_MS + 1000);
    let (_cancel, cancellation) = org_mailbox::cancellation_pair();
    let result = f
        .mailbox
        .wait(
            f.member,
            MsgWaitParams {
                timeout_ms: 1,
                from: None,
                urgent_only: false,
            },
            Instant::now() + Duration::from_secs(1),
            &cancellation,
        )
        .unwrap();
    assert!(result.message.is_some());
    assert_eq!(f.header().state, State::Unread);
    f.changed();
}
#[test]
fn actual_reply_hint_reads_the_committed_reply_without_inventing_answered_state() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let f = Fixture::new(State::Unread, false, false, true);
    f.before();
    f.mailbox
        .send(ADA_ID, send(msg_id(2), "owner", Some(msg_id(1))))
        .unwrap();
    assert_eq!(f.header().state, State::Unread);
    f.changed();
}
