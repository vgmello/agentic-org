use std::io::{ErrorKind, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use org_proto::msg::{MsgBind, MsgBindResult};
use org_proto::{
    frame, location, Call, Hello, HelloParams, HelloResult, OfficeInfo, OfficeRestartParams,
    Organization, Request, API_VERSION, MAX_FRAME_BYTES,
};

use super::*;
use crate::paths::office_home;
use crate::transport::{read_loop_with_poll, write_before_with, Written};

/// A call of these tests' own: one string in (`office_id`, the
/// contract's one-string params), an organization out. What is tested here is the
/// connection (framing, queueing, timeouts), not a method the host serves.
struct Named;
type NamedParams = OfficeRestartParams;

impl Call for Named {
    const METHOD: &'static str = "test.named";
    type Params = NamedParams;
    type Result = Organization;
}

#[test]
fn the_office_home_is_the_override_or_the_systems_base_for_an_absolute_home() {
    let home = "/Users/me";
    let base = org_platform::dirs::application_base(Path::new(home)).ok();
    assert!(base.is_some());
    assert_eq!(
        office_home(Some("/tmp/o1".into()), Some(home.into())),
        Some(PathBuf::from("/tmp/o1"))
    );
    // No override, or one that is empty or relative: the system's base for HOME.
    assert_eq!(office_home(None, Some(home.into())), base);
    for ignored in ["", "tmp/o1"] {
        assert_eq!(office_home(Some(ignored.into()), Some(home.into())), base);
    }
    // A HOME that is unset, empty or relative gives no base.
    assert_eq!(office_home(None, None), None);
    assert_eq!(office_home(None, Some("".into())), None);
    assert_eq!(office_home(None, Some("me".into())), None);
    assert_eq!(office_home(Some("o1".into()), Some("me".into())), None);
    // The override does not need a HOME.
    assert_eq!(
        office_home(Some("/tmp/o1".into()), None),
        Some(PathBuf::from("/tmp/o1"))
    );
}

#[test]
fn ticked_reader_stops_on_a_closed_client_after_one_idle_tick() {
    // Keep the peer open: EOF must come from the client's stop flag, not the socket.
    let (stream, _peer) = UnixStream::pair().unwrap();
    let shared = Shared {
        closed: AtomicBool::new(true),
        next_id: AtomicU64::new(1),
        pending: Mutex::new(HashMap::new()),
        events: Arc::new(Mutex::new(None)),
        next_subscription: AtomicU64::new(1),
        queued_bytes: AtomicUsize::new(0),
        last_waiting_messages: AtomicU32::new(0),
        stream: stream.try_clone().unwrap(),
    };
    let mut polls = 0;
    read_loop_with_poll(&shared, stream, org_socket::READ_TICK, |_, tick| {
        polls += 1;
        assert!(polls < 3, "closed client reader did not stop after a tick");
        assert_eq!(tick, org_socket::READ_TICK);
        Ok(false)
    });
    assert_eq!(polls, 1);
}

#[test]
fn ticked_reader_repolls_a_missed_eof_and_closes_pending_calls() {
    let (stream, peer) = UnixStream::pair().unwrap();
    drop(peer);
    let (answer, pending) = mpsc::sync_channel(1);
    let shared = Shared {
        closed: AtomicBool::new(false),
        next_id: AtomicU64::new(2),
        pending: Mutex::new(HashMap::from([(1, answer)])),
        events: Arc::new(Mutex::new(None)),
        next_subscription: AtomicU64::new(1),
        queued_bytes: AtomicUsize::new(0),
        last_waiting_messages: AtomicU32::new(0),
        stream: stream.try_clone().unwrap(),
    };
    let polls = std::cell::Cell::new(0);
    read_loop_with_poll(&shared, stream, org_socket::READ_TICK, |fd, _| {
        polls.set(polls.get() + 1);
        assert!(polls.get() <= 10, "client reader failed to observe EOF");
        if polls.get() == 1 {
            Ok(false)
        } else {
            org_socket::poll_readable(fd, Duration::from_millis(1))
        }
    });
    assert_eq!(polls.get(), 2);
    assert!(shared.closed.load(Ordering::SeqCst));
    assert!(matches!(
        pending.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn a_concurrent_close_does_not_finish_calls_before_the_first_shutdown_finishes() {
    use std::io::Read;
    let (stream, mut peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let (answer_to, answer) = mpsc::sync_channel(1);
    let shared = Shared {
        closed: AtomicBool::new(false),
        next_id: AtomicU64::new(1),
        pending: Mutex::new(HashMap::from([(1, answer_to)])),
        events: Arc::new(Mutex::new(None)),
        next_subscription: AtomicU64::new(1),
        queued_bytes: AtomicUsize::new(0),
        last_waiting_messages: AtomicU32::new(0),
        stream,
    };
    let (entered, shutdown_started) = mpsc::channel();
    let (release, released) = mpsc::channel();
    std::thread::scope(|scope| {
        let closing = &shared;
        let first = scope.spawn(move || {
            closing.close_with(|stream| {
                entered.send(()).unwrap();
                released.recv_timeout(Duration::from_secs(10)).unwrap();
                stream.shutdown(std::net::Shutdown::Write).unwrap();
            })
        });
        shutdown_started
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        // Another close must not notify the waiting call while the owner's
        // shutdown is still held in progress.
        shared.close_with(|_| {});
        let before_shutdown_completed = answer.try_recv();
        let before_peer_eof = peer.read(&mut [0u8; 1]);
        release.send(()).unwrap();
        first.join().unwrap();
        assert_eq!(before_peer_eof.unwrap_err().kind(), ErrorKind::WouldBlock);
        assert_eq!(peer.read(&mut [0u8; 1]).unwrap(), 0);
        assert!(matches!(
            before_shutdown_completed,
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(matches!(
            answer.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    });
}

/// A server that accepts one connection and never reads or answers, until dropped.
struct StuckServer {
    _dir: tempfile::TempDir,
    path: PathBuf,
    _release: mpsc::Sender<()>,
}

fn stuck_server() -> StuckServer {
    let dir = tempfile::Builder::new().prefix("oc").tempdir().unwrap();
    let path = dir.path().join("s.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let (release, held) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let connection = listener.accept();
        let _ = held.recv(); // returns when the test drops `release`
        drop(connection);
    });
    StuckServer {
        _dir: dir,
        path,
        _release: release,
    }
}

/// A server that answers one connection's first request with `answer(request)`, then holds
/// the connection until the client closes it. Returns the socket and the request it got.
fn answering_server(
    answer: impl FnOnce(&Request) -> Response + Send + 'static,
) -> (tempfile::TempDir, PathBuf, mpsc::Receiver<Request>) {
    let dir = tempfile::Builder::new().prefix("oc").tempdir().unwrap();
    let path = dir.path().join("s.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let (seen, got) = mpsc::channel();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let line = frame::read_line(&mut reader).unwrap().unwrap();
        let request: Request = serde_json::from_slice(&line).unwrap();
        let mut reply = serde_json::to_vec(&answer(&request)).unwrap();
        reply.push(b'\n');
        (&stream).write_all(&reply).unwrap();
        let _ = seen.send(request);
        // Hold the connection until the client goes.
        let _ = frame::read_line(&mut reader);
    });
    (dir, path, got)
}

#[test]
fn bind_member_binds_by_handle_with_no_token() {
    let (_dir, path, got) = answering_server(|request| {
        Response::ok::<MsgBind>(
            request.id,
            &MsgBindResult {
                member_id: Some("M1".into()),
                office_id: "O1".into(),
            },
        )
        .unwrap()
    });
    let client = Client::connect(&path).unwrap();
    let bound = client.bind_member("pixel").unwrap();
    assert_eq!(bound.member_id.as_deref(), Some("M1"));
    assert_eq!(bound.office_id, "O1");
    let request = got.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(request.method, "msg.bind");
    assert_eq!(request.params, serde_json::json!({"member": "pixel"}));
}

#[test]
fn call_unless_gives_up_a_held_call_when_told_to_and_closes_the_connection() {
    let server = stuck_server();
    let client = Client::connect(&server.path).unwrap();
    let started = Instant::now();
    let stop_at = started + Duration::from_millis(200);
    let outcome =
        client.call_unless::<Hello>(&HelloParams::default(), Duration::from_secs(30), || {
            Instant::now() >= stop_at
        });
    assert!(outcome.is_none(), "{outcome:?}");
    let took = started.elapsed();
    assert!(took < Duration::from_secs(5), "{took:?}");
    let after = client.call_with_timeout::<Hello>(&HelloParams::default(), Duration::from_secs(1));
    assert!(matches!(after, Err(ClientError::Disconnected)), "{after:?}");
}

#[test]
fn call_unless_returns_the_answer_when_it_comes_first() {
    let (_dir, path, _got) = answering_server(|request| {
        let hello: HelloResult = serde_json::from_value(serde_json::json!({
            "api_version": API_VERSION, "server_version": "t", "host": "h"
        }))
        .unwrap();
        Response::ok::<Hello>(request.id, &hello).unwrap()
    });
    let client = Client::connect(&path).unwrap();
    let outcome =
        client.call_unless::<Hello>(&HelloParams::default(), Duration::from_secs(10), || false);
    assert_eq!(outcome.unwrap().unwrap().api_version, API_VERSION);
}

#[test]
fn a_client_can_be_shared_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Client>();
}

#[test]
fn every_request_has_ten_seconds_by_default() {
    assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(10));
    assert_eq!(default_timeout("hello"), DEFAULT_TIMEOUT);
    assert_eq!(default_timeout("organization.list"), DEFAULT_TIMEOUT);
    // The retired name-only create has no special case left.
    assert_eq!(default_timeout("organization.create"), DEFAULT_TIMEOUT);
}

#[test]
fn the_write_queue_never_holds_more_than_its_byte_budget_and_does_fill_it() {
    let server = stuck_server();
    let client = Arc::new(Client::connect(&server.path).unwrap());
    // Each request is nearly the largest a frame may be, and nothing is ever read, so the
    // writer sticks on the first and the rest queue up to the budget.
    let name = "x".repeat(MAX_FRAME_BYTES - 1024);
    let callers: Vec<_> = (0..40)
        .map(|_| {
            let client = Arc::clone(&client);
            let name = name.clone();
            std::thread::spawn(move || {
                client.call_with_timeout::<Named>(
                    &NamedParams { office_id: name },
                    Duration::from_secs(60),
                )
            })
        })
        .collect();
    // Watch the counter until the budget is reached (as long as a loaded machine needs),
    // then cut the connection so the callers end at once.
    let mut most = 0;
    let give_up = Instant::now() + Duration::from_secs(20);
    while most + 2 * MAX_FRAME_BYTES < MAX_QUEUED_BYTES && Instant::now() < give_up {
        most = most.max(client.shared.queued_bytes.load(Ordering::SeqCst));
        std::thread::sleep(Duration::from_millis(1));
    }
    // Keep watching a little longer: it must never go over, even once full.
    let settle = Instant::now() + Duration::from_millis(300);
    while Instant::now() < settle {
        most = most.max(client.shared.queued_bytes.load(Ordering::SeqCst));
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(server);
    for caller in callers {
        let outcome = caller.join().unwrap();
        assert!(
            matches!(
                outcome,
                Err(ClientError::Timeout | ClientError::Disconnected)
            ),
            "{outcome:?}"
        );
    }
    assert!(
        most <= MAX_QUEUED_BYTES,
        "{most} bytes were queued; the budget is {MAX_QUEUED_BYTES}"
    );
    assert!(
        most + 2 * MAX_FRAME_BYTES >= MAX_QUEUED_BYTES,
        "only {most} bytes were ever queued: the budget was not what held the callers back"
    );
}

#[test]
fn request_ids_stop_before_they_pass_two_to_the_fifty_third() {
    let server = stuck_server();
    let client = Client::connect(&server.path).unwrap();
    client
        .shared
        .next_id
        .store(MAX_REQUEST_ID, Ordering::SeqCst);
    // The last id is still usable (it times out: nobody answers)...
    let last =
        client.call_with_timeout::<Hello>(&HelloParams::default(), Duration::from_millis(50));
    assert!(matches!(last, Err(ClientError::Timeout)), "{last:?}");
    // ...and the next call is refused rather than sent with an id the peer may misread.
    let next =
        client.call_with_timeout::<Hello>(&HelloParams::default(), Duration::from_millis(50));
    assert!(matches!(next, Err(ClientError::Protocol(_))), "{next:?}");
}
// Script the kernel write's result and clock progress without threads or sleeps.
struct ScriptedWrite<F>(F);

impl<F: FnMut(&[u8]) -> std::io::Result<usize>> Write for ScriptedWrite<F> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        (self.0)(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_mid_write_finished_inside_caller_grace_keeps_the_connection_usable() {
    let (_dir, path, _got) = answering_server(|request| {
        Response::ok::<Hello>(request.id, &HelloResult::host("t", "h")).unwrap()
    });
    let client = Client::connect(path).unwrap();
    let state = AtomicU8::new(WRITING);
    let deadline = Instant::now();
    let clock = std::cell::Cell::new(deadline);
    let withdrew = client.settle_with(
        &state,
        || clock.get(),
        || {
            assert_eq!(
                state.load(Ordering::SeqCst),
                WRITING,
                "waited after completion"
            );
            clock.set(deadline + WRITE_GRACE / 2);
            state.store(WRITTEN, Ordering::SeqCst);
        },
    );
    assert!(!withdrew);
    assert_eq!(client.hello().unwrap().server_version, "t");
}

#[test]
fn a_mid_write_pending_at_caller_grace_expiry_closes_the_connection() {
    let server = stuck_server();
    let client = Client::connect(&server.path).unwrap();
    let state = AtomicU8::new(WRITING);
    let deadline = Instant::now();
    let clock = std::cell::Cell::new(deadline);
    assert!(!client.settle_with(
        &state,
        || clock.get(),
        || {
            assert_eq!(clock.get(), deadline, "waited beyond grace expiry");
            clock.set(clock.get() + WRITE_GRACE);
        }
    ));
    assert_eq!(clock.get(), deadline + WRITE_GRACE);
    assert!(matches!(client.hello(), Err(ClientError::Disconnected)));
}

#[test]
fn a_partial_write_can_finish_after_deadline_inside_writer_grace() {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(1);
    let clock = std::cell::Cell::new(started);
    let mut sent = Vec::new();
    let mut calls = 0;
    let mut writer = ScriptedWrite(|bytes: &[u8]| {
        calls += 1;
        assert!(calls <= 2, "wrote after completing the frame");
        let count = if calls == 1 { 1 } else { bytes.len() };
        sent.extend_from_slice(&bytes[..count]);
        clock.set(deadline + WRITE_GRACE / 2);
        Ok(count)
    });
    let result = write_before_with(
        &mut writer,
        b"frame\n",
        deadline,
        || clock.get(),
        |_, _| Ok(()),
    )
    .unwrap();
    assert!(matches!(result, Written::Whole));
    assert_eq!(sent, b"frame\n");
    assert_eq!(calls, 2);
}

#[test]
fn a_partial_write_stops_at_writer_grace_expiry() {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(1);
    let clock = std::cell::Cell::new(started);
    let mut sent = Vec::new();
    let mut writer = ScriptedWrite(|bytes: &[u8]| {
        assert!(
            sent.is_empty(),
            "no write is allowed at or beyond grace expiry"
        );
        sent.push(bytes[0]);
        clock.set(deadline + WRITE_GRACE);
        Ok(1)
    });
    let error = write_before_with(
        &mut writer,
        b"frame\n",
        deadline,
        || clock.get(),
        |_, _| Ok(()),
    )
    .err()
    .unwrap();
    assert_eq!(error.kind(), ErrorKind::TimedOut);
    assert_eq!(sent, b"f");
}

#[test]
fn a_request_expired_before_its_first_write_is_never_sent() {
    let deadline = Instant::now();
    let mut writer = ScriptedWrite(|_: &[u8]| panic!("expired request was written"));
    let result = write_before_with(
        &mut writer,
        b"frame\n",
        deadline,
        || deadline,
        |_, _| panic!("expired request configured a write"),
    )
    .unwrap();
    assert!(matches!(result, Written::NotStarted));
}

#[test]
fn write_timeout_configuration_errors_are_not_retried_as_write_errors() {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(1);
    let mut writer = ScriptedWrite(|_: &[u8]| panic!("write despite failed timeout configuration"));
    let mut configurations = 0;
    let error = write_before_with(
        &mut writer,
        b"frame\n",
        deadline,
        || started,
        |_, _| {
            configurations += 1;
            assert_eq!(configurations, 1, "retried timeout configuration failure");
            Err(ErrorKind::WouldBlock.into())
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.kind(), ErrorKind::WouldBlock);
}

#[test]
fn connect_selected_office_fails_closed_on_missing_or_mismatched_office_hello() {
    let dir = tempfile::Builder::new()
        .prefix("ocl-sel")
        .tempdir()
        .unwrap();
    let office_id = "0193a5c0-0000-7000-8000-0000000000aa";
    let other_id = "0193a5c0-0000-7000-8000-0000000000bb";
    let sock = location::selected_office_socket_in(dir.path(), office_id).unwrap();
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();

    let server = std::thread::spawn(move || {
        for reply_hello in [
            HelloResult::host("test", "test"),
            HelloResult::office(
                "test",
                "test",
                org_proto::OfficeHello {
                    organization_name: "Other".into(),
                    office: OfficeInfo {
                        id: other_id.into(),
                        organization_id: "0193a5c0-0000-7000-8000-000000000001".into(),
                        name: "Other".into(),
                        role: org_proto::OfficeRole::Hq,
                        host: "test".into(),
                        endpoint: None,
                        status: org_proto::OfficeStatus::Active,
                        status_reason: None,
                        next_restart_at: None,
                    },
                },
            ),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let req: Request = frame::read_message(&mut reader).unwrap().unwrap();
            let resp = Response::ok::<Hello>(req.id, &reply_hello).unwrap();
            frame::write_message(&mut stream, &resp).unwrap();
        }
    });

    assert!(matches!(
        Client::connect_selected_office(office_id, dir.path()),
        Err(ClientError::OfficeNotReachable)
    ));
    assert!(matches!(
        Client::connect_selected_office(office_id, dir.path()),
        Err(ClientError::OfficeNotReachable)
    ));
    server.join().unwrap();
}
