#![allow(unsafe_code)]
use super::*;
use org_office_launch::v3::ChannelKind;
use std::os::unix::net::UnixDatagram;

#[test]
fn decoding_checks_strict_json_bounds_and_each_record_descriptor_count_directly() {
    let revoked = v3::encode_transfer_record(&TransferRecord::Revoked {
        session_ids: vec![],
    })
    .unwrap();
    let channel = v3::encode_transfer_record(&TransferRecord::Channel {
        kind: ChannelKind::Local,
        office_id: "office".into(),
        channel_id: "opaque".into(),
    })
    .unwrap();
    for (payload, expected) in [(&revoked, 0), (&channel, 1)] {
        for actual in 0..=2 {
            assert_eq!(decode(payload, actual).is_ok(), actual == expected);
        }
    }
    for invalid in [
        b"".as_slice(),
        br#"{"type":"unknown"}"#,
        br#"{"type":"revoked","session_ids":[]}{"type":"revoked","session_ids":[]}"#,
        br#"{"type":"revoked","session_ids":[],"extra":0}"#,
        br#"{"type":"revoked","session_ids":[],"session_ids":[]}"#,
    ] {
        assert_eq!(
            decode(invalid, 0).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    let body = v3::encode_transfer_record(&TransferRecord::Revoked {
        session_ids: vec![String::new()],
    })
    .unwrap();
    let record = TransferRecord::Revoked {
        session_ids: vec!["x".repeat(v3::TRANSFER_HEADER_MAX_BYTES - body.len())],
    };
    let exact = v3::encode_transfer_record(&record).unwrap();
    assert_eq!(exact.len(), 1024);
    assert!(decode(&exact, 0).is_ok());
    let mut too_big = exact;
    too_big.push(b' ');
    assert_eq!(
        decode(&too_big, 0).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn service_drop_joins_the_receiver_and_closes_the_owned_transport() {
    let (host, child) = UnixDatagram::pair().unwrap();
    let state = Arc::new(Mutex::new(Revocations::default()));
    let transport = startup::Transfer {
        receiver: org_socket::rights::DatagramReceiver::new(child),
        fence: Arc::default(),
    };
    let service = Service::start(transport, Arc::clone(&state)).unwrap();
    let (done, joined) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(service);
        done.send(()).unwrap();
    });
    joined
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("service shutdown hung");
    assert_eq!(
        Arc::strong_count(&state),
        1,
        "a detached service still owns state"
    );
    assert!(
        host.send(b"after shutdown").is_err(),
        "service must close receiver before Drop returns"
    );
}

// This seam only forces otherwise rare readiness errors; normal calls execute the
// real poll with a live receiver. Tests own the socket/fds throughout.
type PollOnce = Box<dyn FnOnce() -> io::Result<bool>>;
thread_local! { static NEXT_POLL: std::cell::RefCell<Option<PollOnce>> = const { std::cell::RefCell::new(None) }; }
pub(super) fn poll_readable(fd: std::os::fd::RawFd, tick: Duration) -> io::Result<bool> {
    if let Some(call) = NEXT_POLL.with(|next| next.borrow_mut().take()) {
        call()
    } else {
        org_socket::poll_readable(fd, tick)
    }
}

#[test]
fn interrupted_poll_retries_before_receiving_but_other_poll_errors_refuse() {
    for kind in [io::ErrorKind::Interrupted, io::ErrorKind::PermissionDenied] {
        let (host, child) = UnixDatagram::pair().unwrap();
        NEXT_POLL.with(|next| {
            *next.borrow_mut() = Some(Box::new(move || {
                let body = v3::encode_transfer_record(&TransferRecord::Revoked {
                    session_ids: vec!["after-poll".into()],
                })
                .unwrap();
                host.send(&body).unwrap();
                host.send(b"invalid").unwrap();
                Err(io::Error::from(kind))
            }))
        });
        let state = Arc::new(Mutex::new(Revocations::default()));
        let transport = startup::Transfer {
            receiver: org_socket::rights::DatagramReceiver::new(child),
            fence: Arc::default(),
        };
        let result = run(
            transport,
            Arc::clone(&state),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(result.is_err());
        state.lock().unwrap().apply_batch(vec![]).unwrap();
        // A later batch either duplicates the already received ID or adds it.
        // Fill to the boundary so this observes actual receipt, not just a call count.
        let mut observed = state.lock().unwrap();
        observed
            .apply_batch((0..4095).map(|n| format!("id-{n}")).collect())
            .unwrap();
        assert_eq!(
            observed.apply_batch(vec!["boundary".into()]).is_err(),
            kind == io::ErrorKind::Interrupted
        );
    }
}

#[test]
fn a_poisoned_revocations_mutex_refuses_before_applying_the_record() {
    let state = Arc::new(Mutex::new(Revocations::default()));
    let poison = Arc::clone(&state);
    assert!(std::thread::spawn(move || {
        let _guard = poison.lock().unwrap();
        panic!("test poison");
    })
    .join()
    .is_err());
    let (host, child) = UnixDatagram::pair().unwrap();
    host.send(
        &v3::encode_transfer_record(&TransferRecord::Revoked {
            session_ids: vec!["must-not-apply".into()],
        })
        .unwrap(),
    )
    .unwrap();
    host.send(b"invalid").unwrap();
    let transport = startup::Transfer {
        receiver: org_socket::rights::DatagramReceiver::new(child),
        fence: Arc::default(),
    };
    assert!(run(
        transport,
        Arc::clone(&state),
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
    let mut retained = match state.lock() {
        Err(error) => error.into_inner(),
        Ok(_) => panic!("poison was cleared"),
    };
    assert!(retained
        .apply_batch((0..4096).map(|n| format!("id-{n}")).collect())
        .is_ok());
}

#[test]
fn drop_requests_stop_and_waits_for_the_thread_to_finish() {
    use std::sync::mpsc;
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&stop);
    let (observed, stopped) = mpsc::channel();
    let (release, finish) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !stopping.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < deadline,
                "stop was not requested"
            );
            std::thread::yield_now();
        }
        observed.send(()).unwrap();
        finish.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    let service = Service {
        stop,
        thread: Some(worker),
    };
    let (done, joined) = mpsc::channel();
    let dropper = std::thread::spawn(move || {
        drop(service);
        done.send(()).unwrap();
    });
    stopped
        .recv_timeout(Duration::from_secs(2))
        .expect("Drop did not request stop");
    assert!(
        matches!(
            joined.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ),
        "Drop returned before joining its worker"
    );
    release.send(()).unwrap();
    joined.recv_timeout(Duration::from_secs(2)).unwrap();
    dropper.join().unwrap();
}

#[test]
fn real_datagram_readiness_records_empty_payload_and_peer_close_are_distinct_states() {
    let (host, child) = UnixDatagram::pair().unwrap();
    let raw = child.as_raw_fd();
    let mut receiver = org_socket::rights::DatagramReceiver::new(child);
    let fence = Mutex::new(());
    let mut payload = [0; 1024];
    assert!(matches!(
        receiver.receive_up_to(&mut payload, 1, &fence),
        Err(ReceiveError::WouldBlock)
    ));
    assert!(!org_socket::poll_readable(raw, Duration::from_millis(1)).unwrap());
    host.send(b"record").unwrap();
    assert!(org_socket::poll_readable(raw, Duration::from_secs(1)).unwrap());
    let record = receiver.receive_up_to(&mut payload, 1, &fence).unwrap();
    assert_eq!(&payload[..record.bytes], b"record");
    assert!(record.descriptors.is_empty());
    host.send(b"").unwrap();
    let empty = receiver.receive_up_to(&mut payload, 1, &fence).unwrap();
    assert_eq!(empty.bytes, 0);
    assert!(empty.descriptors.is_empty());
    assert!(decode(&payload[..empty.bytes], 0).is_err());
    drop(host);
    let closed = receiver.receive_up_to(&mut payload, 1, &fence);
    eprintln!(
        "measured datagram peer-close: {closed:?}; receiver_open={}",
        receiver.fd().is_some()
    );
    #[cfg(target_os = "macos")]
    // Isolated probes observed RESET; the 7952aabc full-workspace gate observed
    // WouldBlock instead. Retain its owned fd; a following poll is only logged,
    // since state can change between calls. No eventual RESET is promised.
    // Rendezvous owns host lifetime; this makes no kernel-cause claim.
    match closed {
        Err(ReceiveError::WouldBlock) => {
            assert_eq!(receiver.fd().unwrap().as_raw_fd(), raw);
            let readable = org_socket::poll_readable(raw, Duration::from_millis(1)).unwrap();
            eprintln!("measured Mac datagram peer-close WouldBlock: POLLIN={readable}");
        }
        Err(ReceiveError::Fatal(error)) => {
            assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
            assert_eq!(error.raw_os_error(), Some(libc::ECONNRESET));
            assert!(receiver.fd().is_none());
        }
        other => panic!("Mac datagram peer close: {other:?}"),
    }
    #[cfg(target_os = "linux")]
    {
        assert!(matches!(closed, Err(ReceiveError::WouldBlock)));
        assert!(!org_socket::poll_readable(raw, Duration::from_millis(1)).unwrap());
    }
    eprintln!("measured transfer: idle WouldBlock/poll=false; record readable/6bytes/0fds; empty datagram=0bytes/0fds and strict refusal");
}

#[test]
fn real_datagram_sender_full_buffer_and_oversize_report_distinct_errors() {
    let (host, child) = UnixDatagram::pair().unwrap();
    host.set_nonblocking(true).unwrap();
    let payload = [b'x'; 866];
    let mut queued = 0;
    let full = loop {
        match host.send(&payload) {
            Ok(sent) => {
                assert_eq!(sent, payload.len());
                queued += 1;
                assert!(queued < 10_000, "test did not fill the datagram buffer");
            }
            Err(error) => break error,
        }
    };
    let mut readiness = libc::pollfd {
        fd: host.as_raw_fd(),
        events: libc::POLLOUT,
        revents: 0,
    };
    // SAFETY: readiness is a live, initialized single-element poll array.
    let polled = unsafe { libc::poll(&mut readiness, 1, 0) };
    eprintln!(
        "measured sender full: queued={queued} bytes=866 kind={:?} errno={:?} poll={polled} revents={}",
        full.kind(), full.raw_os_error(), readiness.revents
    );
    #[cfg(target_os = "macos")]
    assert_eq!(full.raw_os_error(), Some(libc::ENOBUFS));
    #[cfg(target_os = "linux")]
    assert_eq!(full.kind(), io::ErrorKind::WouldBlock);
    let large = vec![0; 1024 * 1024];
    let oversized = host.send(&large).unwrap_err();
    eprintln!(
        "measured sender oversize: bytes={} kind={:?} errno={:?}",
        large.len(),
        oversized.kind(),
        oversized.raw_os_error()
    );
    assert_eq!(oversized.raw_os_error(), Some(libc::EMSGSIZE));
    let mut received = [0; 866];
    for _ in 0..queued {
        assert_eq!(child.recv(&mut received).unwrap(), payload.len());
        assert_eq!(received, payload);
    }
    assert_eq!(host.send(&payload).unwrap(), payload.len());
}

#[test]
fn receipt_bounds_are_enforced_before_json_or_record_fd_count_can_mask_them() {
    if std::env::var_os("ORG_B1_RECEIPT_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "transfer::tests::receipt_bounds_are_enforced_before_json_or_record_fd_count_can_mask_them",
                "--nocapture",
            ])
            .env("ORG_B1_RECEIPT_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{:?}: {}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    // Ancillary truncation may install unreported rights on macOS. Isolate this
    // refusal so process exit releases those rights, as the production policy requires.
    use std::io::Read;
    use std::os::unix::net::UnixStream;
    for count in 0..=2 {
        let (host, child) = UnixDatagram::pair().unwrap();
        let (source, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        send_rights(&host, &[b'x'; 1024], &vec![source.as_raw_fd(); count]);
        let mut transport = startup::Transfer {
            receiver: org_socket::rights::DatagramReceiver::new(child),
            fence: Arc::default(),
        };
        match receive(&mut transport) {
            Ok((payload, received)) => {
                assert!(count <= 1, "receipt accepted extra rights");
                assert_eq!(received.bytes, 1024);
                assert_eq!(payload.as_slice(), &[b'x'; 1024]);
                assert_eq!(received.descriptors.len(), count);
                for descriptor in received.descriptors {
                    // SAFETY: F_GETFD only queries this owned received descriptor.
                    let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFD) };
                    assert_ne!(flags, -1);
                    assert_ne!(flags & libc::FD_CLOEXEC, 0);
                }
            }
            Err(ReceiveError::Fatal(_)) => {
                assert_eq!(count, 2);
                assert!(transport.receiver.fd().is_none());
            }
            Err(error) => panic!("queued record did not receive: {error}"),
        }
        drop(source);
        if count <= 1 {
            assert_eq!(peer.read(&mut [0]).unwrap(), 0, "received right leaked");
        }
    }
    for length in [1023, 1025] {
        let (host, child) = UnixDatagram::pair().unwrap();
        host.send(&vec![b'x'; length]).unwrap();
        let mut transport = startup::Transfer {
            receiver: org_socket::rights::DatagramReceiver::new(child),
            fence: Arc::default(),
        };
        if length == 1023 {
            let (_, received) = receive(&mut transport).unwrap();
            assert_eq!(received.bytes, 1023);
            assert!(received.descriptors.is_empty());
        } else {
            assert!(matches!(
                receive(&mut transport),
                Err(ReceiveError::Fatal(_))
            ));
            assert!(transport.receiver.fd().is_none());
        }
    }
}

fn send_rights(socket: &UnixDatagram, payload: &[u8], descriptors: &[std::os::fd::RawFd]) {
    // SAFETY: CMSG_SPACE computes storage for the bounded test-owned descriptor slice.
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of_val(descriptors) as u32) } as usize;
    let mut control = vec![0usize; space.div_ceil(std::mem::size_of::<usize>())];
    let mut data = libc::iovec {
        iov_base: payload.as_ptr().cast_mut().cast(),
        iov_len: payload.len(),
    };
    // SAFETY: live aligned buffers cover all lengths; passed fds stay owned by this test.
    let sent = unsafe {
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut data;
        message.msg_iovlen = 1;
        if !descriptors.is_empty() {
            message.msg_control = control.as_mut_ptr().cast();
            message.msg_controllen = space as _;
            let header = libc::CMSG_FIRSTHDR(&message);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(descriptors) as u32) as _;
            std::ptr::copy_nonoverlapping(
                descriptors.as_ptr(),
                libc::CMSG_DATA(header).cast(),
                descriptors.len(),
            );
        }
        libc::sendmsg(socket.as_raw_fd(), &message, 0)
    };
    assert_eq!(
        sent,
        payload.len() as isize,
        "{}",
        io::Error::last_os_error()
    );
}

