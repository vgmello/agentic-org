use super::*;
use org_proto::inbox::{InboxChangedData, InboxPosition};
use std::os::fd::AsRawFd;

fn data() -> InboxChangedData {
    InboxChangedData {
        counts: InboxCounts {
            problems: 1,
            total: 1,
            ..InboxCounts::default()
        },
        as_of: InboxPosition {
            epoch: "probe".into(),
            seq: 3,
        },
        degraded: vec![],
    }
}

#[test]
fn hint_delivery_owner_and_every_denied_binding_share_the_same_payload_without_recount() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let mut shared = shared();
    let source = rows(1);
    shared.register_inbox(hq(source.clone()));
    let (stream, peer) = UnixStream::pair().unwrap();
    let out = Mutex::new(stream);
    let generation = AtomicU64::new(1);
    let sent = AtomicU64::new(0);
    let wire = follow::Wire {
        out: &out,
        generation: &generation,
        mine: 1,
        seq: &sent,
        timing: Timing::default(),
    };
    let payload = data();
    assert!(deliver_hint(
        &shared,
        &Subscriber {
            binding: &owner(),
            wire: &wire
        },
        &payload
    ));
    assert_eq!(
        sent.load(Ordering::SeqCst),
        0,
        "a disabled hint capability delivers nothing"
    );
    shared.enable_inbox_hints().unwrap();
    for binding in [
        Binding::Anonymous,
        Binding::App,
        Binding::Member {
            id: PAT_ID.into(),
            with_app_token: true,
        },
        Binding::Member {
            id: ADA_ID.into(),
            with_app_token: false,
        },
    ] {
        assert!(deliver_hint(
            &shared,
            &Subscriber {
                binding: &binding,
                wire: &wire
            },
            &payload
        ));
        assert_eq!(sent.load(Ordering::SeqCst), 0);
    }
    assert!(!org_socket::poll_readable(peer.as_raw_fd(), Duration::from_millis(50)).unwrap());
    assert!(deliver_hint(
        &shared,
        &Subscriber {
            binding: &owner(),
            wire: &wire
        },
        &payload
    ));
    assert_eq!(sent.load(Ordering::SeqCst), 1);
    let mut reader = std::io::BufReader::new(peer);
    let event: org_proto::Event = frame::read_message(&mut reader).unwrap().unwrap();
    assert_eq!(event.event, "inbox.changed");
    assert_eq!(event.seq, 1);
    assert_eq!(
        serde_json::from_value::<InboxChangedData>(event.data).unwrap(),
        payload
    );
    assert_eq!(
        source.reads.load(Ordering::SeqCst),
        0,
        "delivery must reuse the pump's recount"
    );
}

#[test]
fn hint_delivery_rechecks_live_ownership_and_lookup_errors_deliver_nothing() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let mut shared = shared();
    shared.register_inbox(hq(rows(1)));
    shared.enable_inbox_hints().unwrap();
    let canonical = org_mailbox::testkit::fixture::members()
        .into_iter()
        .find(|m| m.owner)
        .unwrap();
    let directory = Arc::new(Directory {
        member: Mutex::new(Some(canonical)),
        failed: AtomicBool::new(true),
    });
    shared.ports.directory = directory.clone();
    let (stream, peer) = UnixStream::pair().unwrap();
    let out = Mutex::new(stream);
    let generation = AtomicU64::new(1);
    let sent = AtomicU64::new(0);
    let wire = follow::Wire {
        out: &out,
        generation: &generation,
        mine: 1,
        seq: &sent,
        timing: Timing::default(),
    };
    let binding = owner();
    let subscriber = Subscriber {
        binding: &binding,
        wire: &wire,
    };
    assert!(deliver_hint(&shared, &subscriber, &data()));
    assert_eq!(sent.load(Ordering::SeqCst), 0);
    assert!(!org_socket::poll_readable(peer.as_raw_fd(), Duration::from_millis(50)).unwrap());
    directory.failed.store(false, Ordering::SeqCst);
    assert!(deliver_hint(&shared, &subscriber, &data()));
    assert_eq!(sent.load(Ordering::SeqCst), 1);
    let mut reader = std::io::BufReader::new(peer);
    let event: org_proto::Event = frame::read_message(&mut reader).unwrap().unwrap();
    assert_eq!(event.seq, 1);
    directory.member.lock().unwrap().as_mut().unwrap().owner = false;
    assert!(deliver_hint(&shared, &subscriber, &data()));
    assert_eq!(sent.load(Ordering::SeqCst), 1);
    assert!(
        !org_socket::poll_readable(reader.get_ref().as_raw_fd(), Duration::from_millis(50))
            .unwrap()
    );
}

#[test]
fn hint_capability_and_initial_cursor_are_enabled_only_for_registered_hq() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let mut shared = shared();
    shared.enable_inbox_hints().unwrap();
    assert!(!shared.hello.supports("inbox.changed"));
    shared.hello.office.as_mut().unwrap().office.role = OfficeRole::Satellite;
    shared.register_inbox(hq(rows(1)));
    shared.enable_inbox_hints().unwrap();
    assert!(!shared.hello.supports("inbox.changed"));
    shared.hello.office.as_mut().unwrap().office.role = OfficeRole::Hq;
    shared.register_inbox(hq(rows(1)));
    shared.enable_inbox_hints().unwrap();
    shared.enable_inbox_hints().unwrap();
    assert_eq!(
        shared
            .hello
            .capabilities
            .iter()
            .filter(|name| *name == "inbox.changed")
            .count(),
        1
    );
}

#[test]
fn hint_cursor_failure_is_returned_without_advertising_a_working_capability() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    struct NoHead;
    impl OrgFeed for NoHead {
        fn head(&self) -> Result<org_proto::org::Position, String> {
            Err("injected unavailable head".into())
        }
        fn can_resume_from(&self, _: org_proto::org::Position) -> Result<bool, String> {
            unreachable!()
        }
        fn changes_after(
            &self,
            _: org_proto::org::Position,
            _: usize,
        ) -> Result<Vec<org_proto::org::OrgChangedData>, String> {
            unreachable!()
        }
        fn token(&self) -> u64 {
            0
        }
        fn wait(&self, _: u64, _: Duration) -> u64 {
            0
        }
    }
    let mut shared = shared();
    shared.register_inbox(hq(rows(1)));
    shared.ports.feed = Box::new(NoHead);
    assert!(shared.enable_inbox_hints().is_err());
    assert!(!shared.hello.supports("inbox.changed"));
    assert!(shared.hint.is_none());
}
