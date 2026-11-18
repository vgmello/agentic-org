//! Strict transfer/revocation service and real rendezvous reset cleanup.
use super::*;
use org_office_launch::{ChannelKind, TransferRecord};
fn assert_transfer_fatal(run: &mut Run, case: &str) {
    let status = run.wait();
    let stderr = run.stderr();
    assert_eq!(status.code(), Some(1), "{case}: {status:?}: {stderr}");
    assert_eq!(
        stderr, "agentic-org-office: fatal transfer failure\n",
        "{case}"
    );
}
pub(super) fn send_record(socket: &UnixDatagram, payload: &[u8], fds: &[RawFd]) {
    send_until(
        socket,
        payload,
        fds,
        std::time::Instant::now() + Duration::from_secs(5),
        None,
    )
    .unwrap();
}

fn send_until(
    socket: &UnixDatagram,
    payload: &[u8],
    fds: &[RawFd],
    deadline: std::time::Instant,
    retry: Option<&std::sync::mpsc::Sender<()>>,
) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    // SAFETY: CMSG_SPACE computes storage for this bounded test-owned fd slice.
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of_val(fds) as u32) } as usize;
    let mut control = vec![0usize; space.div_ceil(std::mem::size_of::<usize>())];
    let mut data = libc::iovec {
        iov_base: payload.as_ptr().cast_mut().cast(),
        iov_len: payload.len(),
    };
    // SAFETY: live aligned buffers cover all lengths; passed fds remain sender-owned.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut data;
    message.msg_iovlen = 1;
    if !fds.is_empty() {
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = space as _;
        // SAFETY: nonempty aligned ancillary storage fits exactly this SCM_RIGHTS message.
        unsafe {
            let header = libc::CMSG_FIRSTHDR(&message);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(fds) as u32) as _;
            std::ptr::copy_nonoverlapping(fds.as_ptr(), libc::CMSG_DATA(header).cast(), fds.len());
        }
    }
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(Error::new(
                ErrorKind::TimedOut,
                "transfer send deadline expired",
            ));
        }
        let error = match attempt_send(socket, &message) {
            Ok(sent) if sent == payload.len() => return Ok(()),
            Ok(_) => {
                return Err(Error::new(
                    ErrorKind::WriteZero,
                    "partial transfer datagram",
                ))
            }
            Err(error) => error,
        };
        if error.kind() == ErrorKind::Interrupted {
            continue;
        }
        if cfg!(target_os = "macos") && error.raw_os_error() == Some(libc::ENOBUFS) {
            if let Some(retry) = retry {
                let _ = retry.send(());
            }
            std::thread::sleep(
                Duration::from_millis(2)
                    .min(deadline.saturating_duration_since(std::time::Instant::now())),
            );
            continue;
        }
        if error.kind() == ErrorKind::WouldBlock {
            if let Some(retry) = retry {
                let _ = retry.send(());
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let mut readiness = libc::pollfd {
                fd: socket.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            // SAFETY: one initialized pollfd; flooring milliseconds never extends the deadline.
            let result = unsafe {
                libc::poll(
                    &mut readiness,
                    1,
                    remaining.as_millis().min(i32::MAX as u128) as i32,
                )
            };
            if result < 0 {
                let error = Error::last_os_error();
                if error.kind() != ErrorKind::Interrupted {
                    return Err(error);
                }
            }
            continue;
        }
        return Err(error);
    }
}

thread_local! {
    static NEXT_SEND_ERROR: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}
fn attempt_send(socket: &UnixDatagram, message: &libc::msghdr) -> std::io::Result<usize> {
    if let Some(raw) = NEXT_SEND_ERROR.with(|next| next.take()) {
        return Err(std::io::Error::from_raw_os_error(raw));
    }
    // SAFETY: send_until retains all borrowed buffers/fds through this call.
    // DONTWAIT keeps a full Linux datagram queue inside the overall deadline.
    let sent = unsafe { libc::sendmsg(socket.as_raw_fd(), message, libc::MSG_DONTWAIT) };
    if sent < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(sent as usize)
    }
}