#[test]
fn every_poll_result_retries_receipt_and_fatal_receipt_stops_before_later_records() {
    for ready in [false, true] {
        let (host, child) = UnixDatagram::pair().unwrap();
        NEXT_POLL.with(|next| {
            *next.borrow_mut() = Some(Box::new(move || {
                host.send(
                    &v3::encode_transfer_record(&TransferRecord::Revoked {
                        session_ids: vec!["after-readiness".into()],
                    })
                    .unwrap(),
                )
                .unwrap();
                host.send(b"invalid").unwrap();
                Ok(ready)
            }))
        });
        let state = Arc::new(Mutex::new(Revocations::default()));
        let transport = startup::Transfer {
            receiver: org_socket::rights::DatagramReceiver::new(child),
            fence: Arc::default(),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let (cancel, cancelled) = std::sync::mpsc::channel::<()>();
        let watchdog = std::thread::spawn(move || {
            if matches!(
                cancelled.recv_timeout(Duration::from_secs(2)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                stopping.store(true, Ordering::SeqCst);
            }
        });
        let result = run(transport, Arc::clone(&state), stop);
        drop(cancel);
        watchdog.join().unwrap();
        assert!(result.is_err(), "receipt did not process the queued record");
        let mut observed = state.lock().unwrap();
        observed
            .apply_batch((0..4095).map(|n| format!("id-{n}")).collect())
            .unwrap();
        assert!(observed.apply_batch(vec!["overflow".into()]).is_err());
    }
    let (host, child) = UnixDatagram::pair().unwrap();
    host.send(&[b'x'; 1025]).unwrap();
    host.send(
        &v3::encode_transfer_record(&TransferRecord::Revoked {
            session_ids: vec!["must-not-apply".into()],
        })
        .unwrap(),
    )
    .unwrap();
    let state = Arc::new(Mutex::new(Revocations::default()));
    let transport = startup::Transfer {
        receiver: org_socket::rights::DatagramReceiver::new(child),
        fence: Arc::default(),
    };
    assert!(run(
        transport,
        Arc::clone(&state),
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
    state
        .lock()
        .unwrap()
        .apply_batch((0..4096).map(|n| format!("id-{n}")).collect())
        .unwrap();
}

#[test]
fn staged_channel_refuses_before_any_later_revocation_can_be_applied() {
    use std::io::Read;
    use std::os::unix::net::UnixStream;
    let (host, child) = UnixDatagram::pair().unwrap();
    let (source, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    let channel = v3::encode_transfer_record(&TransferRecord::Channel {
        kind: ChannelKind::Local,
        office_id: "office".into(),
        channel_id: "staged".into(),
    })
    .unwrap();
    send_rights(&host, &channel, &[source.as_raw_fd()]);
    host.send(
        &v3::encode_transfer_record(&TransferRecord::Revoked {
            session_ids: vec!["must-not-apply".into()],
        })
        .unwrap(),
    )
    .unwrap();
    host.send(b"terminal-invalid").unwrap();
    let state = Arc::new(Mutex::new(Revocations::default()));
    let transport = startup::Transfer {
        receiver: org_socket::rights::DatagramReceiver::new(child),
        fence: Arc::default(),
    };
    assert!(run(
        transport,
        Arc::clone(&state),
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
    state
        .lock()
        .unwrap()
        .apply_batch((0..4096).map(|n| format!("id-{n}")).collect())
        .unwrap();
    drop(source);
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

#[test]
fn actual_transfer_applies_fifo_records_then_refuses_atomic_overflow_without_a_prefix() {
    use org_office_launch::v3::{self, TransferRecord};
    use std::os::unix::net::UnixDatagram;
    use std::sync::{atomic::AtomicBool, Arc, Mutex};
    let state = Arc::new(Mutex::new(Revocations::default()));
    state
        .lock()
        .unwrap()
        .apply_batch((0..4094).map(|n| format!("id-{n}")).collect())
        .unwrap();
    let (host, child) = UnixDatagram::pair().unwrap();
    for ids in [
        vec!["first".into(), "id-1".into(), "first".into()],
        vec!["second".into()],
        vec![
            "overflow-prefix".into(),
            "second".into(),
            "overflow-tail".into(),
        ],
        vec!["unreachable-after-overflow".into()],
    ] {
        let body =
            v3::encode_transfer_record(&TransferRecord::Revoked { session_ids: ids }).unwrap();
        host.send(&body).unwrap();
    }
    host.send(b"terminal-invalid").unwrap();
    let transport = crate::startup::Transfer {
        receiver: org_socket::rights::DatagramReceiver::new(child),
        fence: Arc::default(),
    };
    let result = run(
        transport,
        Arc::clone(&state),
        Arc::new(AtomicBool::new(false)),
    );
    assert!(result.is_err());
    let mut observed = state.lock().unwrap();
    assert!(observed.apply_batch(vec!["capacity-probe".into()]).is_err());
    observed
        .apply_batch(vec![
            "first".into(),
            "second".into(),
            "id-0".into(),
            "id-4093".into(),
        ])
        .unwrap();
    for absent in [
        "overflow-prefix",
        "overflow-tail",
        "unreachable-after-overflow",
    ] {
        assert!(observed.apply_batch(vec![absent.into()]).is_err());
    }
}

#[test]
fn stream_handoff_with_concatenated_records_refuses_without_applying_a_prefix() {
    use org_office_launch::v3::{self, TransferRecord};
    use std::io::Write;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::{UnixDatagram, UnixStream};
    use std::sync::{atomic::AtomicBool, Arc, Mutex};
    let (mut host, child) = UnixStream::pair().unwrap();
    let mut body = Vec::new();
    for id in ["prefix", "tail"] {
        body.extend(
            v3::encode_transfer_record(&TransferRecord::Revoked {
                session_ids: vec![id.into()],
            })
            .unwrap(),
        );
    }
    // Deliberately violate the trusted-host datagram precondition. Queue both
    // records before receiving; EOF bounds the test if only a prefix is decoded.
    host.write_all(&body).unwrap();
    host.shutdown(std::net::Shutdown::Write).unwrap();
    let state = Arc::new(Mutex::new(Revocations::default()));
    let transport = crate::startup::Transfer {
        receiver: org_socket::rights::DatagramReceiver::new(UnixDatagram::from(OwnedFd::from(
            child,
        ))),
        fence: Arc::default(),
    };
    let result = run(
        transport,
        Arc::clone(&state),
        Arc::new(AtomicBool::new(false)),
    );
    assert!(result.is_err());
    state
        .lock()
        .unwrap()
        .apply_batch((0..4096).map(|n| format!("empty-probe-{n}")).collect())
        .expect("a concatenated stream applied a prefix");
}
