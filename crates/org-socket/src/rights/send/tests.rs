use super::*;
use crate::rights::{DatagramReceiver, StreamReceiver};
use std::cell::RefCell;
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
fn too_large_and_other_errors_end_at_once() {
    let (sent, calls, _) = drive(&[libc::EMSGSIZE]);
    assert!(matches!(sent, Err(SendError::TooLarge)));
    assert_eq!(calls, 1);
    let (sent, calls, _) = drive(&[libc::EPIPE]);
    assert!(matches!(sent, Err(SendError::Io(e)) if e.raw_os_error() == Some(libc::EPIPE)));
    assert_eq!(calls, 1);
}

#[test]
fn a_partial_stream_send_is_out_of_step() {
    let sent = send_with(0, b"ab", &[], later(), |_, _| Ok(1), |_, _| Ok(()));
    assert!(matches!(sent, Err(SendError::Short)));
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
