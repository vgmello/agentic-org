use super::*;
use std::os::fd::AsRawFd;

#[test]
fn registry_rejects_each_identity_fault_and_tracks_only_live_opaque_ids() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let registry: Registry = Arc::default();
    let (socket, _peer) = UnixStream::pair().unwrap();
    for id in ["", "handoff"] {
        assert!(register(&registry, id, ChannelKind::Local, &socket).is_err());
        assert!(registry.lock().unwrap().is_empty());
    }
    let first = register(&registry, "opaque/Exact:id", ChannelKind::Gateway, &socket).unwrap();
    assert!(register(&registry, "opaque/Exact:id", ChannelKind::Local, &socket).is_err());
    let case_distinct =
        register(&registry, "OPAQUE/Exact:id", ChannelKind::Gateway, &socket).unwrap();
    drop(first);
    assert!(!registry.lock().unwrap().contains_key("opaque/Exact:id"));
    let reused = register(&registry, "opaque/Exact:id", ChannelKind::Local, &socket).unwrap();
    let entries = registry.lock().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries["opaque/Exact:id"].0, ChannelKind::Local);
    assert_eq!(entries["OPAQUE/Exact:id"].0, ChannelKind::Gateway);
    drop(entries);
    drop((reused, case_distinct));
    assert!(registry.lock().unwrap().is_empty());
}

#[test]
fn total_and_gateway_bounds_are_independent_and_release_on_drop() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (socket, _peer) = UnixStream::pair().unwrap();
    for local in [true, false] {
        let registry: Registry = Arc::default();
        let mut slots = Vec::new();
        for n in 0..7 {
            slots.push(
                register(
                    &registry,
                    &n.to_string(),
                    if local {
                        ChannelKind::Local
                    } else {
                        ChannelKind::Gateway
                    },
                    &socket,
                )
                .unwrap(),
            );
        }
        if !local {
            assert!(register(&registry, "eighth-gateway", ChannelKind::Gateway, &socket).is_err());
        }
        slots.push(register(&registry, "eighth-local", ChannelKind::Local, &socket).unwrap());
        assert!(register(&registry, "ninth-local", ChannelKind::Local, &socket).is_err());
        assert!(register(&registry, "ninth-gateway", ChannelKind::Gateway, &socket).is_err());
        slots.remove(0);
        slots.push(register(&registry, "replacement", ChannelKind::Gateway, &socket).unwrap());
        assert_eq!(registry.lock().unwrap().len(), 8);
        drop(slots);
        assert!(registry.lock().unwrap().is_empty());
    }
}