#[test]
fn sender_retries_injected_eintr_but_refuses_other_errors_without_sending() {
    let _process = process_isolation();
    for raw in [libc::EINTR, libc::EACCES] {
        let (host, child) = UnixDatagram::pair().unwrap();
        child.set_nonblocking(true).unwrap();
        NEXT_SEND_ERROR.with(|next| next.set(Some(raw)));
        let result = send_until(
            &host,
            b"exactly-once",
            &[],
            std::time::Instant::now() + Duration::from_secs(5),
            None,
        );
        let mut payload = [0; 32];
        if raw == libc::EINTR {
            result.unwrap();
            let count = child.recv(&mut payload).unwrap();
            assert_eq!(&payload[..count], b"exactly-once");
        } else {
            assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::EACCES));
        }
        assert_eq!(
            child.recv(&mut payload).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}

#[test]
fn unread_created_and_ready_host_close_has_normal_cleanup_including_linux_reset() {
    let _process = process_isolation();
    let world = World::new();
    let session = handshake(
        &world,
        FirstLine::Create,
        INSTANCE,
        Some(office_secrets_key()),
    );
    assert!(
        session.reader.buffer().is_empty(),
        "reset probe must not prefetch events"
    );
    let deadline = std::time::Instant::now() + WAIT;
    // A successful office hello proves Ready has been emitted, without consuming
    // Created/Ready on rendezvous. Poll readiness rather than retrying the test.
    loop {
        match Client::connect(socket(&world)) {
            Ok(client) => {
                assert_eq!(client.hello().unwrap().api_version, 1);
                break;
            }
            Err(org_client::ClientError::NotRunning(_)) => {
                assert!(std::time::Instant::now() < deadline);
                org_socket::poll_readable(session.stream.as_raw_fd(), Duration::from_millis(10))
                    .unwrap();
            }
            Err(error) => panic!("office readiness failed: {error}"),
        }
    }
    let Session {
        mut run,
        stream,
        reader,
        transfer: _transfer,
        ..
    } = session;
    // Last host descriptor close with unread worker events induces Linux ECONNRESET.
    // Do not shutdown first or consume the events; stdin and transfer stay open.
    drop(reader);
    drop(stream);
    assert_eq!(run.wait().code(), Some(0));
    assert!(!socket(&world).exists());
    assert_reopens(&world);
}

