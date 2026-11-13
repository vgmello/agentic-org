use super::*;
use crate::rights::{DatagramReceiver, StreamReceiver};
use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::sync::Mutex;

const MARKER: u8 = 0;

fn later() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[test]
fn a_marker_and_one_descriptor_arrive_together_and_nothing_else() {
    let (host, worker) = UnixStream::pair().unwrap();
    let (passed, _peer) = UnixDatagram::pair().unwrap();
    send(host.as_fd(), &[MARKER], &[passed.as_fd()], later()).unwrap();
    let fence = Mutex::new(());
    let mut receiver = StreamReceiver::new(worker);
    let fds = receiver.receive_marker(1, MARKER, &fence).unwrap();
    assert_eq!(fds.len(), 1);
    // The sender still holds its own copy: sending never closes it.
    assert!(passed.as_fd().as_raw_fd() >= 0);
    send(host.as_fd(), &[MARKER], &[], later()).unwrap();
    assert!(receiver
        .receive_marker(0, MARKER, &fence)
        .unwrap()
        .is_empty());
}

#[test]
fn a_datagram_header_keeps_its_bytes_and_descriptor() {
    let (host, worker) = UnixDatagram::pair().unwrap();
    let (passed, _peer) = UnixStream::pair().unwrap();
    send(
        host.as_fd(),
        b"{\"type\":\"x\"}",
        &[passed.as_fd()],
        later(),
    )
    .unwrap();
    let mut payload = [0u8; 64];
    let received = DatagramReceiver::new(worker)
        .receive(&mut payload, 1, &Mutex::new(()))
        .unwrap();
    assert_eq!(&payload[..received.bytes], b"{\"type\":\"x\"}");
    assert_eq!(received.descriptors.len(), 1);
}

#[test]
fn two_descriptors_keep_their_order_and_the_senders_copies() {
    let (host, worker) = UnixStream::pair().unwrap();
    let (first, mut first_peer) = UnixStream::pair().unwrap();
    let (second, mut second_peer) = UnixStream::pair().unwrap();
    first_peer.write_all(b"a").unwrap();
    second_peer.write_all(b"b").unwrap();
    send(
        host.as_fd(),
        &[MARKER],
        &[first.as_fd(), second.as_fd()],
        later(),
    )
    .unwrap();
    let fds = StreamReceiver::new(worker)
        .receive_marker(2, MARKER, &Mutex::new(()))
        .unwrap();
    for (fd, expected) in fds.into_iter().zip(*b"ab") {
        let mut stream = UnixStream::from(fd);
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [expected]);
    }
    // Dropping both received copies must leave the sender's originals usable.
    first_peer.write_all(b"x").unwrap();
    second_peer.write_all(b"y").unwrap();
    for (mut stream, expected) in [(first, b'x'), (second, b'y')] {
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [expected]);
    }
}

#[test]
fn a_datagram_record_without_descriptors_keeps_its_payload() {
    let (host, worker) = UnixDatagram::pair().unwrap();
    let payload = br#"{"type":"revoked","session_ids":[]}"#;
    send(host.as_fd(), payload, &[], later()).unwrap();
    let mut bytes = [0; 128];
    let record = DatagramReceiver::new(worker)
        .receive(&mut bytes, 0, &Mutex::new(()))
        .unwrap();
    assert_eq!(&bytes[..record.bytes], payload);
    assert!(record.descriptors.is_empty());
}

#[test]
fn an_empty_payload_is_refused_before_any_call() {
    let calls = RefCell::new(0);
    let sent = send_with(
        0,
        &[],
        &[],
        later(),
        |_, _| {
            *calls.borrow_mut() += 1;
            Ok(1)
        },
        |_, _| Ok(()),
    );
    assert!(matches!(sent, Err(SendError::Io(_))));
    assert_eq!(*calls.borrow(), 0);
}

/// Each errno through the real loop: the attempts and waits it causes.
fn drive(errors: &[i32]) -> (Result<(), SendError>, usize, Vec<Duration>) {
    let calls = RefCell::new(0usize);
    let waits = RefCell::new(Vec::new());
    let sent = send_with(
        0,
        &[MARKER],
        &[],
        later(),
        |_, message| {
            assert_eq!(message.msg_controllen, 0, "no control without descriptors");
            let index = {
                let mut calls = calls.borrow_mut();
                *calls += 1;
                *calls - 1
            };
            match errors.get(index) {
                Some(&errno) => Err(io::Error::from_raw_os_error(errno)),
                None => Ok(1),
            }
        },
        |_, left| {
            waits.borrow_mut().push(left);
            Ok(())
        },
    );
    (sent, calls.into_inner(), waits.into_inner())
}