fn shared_fixture() -> Arc<serve::Shared> {
    shared_with_snapshot(Box::new(|| panic!("host touched Office snapshot")))
}
fn shared_with_snapshot(snapshot: serve::SnapshotFn) -> Arc<serve::Shared> {
    use crate::follow::OrgFeed;
    use crate::live::LiveEvents;
    use org_mailbox::testkit::{fixture::*, InMemoryDirectory, InMemoryStore};
    struct DarkFeed;
    impl OrgFeed for DarkFeed {
        fn head(&self) -> Result<org_proto::Position, String> {
            panic!("host touched Office feed")
        }
        fn can_resume_from(&self, _: org_proto::Position) -> Result<bool, String> {
            panic!("host touched Office feed")
        }
        fn changes_after(
            &self,
            _: org_proto::Position,
            _: usize,
        ) -> Result<Vec<org_proto::org::OrgChangedData>, String> {
            panic!("host touched Office feed")
        }
        fn token(&self) -> u64 {
            panic!("host touched Office feed")
        }
        fn wait(&self, _: u64, _: std::time::Duration) -> u64 {
            panic!("host touched Office feed")
        }
    }
    let directory = Arc::new(InMemoryDirectory::new(members(), POSITION));
    let live = Arc::new(LiveEvents::new(Box::new(|| {
        panic!("host touched Office live events")
    })));
    let mailbox = Arc::new(org_mailbox::Mailbox::new(
        Arc::new(InMemoryStore::new()),
        directory.clone(),
        org_mailbox::OfficeIdentity {
            office_id: OFFICE_ID.into(),
            incarnation: INCARNATION.into(),
            is_hq: true,
        },
        Arc::new(ManualClock::new(NOW_MS)),
        live.clone(),
    ));
    Arc::new(serve::Shared::new(
        &org_office::OfficeInfo {
            office_id: OFFICE_ID.into(),
            office_name: "HQ".into(),
            role: org_office::OfficeRole::Hq,
            org_id: "ORG1".into(),
            org_name: "Acme".into(),
            created_at: NOW_UTC.into(),
        },
        std::path::Path::new("/unused"),
        serve::OfficePorts {
            snapshot,
            submit: Box::new(|_| panic!("host touched Office submit")),
            work: Box::new(|_, _| panic!("host touched Office work")),
            operation_get: Box::new(|_| panic!("host touched Office operation")),
            who: Box::new(|_| panic!("host touched Office who")),
            status: Box::new(|_, _| panic!("host touched Office status")),
            feed: Box::new(DarkFeed),
            live,
            mailbox,
            directory,
        },
        org_proto::AppToken::from_bytes([7; org_proto::APP_TOKEN_BYTES]),
    ))
}
fn manager() -> (
    Channels,
    UnixStream,
    Arc<serve::Shared>,
    Arc<Mutex<Revocations>>,
) {
    let (out, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let shared = shared_fixture();
    let revocations = Arc::default();
    let channels = Channels::new(
        shared.clone(),
        "verified-instance".into(),
        Arc::new(Mutex::new(out)),
        Arc::clone(&revocations),
    );
    (channels, peer, shared, revocations)
}
fn owned_channel(stream: UnixStream, office: &str, id: &str) -> Channel {
    Channel {
        kind: ChannelKind::Local,
        office_id: office.into(),
        channel_id: id.into(),
        fd: stream.into(),
    }
}
#[test]
fn owned_admission_rejects_wrong_office_and_datagram_without_registration_or_ack() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use org_mailbox::testkit::fixture::OFFICE_ID;
    use std::io::Read;
    let (mut channels, events, _, _) = manager();
    let (stream, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    assert!(channels
        .open(owned_channel(stream, "other-office", "wrong-office"))
        .is_err());
    assert_eq!(
        peer.read(&mut [0]).unwrap(),
        0,
        "refused owned stream leaked"
    );
    let (socket, peer) = std::os::unix::net::UnixDatagram::pair().unwrap();
    assert!(channels
        .open(Channel {
            kind: ChannelKind::Local,
            office_id: OFFICE_ID.into(),
            channel_id: "datagram".into(),
            fd: socket.into()
        })
        .is_err());
    assert!(peer.send(b"receiver closed").is_err());
    assert!(channels.registry.lock().unwrap().is_empty());
    assert!(channels.threads.is_empty());
    assert!(
        !org_socket::poll_readable(events.as_raw_fd(), std::time::Duration::from_millis(10))
            .unwrap(),
        "refusal emitted ACK"
    );
}
#[test]
fn admission_ack_keeps_owned_stream_registry_thread_and_shared_policy_until_joined_shutdown() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use org_mailbox::testkit::fixture::OFFICE_ID;
    let (mut channels, events, shared, policy) = manager();
    let (stream, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    channels
        .open(owned_channel(stream, OFFICE_ID, "live"))
        .unwrap();
    let registry = Arc::clone(&channels.registry);
    assert!(registry.lock().unwrap().contains_key("live"));
    assert_eq!(channels.threads.len(), 1);
    assert!(
        Arc::strong_count(&policy) >= 3,
        "serving context lost the same policy Arc"
    );
    let mut events = std::io::BufReader::new(events);
    assert_eq!(
        org_office_launch::v3::read_server_event(&mut events).unwrap(),
        Some(RendezvousEvent::FdsReceived { of: "live".into() })
    );
    let mut peer = std::io::BufReader::new(peer);
    org_proto::frame::write_message(
        peer.get_mut(),
        &serde_json::json!({"id":1,"method":"hello","params":{"api":{"min":2,"max":2}}}),
    )
    .unwrap();
    let answer: serde_json::Value = org_proto::frame::read_message(&mut peer).unwrap().unwrap();
    assert_eq!(answer["result"]["instance_id"], "verified-instance");
    let stopped = Arc::clone(&channels.stop);
    drop(channels);
    assert!(stopped.load(Ordering::SeqCst));
    assert!(registry.lock().unwrap().is_empty());
    assert_eq!(Arc::strong_count(&policy), 1);
    assert_eq!(
        Arc::strong_count(&shared),
        1,
        "channel thread survived joined shutdown"
    );
    assert!(org_proto::frame::read_line(&mut peer).unwrap().is_none());
}
#[test]
fn event_write_failure_propagates_after_ownership_then_shutdown_releases_all_handles() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use org_mailbox::testkit::fixture::OFFICE_ID;
    let (mut channels, _events, _, policy) = manager();
    channels
        .out
        .lock()
        .unwrap()
        .shutdown(std::net::Shutdown::Write)
        .unwrap();
    let (stream, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    assert!(channels
        .open(owned_channel(stream, OFFICE_ID, "unacknowledged"))
        .is_err());
    assert!(channels
        .registry
        .lock()
        .unwrap()
        .contains_key("unacknowledged"));
    drop(channels);
    let mut peer = std::io::BufReader::new(peer);
    assert!(org_proto::frame::read_line(&mut peer).unwrap().is_none());
    assert_eq!(Arc::strong_count(&policy), 1);
}

thread_local! { static HOLD_THREAD_EXIT: std::cell::RefCell<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>> = const { std::cell::RefCell::new(None) }; }
thread_local! { static REFUSE_SPAWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
// Test-only adapter forwards normal calls to the real std thread builder. The
// rare OS creation error is injected with the owned closure already passed in,
// so its captures exercise the same registration/stream rollback ownership.
pub(super) struct Builder(std::thread::Builder);
impl Builder {
    pub(super) fn new() -> Self {
        Self(std::thread::Builder::new())
    }
    pub(super) fn name(self, name: String) -> Self {
        Self(self.0.name(name))
    }
    pub(super) fn spawn(self, serve: impl FnOnce() + Send + 'static) -> io::Result<JoinHandle<()>> {
        if REFUSE_SPAWN.with(|fail| fail.replace(false)) {
            return Err(io::Error::other("synthetic thread creation refusal"));
        }
        let hold = HOLD_THREAD_EXIT.with(|gate| gate.borrow_mut().take());
        self.0.spawn(move || {
            serve();
            if let Some((entered, release)) = hold {
                entered.send(()).unwrap();
                release
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
            }
        })
    }
}
#[test]
fn thread_creation_refusal_drops_owned_closure_registration_and_fd_before_any_ack() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use org_mailbox::testkit::fixture::OFFICE_ID;
    use std::io::Read;
    let (mut channels, events, shared, policy) = manager();
    let (socket, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    REFUSE_SPAWN.with(|fail| fail.set(true));
    let error = channels
        .open(owned_channel(socket, OFFICE_ID, "retryable-live-id"))
        .unwrap_err();
    assert_eq!(error.to_string(), "synthetic thread creation refusal");
    assert!(channels.registry.lock().unwrap().is_empty());
    assert!(channels.threads.is_empty());
    assert_eq!(Arc::strong_count(&shared), 2);
    assert_eq!(Arc::strong_count(&policy), 2);
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    assert!(
        !org_socket::poll_readable(events.as_raw_fd(), std::time::Duration::from_millis(10))
            .unwrap()
    );
    let (socket, _peer) = UnixStream::pair().unwrap();
    channels
        .open(owned_channel(socket, OFFICE_ID, "retryable-live-id"))
        .unwrap();
}

#[test]
fn blocked_office_snapshot_does_not_block_fifo_revocation_or_channel_ownership_ack() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use org_mailbox::testkit::fixture::OFFICE_ID;
    use org_proto::frame;
    use serde_json::{json, Value};
    use std::io::BufReader;
    let (entered, observed) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let released = Mutex::new(released);
    let shared = shared_with_snapshot(Box::new(move || {
        entered.send(()).unwrap();
        released
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("test did not release blocked Office port");
        Err(serve::SnapshotFailure::Storage)
    }));
    let (office, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let office_shared = Arc::clone(&shared);
    let office_thread = std::thread::spawn(move || {
        serve::serve_context(
            office,
            &office_shared,
            9,
            &Default::default(),
            serve::Context::OfficeSocket,
        )
    });
    let mut office_peer = BufReader::new(peer);
    frame::write_message(
        office_peer.get_mut(),
        &json!({"id":1,"method":"hello","params":{}}),
    )
    .unwrap();
    let hello: Value = frame::read_message(&mut office_peer).unwrap().unwrap();
    assert_eq!(hello["result"]["api_version"], 1);
    frame::write_message(office_peer.get_mut(), &json!({"id":2,"method":"msg.bind","params":{"app_token":org_proto::AppToken::from_bytes([7; org_proto::APP_TOKEN_BYTES])}})).unwrap();
    let bound: Value = frame::read_message(&mut office_peer).unwrap().unwrap();
    assert!(bound.get("result").is_some(), "{bound}");
    frame::write_message(
        office_peer.get_mut(),
        &json!({"id":3,"method":"org.snapshot","params":{}}),
    )
    .unwrap();
    observed
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("Office snapshot did not enter the blocking port");
    let policy = Arc::new(Mutex::new(Revocations::default()));
    policy
        .lock()
        .unwrap()
        .apply_batch((0..4095).map(|n| format!("preload-{n}")).collect())
        .unwrap();
    let (out, events) = UnixStream::pair().unwrap();
    events
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let mut channels = Channels::new(
        Arc::clone(&shared),
        "verified-instance".into(),
        Arc::new(Mutex::new(out)),
        Arc::clone(&policy),
    );
    let (host, child) = std::os::unix::net::UnixDatagram::pair().unwrap();
    let service = super::super::Service::start(
        startup::Transfer {
            receiver: org_socket::rights::DatagramReceiver::new(child),
            fence: Arc::default(),
        },
        Arc::clone(&policy),
        move |channel| channels.open(channel),
    )
    .unwrap();
    let revoked = org_office_launch::v3::encode_transfer_record(
        &org_office_launch::v3::TransferRecord::Revoked {
            session_ids: vec!["fifo-tombstone".into()],
        },
    )
    .unwrap();
    super::super::tests::send_rights(&host, &revoked, &[]);
    let (source, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let record = org_office_launch::v3::encode_transfer_record(
        &org_office_launch::v3::TransferRecord::Channel {
            kind: ChannelKind::Local,
            office_id: OFFICE_ID.into(),
            channel_id: "independent".into(),
        },
    )
    .unwrap();
    super::super::tests::send_rights(&host, &record, &[source.as_raw_fd()]);
    let mut events = BufReader::new(events);
    assert_eq!(
        org_office_launch::v3::read_server_event(&mut events).unwrap(),
        Some(RendezvousEvent::FdsReceived {
            of: "independent".into()
        })
    );
    drop(source);
    // Only the channel gets an ACK. FIFO makes its ACK a fence after applying
    // the preceding tombstone; boundary observations avoid a test-only query API.
    assert!(policy
        .lock()
        .unwrap()
        .apply_batch(vec!["new-overflow".into()])
        .is_err());
    policy
        .lock()
        .unwrap()
        .apply_batch(vec!["fifo-tombstone".into()])
        .unwrap();
    let mut peer = BufReader::new(peer);
    frame::write_message(
        peer.get_mut(),
        &json!({"id":4,"method":"hello","params":{"api":{"min":2,"max":2}}}),
    )
    .unwrap();
    let hello: Value = frame::read_message(&mut peer).unwrap().unwrap();
    assert_eq!(hello["result"]["api_version"], 2);
    drop(service);
    assert!(frame::read_line(&mut peer).unwrap().is_none());
    assert_eq!(Arc::strong_count(&policy), 1);
    release.send(()).unwrap();
    let snapshot: Value = frame::read_message(&mut office_peer).unwrap().unwrap();
    assert_eq!(snapshot["id"], 3);
    assert_eq!(snapshot["error"]["code"], "storage_error");
    drop(office_peer);
    office_thread.join().unwrap();
}

#[test]
fn blocked_ack_writer_observes_registry_and_a_serving_owner_before_ack_is_possible() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use org_mailbox::testkit::fixture::OFFICE_ID;
    use org_proto::frame;
    use std::io::BufReader;
    use std::time::{Duration, Instant};
    let (mut channels, events, _, _) = manager();
    let registry = Arc::clone(&channels.registry);
    let writer = Arc::clone(&channels.out);
    let locked = writer.lock().unwrap();
    let (socket, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let admission = std::thread::spawn(move || {
        channels
            .open(owned_channel(socket, OFFICE_ID, "before-ack"))
            .unwrap();
        channels
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if registry.lock().unwrap().contains_key("before-ack") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "registry was not owned before waiting to ACK"
        );
        std::thread::yield_now();
    }
    // ACK is impossible while this exact writer lock is held. The peer's answer
    // therefore proves a spawned serving owner already exists before the ACK.
    let mut peer = BufReader::new(peer);
    frame::write_message(
        peer.get_mut(),
        &serde_json::json!({"id":1,"method":"hello","params":{"api":{"min":2,"max":2}}}),
    )
    .unwrap();
    let answer: serde_json::Value = frame::read_message(&mut peer).unwrap().unwrap();
    assert_eq!(answer["result"]["api_version"], 2);
    assert!(!org_socket::poll_readable(events.as_raw_fd(), Duration::from_millis(10)).unwrap());
    drop(locked);
    let mut events = BufReader::new(events);
    assert_eq!(
        org_office_launch::v3::read_server_event(&mut events).unwrap(),
        Some(RendezvousEvent::FdsReceived {
            of: "before-ack".into()
        })
    );
    let channels = admission.join().unwrap();
    assert_eq!(channels.threads.len(), 1);
    drop(channels);
    assert!(frame::read_line(&mut peer).unwrap().is_none());
}
#[test]
fn poisoned_registry_refuses_registration_and_ack_while_cleanup_still_recovers() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (mut channels, events, _, _) = manager();
    let registry = Arc::clone(&channels.registry);
    assert!(std::thread::spawn(move || {
        let _lock = registry.lock().unwrap();
        panic!("synthetic registry poison");
    })
    .join()
    .is_err());
    let (socket, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    assert!(channels
        .open(owned_channel(
            socket,
            org_mailbox::testkit::fixture::OFFICE_ID,
            "refused"
        ))
        .is_err());
    use std::io::Read;
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    assert!(
        !org_socket::poll_readable(events.as_raw_fd(), std::time::Duration::from_millis(10))
            .unwrap()
    );
    drop(channels);
}

#[test]
#[allow(unsafe_code)]
fn nonblocking_source_delivers_a_complete_reply_larger_than_its_send_buffer() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use std::io::Write;
    use std::time::Duration;
    let (mut channels, _events, _, _) = manager();
    channels.hello.server_version = "x".repeat(org_proto::MAX_FRAME_BYTES / 2);
    let (entered, at_exit) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    HOLD_THREAD_EXIT.with(|gate| *gate.borrow_mut() = Some((entered, released)));
    let (socket, mut peer) = UnixStream::pair().unwrap();
    socket.set_nonblocking(true).unwrap();
    let small: libc::c_int = 4096;
    // SAFETY: this test owns the live socket and passes a correctly sized int.
    assert_eq!(
        unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&small as *const libc::c_int).cast(),
                std::mem::size_of_val(&small) as _,
            )
        },
        0
    );
    channels
        .open(owned_channel(
            socket,
            org_mailbox::testkit::fixture::OFFICE_ID,
            "backpressured",
        ))
        .unwrap();
    peer.write_all(b"{\"id\":1,\"method\":\"hello\",\"params\":{\"api\":{\"min\":2,\"max\":2}}}\n")
        .unwrap();
    assert!(
        org_socket::poll_readable(peer.as_raw_fd(), Duration::from_secs(2)).unwrap(),
        "channel never entered its large response write"
    );
    let early_exit = at_exit.recv_timeout(Duration::from_millis(100));
    release.send(()).unwrap();
    assert!(
        matches!(early_exit, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
        "the channel abandoned the backpressured reply before its peer drained it"
    );
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut peer = std::io::BufReader::new(peer);
    let response: serde_json::Value = org_proto::frame::read_message(&mut peer).unwrap().unwrap();
    assert_eq!(
        response["result"]["server_version"],
        channels.hello.server_version
    );
    assert_eq!(response["result"]["api_version"], 2);
    drop(channels);
}