#[test]
fn channels_are_owned_before_ack_then_require_api2_default_deny_and_release_live_ids() {
    let _process = process_isolation();
    let world = World::new();
    let mut session = ready_create(&world);
    let mut live_peer = None;
    for phase in 0..2 {
        let (source, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let record = org_office_launch::encode_transfer_record(&TransferRecord::Channel {
            kind: ChannelKind::Local,
            office_id: OFFICE.into(),
            channel_id: "opaque/Exact:id".into(),
        })
        .unwrap();
        send_record(
            session.transfer.as_ref().unwrap(),
            &record,
            &[source.as_raw_fd()],
        );
        assert_eq!(
            session.event(),
            json!({"v":1,"event":"fds_received","of":"opaque/Exact:id"})
        );
        // The fake host retains its sender copy until this record's own ACK.
        drop(source);
        let mut peer = BufReader::with_capacity(1, peer);
        if phase == 0 {
            frame::write_message(
                peer.get_mut(),
                &json!({"id":41,"method":"org.snapshot","params":{}}),
            )
            .unwrap();
            let answer: Value = frame::read_message(&mut peer).unwrap().unwrap();
            assert_eq!(answer["id"], 41);
            assert_eq!(answer["error"]["code"], "unsupported_version");
            assert!(frame::read_line(&mut peer).unwrap().is_none());
            // The worker has disposed its endpoint; consume this ID's close before reuse.
            drop(peer);
            assert_eq!(
                session.event(),
                json!({"v":1,"event":"channel_closed","of":"opaque/Exact:id"})
            );
        } else {
            frame::write_message(peer.get_mut(), &json!({"id":42,"method":"hello","params":{"api":{"min":1,"max":3}},"principal":{"malformed":"ignored-on-hello"}})).unwrap();
            let answer: Value = frame::read_message(&mut peer).unwrap().unwrap();
            assert_eq!(answer["id"], 42);
            assert_eq!(answer["result"]["api_version"], 2);
            assert_eq!(answer["result"]["instance_id"], INSTANCE);
            assert_eq!(answer["result"]["office"]["office"]["id"], OFFICE);
            for (n, method) in [
                "msg.bind",
                "msg.wait",
                "status.set",
                "org.snapshot",
                "who",
                "team.create",
                "work.list",
                "task.create",
                "project.list",
                "area.list",
                "msg.send",
                "msg.inbox",
                "msg.read",
                "subscribe",
                "session.open",
                "channel.open",
                "bridge.open",
                "agent.add",
                "run.start",
                "approval.accept",
                "unknown.method",
            ]
            .into_iter()
            .enumerate()
            {
                let id = 100 + n;
                frame::write_message(
                    peer.get_mut(),
                    &json!({"id":id,"method":method,"params":{},"principal":null}),
                )
                .unwrap();
                let answer: Value = frame::read_message(&mut peer).unwrap().unwrap();
                assert_eq!(answer["id"], id);
                let expected = if n < 3 {
                    "invalid_request"
                } else {
                    "not_authorized"
                };
                assert_eq!(answer["error"]["code"], expected, "{method}");
            }
            live_peer = Some(peer);
        }
    }
    assert!(
        !org_socket::poll_readable(session.stream.as_raw_fd(), Duration::from_millis(10)).unwrap(),
        "duplicate channel ACK"
    );
    session.stop();
    assert!(frame::read_line(&mut live_peer.unwrap()).unwrap().is_none());
    assert_reopens(&world);
}

#[test]
fn malformed_and_wrong_rights_records_are_fatal_without_any_revoked_ack() {
    let _process = process_isolation();
    for case in [
        "empty",
        "unknown",
        "duplicate",
        "revoked-one",
        "two-fds",
        "control-truncated",
        "payload-truncated",
    ] {
        let world = World::new();
        let mut session = ready_create(&world);
        session
            .stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (source, _peer) = UnixStream::pair().unwrap();
        let revoked = org_office_launch::encode_transfer_record(&TransferRecord::Revoked {
            session_ids: vec![],
        })
        .unwrap();
        let (body, count): (&[u8], usize) = match case {
            "empty" => (b"", 0),
            "unknown" => (br#"{"type":"unknown"}"#, 0),
            "duplicate" => (
                br#"{"type":"revoked","session_ids":[],"session_ids":[]}"#,
                0,
            ),
            "revoked-one" => (&revoked, 1),
            "two-fds" => (&revoked, 2),
            "control-truncated" => (&revoked, 8),
            _ => (&[b'x'; 1025], 1),
        };
        send_record(
            session.transfer.as_ref().unwrap(),
            body,
            &vec![source.as_raw_fd(); count],
        );
        assert!(
            frame::read_line(&mut session.reader).unwrap().is_none(),
            "{case}: unexpected ACK"
        );
        assert_transfer_fatal(&mut session.run, case);
        drop(source);
        session.no_stdout();
    }
}

#[test]
fn valid_revocations_have_no_ack_and_overflow_is_fatal_after_fifo_application() {
    let _process = process_isolation();
    use std::io::Read;
    let world = World::new();
    let mut session = ready_create(&world);
    let ids: Vec<String> = (0..4096).map(|n| format!("opaque-{n}")).collect();
    for chunk in ids.chunks(64) {
        let body = org_office_launch::encode_transfer_record(&TransferRecord::Revoked {
            session_ids: chunk.to_vec(),
        })
        .unwrap();
        send_record(session.transfer.as_ref().unwrap(), &body, &[]);
    }
    for ids in [vec![], ids[..64].to_vec()] {
        let body = org_office_launch::encode_transfer_record(&TransferRecord::Revoked {
            session_ids: ids,
        })
        .unwrap();
        send_record(session.transfer.as_ref().unwrap(), &body, &[]);
    }
    assert_eq!(hello(&world).organization_name, "Acme");
    assert!(session.reader.buffer().is_empty());
    session
        .stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let error = session
        .stream
        .read(&mut [0])
        .expect_err("revocations unexpectedly produced output or EOF");
    assert!(matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ));
    eprintln!(
        "measured no revocation ACK: kind={:?} errno={:?}",
        error.kind(),
        error.raw_os_error()
    );
    assert_eq!(hello(&world).organization_name, "Acme");
    let overflow = org_office_launch::encode_transfer_record(&TransferRecord::Revoked {
        session_ids: vec!["overflow".into()],
    })
    .unwrap();
    send_record(session.transfer.as_ref().unwrap(), &overflow, &[]);
    session
        .stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert!(frame::read_line(&mut session.reader).unwrap().is_none());
    assert_transfer_fatal(&mut session.run, "overflow");
    session.no_stdout();
}

#[test]
fn stream_handoff_with_concatenated_records_is_fatal_without_ack() {
    let _process = process_isolation();
    let world = World::new();
    let path = world.home.join("r.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let mut run = world.spawn(&["--create"]);
    org_office_launch::write_first_line(run.stdin.as_mut().unwrap(), &create_line(&path)).unwrap();
    let mut stream = accepted(&listener, &mut run);
    let mut reader = BufReader::with_capacity(1, stream.try_clone().unwrap());
    assert_eq!(line(&mut reader)["event"], "ready_for_secrets");
    let handoff = Handoff::Create {
        office_secrets_key: office_secrets_key(),
        grant_key: GrantKey::from_bytes([7; 32]),
        recovery_code: Some(recovery_code()),
        fds: vec![HandoffFd::Transfer],
    };
    let mut header = org_office_launch::encode_handoff(&handoff).unwrap();
    stream.write_all(&header).unwrap();
    header.fill(0);
    let (mut host, child) = UnixStream::pair().unwrap();
    let mut body = Vec::new();
    for id in ["prefix", "tail"] {
        body.extend(
            org_office_launch::encode_transfer_record(&TransferRecord::Revoked {
                session_ids: vec![id.into()],
            })
            .unwrap(),
        );
    }
    // This intentionally violates the trusted-host datagram precondition.
    // Queue one concatenated write before the worker can receive the endpoint.
    host.write_all(&body).unwrap();
    host.shutdown(std::net::Shutdown::Write).unwrap();
    pass_fd(&stream, child.as_raw_fd());
    assert_eq!(
        line(&mut reader),
        json!({"v":1,"event":"fds_received","of":"handoff"})
    );
    drop(child);
    assert_eq!(line(&mut reader)["event"], "created");
    assert_eq!(line(&mut reader)["event"], "ready");
    // Darwin answers EINVAL to a timeout change once the worker has exited and closed its end;
    // the read below then ends at EOF without blocking, so the timeout is not needed.
    match stream.set_read_timeout(Some(Duration::from_secs(2))) {
        Err(error) if error.raw_os_error() == Some(libc::EINVAL) => {}
        other => other.unwrap(),
    }
    assert!(
        frame::read_line(&mut reader).unwrap().is_none(),
        "concatenated records received an ACK"
    );
    assert_transfer_fatal(&mut run, "concatenated stream");
    assert!(matches!(
        run.events.recv_timeout(WAIT),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    ));
}

fn fill_sender(socket: &UnixDatagram) -> usize {
    socket.set_nonblocking(true).unwrap();
    for queued in 0..10_000 {
        match socket.send(&[b'x'; 866]) {
            Ok(866) => {}
            Err(error) => {
                #[cfg(target_os = "macos")]
                assert_eq!(error.raw_os_error(), Some(libc::ENOBUFS));
                #[cfg(target_os = "linux")]
                assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
                return queued;
            }
            other => panic!("buffer fill: {other:?}"),
        }
    }
    panic!("test did not fill the sender queue")
}

#[test]
fn sender_retries_actual_backpressure_then_refuses_oversize_and_expired_deadline() {
    let _process = process_isolation();
    let (host, child) = UnixDatagram::pair().unwrap();
    let queued = fill_sender(&host);
    let (observed, retry) = std::sync::mpsc::channel();
    let drainer = std::thread::spawn(move || {
        // Never drain before the sender actually encounters backpressure.
        if retry.recv_timeout(Duration::from_secs(1)).is_ok() {
            let mut payload = [0; 866];
            for _ in 0..queued {
                assert_eq!(child.recv(&mut payload).unwrap(), 866);
                assert_eq!(payload, [b'x'; 866]);
            }
        }
        child
    });
    let sent = send_until(
        &host,
        &[b'y'; 866],
        &[],
        std::time::Instant::now() + Duration::from_secs(5),
        Some(&observed),
    );
    let child = drainer.join().unwrap();
    sent.unwrap();
    let mut payload = [0; 866];
    assert_eq!(child.recv(&mut payload).unwrap(), 866);
    assert_eq!(payload, [b'y'; 866]);
    let oversized = send_until(
        &host,
        &vec![0; 1024 * 1024],
        &[],
        std::time::Instant::now() + Duration::from_secs(5),
        None,
    )
    .unwrap_err();
    assert_eq!(oversized.raw_os_error(), Some(libc::EMSGSIZE));
    fill_sender(&host);
    let (cancel, cancelled) = std::sync::mpsc::channel::<()>();
    let closer = std::thread::spawn(move || {
        // A correct deadline returns before any drain. If a broken deadline spins,
        // release one slot after 200ms so the counterfactual fails without a hung harness.
        if matches!(
            cancelled.recv_timeout(Duration::from_millis(200)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ) {
            child.recv(&mut [0; 866]).unwrap();
        }
        child
    });
    let expired = send_until(
        &host,
        &[b'z'; 866],
        &[],
        std::time::Instant::now() + Duration::from_millis(20),
        None,
    )
    .unwrap_err();
    drop(cancel);
    closer.join().unwrap();
    assert_eq!(expired.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(expired.to_string(), "transfer send deadline expired");
}

#[test]
fn losing_all_host_channels_terminates_without_ack_or_orphan_and_reopens_the_db() {
    let _process = process_isolation();
    let world = World::new();
    let session = ready_create(&world);
    let Session {
        mut run,
        stream,
        reader,
        transfer,
        ..
    } = session;
    // This models a whole host exit. On macOS, transfer ECONNRESET can win
    // the race with clean rendezvous EOF, so either exact status 0 or 1 is valid.
    // Rendezvous-only EOF/reset still has its separate exact-status-0 regression.
    drop(transfer);
    drop(reader);
    drop(stream);
    drop(run.stdin.take());
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = run.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "orphaned worker after host exit"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(
        matches!(status.code(), Some(0 | 1)),
        "{status:?}: {}",
        run.stderr()
    );
    assert!(run.child.try_wait().unwrap().is_some());
    assert!(
        matches!(
            run.events.recv_timeout(Duration::from_secs(2)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        ),
        "host exit produced a stdout ACK/event"
    );
    assert_reopens(&world);
    eprintln!(
        "measured all-host-channel exit: code={:?} elapsed={:?}; no stdout ACK/orphan; DB reopened",
        status.code(),
        started.elapsed()
    );
}

fn open_live_channel(session: &mut Session, kind: ChannelKind, id: &str) -> BufReader<UnixStream> {
    let (source, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let record = org_office_launch::encode_transfer_record(&TransferRecord::Channel {
        kind,
        office_id: OFFICE.into(),
        channel_id: id.into(),
    })
    .unwrap();
    send_record(
        session.transfer.as_ref().unwrap(),
        &record,
        &[source.as_raw_fd()],
    );
    assert_eq!(
        session.event(),
        json!({"v":1,"event":"fds_received","of":id})
    );
    drop(source);
    BufReader::with_capacity(1, peer)
}
fn assert_channel_hello(peer: &mut BufReader<UnixStream>) {
    frame::write_message(
        peer.get_mut(),
        &json!({"id":1,"method":"hello","params":{"api":{"min":2,"max":2}}}),
    )
    .unwrap();
    let answer: Value = frame::read_message(peer).unwrap().unwrap();
    assert_eq!(answer["id"], 1);
    assert_eq!(answer["result"]["api_version"], 2);
    assert_eq!(answer["result"]["instance_id"], INSTANCE);
    assert_eq!(answer["result"]["office"]["office"]["id"], OFFICE);
}

fn assert_capacity_refused(session: &mut Session, kind: ChannelKind) {
    let (source, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let record = org_office_launch::encode_transfer_record(&TransferRecord::Channel {
        kind,
        office_id: OFFICE.into(),
        channel_id: "over-limit".into(),
    })
    .unwrap();
    send_record(
        session.transfer.as_ref().unwrap(),
        &record,
        &[source.as_raw_fd()],
    );
    assert_eq!(
        session.event(),
        json!({"v":1,"event":"fds_refused","of":"over-limit","reason":"capacity"})
    );
    // The refusal proves worker disposal. Only then release the sender copy.
    drop(source);
    assert!(frame::read_line(&mut BufReader::new(peer))
        .unwrap()
        .is_none());
    assert!(session.run.child.try_wait().unwrap().is_none());
}

fn close_live_channel(session: &mut Session, id: &str, peer: BufReader<UnixStream>) {
    // No source descriptor remains after its positive ACK. Closing this peer
    // causes actual EOF, disposal, and the one precisely correlated close event.
    drop(peer);
    assert_eq!(
        session.event(),
        json!({"v":1,"event":"channel_closed","of":id})
    );
}

#[test]
fn real_channel_limits_refuse_capacity_without_losing_worker_or_live_rpc() {
    let _process = process_isolation();
    for case in ["eighth-web", "ninth-local", "ninth-web", "ninth-all-local"] {
        let world = World::new();
        let mut session = ready_create(&world);
        let mut peers = Vec::new();
        for n in 0..7 {
            let id = format!("live-{n}");
            let peer = open_live_channel(
                &mut session,
                if case == "ninth-all-local" {
                    ChannelKind::Local
                } else {
                    ChannelKind::Web
                },
                &id,
            );
            peers.push((id, peer));
        }
        if case.starts_with("ninth-") {
            peers.push((
                "eighth-local".into(),
                open_live_channel(&mut session, ChannelKind::Local, "eighth-local"),
            ));
        }
        let refused_kind = if case.ends_with("web") {
            ChannelKind::Web
        } else {
            ChannelKind::Local
        };
        assert_capacity_refused(&mut session, refused_kind);
        // Every existing channel still serves real API-2 RPC after the refusal.
        for (_, peer) in &mut peers {
            assert_channel_hello(peer);
        }
        let (closed_id, closed_peer) = peers.remove(0);
        close_live_channel(&mut session, &closed_id, closed_peer);
        // The refused ID was never registered; freed physical capacity admits it.
        let mut replacement = open_live_channel(&mut session, refused_kind, "over-limit");
        assert_channel_hello(&mut replacement);
        peers.push(("over-limit".into(), replacement));
        for (id, mut peer) in peers {
            frame::write_message(
                peer.get_mut(),
                &json!({"id":2,"method":"org.snapshot","params":{}}),
            )
            .unwrap();
            let answer: Value = frame::read_message(&mut peer).unwrap().unwrap();
            assert_eq!(answer["id"], 2);
            assert_eq!(answer["error"]["code"], "not_authorized");
            close_live_channel(&mut session, &id, peer);
        }
        assert!(session.run.child.try_wait().unwrap().is_none(), "{case}");
        assert!(
            !org_socket::poll_readable(session.stream.as_raw_fd(), Duration::from_millis(10))
                .unwrap(),
            "{case}: unexpected ownership/lifecycle event"
        );
        session.stop();
        assert_reopens(&world);
    }
}

#[test]
fn duplicate_live_channel_remains_fatal_without_ack_and_reaps_every_peer() {
    let _process = process_isolation();
    let world = World::new();
    let mut session = ready_create(&world);
    let mut existing = open_live_channel(&mut session, ChannelKind::Web, "live-0");
    let (source, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let record = org_office_launch::encode_transfer_record(&TransferRecord::Channel {
        kind: ChannelKind::Local,
        office_id: OFFICE.into(),
        channel_id: "live-0".into(),
    })
    .unwrap();
    send_record(
        session.transfer.as_ref().unwrap(),
        &record,
        &[source.as_raw_fd()],
    );
    assert_transfer_fatal(&mut session.run, "duplicate-live");
    session.no_stdout();
    assert!(
        frame::read_line(&mut session.reader).unwrap().is_none(),
        "duplicate-live: unexpected ACK"
    );
    drop(source);
    for peer in [&mut existing, &mut BufReader::new(peer)] {
        assert!(
            frame::read_line(peer).unwrap().is_none(),
            "duplicate-live: worker retained channel"
        );
    }
    assert_reopens(&world);
}
#[test]
fn real_channel_admission_rejects_each_reserved_identity_and_wrong_descriptor_without_ack() {
    let _process = process_isolation();
    for case in ["empty-id", "handoff-id", "wrong-office", "datagram", "file"] {
        let world = World::new();
        let mut session = ready_create(&world);
        let record = org_office_launch::encode_transfer_record(&TransferRecord::Channel {
            kind: ChannelKind::Local,
            office_id: if case == "wrong-office" {
                "other-office"
            } else {
                OFFICE
            }
            .into(),
            channel_id: match case {
                "empty-id" => "",
                "handoff-id" => "handoff",
                _ => "valid-id",
            }
            .into(),
        })
        .unwrap();
        let (source, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let (datagram, _datagram_peer) = UnixDatagram::pair().unwrap();
        let file = tempfile::tempfile().unwrap();
        let fd = match case {
            "datagram" => datagram.as_raw_fd(),
            "file" => file.as_raw_fd(),
            _ => source.as_raw_fd(),
        };
        send_record(session.transfer.as_ref().unwrap(), &record, &[fd]);
        assert_transfer_fatal(&mut session.run, case);
        session.no_stdout();
        assert!(
            frame::read_line(&mut session.reader).unwrap().is_none(),
            "{case}: unexpected ACK"
        );
        drop(source);
        use std::io::Read;
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        assert_reopens(&world);
    }
}

#[test]
fn host_grants_allow_owner_requests_without_binding_and_observe_fifo_revocation() {
    let _process = process_isolation();
    let world = World::new();
    ready_create(&world).stop();
    let office = org_office::Office::open_for_instance(
        world.folder(OFFICE),
        org_office::Key::from_office_secrets_key(office_secrets_key()),
        INSTANCE,
    )
    .unwrap();
    let owner = office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.owner)
        .unwrap()
        .id;
    drop(office);
    let mut session = handshake(
        &world,
        FirstLine::Start,
        INSTANCE,
        Some(office_secrets_key()),
    );
    assert_eq!(session.event()["event"], "ready");
    let (source, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(WAIT)).unwrap();
    let body = org_office_launch::encode_transfer_record(&TransferRecord::Channel {
        kind: ChannelKind::Local,
        office_id: OFFICE.into(),
        channel_id: "grant-proof".into(),
    })
    .unwrap();
    send_record(
        session.transfer.as_ref().unwrap(),
        &body,
        &[source.as_raw_fd()],
    );
    assert_eq!(
        session.event(),
        json!({"v":1,"event":"fds_received","of":"grant-proof"})
    );
    drop(source);
    let mut peer = BufReader::with_capacity(1, peer);
    let mut call = |value: Value| {
        frame::write_message(peer.get_mut(), &value).unwrap();
        frame::read_message::<Value>(&mut peer).unwrap().unwrap()
    };
    assert_eq!(
        call(
            json!({"id":1,"method":"hello","params":{"api":{"min":2,"max":2}},"principal":{"ignored":true}})
        )["result"]["api_version"],
        2
    );
    // Sign for the actual stored owner; owner_member_id activation is a later slice.
    let mut grant = json!({"account":"test-account","member":owner,"org_id":ORG,"office_id":OFFICE,
        "channel_id":"grant-proof","session_id":"grant-session","exp":u64::MAX,"mac":"fa1316830c08b2d2648ec24d6d852e06524879c028e5f90cdfe691482adae01d"});
    sign_test_grant(&mut grant);
    let allowed =
        call(json!({"id":2,"method":"org.snapshot","params":{},"principal":{"grant":grant}}));
    if allowed.get("result").is_none() {
        session.stream.shutdown(std::net::Shutdown::Both).unwrap();
        session.run.wait();
        let log = session.run.stderr();
        panic!(
            "valid host owner grant must reach the real snapshot port: {allowed}; stderr: {log}"
        );
    }
    assert_eq!(
        call(json!({"id":3,"method":"org.snapshot","params":{}}))["error"]["code"],
        "not_authorized"
    );
    let refused =
        call(json!({"id":4,"method":"session.open","params":{},"principal":{"grant":grant}}));
    assert_eq!(refused["error"]["code"], "not_authorized");
    let revoke = org_office_launch::encode_transfer_record(&TransferRecord::Revoked {
        session_ids: vec!["grant-session".into()],
    })
    .unwrap();
    send_record(session.transfer.as_ref().unwrap(), &revoke, &[]);
    // Same FIFO's next channel ACK is a barrier after apply_batch returns; no sleeps.
    let (barrier_source, barrier_peer) = UnixStream::pair().unwrap();
    let barrier = org_office_launch::encode_transfer_record(&TransferRecord::Channel {
        kind: ChannelKind::Local,
        office_id: OFFICE.into(),
        channel_id: "revocation-barrier".into(),
    })
    .unwrap();
    send_record(
        session.transfer.as_ref().unwrap(),
        &barrier,
        &[barrier_source.as_raw_fd()],
    );
    assert_eq!(
        session.event(),
        json!({"v":1,"event":"fds_received","of":"revocation-barrier"})
    );
    drop(barrier_source);
    let revoked =
        call(json!({"id":5,"method":"org.snapshot","params":{},"principal":{"grant":grant}}));
    assert_eq!(revoked["error"]["code"], "not_authorized");
    grant["session_id"] = json!("other-session");
    sign_test_grant(&mut grant);
    let other =
        call(json!({"id":6,"method":"org.snapshot","params":{},"principal":{"grant":grant}}));
    assert!(
        other.get("result").is_some(),
        "revocation must leave other sessions valid: {other}"
    );
    session.stop();
    drop(barrier_peer);
    drop(peer);
    assert_reopens(&world);
}

fn sign_test_grant(value: &mut Value) {
    use hmac::Mac;
    let grant: org_proto::Grant = serde_json::from_value(value.clone()).unwrap();
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&[7; 32]).unwrap();
    mac.update(&org_proto::grant_mac_input(&grant.claims).unwrap());
    let mut tag = mac.finalize().into_bytes();
    value["mac"] = tag
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .into();
    tag.fill(0);
}
