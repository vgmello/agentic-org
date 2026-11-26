use std::io::{BufRead, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;
use std::{fs, io};

use org_proto::{ApiError, ErrorCode, Request, Response, UNREADABLE_REQUEST_ID};
use serde_json::Value;

use crate::cloexec::SPAWN_LOCK;
use crate::peer::{Accepted, Received};
use crate::sync::lock;

use super::connection::{Connection, Gateway, ServingReport};
use super::connections::WebConnection;
use super::outbound::{write_loop, Outbound};
use super::serve::{serve, serve_with_poll};
use super::*;

#[test]
fn web_serving_is_taken_only_from_the_web_after_hello_and_tls_fetch() {
    let plain = Connection::for_test();
    plain.complete_hello();
    plain.note_listener_fetched();
    assert!(matches!(plain.report_serving(), ServingReport::NotWeb));
    let (host, _peer) = UnixStream::pair().unwrap();
    let (transfer, _received) = std::os::unix::net::UnixDatagram::pair().unwrap();
    let (mut connection, _queue) =
        Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
    Arc::get_mut(&mut connection).unwrap().gateway = Some(Gateway {
        transfer: Arc::new(transfer),
        listen: serde_json::from_str(r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#).unwrap(),
        fetched: AtomicBool::new(false),
        serving: AtomicBool::new(false),
        serving_refused: AtomicBool::new(false),
    });
    connection.note_listener_fetched();
    assert!(
        matches!(connection.report_serving(), ServingReport::OutOfOrder),
        "tls.fetch without hello"
    );
    connection.complete_hello();
    assert!(matches!(
        connection.report_serving(),
        ServingReport::Accepted
    ));
    assert!(matches!(
        connection.report_serving(),
        ServingReport::OutOfOrder
    ));
}

#[test]
fn gateway_descriptor_uses_datagram_and_receipt_never_stream_rights() {
    use std::os::fd::AsFd;
    let (host, peer) = UnixStream::pair().unwrap();
    let (transfer, received) = std::os::unix::net::UnixDatagram::pair().unwrap();
    let (mut connection, queue) =
        Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
    Arc::get_mut(&mut connection).unwrap().gateway = Some(Gateway {
        transfer: Arc::new(transfer),
        listen: serde_json::from_str(r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#).unwrap(),
        fetched: AtomicBool::new(false),
        serving: AtomicBool::new(false),
        serving_refused: AtomicBool::new(false),
    });
    assert_eq!(
        connection.channel_kind(),
        org_office_launch::ChannelKind::Web
    );
    assert!(connection.web_listen().is_some());
    let closed = Arc::clone(&connection.closed);
    let writer = std::thread::spawn(move || write_loop(&host, queue, &closed));
    let (channel, mut worker) = UnixStream::pair().unwrap();
    connection
        .send_descriptor(
            Response::ok::<org_host_control::ops::OfficeOpenChannel>(
                3,
                &org_host_control::ops::OfficeOpenChannelResult {
                    channel_id: "channel".into(),
                },
            )
            .unwrap(),
            "channel".into(),
            channel.into(),
            "office",
            crate::supervisor::channels::ChannelSlot::for_test(),
        )
        .unwrap();
    connection.send_response(Response {
        id: 4,
        outcome: org_proto::Outcome::Result(Value::Bool(true)),
        waiting_messages: None,
    });
    let mut stream = org_socket::rights::StreamReceiver::new(peer);
    let until = std::time::Instant::now() + Duration::from_secs(3);
    let mut line = Vec::new();
    loop {
        match stream.receive_byte(0, &SPAWN_LOCK) {
            Ok((b'\n', fds)) => {
                assert!(fds.is_empty());
                break;
            }
            Ok((byte, fds)) => {
                assert!(fds.is_empty());
                line.push(byte);
            }
            Err(org_socket::rights::ReceiveError::WouldBlock)
                if std::time::Instant::now() < until =>
            {
                std::thread::yield_now()
            }
            Err(error) => panic!("{error}"),
        }
    }
    assert_eq!(serde_json::from_slice::<Response>(&line).unwrap().id, 3);
    let mut datagrams = org_socket::rights::DatagramReceiver::new(received);
    let mut header = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
    let mut record = loop {
        match datagrams.receive(&mut header, 1, &SPAWN_LOCK) {
            Ok(record) => break record,
            Err(org_socket::rights::ReceiveError::WouldBlock)
                if std::time::Instant::now() < until =>
            {
                std::thread::yield_now()
            }
            Err(error) => panic!("{error}"),
        }
    };
    assert!(
        matches!(org_office_launch::decode_transfer_record(&header[..record.bytes]).unwrap(),
        org_office_launch::TransferRecord::Channel { kind:org_office_launch::ChannelKind::Web, office_id, channel_id } if office_id=="office" && channel_id=="channel")
    );
    let mut channel =
        org_socket::rights::connected_unix_stream(record.descriptors.pop().unwrap()).unwrap();
    org_socket::rights::set_buffer_sizes(channel.as_fd(), 4096).unwrap();
    channel.write_all(b"through").unwrap();
    let mut bytes = [0; 7];
    std::io::Read::read_exact(&mut worker, &mut bytes).unwrap();
    assert_eq!(&bytes, b"through");
    assert!(matches!(
        stream.receive_byte(0, &SPAWN_LOCK),
        Err(org_socket::rights::ReceiveError::WouldBlock)
    ));
    assert!(!connection.channel_received("channel"));
    assert!(!connection.descriptor_received("foreign"));
    assert!(connection.descriptor_received("channel"));
    assert!(connection.channel_received("channel"));
    assert!(!connection.descriptor_received("channel"));
    connection.finish();
    writer.join().unwrap();
}

#[test]
fn local_descriptor_response_is_atomic_and_kept_until_correlated_receipt() {
    let (host, peer) = UnixStream::pair().unwrap();
    let (connection, queue) = Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
    let closed = Arc::clone(&connection.closed);
    let writer = std::thread::spawn(move || write_loop(&host, queue, &closed));
    let (client, mut worker) = UnixStream::pair().unwrap();
    let result = org_host_control::ops::OfficeOpenChannelResult {
        channel_id: "channel".into(),
    };
    connection
        .send_descriptor(
            Response::ok::<org_host_control::ops::OfficeOpenChannel>(3, &result).unwrap(),
            "channel".into(),
            client.into(),
            "office",
            crate::supervisor::channels::ChannelSlot::for_test(),
        )
        .unwrap();
    connection.send_response(Response {
        id: 4,
        outcome: org_proto::Outcome::Result(Value::Bool(true)),
        waiting_messages: None,
    });
    let mut receiver = org_socket::rights::StreamReceiver::new(peer);
    let receive = |receiver: &mut org_socket::rights::StreamReceiver, count| {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            match receiver.receive_byte(count, &SPAWN_LOCK) {
                Ok(value) => return value,
                Err(org_socket::rights::ReceiveError::WouldBlock)
                    if std::time::Instant::now() < deadline =>
                {
                    std::thread::yield_now()
                }
                Err(error) => panic!("{error}"),
            }
        }
    };
    let mut line = Vec::new();
    loop {
        let (byte, fds) = receive(&mut receiver, 0);
        assert!(fds.is_empty());
        if byte == b'\n' {
            break;
        }
        line.push(byte);
    }
    let response: Response = serde_json::from_slice(&line).unwrap();
    assert_eq!(response.id, 3);
    assert_eq!(
        response
            .into_result::<org_host_control::ops::OfficeOpenChannel>()
            .unwrap(),
        result
    );
    let (marker, mut descriptors) = receive(&mut receiver, 1);
    assert_eq!(marker, org_host_control::ops::HOST_SOCK_MARKER);
    let mut channel =
        org_socket::rights::connected_unix_stream(descriptors.pop().unwrap()).unwrap();
    channel.write_all(b"real channel").unwrap();
    let mut expected = [0; 12];
    std::io::Read::read_exact(&mut worker, &mut expected).unwrap();
    assert_eq!(&expected, b"real channel");
    // The next queued response is withheld until descriptor ownership is acknowledged.
    assert!(matches!(
        receiver.receive_byte(0, &SPAWN_LOCK),
        Err(org_socket::rights::ReceiveError::WouldBlock)
    ));
    assert!(!connection.descriptor_received("other-channel"));
    assert!(connection.descriptor_received("channel"));
    assert!(connection.channel_received("channel"));
    assert!(!connection.descriptor_received("channel"));
    line.clear();
    loop {
        let (byte, fds) = receive(&mut receiver, 0);
        assert!(fds.is_empty());
        if byte == b'\n' {
            break;
        }
        line.push(byte);
    }
    assert_eq!(serde_json::from_slice::<Response>(&line).unwrap().id, 4);
    connection.finish();
    writer.join().unwrap();
}

#[test]
fn refusal_marks_reader_closed_and_queues_response_before_close() {
    let (stream, _peer) = UnixStream::pair().unwrap();
    let (connection, queue) =
        Connection::new(stream.try_clone().unwrap(), Accepted::sample(&stream));
    connection.refuse_and_finish(Response::error(
        9,
        ApiError::new(ErrorCode::UnsupportedVersion, "unsupported"),
    ));
    assert!(connection.closed.load(Ordering::SeqCst));
    assert!(matches!(queue.recv().unwrap(),Outbound::Response(response) if response.id==9));
    assert!(matches!(queue.recv().unwrap(), Outbound::Close));
}

#[test]
fn refusal_prevents_dispatch_of_an_already_buffered_suffix() {
    use std::sync::atomic::AtomicUsize;
    struct Closing(Arc<AtomicUsize>);
    impl Handler for Closing {
        fn inline(&self, _: &Request, _: &Connection) -> bool {
            true
        }
        fn handle(&self, request: Request, connection: &Arc<Connection>, received: Received) {
            let call = self.0.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                assert!(
                    received.buffered_after > 0,
                    "suffix must already be buffered"
                );
            }
            connection.refuse_and_finish(Response::error(
                request.id,
                ApiError::new(ErrorCode::UnsupportedVersion, "unsupported"),
            ));
        }
    }
    let (stream, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    peer.write_all(b"{\"id\":1,\"method\":\"hello\"}\n{\"id\":2,\"method\":\"suffix\"}\n")
        .unwrap();
    let (connection, queue) =
        Connection::new(stream.try_clone().unwrap(), Accepted::sample(&stream));
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let worker =
        thread::spawn(move || serve(stream, connection, queue, Arc::new(Closing(observed)), 4));
    let mut reader = io::BufReader::new(peer);
    let mut line = String::new();
    assert!(reader.read_line(&mut line).unwrap() > 0);
    assert_eq!(serde_json::from_str::<Response>(&line).unwrap().id, 1);
    line.clear();
    assert_eq!(reader.read_line(&mut line).unwrap(), 0);
    worker.join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// SIGN-33: the host's own user is admitted as itself, root as the local authority
/// (asked only when the peer is not the host's user), and nobody else at all.
#[test]
fn only_the_hosts_user_and_root_are_admitted_and_root_is_asked_last() {
    let unasked = || -> bool { panic!("asked about root for the host's own user") };
    assert_eq!(admission(true, unasked), Some(Admitted::User));
    assert_eq!(admission(false, || true), Some(Admitted::Root));
    assert_eq!(admission(false, || false), None);
    // This process's own peer is its own user: the platform never names it root unless
    // the tests run as root, and where a peer is identified by its image, never.
    let (stream, _peer) = UnixStream::pair().unwrap();
    let named_root = crate::peer::identity(&stream).is_ok_and(|i| i.uid == Some(0));
    assert_eq!(root_peer(&stream), named_root);
    assert!(!named_root || crate::home::euid() == 0);
}

/// A connection admitted as root reaches a handler with the greeting and
/// `admin.reset` only: every other method is answered `not_authorized` before any
/// handler sees it. The same requests on the host's own user's connection all reach
/// the handler, which judges each by its own rule.
#[test]
fn root_is_admitted_for_admin_reset_and_the_greeting_and_nothing_else() {
    const METHODS: [&str; 14] = [
        "hello",
        "instance.status",
        "admin.link.initial",
        "admin.reset",
        "tls.fetch",
        "web.serving",
        "organization.provision",
        "operation.get",
        "recovery.release",
        "recovery.acknowledge",
        "session.grant",
        "office.app_token",
        "office.restart",
        "no.such.method",
    ];
    for root in [true, false] {
        let (host, peer) = UnixStream::pair().unwrap();
        let (connection, queue) =
            Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
        assert!(!connection.root_authority());
        if root {
            connection.mark_root_authority();
        }
        let serving = Arc::clone(&connection);
        let server = thread::spawn(move || serve(host, serving, queue, Arc::new(Echo), 4));
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut reader = io::BufReader::new(peer.try_clone().unwrap());
        for (id, method) in METHODS.iter().enumerate() {
            writeln!(
                &peer,
                r#"{{"id":{},"method":"{method}","params":{{}}}}"#,
                id + 1
            )
            .unwrap();
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let response: Response = serde_json::from_str(&line).unwrap();
            assert_eq!(response.id, id as u64 + 1);
            let reached = match response.outcome {
                org_proto::Outcome::Result(value) => {
                    assert_eq!(value, Value::String((*method).into()));
                    true
                }
                org_proto::Outcome::Error(error) => {
                    assert_eq!(error.code, ErrorCode::NotAuthorized, "{method}");
                    assert!(error.message.contains("admin.reset only"), "{method}");
                    false
                }
            };
            let allowed = !root || matches!(*method, "hello" | "admin.reset");
            assert_eq!(reached, allowed, "root={root} {method}");
        }
        drop(reader);
        drop(peer);
        server.join().unwrap();
    }
}

#[test]
fn ticked_reader_repolls_a_missed_eof_at_the_host_call_site() {
    let (stream, peer) = UnixStream::pair().unwrap();
    drop(peer);
    let (connection, queue) =
        Connection::new(stream.try_clone().unwrap(), Accepted::sample(&stream));
    let polls = std::cell::Cell::new(0);
    serve_with_poll(
        stream,
        Arc::clone(&connection),
        queue,
        Arc::new(Echo),
        1,
        org_socket::READ_TICK,
        |fd, _| {
            polls.set(polls.get() + 1);
            if polls.get() == 1 {
                Ok(false)
            } else {
                org_socket::poll_readable(fd, Duration::from_millis(1))
            }
        },
    );
    assert_eq!(polls.get(), 2);
    assert!(connection.closed.load(Ordering::SeqCst));
}

#[test]
fn ticked_reader_observes_host_close_at_the_first_idle_tick() {
    let (stream, _peer) = UnixStream::pair().unwrap();
    let (connection, queue) =
        Connection::new(stream.try_clone().unwrap(), Accepted::sample(&stream));
    let polls = std::cell::Cell::new(0);
    serve_with_poll(
        stream,
        Arc::clone(&connection),
        queue,
        Arc::new(Echo),
        1,
        org_socket::READ_TICK,
        |_, _| {
            polls.set(polls.get() + 1);
            connection.close();
            Ok(false)
        },
    );
    assert_eq!(polls.get(), 1);
}

fn loopback_listen() -> org_host_control::listen::ListenConfig {
    serde_json::from_str(r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#).unwrap()
}

#[test]
fn only_the_admitted_web_connection_is_web_kind() {
    let (host, peer) = UnixStream::pair().unwrap();
    let (transfer, _child) = std::os::unix::net::UnixDatagram::pair().unwrap();
    let web = WebConnection::start(host, transfer, loopback_listen(), Arc::new(Echo)).unwrap();
    assert_eq!(
        web.connection.channel_kind(),
        org_office_launch::ChannelKind::Web
    );
    assert!(web.connection.web_listen().is_some());
    assert!(web.live());

    // Every connection the host.sock listener accepts is built by `Connection::new`,
    // which carries no web process.
    let (accepted, _accepted_peer) = UnixStream::pair().unwrap();
    let (plain, _queue) =
        Connection::new(accepted.try_clone().unwrap(), Accepted::sample(&accepted));
    assert_eq!(plain.channel_kind(), org_office_launch::ChannelKind::Local);

    // Peer EOF ends the web process control connection.
    drop(peer);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while web.live() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!web.live());
}

#[test]
fn host_sock_connections_are_local() {
    let dir = short_tempdir();
    let (listener, path) = start(dir.path(), org_socket::effective_uid());
    let _client = UnixStream::connect(&path).unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let kinds: Vec<_> = lock(&listener.live)
            .values()
            .map(|live| live.connection.channel_kind())
            .collect();
        if !kinds.is_empty() {
            assert!(kinds
                .iter()
                .all(|k| *k == org_office_launch::ChannelKind::Local));
            break;
        }
        assert!(std::time::Instant::now() < until, "no accepted connection");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

struct Echo;

impl Handler for Echo {
    fn handle(&self, request: Request, connection: &Arc<Connection>, _received: Received) {
        connection.send_response(Response {
            id: request.id,
            outcome: org_proto::Outcome::Result(Value::String(request.method)),
            waiting_messages: None,
        });
    }
}

fn start(dir: &Path, expected_uid: u32) -> (Listener, PathBuf) {
    let path = dir.join("t.sock");
    let config = ListenerConfig {
        max_connections: 2,
        max_in_flight: 4,
        expected_uid,
    };
    let listener = Listener::start(bind(&path).unwrap(), config, Arc::new(Echo));
    (listener.unwrap(), path)
}

fn short_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("ohl")
        .tempdir_in("/tmp")
        .unwrap()
}

/// Connects with a read timeout, set at once: macOS refuses (EINVAL) to set one on a
/// socket whose peer already closed, and a closed socket reads as EOF anyway.
fn connect(path: &Path) -> UnixStream {
    let stream = UnixStream::connect(path).unwrap();
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    stream
}

fn read_line(stream: &UnixStream) -> Option<String> {
    let mut line = String::new();
    let n = io::BufReader::new(stream).read_line(&mut line).unwrap_or(0);
    (n > 0).then_some(line)
}

#[test]
fn a_peer_of_another_uid_is_refused() {
    use std::io::Read;

    let me = crate::home::euid();
    for closed in [false, true] {
        let (mut stream, peer) = UnixStream::pair().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let _peer = if closed {
            drop(peer);
            assert_eq!(stream.read(&mut [0]).unwrap(), 0);
            None
        } else {
            Some(peer)
        };
        assert!(org_socket::peer_allowed(&stream, me));
        assert!(!org_socket::peer_allowed(&stream, me.wrapping_add(1)));
        println!("pair peer_closed={closed} uid={me}: match=true mismatch=false");
    }
}

#[test]
fn expected_uid_is_checked_before_host_dispatch() {
    use std::io::Read;
    use std::sync::atomic::AtomicUsize;

    struct Counted(Arc<AtomicUsize>);
    impl Handler for Counted {
        fn handle(&self, request: Request, connection: &Arc<Connection>, _: Received) {
            self.0.fetch_add(1, Ordering::SeqCst);
            connection.send_response(Response {
                id: request.id,
                outcome: org_proto::Outcome::Result(Value::String(request.method)),
                waiting_messages: None,
            });
        }
    }

    let uid = crate::home::euid();
    for expected_uid in [uid, uid.wrapping_add(1)] {
        let dir = short_tempdir();
        let path = dir.path().join("uid.sock");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut listener = Listener::start(
            bind(&path).unwrap(),
            ListenerConfig {
                max_connections: 2,
                max_in_flight: 4,
                expected_uid,
            },
            Arc::new(Counted(Arc::clone(&calls))),
        )
        .unwrap();
        let mut stream = connect(&path);
        let _ = stream.write_all(b"{\"id\":1,\"method\":\"hello\"}\n");
        if expected_uid == uid {
            let response: Response =
                serde_json::from_str(&read_line(&stream).expect("matching peer served")).unwrap();
            assert_eq!(response.id, 1);
            assert_eq!(
                response.outcome,
                org_proto::Outcome::Result(Value::String("hello".into()))
            );
            println!("accepted uid={uid} expected={expected_uid}: served");
        } else {
            let result = stream.read(&mut [0]);
            assert!(
                matches!(result, Ok(0))
                    || matches!(result, Err(ref error) if error.kind() == io::ErrorKind::ConnectionReset),
                "wrong UID reached dispatch or was not closed: {result:?}"
            );
            println!("accepted uid={uid} expected={expected_uid}: {result:?}");
        }
        listener.close();
        listener.join();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            usize::from(expected_uid == uid)
        );
    }
}

/// X5 review V2: a stopping host waits for a request still running only until its
/// bound. A request that waits for something else (here a lock the test holds, as a
/// write lock of the registry would be held) is left running, and the wait ends at the
/// bound.
#[test]
fn a_bounded_join_leaves_a_request_that_does_not_end_by_its_bound() {
    struct Waits(Arc<Mutex<()>>, Arc<AtomicBool>);
    impl Handler for Waits {
        fn handle(&self, _: Request, _: &Arc<Connection>, _: Received) {
            self.1.store(true, Ordering::SeqCst);
            drop(lock(&self.0));
        }
    }
    let dir = short_tempdir();
    let path = dir.path().join("w.sock");
    let held = Arc::new(Mutex::new(()));
    let entered = Arc::new(AtomicBool::new(false));
    let holding = lock(&held);
    let mut listener = Listener::start(
        bind(&path).unwrap(),
        ListenerConfig {
            max_connections: 2,
            max_in_flight: 4,
            expected_uid: crate::home::euid(),
        },
        Arc::new(Waits(Arc::clone(&held), Arc::clone(&entered))),
    )
    .unwrap();
    let mut stream = connect(&path);
    stream
        .write_all(b"{\"id\":1,\"method\":\"slow\"}\n")
        .unwrap();
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while !entered.load(Ordering::SeqCst) {
        assert!(std::time::Instant::now() < until, "the request never ran");
        std::thread::sleep(Duration::from_millis(5));
    }
    listener.close();
    let asked = std::time::Instant::now();
    assert!(!listener.join_by(asked + Duration::from_millis(300)));
    let took = asked.elapsed();
    assert!(took >= Duration::from_millis(300), "{took:?}");
    assert!(took < Duration::from_secs(2), "{took:?}");
    // The request, left to run, ends once its lock is free.
    drop(holding);
}

/// X5 review (third round, P1): closing the listener does not wait for its accept
/// thread, which may be waiting for `SPAWN_LOCK` while a server is spawned, and the
/// bounded join leaves that thread at its bound. The connection it accepts once the
/// lock is free is closed on arrival and never served.
#[test]
fn a_stop_never_waits_for_an_accept_that_waits_for_the_spawn_lock() {
    let dir = short_tempdir();
    let (mut listener, path) = start(dir.path(), crate::home::euid());
    let spawning = lock(&SPAWN_LOCK);
    let mut stream = connect(&path);
    stream
        .write_all(b"{\"id\":1,\"method\":\"hello\"}\n")
        .unwrap();
    // The accept thread sees the connection at once (its poll returns when one waits)
    // and waits for the lock. The margin is for a loaded machine.
    std::thread::sleep(ACCEPT_POLL * 10);
    let stopping = std::thread::spawn(move || {
        let asked = std::time::Instant::now();
        listener.close();
        let closed = asked.elapsed();
        let joined = listener.join_by(asked + Duration::from_millis(300));
        // Returned, not dropped here: dropping it waits for every thread.
        (closed, asked.elapsed(), joined, listener)
    });
    let until = std::time::Instant::now() + Duration::from_secs(3);
    while !stopping.is_finished() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    let finished = stopping.is_finished();
    drop(spawning);
    assert!(finished, "the stop waited for the accept thread");
    let (closed, took, joined, listener) = stopping.join().unwrap();
    assert!(closed < Duration::from_millis(100), "{closed:?}");
    assert!(
        !joined,
        "the accept thread waited for the lock until the bound"
    );
    assert!(took >= Duration::from_millis(300), "{took:?}");
    assert!(took < Duration::from_secs(2), "{took:?}");
    // The lock is free: the accept ends, and its connection is closed unanswered.
    assert_eq!(read_line(&stream), None);
    drop(listener);
}

#[test]
fn the_listener_drops_a_peer_of_another_uid() {
    let dir = short_tempdir();
    let (_listener, path) = start(dir.path(), crate::home::euid().wrapping_add(1));
    let mut stream = connect(&path);
    let _ = stream.write_all(b"{\"id\":1,\"method\":\"hello\"}\n");
    assert_eq!(read_line(&stream), None);
}

#[test]
fn unreadable_lines_get_invalid_request_and_the_connection_goes_on() {
    let dir = short_tempdir();
    let (_listener, path) = start(dir.path(), crate::home::euid());
    let mut stream = connect(&path);
    stream.write_all(b"{not json\n").unwrap();
    let reply: Response = serde_json::from_str(&read_line(&stream).unwrap()).unwrap();
    assert_eq!(reply.id, UNREADABLE_REQUEST_ID);
    assert!(
        matches!(reply.outcome, org_proto::Outcome::Error(ref e) if e.code == ErrorCode::InvalidRequest)
    );

    stream.write_all(b"{\"id\":7,\"method\":5}\n").unwrap();
    let reply: Response = serde_json::from_str(&read_line(&stream).unwrap()).unwrap();
    assert_eq!(reply.id, 7);

    stream.write_all(b"{\"id\":8,\"method\":\"x\"}\n").unwrap();
    let reply: Response = serde_json::from_str(&read_line(&stream).unwrap()).unwrap();
    assert_eq!(
        reply.outcome,
        org_proto::Outcome::Result(Value::String("x".into()))
    );
}

/// S1: a client that half-closes after its requests gets their answers, then EOF.
#[test]
fn a_half_closed_connection_is_answered_then_closed() {
    use std::io::Read;
    let dir = short_tempdir();
    let (_listener, path) = start(dir.path(), crate::home::euid());
    let mut stream = connect(&path);
    stream
        .write_all(b"{\"id\":1,\"method\":\"hello\"}\n")
        .unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut all = String::new();
    stream
        .read_to_string(&mut all)
        .expect("EOF within the read timeout");
    assert_eq!(all.lines().count(), 1, "{all}");
}

#[test]
fn connections_beyond_the_cap_are_closed() {
    let dir = short_tempdir();
    let (_listener, path) = start(dir.path(), crate::home::euid());
    let mut held = Vec::new();
    for _ in 0..2 {
        let mut s = connect(&path);
        s.write_all(b"{\"id\":1,\"method\":\"a\"}\n").unwrap();
        assert!(read_line(&s).is_some());
        held.push(s);
    }
    let mut third = connect(&path);
    let _ = third.write_all(b"{\"id\":1,\"method\":\"a\"}\n");
    assert_eq!(read_line(&third), None);
}

/// Records where each request began and what the accept noted.
struct Recorder(Mutex<Vec<(String, u64, u64)>>);

impl Handler for Recorder {
    fn handle(&self, request: Request, connection: &Arc<Connection>, received: Received) {
        lock(&self.0).push((
            request.method,
            received.sent_at,
            connection.accepted().buffered,
        ));
    }
}

/// The N0 rule's two halves: what the accept notes as already buffered, and each request's
/// offset in the stream. A request that was already buffered when the connection was
/// accepted starts before N0 (an app-only call there is refused); one sent after starts at or
/// after it. (An integration test can't place a request before the host's accept reliably;
/// here the "accept" is the sample taken after the first request was written.)
#[test]
fn the_accept_notes_what_was_buffered_and_requests_know_where_they_began() {
    let (client, server) = UnixStream::pair().unwrap();
    let first = b"{\"id\":1,\"method\":\"early\"}\n";
    let second = b"{\"id\":2,\"method\":\"after\"}\n";
    (&client).write_all(first).unwrap();
    let accepted = Accepted::sample(&server);
    assert_eq!(accepted.buffered, first.len() as u64);

    let recorder = Arc::new(Recorder(Mutex::new(Vec::new())));
    let (connection, queue) = Connection::new(server.try_clone().unwrap(), accepted);
    let handler: Arc<dyn Handler> = recorder.clone();
    let serving = thread::spawn(move || serve(server, connection, queue, handler, 4));
    (&client).write_all(second).unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    serving.join().unwrap();

    let mut seen = lock(&recorder.0).clone();
    seen.sort_by_key(|(_, at, _)| *at);
    let n0 = first.len() as u64;
    assert_eq!(
        seen,
        [
            ("early".to_owned(), 0, n0),
            ("after".to_owned(), first.len() as u64, n0),
        ]
    );
    // The early one began before N0; the later one at it.
    assert!(seen[0].1 < seen[0].2 && seen[1].1 >= seen[1].2);
}

/// A handler whose `app` calls (inline when `inline_app`) wait until the client has written
/// its next line, then note how many bytes are waiting on the socket. Its other calls note
/// that they ran.
struct Probe {
    inline_app: bool,
    started: Mutex<mpsc::Sender<()>>,
    next_written: Mutex<mpsc::Receiver<()>>,
    next_handled: (Mutex<bool>, Condvar),
    waiting: Mutex<Option<u64>>,
}

impl Handler for Probe {
    fn inline(&self, request: &Request, _: &Connection) -> bool {
        self.inline_app && request.method == "app"
    }

    fn handle(&self, request: Request, connection: &Arc<Connection>, _received: Received) {
        let (handled, changed) = &self.next_handled;
        if request.method != "app" {
            *lock(handled) = true;
            changed.notify_all();
            return;
        }
        lock(&self.started).send(()).unwrap();
        lock(&self.next_written).recv().unwrap();
        // A reader that isn't held back reads the next line and runs it at once: give it
        // the time to.
        let guard = lock(handled);
        drop(
            changed
                .wait_timeout_while(guard, Duration::from_millis(500), |handled| !*handled)
                .unwrap(),
        );
        *lock(&self.waiting) = Some(crate::peer::bytes_waiting(connection.socket()).unwrap());
    }
}

/// How many bytes were waiting on the socket while an `app` call was handled, once the
/// client had written its next line.
fn bytes_waiting_during_an_app_call(inline_app: bool) -> u64 {
    let (client, server) = UnixStream::pair().unwrap();
    let (started, has_started) = mpsc::channel();
    let (written, next_written) = mpsc::channel();
    let probe = Arc::new(Probe {
        inline_app,
        started: Mutex::new(started),
        next_written: Mutex::new(next_written),
        next_handled: (Mutex::new(false), Condvar::new()),
        waiting: Mutex::new(None),
    });
    let accepted = Accepted::sample(&server);
    let (connection, queue) = Connection::new(server.try_clone().unwrap(), accepted);
    let handler: Arc<dyn Handler> = probe.clone();
    let serving = thread::spawn(move || serve(server, connection, queue, handler, 4));
    (&client)
        .write_all(b"{\"id\":1,\"method\":\"app\"}\n")
        .unwrap();
    // The reader has read the call (alone: nothing else was written yet).
    has_started.recv_timeout(Duration::from_secs(5)).unwrap();
    (&client)
        .write_all(b"{\"id\":2,\"method\":\"next\"}\n")
        .unwrap();
    written.send(()).unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    serving.join().unwrap();
    let waiting = lock(&probe.waiting).expect("the app call ran");
    waiting
}

/// L1: the calls `Handler::inline` picks (the app-only calls) run on the connection's
/// reader thread, which reads nothing more until they're answered: a line the client wrote
/// meanwhile is still waiting on the socket, which is what their check (`app_auth`,
/// "nothing after the request") relies on. A call run on its own thread lets the reader read
/// on, so nothing is waiting.
#[test]
fn an_inline_call_runs_before_anything_more_is_read() {
    let next_line = b"{\"id\":2,\"method\":\"next\"}\n".len() as u64;
    assert_eq!(bytes_waiting_during_an_app_call(true), next_line);
    assert_eq!(bytes_waiting_during_an_app_call(false), 0);
}

#[test]
fn bind_replaces_a_stale_socket_but_nothing_else() {
    let dir = short_tempdir();
    let path = dir.path().join("s.sock");
    drop(UnixListener::bind(&path).unwrap()); // leaves a stale socket file
    let listener = bind(&path).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    drop(listener);

    let file = dir.path().join("f.sock");
    fs::write(&file, b"not a socket").unwrap();
    assert!(matches!(bind(&file), Err(BindError::NotASocket)));
    assert_eq!(fs::read(&file).unwrap(), b"not a socket");
}