#[test]
#[allow(unsafe_code)]
fn shutdown_interrupts_a_channel_blocked_writing_before_joining_it() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use std::io::Write;
    use std::time::Duration;
    let (mut channels, _events, _, _) = manager();
    channels.hello.server_version = "x".repeat(org_proto::MAX_FRAME_BYTES / 2);
    let (socket, mut peer) = UnixStream::pair().unwrap();
    let small: libc::c_int = 4096;
    // SAFETY: this test owns the live socket and passes a correctly sized int.
    assert_eq!(
        unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&small as *const libc::c_int).cast(),
                std::mem::size_of_val(&small) as _,
            )
        },
        0
    );
    channels
        .open(owned_channel(
            socket,
            org_mailbox::testkit::fixture::OFFICE_ID,
            "backpressured",
        ))
        .unwrap();
    peer.write_all(b"{\"id\":1,\"method\":\"hello\",\"params\":{\"api\":{\"min\":2,\"max\":2}}}\n")
        .unwrap();
    assert!(
        org_socket::poll_readable(peer.as_raw_fd(), Duration::from_secs(2)).unwrap(),
        "channel never entered its large response write"
    );
    let (done, dropped) = std::sync::mpsc::channel();
    let closer = std::thread::spawn(move || {
        drop(channels);
        done.send(()).unwrap();
    });
    let timely = dropped.recv_timeout(Duration::from_secs(2));
    // On the counterfactual, closing the peer releases the writer before the
    // final assertion. A semantic two-second join bound is not a hung harness.
    drop(peer);
    closer.join().unwrap();
    assert!(
        timely.is_ok(),
        "shutdown did not interrupt the blocked channel write before join"
    );
}

