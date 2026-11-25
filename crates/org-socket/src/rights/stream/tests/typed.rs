use super::*;
use std::net::{TcpListener, TcpStream, UdpSocket};

#[test]
fn unix_datagram_requires_kind_family_and_cloexec_and_closes_refusals() {
    for case in [
        "dgram-pair",
        "dgram-unbound",
        "dgram-stream",
        "dgram-inet",
        "dgram-no-cloexec",
    ] {
        run_typed(case);
    }
}

#[test]
fn tcp_listener_requires_inet_stream_listening_and_cloexec_and_closes_refusals() {
    for case in [
        "tcp-v4",
        "tcp-v6",
        "tcp-dgram",
        "tcp-unix",
        "tcp-connected",
        "tcp-no-cloexec",
    ] {
        if case == "tcp-v6" && !ipv6_loopback() {
            continue;
        }
        run_typed(case);
    }
}

/// Whether this machine has an IPv6 loopback to bind. A machine without one (a sandbox
/// or a container with IPv6 switched off) answers "address family not supported" or
/// "address not available": the IPv6 case is then skipped, with the reason printed, and
/// the IPv4 case still runs. Any other failure to bind is a failure of the test.
fn ipv6_loopback() -> bool {
    match TcpListener::bind("[::1]:0") {
        Ok(_) => true,
        Err(error)
            if error.kind() == std::io::ErrorKind::AddrNotAvailable
                || error.raw_os_error() == Some(libc::EAFNOSUPPORT) =>
        {
            eprintln!("skipped the IPv6 case: this machine has no IPv6 loopback ({error})");
            false
        }
        Err(error) => panic!("binding the IPv6 loopback: {error}"),
    }
}

fn run_typed(case: &str) {
    run_child(
        "rights::stream::tests::typed::typed_child",
        "ORG_SOCKET_TYPED_CASE",
        case,
    );
}