#[test]
fn interrupted_and_full_sends_retry_within_the_deadline() {
    let (sent, calls, waits) = drive(&[libc::EINTR, libc::EAGAIN, libc::ENOBUFS]);
    sent.unwrap();
    assert_eq!(calls, 4);
    // Only EAGAIN polls, and with no more than the time left.
    assert_eq!(waits.len(), 1);
    assert!(waits[0] <= Duration::from_secs(5));
}

#[test]
fn writable_wait_uses_only_the_time_remaining_after_sendmsg_returns() {
    let deadline = Instant::now() + Duration::from_secs(60);
    let call_returned = RefCell::new(None);
    let mut calls = 0;
    let mut waits = 0;
    send_with(
        0,
        &[MARKER],
        &[],
        deadline,
        |_, _| {
            calls += 1;
            if calls == 1 {
                // Model time spent in a nonblocking syscall or descheduled before its return.
                std::thread::sleep(Duration::from_millis(10));
                *call_returned.borrow_mut() = Some(Instant::now());
                Err(io::Error::from_raw_os_error(libc::EAGAIN))
            } else {
                Ok(1)
            }
        },
        |_, left| {
            waits += 1;
            let maximum = deadline.saturating_duration_since(call_returned.borrow().unwrap());
            assert!(left <= maximum, "wait reused the pre-sendmsg time budget");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!((calls, waits), (2, 1));
}

#[test]
fn retryable_errors_do_not_wait_after_the_call_exhausts_the_budget() {
    for errno in [libc::EINTR, libc::EAGAIN, libc::ENOBUFS] {
        let started = Instant::now();
        let deadline = started + Duration::from_secs(1);
        let clock = Cell::new(started);
        let mut calls = 0;
        let result = send_using(
            &[MARKER],
            &[],
            deadline,
            |_| {
                calls += 1;
                clock.set(deadline);
                Err(io::Error::from_raw_os_error(errno))
            },
            |_| panic!("wait after deadline for errno {errno}"),
            |_| panic!("backoff after deadline for errno {errno}"),
            || clock.get(),
        );
        assert!(matches!(result, Err(SendError::TimedOut)));
        assert_eq!(calls, 1);
    }
}

#[test]
fn interrupted_attempts_recheck_the_deadline_before_each_retry() {
    let started = Instant::now();
    let tick = Duration::from_millis(1);
    let deadline = started + tick * 3;
    let clock = Cell::new(started);
    let mut calls = 0;
    let result = send_using(
        &[MARKER],
        &[],
        deadline,
        |_| {
            calls += 1;
            clock.set(clock.get() + tick);
            Err(io::Error::from_raw_os_error(libc::EINTR))
        },
        |_| panic!("interrupted sends must not poll"),
        |_| panic!("interrupted sends must not back off"),
        || clock.get(),
    );
    assert!(matches!(result, Err(SendError::TimedOut)));
    assert_eq!(calls, 3);
}

#[test]
fn retry_waits_and_backoffs_use_the_remaining_budget_and_backoff_cap() {
    for (errno, budget, expected) in [
        (libc::EAGAIN, Duration::from_secs(1), Duration::from_secs(1)),
        (
            libc::ENOBUFS,
            Duration::from_secs(1),
            Duration::from_millis(2),
        ),
        (
            libc::ENOBUFS,
            Duration::from_micros(500),
            Duration::from_micros(500),
        ),
    ] {
        let started = Instant::now();
        let spent = Duration::from_secs(1);
        let deadline = started + spent + budget;
        let clock = Cell::new(started);
        let mut calls = 0;
        let mut waits = Vec::new();
        let mut pauses = Vec::new();
        send_using(
            &[MARKER],
            &[],
            deadline,
            |_| {
                calls += 1;
                if calls == 1 {
                    clock.set(started + spent);
                    Err(io::Error::from_raw_os_error(errno))
                } else {
                    Ok(1)
                }
            },
            |left| {
                waits.push(left);
                Ok(())
            },
            |left| pauses.push(left),
            || clock.get(),
        )
        .unwrap();
        assert_eq!(calls, 2);
        if errno == libc::EAGAIN {
            assert_eq!(waits, [expected]);
            assert!(pauses.is_empty());
        } else {
            assert_eq!(pauses, [expected]);
            assert!(waits.is_empty());
        }
    }
}

#[test]
fn too_large_and_other_errors_end_at_once() {
    let (sent, calls, _) = drive(&[libc::EMSGSIZE]);
    assert!(matches!(sent, Err(SendError::TooLarge)));
    assert_eq!(calls, 1);
    let (sent, calls, _) = drive(&[libc::EPIPE]);
    assert!(matches!(sent, Err(SendError::Io(e)) if e.raw_os_error() == Some(libc::EPIPE)));
    assert_eq!(calls, 1);
}

#[test]
fn interrupted_waits_retry_but_other_wait_errors_end_without_another_send() {
    for kind in [io::ErrorKind::Interrupted, io::ErrorKind::PermissionDenied] {
        let mut calls = 0;
        let mut waits = 0;
        let result = send_with(
            0,
            &[MARKER],
            &[],
            later(),
            |_, _| {
                calls += 1;
                if calls == 1 {
                    Err(io::Error::from_raw_os_error(libc::EAGAIN))
                } else {
                    Ok(1)
                }
            },
            |_, _| {
                waits += 1;
                Err(io::Error::from(kind))
            },
        );
        if kind == io::ErrorKind::Interrupted {
            result.unwrap();
            assert_eq!(calls, 2);
        } else {
            assert!(matches!(result, Err(SendError::Io(e)) if e.kind() == kind));
            assert_eq!(calls, 1);
        }
        assert_eq!(waits, 1);
    }
}

#[test]
fn poll_arguments_clamp_both_bounds_and_preserve_ready_and_timeout_results() {
    for (left, expected) in [
        (Duration::ZERO, 1),
        (Duration::from_micros(10), 1),
        (Duration::from_secs(1), 1_000),
        (Duration::MAX, libc::c_int::MAX),
    ] {
        for result in [0, 1] {
            wait_writable_with(42, left, |slot, millis| {
                assert_eq!(slot.fd, 42);
                assert_eq!(slot.events, libc::POLLOUT);
                assert_eq!(slot.revents, 0);
                assert_eq!(millis, expected);
                slot.revents = if result == 0 { 0 } else { libc::POLLOUT };
                result
            })
            .unwrap();
        }
    }
}

#[test]
fn poll_errors_and_invalid_descriptors_are_preserved() {
    assert!(wait_writable_with(42, Duration::from_secs(1), |_, _| -1).is_err());
    let error = wait_writable_with(42, Duration::from_secs(1), |slot, _| {
        slot.revents = libc::POLLNVAL;
        1
    })
    .unwrap_err();
    assert_eq!(error.raw_os_error(), Some(libc::EBADF));
    // This positive integer cannot name an open descriptor on either supported platform.
    let error = wait_writable(libc::c_int::MAX, Duration::from_millis(1)).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(libc::EBADF));
}

#[test]
fn a_real_send_failure_never_formats_the_payload() {
    let (host, peer) = UnixDatagram::pair().unwrap();
    drop(peer);
    let sentinel = "private-sender-payload-sentinel";
    let error = send(host.as_fd(), sentinel.as_bytes(), &[], later()).unwrap_err();
    assert!(matches!(error, SendError::Io(_)));
    assert!(!error.to_string().contains(sentinel));
    assert!(!format!("{error:?}").contains(sentinel));
}

#[test]
fn a_partial_stream_send_is_out_of_step() {
    for sent in [0, 1, 3] {
        let result = send_with(0, b"ab", &[], later(), |_, _| Ok(sent), |_, _| Ok(()));
        assert!(matches!(result, Err(SendError::Short)));
    }
}

#[test]
fn the_single_message_has_exact_payload_and_ancillary_layout() {
    let mut calls = 0;
    send_with(
        42,
        b"payload",
        &[10, 20],
        later(),
        |socket, message| {
            calls += 1;
            assert_eq!(socket, 42);
            assert!(message.msg_name.is_null());
            assert_eq!(message.msg_namelen, 0);
            assert_eq!(message.msg_iovlen, 1);
            assert!(!message.msg_iov.is_null());
            assert!(!message.msg_control.is_null());
            assert_eq!(message.msg_controllen, control_space(2).unwrap() as _);
            // SAFETY: send_with keeps its initialized iovec and aligned control buffer live
            // throughout this interposed call; this fixture never passes fake FDs to the OS.
            unsafe {
                let data = &*message.msg_iov;
                assert_eq!(data.iov_len, 7);
                assert!(!data.iov_base.is_null());
                assert_eq!(
                    std::slice::from_raw_parts(data.iov_base.cast::<u8>(), 7),
                    b"payload"
                );
                let header = libc::CMSG_FIRSTHDR(message);
                assert!(!header.is_null());
                assert_eq!((*header).cmsg_level, libc::SOL_SOCKET);
                assert_eq!((*header).cmsg_type, libc::SCM_RIGHTS);
                assert_eq!((*header).cmsg_len, libc::CMSG_LEN(8) as _);
                assert_eq!(
                    std::slice::from_raw_parts(libc::CMSG_DATA(header).cast::<RawFd>(), 2),
                    &[10, 20]
                );
                assert!(libc::CMSG_NXTHDR(message, header).is_null());
            }
            Ok(7)
        },
        |_, _| panic!("successful send must not wait"),
    )
    .unwrap();
    assert_eq!(calls, 1);
}

#[test]
fn an_expired_deadline_sends_nothing() {
    let calls = RefCell::new(0);
    let sent = send_with(
        0,
        &[MARKER],
        &[],
        Instant::now(),
        |_, _| {
            *calls.borrow_mut() += 1;
            Ok(1)
        },
        |_, _| Ok(()),
    );
    assert!(matches!(sent, Err(SendError::TimedOut)));
    assert_eq!(*calls.borrow(), 0);
}

/// Rule 14: a peer that never drains is given up on at the deadline, observed on a real full
/// socket (macOS answers ENOBUFS, Linux EAGAIN): the send neither spins forever nor blocks.
#[test]
fn a_full_peer_times_out_at_the_deadline() {
    let (host, _worker) = UnixDatagram::pair().unwrap();
    host.set_nonblocking(true).unwrap();
    while host.send(&[0u8; 512]).is_ok() {}
    while host.send(&[0u8; 1]).is_ok() {}
    let started = Instant::now();
    let sent = send(
        host.as_fd(),
        &[MARKER],
        &[],
        started + Duration::from_millis(100),
    );
    assert!(matches!(sent, Err(SendError::TimedOut)), "{sent:?}");
    let took = started.elapsed();
    assert!(took >= Duration::from_millis(100), "{took:?}");
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
fn a_blocking_socket_still_sends_nonblocking_under_the_deadline() {
    let (host, worker) = UnixDatagram::pair().unwrap();
    host.set_nonblocking(true).unwrap();
    while host.send(&[0u8; 512]).is_ok() {}
    while host.send(&[0u8; 1]).is_ok() {}
    host.set_nonblocking(false).unwrap();
    let (finished, release_peer) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        // A missing MSG_DONTWAIT may block; closing the peer bounds that negative control
        // and produces an I/O error instead of the required deadline result.
        let _ = release_peer.recv_timeout(Duration::from_secs(1));
        drop(worker);
    });
    let result = send(
        host.as_fd(),
        &[MARKER],
        &[],
        Instant::now() + Duration::from_millis(50),
    );
    drop(finished);
    peer.join().unwrap();
    assert!(matches!(result, Err(SendError::TimedOut)), "{result:?}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_closed_stream_does_not_raise_sigpipe() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "rights::send::tests::sigpipe_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("ORG_SOCKET_SEND_SIGPIPE_CHILD", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("EPIPE without SIGPIPE"));
}

#[cfg(target_os = "linux")]
#[test]
fn sigpipe_child() {
    if std::env::var_os("ORG_SOCKET_SEND_SIGPIPE_CHILD").is_none() {
        return;
    }
    let (socket, peer) = UnixStream::pair().unwrap();
    drop(peer);
    // SAFETY: this is a fresh process running only this test. Reset Rust's ignored SIGPIPE
    // so MSG_NOSIGNAL is independently observable without changing the parent test process.
    assert_ne!(
        unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) },
        libc::SIG_ERR
    );
    let result = send(socket.as_fd(), &[MARKER], &[], later());
    assert!(matches!(result, Err(SendError::Io(e)) if e.raw_os_error() == Some(libc::EPIPE)));
    println!("EPIPE without SIGPIPE");
}

#[test]
fn buffer_sizes_are_set_both_ways() {
    let (a, _b) = UnixDatagram::pair().unwrap();
    let get = |option| {
        let mut value: libc::c_int = 0;
        let mut len = mem::size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: a live socket and an out-value of `len` bytes.
        let rc = unsafe {
            libc::getsockopt(
                a.as_raw_fd(),
                libc::SOL_SOCKET,
                option,
                (&mut value as *mut libc::c_int).cast(),
                &mut len,
            )
        };
        assert_eq!(rc, 0);
        value as usize
    };
    set_buffer_sizes(a.as_fd(), 12 * 1024).unwrap();
    // Linux reports double what was asked (its bookkeeping); macOS what was asked.
    for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
        assert!([12 * 1024, 24 * 1024].contains(&get(option)), "{option}");
    }
}

#[test]
fn invalid_buffer_sizes_and_non_socket_descriptors_are_refused() {
    let (socket, _) = UnixDatagram::pair().unwrap();
    let error = set_buffer_sizes(socket.as_fd(), usize::MAX).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    let file = std::fs::File::open("/dev/null").unwrap();
    let error = set_buffer_sizes(file.as_fd(), 64 * 1024).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(libc::ENOTSOCK));
}