#[test]
fn channel_drop_waits_for_the_actual_thread_exit_even_after_its_captures_are_gone() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use std::time::Duration;
    let (mut channels, _events, _, _) = manager();
    let (entered, at_exit) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    HOLD_THREAD_EXIT.with(|gate| *gate.borrow_mut() = Some((entered, released)));
    let (socket, peer) = UnixStream::pair().unwrap();
    channels
        .open(owned_channel(
            socket,
            org_mailbox::testkit::fixture::OFFICE_ID,
            "joined",
        ))
        .unwrap();
    peer.shutdown(std::net::Shutdown::Both).unwrap();
    at_exit.recv_timeout(Duration::from_secs(2)).unwrap();
    let (socket, _second_peer) = UnixStream::pair().unwrap();
    channels
        .open(owned_channel(
            socket,
            org_mailbox::testkit::fixture::OFFICE_ID,
            "second-live",
        ))
        .unwrap();
    let (done, dropped) = std::sync::mpsc::channel();
    let closer = std::thread::spawn(move || {
        drop(channels);
        done.send(()).unwrap();
    });
    let premature = dropped.recv_timeout(Duration::from_millis(100));
    release.send(()).unwrap();
    closer.join().unwrap();
    assert!(
        matches!(premature, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
        "Drop returned without joining the still-live serving thread"
    );
}