// Only this test runs in its child, so no concurrent allocator can reuse a
// refused fd before the EBADF observation or the subsequent replacement check.
#[test]
fn typed_child() {
    let Ok(case) = std::env::var("ORG_SOCKET_TYPED_CASE") else {
        return;
    };
    let dir = tempfile_dir();
    let mut datagram_peer = None;
    let mut stream_peer = None;
    let mut tcp_peer = None;
    let descriptor: OwnedFd = match case.as_str() {
        "dgram-pair" | "dgram-no-cloexec" => {
            let (socket, peer) = UnixDatagram::pair().unwrap();
            datagram_peer = Some(peer);
            socket.into()
        }
        "dgram-unbound" => UnixDatagram::unbound().unwrap().into(),
        "dgram-stream" => {
            let (socket, peer) = UnixStream::pair().unwrap();
            stream_peer = Some(peer);
            socket.into()
        }
        "dgram-inet" | "tcp-dgram" => UdpSocket::bind("127.0.0.1:0").unwrap().into(),
        "tcp-v4" | "tcp-no-cloexec" => TcpListener::bind("127.0.0.1:0").unwrap().into(),
        "tcp-v6" => TcpListener::bind("[::1]:0").unwrap().into(),
        "tcp-unix" => UnixListener::bind(dir.join("listener")).unwrap().into(),
        "tcp-connected" => {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (peer, _) = listener.accept().unwrap();
            // Keep both ends owned until adoption completes.
            tcp_peer = Some(peer);
            stream.into()
        }
        _ if case.starts_with("dgram-fault-") => UnixDatagram::unbound().unwrap().into(),
        _ if case.starts_with("tcp-fault-") => TcpListener::bind("127.0.0.1:0").unwrap().into(),
        _ => panic!("unknown typed case {case}"),
    };
    let raw = descriptor.as_raw_fd();
    if case.ends_with("no-cloexec") {
        // SAFETY: change only the flags of this test-owned descriptor.
        assert_eq!(unsafe { libc::fcntl(raw, libc::F_SETFD, 0) }, 0);
    }
    if let Some(name) = case.split_once("-fault-").map(|(_, name)| name) {
        let fault = match name {
            "kind" => Fault::Kind,
            "family" => Fault::Family,
            "flags" => Fault::Flags,
            "listener" => Fault::Listener,
            _ => panic!("unknown fault {name}"),
        };
        FAULT.with(|slot| slot.set(Some(fault)));
    }
    // SAFETY: query flags without mutating this live owned descriptor.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    if case.starts_with("dgram-") {
        let result = unix_datagram(descriptor);
        if matches!(case.as_str(), "dgram-pair" | "dgram-unbound") {
            let socket = result.unwrap();
            assert_eq!(socket.as_raw_fd(), raw, "must adopt without duplication");
            assert!(socket.local_addr().unwrap().is_unnamed());
            if let Some(peer) = datagram_peer {
                socket.send(b"out").unwrap();
                assert!(crate::poll_readable(peer.as_raw_fd(), Duration::from_secs(1)).unwrap());
                let mut bytes = [0; 3];
                assert_eq!(peer.recv(&mut bytes).unwrap(), 3);
                assert_eq!(&bytes, b"out");
                peer.send(b"in").unwrap();
                assert!(crate::poll_readable(raw, Duration::from_secs(1)).unwrap());
                assert_eq!(socket.recv(&mut bytes).unwrap(), 2);
                assert_eq!(&bytes[..2], b"in");
            }
            check_flags_and_drop(socket.into(), raw, flags);
        } else {
            check_refusal(result, raw, &case);
        }
    } else {
        let result = tcp_listener(descriptor);
        if matches!(case.as_str(), "tcp-v4" | "tcp-v6") {
            let listener = result.unwrap();
            assert_eq!(listener.as_raw_fd(), raw, "must adopt without duplication");
            let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            assert!(crate::poll_readable(raw, Duration::from_secs(1)).unwrap());
            let (mut accepted, _) = listener.accept().unwrap();
            client.write_all(b"x").unwrap();
            assert!(crate::poll_readable(accepted.as_raw_fd(), Duration::from_secs(1)).unwrap());
            let mut byte = [0];
            accepted.read_exact(&mut byte).unwrap();
            assert_eq!(byte, [b'x']);
            check_flags_and_drop(listener.into(), raw, flags);
        } else {
            check_refusal(result, raw, &case);
        }
    }
    drop(stream_peer);
    drop(tcp_peer);
    println!("verified and closed");
}

fn check_flags_and_drop(descriptor: OwnedFd, raw: RawFd, flags: libc::c_int) {
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
    // SAFETY: descriptor owns raw; query has no side effects.
    assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, flags);
    drop(descriptor);
    check_closed(raw);
}

fn check_refusal<T: std::fmt::Debug>(result: io::Result<T>, raw: RawFd, case: &str) {
    let error = result.expect_err("wrong descriptor admitted by typed adoption");
    FAULT.with(|slot| slot.set(None));
    if case.contains("-fault-") {
        assert_eq!(error.raw_os_error(), Some(libc::EBADF));
    } else {
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
    check_closed(raw);
    // Reuse the exact number, then drop the returned error. A stale owner must
    // not close the replacement, and refusal must not leak a duplicate fd.
    let replacement = std::fs::File::open("/dev/null").unwrap();
    let replacement: OwnedFd = if replacement.as_raw_fd() == raw {
        replacement.into()
    } else {
        // SAFETY: raw was observed closed in this single-test child; dup2
        // installs a new independently owned copy at exactly that number.
        assert_eq!(unsafe { libc::dup2(replacement.as_raw_fd(), raw) }, raw);
        // SAFETY: successful dup2 created raw, and no other owner wraps it.
        unsafe { OwnedFd::from_raw_fd(raw) }
    };
    assert_eq!(replacement.as_raw_fd(), raw);
    drop(error);
    // SAFETY: the replacement is live and owns this descriptor number.
    assert!(unsafe { libc::fcntl(raw, libc::F_GETFD) } >= 0);
}

fn check_closed(raw: RawFd) {
    // SAFETY: F_GETFD queries a number even when it is closed; child is isolated.
    assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
}

#[test]
fn inet_family_policy_covers_each_admitted_arm_and_wrong_families() {
    for family in [0, libc::AF_UNIX, libc::AF_INET, libc::AF_INET6] {
        assert_eq!(
            require_inet(family).is_ok(),
            family == libc::AF_INET || family == libc::AF_INET6
        );
    }
}

#[test]
fn each_typed_metadata_error_refuses_and_drops_the_owned_descriptor() {
    for adapter in ["dgram", "tcp"] {
        for fault in ["kind", "family", "flags"] {
            run_typed(&format!("{adapter}-fault-{fault}"));
        }
    }
    run_typed("tcp-fault-listener");
}

#[test]
fn received_typed_descriptors_keep_the_receive_fence_flags_without_repair() {
    let (sender, receiver) = UnixDatagram::pair().unwrap();
    let (datagram, peer) = UnixDatagram::pair().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    crate::rights::tests::send_rights(
        sender.as_raw_fd(),
        b"!",
        &[datagram.as_raw_fd(), listener.as_raw_fd()],
    );
    let mut receiver = crate::rights::DatagramReceiver::new(receiver);
    let mut received = receiver
        .receive_up_to(&mut [0], 2, &std::sync::Mutex::new(()))
        .unwrap();
    assert_eq!(received.descriptors.len(), 2);
    let listener_fd = received.descriptors.pop().unwrap();
    let datagram_fd = received.descriptors.pop().unwrap();
    let listener_raw = listener_fd.as_raw_fd();
    let datagram_raw = datagram_fd.as_raw_fd();
    // SAFETY: both just-received descriptors are live and uniquely owned.
    let flags = unsafe {
        [
            libc::fcntl(datagram_raw, libc::F_GETFD),
            libc::fcntl(listener_raw, libc::F_GETFD),
        ]
    };
    assert!(flags.iter().all(|value| value & libc::FD_CLOEXEC != 0));
    let datagram_adopted = unix_datagram(datagram_fd).unwrap();
    let listener_adopted = tcp_listener(listener_fd).unwrap();
    assert_eq!(
        listener_adopted.local_addr().unwrap(),
        listener.local_addr().unwrap()
    );
    assert_eq!(datagram_adopted.as_raw_fd(), datagram_raw);
    assert_eq!(listener_adopted.as_raw_fd(), listener_raw);
    // SAFETY: adoption retained ownership of the same descriptors and must not change flags.
    let after = unsafe {
        [
            libc::fcntl(datagram_raw, libc::F_GETFD),
            libc::fcntl(listener_raw, libc::F_GETFD),
        ]
    };
    assert_eq!(after, flags);
    datagram_adopted.send(b"ok").unwrap();
    assert!(crate::poll_readable(peer.as_raw_fd(), Duration::from_secs(1)).unwrap());
    let mut bytes = [0; 2];
    assert_eq!(peer.recv(&mut bytes).unwrap(), 2);
    assert_eq!(&bytes, b"ok");
    // Source handles stay alive until receipt and validation finish (the macOS
    // passed-socket lifetime rule); no ACK or production handoff is implemented here.
    drop((datagram, listener));
}

// UDP is intrinsically not listening, so the listener guard can mask a deleted
// type check. Exercise the shared type policy without any later shape checks.
#[test]
fn socket_kind_policy_is_independent_of_family_and_listening_guards() {
    let datagram = UnixDatagram::unbound().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    for (fd, actual) in [
        (datagram.as_raw_fd(), libc::SOCK_DGRAM),
        (listener.as_raw_fd(), libc::SOCK_STREAM),
    ] {
        for expected in [libc::SOCK_DGRAM, libc::SOCK_STREAM] {
            assert_eq!(require_kind(fd, expected).is_ok(), actual == expected);
        }
    }
}