#[test]
fn stream_validation_precedes_even_a_poisoned_registry_refusal() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (mut channels, _events, _, _) = manager();
    let registry = Arc::clone(&channels.registry);
    assert!(std::thread::spawn(move || {
        let _guard = registry.lock().unwrap();
        panic!("synthetic registry poison");
    })
    .join()
    .is_err());
    let (datagram, _peer) = std::os::unix::net::UnixDatagram::pair().unwrap();
    let error = channels
        .open(Channel {
            kind: ChannelKind::Local,
            office_id: org_mailbox::testkit::fixture::OFFICE_ID.into(),
            channel_id: "invalid-before-registry".into(),
            fd: datagram.into(),
        })
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "channel descriptor is not a stream",
        "registry refusal masked descriptor validation order"
    );
}
#[test]
fn already_closed_peer_is_adopted_and_acknowledged_then_reaped_normally() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (mut channels, events, _, _) = manager();
    let (socket, peer) = UnixStream::pair().unwrap();
    peer.shutdown(std::net::Shutdown::Both).unwrap();
    drop(peer);
    channels
        .open(owned_channel(
            socket,
            org_mailbox::testkit::fixture::OFFICE_ID,
            "peer-already-gone",
        ))
        .unwrap();
    let mut events = std::io::BufReader::new(events);
    assert_eq!(
        org_office_launch::v3::read_server_event(&mut events).unwrap(),
        Some(RendezvousEvent::FdsReceived {
            of: "peer-already-gone".into()
        })
    );
    drop(channels);
}
#[test]
fn admission_reaps_finished_handles_but_retains_every_still_live_thread() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use std::time::{Duration, Instant};
    let (mut channels, _events, _, _) = manager();
    let (socket, peer) = UnixStream::pair().unwrap();
    channels
        .open(owned_channel(
            socket,
            org_mailbox::testkit::fixture::OFFICE_ID,
            "finished",
        ))
        .unwrap();
    peer.shutdown(std::net::Shutdown::Both).unwrap();
    drop(peer);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !channels.threads[0].is_finished() {
        assert!(Instant::now() < deadline, "closed channel did not finish");
        std::thread::yield_now();
    }
    let (socket, first) = UnixStream::pair().unwrap();
    channels
        .open(owned_channel(
            socket,
            org_mailbox::testkit::fixture::OFFICE_ID,
            "live-first",
        ))
        .unwrap();
    assert_eq!(
        channels.threads.len(),
        1,
        "finished thread handle was retained"
    );
    let (socket, second) = UnixStream::pair().unwrap();
    channels
        .open(owned_channel(
            socket,
            org_mailbox::testkit::fixture::OFFICE_ID,
            "live-second",
        ))
        .unwrap();
    assert_eq!(
        channels.threads.len(),
        2,
        "a live thread handle was discarded"
    );
    drop((first, second));
    drop(channels);
}
