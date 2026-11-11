use super::*;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// X-02 rev15 §5.2: retain sender descriptors through receive, then drop them
// before any received-fd use. Last-reference close in flight can flush UNIX
// sockets on macOS, even with the passed peer open in a third process.
// Probe (agents repo): projects/ac/agents/ac-software-engineer-kai/
// knowledge/evidence/b1-07.2/fork-probe.rs
// Logs: /private/tmp/kai-b1072-fork-probe-{mac,linux}.log
const MARKER: u8 = 0x3a;

fn send_rights(socket: RawFd, payload: &[u8], descriptors: &[RawFd]) {
    let space = control_space(descriptors.len()).unwrap();
    let mut control = vec![0usize; space.div_ceil(mem::size_of::<usize>())];
    let mut data = libc::iovec {
        iov_base: payload.as_ptr().cast_mut().cast(),
        iov_len: payload.len(),
    };
    // SAFETY: all pointers cover live buffers; SCM_RIGHTS borrows the sender's descriptors.
    unsafe {
        let mut message: libc::msghdr = mem::zeroed();
        message.msg_iov = &mut data;
        message.msg_iovlen = 1;
        if !descriptors.is_empty() {
            message.msg_control = control.as_mut_ptr().cast();
            message.msg_controllen = space as _;
            let header = libc::CMSG_FIRSTHDR(&message);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(mem::size_of_val(descriptors) as u32) as _;
            std::ptr::copy_nonoverlapping(
                descriptors.as_ptr(),
                libc::CMSG_DATA(header).cast::<RawFd>(),
                descriptors.len(),
            );
        }
        assert_eq!(libc::sendmsg(socket, &message, 0), payload.len() as isize);
    }
}

fn assert_cloexec(descriptor: &OwnedFd) {
    // SAFETY: a live owned descriptor is inspected, not modified.
    let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFD) };
    assert!(flags >= 0);
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
}

fn assert_closed(mut peer: UnixStream) {
    assert!(
        crate::poll_readable(peer.as_raw_fd(), Duration::from_secs(1)).unwrap(),
        "reported fd leaked"
    );
    assert_eq!(peer.read(&mut [0]).unwrap(), 0, "reported fd leaked");
}

#[test]
fn impossible_control_capacity_returns_an_error_instead_of_panicking() {
    assert!(control_buffer(usize::MAX).is_err());
    assert!(close_on_exec(-1).is_err());
}

#[test]
fn stream_counts_arrival_order_cloexec_and_one_byte_boundary() {
    // X-02 §5.1/5.2: worker=1, gateway=2; the primitive accepts the caller's exact count.
    for count in 0..=3 {
        let (sender, stream) = UnixStream::pair().unwrap();
        let mut receiver = StreamReceiver::new(stream);
        let mut originals = Vec::new();
        let mut peers = Vec::new();
        for tag in 0..count {
            let (fd, mut peer) = UnixStream::pair().unwrap();
            peer.write_all(&[tag as u8]).unwrap();
            originals.push(fd);
            peers.push(peer);
        }
        let raw: Vec<_> = originals.iter().map(AsRawFd::as_raw_fd).collect();
        send_rights(sender.as_raw_fd(), &[MARKER, b'X'], &raw);
        let fence = Mutex::new(());
        let descriptors = receiver.receive_marker(count, MARKER, &fence).unwrap();
        drop(originals);
        assert_eq!(descriptors.len(), count);
        for (tag, descriptor) in descriptors.into_iter().enumerate() {
            assert_cloexec(&descriptor);
            let mut stream = UnixStream::from(descriptor);
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            assert_eq!(byte, [tag as u8], "descriptor order changed");
        }
        let (next, descriptors) = receiver.receive_byte(0, &fence).unwrap();
        assert_eq!(next, b'X', "marker receive consumed the following byte");
        assert!(descriptors.is_empty());
        for peer in peers {
            assert_closed(peer);
        }
    }
}

#[test]
fn datagram_preserves_payload_and_descriptor_order() {
    let (sender, socket) = UnixDatagram::pair().unwrap();
    let (first, mut first_peer) = UnixStream::pair().unwrap();
    let (second, mut second_peer) = UnixStream::pair().unwrap();
    first_peer.write_all(b"A").unwrap();
    second_peer.write_all(b"B").unwrap();
    send_rights(
        sender.as_raw_fd(),
        b"payload",
        &[first.as_raw_fd(), second.as_raw_fd()],
    );
    let mut receiver = DatagramReceiver::new(socket);
    let mut payload = [0; 16];
    let received = receiver.receive(&mut payload, 2, &Mutex::new(())).unwrap();
    drop((first, second));
    assert_eq!(&payload[..received.bytes], b"payload");
    assert_eq!(received.descriptors.len(), 2);
    for (descriptor, expected) in received.descriptors.into_iter().zip(*b"AB") {
        assert_cloexec(&descriptor);
        let mut byte = [0];
        UnixStream::from(descriptor).read_exact(&mut byte).unwrap();
        assert_eq!(byte, [expected]);
    }
    assert_closed(first_peer);
    assert_closed(second_peer);
}

#[test]
fn would_block_keeps_both_receivers_usable_and_empty_datagrams_are_valid() {
    let fence = Mutex::new(());
    let (mut sender, stream) = UnixStream::pair().unwrap();
    let mut receiver = StreamReceiver::new(stream);
    assert!(matches!(
        receiver.receive_byte(0, &fence),
        Err(ReceiveError::WouldBlock)
    ));
    assert!(receiver.fd().is_some());
    sender.write_all(&[MARKER]).unwrap();
    assert!(receiver
        .receive_marker(0, MARKER, &fence)
        .unwrap()
        .is_empty());
    let (sender, socket) = UnixDatagram::pair().unwrap();
    let mut receiver = DatagramReceiver::new(socket);
    assert!(matches!(
        receiver.receive(&mut [0; 1], 0, &fence),
        Err(ReceiveError::WouldBlock)
    ));
    assert!(receiver.fd().is_some());
    sender.send(b"").unwrap();
    let received = receiver.receive(&mut [0; 1], 0, &fence).unwrap();
    assert_eq!(received.bytes, 0);
    assert!(received.descriptors.is_empty());
    sender.send(b"next").unwrap();
    let mut payload = [0; 4];
    assert_eq!(receiver.receive(&mut payload, 0, &fence).unwrap().bytes, 4);
    assert_eq!(&payload, b"next");
}

#[test]
fn exact_control_capacity_nonblocking_flags_and_interrupted_retry() {
    // X-02 §5.2: CMSG_SPACE(expected * sizeof(int)), rebuilt after EINTR.
    let (sender, socket) = UnixDatagram::pair().unwrap();
    let (canary, peer) = UnixStream::pair().unwrap();
    send_rights(sender.as_raw_fd(), b"!", &[canary.as_raw_fd()]);
    let mut receiver = Receiver(Some(socket.into()));
    let mut calls = 0;
    let received = receiver
        .receive_with(
            &mut [0],
            1,
            &Mutex::new(()),
            |fd, message, flags| {
                calls += 1;
                assert_eq!(message.msg_controllen as usize, control_space(1).unwrap());
                assert_ne!(flags & libc::MSG_DONTWAIT, 0);
                #[cfg(target_os = "linux")]
                assert_ne!(flags & libc::MSG_CMSG_CLOEXEC, 0);
                if calls == 1 {
                    // Simulate a syscall changing its output count on an interrupted attempt.
                    message.msg_controllen = 0;
                    Err(io::Error::from_raw_os_error(libc::EINTR))
                } else {
                    system_receive(fd, message, flags)
                }
            },
            close_on_exec,
            control_buffer,
        )
        .unwrap();
    drop(canary);
    assert_eq!(calls, 2);
    assert_eq!(received.descriptors.len(), 1);
    assert_cloexec(&received.descriptors[0]);
    drop(received);
    assert_closed(peer);
}

#[cfg(not(target_os = "linux"))]
#[test]
fn mac_fence_covers_recvmsg_through_every_cloexec_operation() {
    // X-02 §5.2: use the caller's spawn fence, not a new unrelated private lock.
    let fence = Mutex::new(());
    let (sender, socket) = UnixDatagram::pair().unwrap();
    let (canary, peer) = UnixStream::pair().unwrap();
    send_rights(
        sender.as_raw_fd(),
        b"!",
        &[canary.as_raw_fd(), canary.as_raw_fd()],
    );
    let mut receiver = Receiver(Some(socket.into()));
    let mut set_calls = 0;
    let received = receiver
        .receive_with(
            &mut [0],
            2,
            &fence,
            |fd, message, flags| {
                assert!(matches!(
                    fence.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                ));
                system_receive(fd, message, flags)
            },
            |fd| {
                assert!(matches!(
                    fence.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                ));
                set_calls += 1;
                close_on_exec(fd)
            },
            control_buffer,
        )
        .unwrap();
    drop(canary);
    assert_eq!(set_calls, 2);
    assert!(fence.try_lock().is_ok());
    for descriptor in &received.descriptors {
        assert_cloexec(descriptor);
    }
    drop(received);
    assert_closed(peer);
}

#[test]
fn fatal_paths_close_reported_fds_poison_receiver_and_return_to_caller() {
    // X-02 §5.2: unreported macOS truncation fds require process termination.
    // Each fatal path runs in its own process; the library must return, not abort/exit.
    let cases = [
        "short",
        "extra",
        "control-truncated",
        "discard",
        "payload-truncated",
        "marker",
        "eof",
        "overflow",
        "addition-overflow",
        "macro-overflow",
        "io",
        "allocation",
        "reported-short",
        "reported-control",
        "reported-payload",
    ];
    for case in cases {
        run_fatal_child(case);
    }
    #[cfg(not(target_os = "linux"))]
    for case in ["fcntl", "poisoned-fence", "reported-fcntl"] {
        run_fatal_child(case);
    }
}

fn run_fatal_child(case: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "rights::tests::fatal_child", "--nocapture"])
        .env("ORG_SOCKET_FATAL_CASE", case)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("fatal case {case} hung: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "fatal case {case}: {output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("fatal returned; caller terminates"));
}

#[test]
fn fatal_child() {
    let Ok(case) = std::env::var("ORG_SOCKET_FATAL_CASE") else {
        return;
    };
    let fence = Mutex::new(());
    let (sender, stream) = UnixStream::pair().unwrap();
    let mut receiver = StreamReceiver::new(stream);
    let (canary, peer) = UnixStream::pair().unwrap();
    match case.as_str() {
        "marker" => {
            send_rights(sender.as_raw_fd(), b"?", &[canary.as_raw_fd()]);
            send_rights(sender.as_raw_fd(), &[MARKER], &[]);
            assert!(matches!(
                receiver.receive_marker(1, MARKER, &fence),
                Err(ReceiveError::Fatal(_))
            ));
            drop(canary);
            assert_closed(peer);
        }
        "eof" => {
            drop(sender);
            // Zero is intentional: missing EOF guard must not become a valid zero marker.
            assert!(matches!(
                receiver.receive_marker(0, 0, &fence),
                Err(ReceiveError::Fatal(_))
            ));
        }
        "overflow" | "addition-overflow" | "macro-overflow" => {
            let expected = match case.as_str() {
                "overflow" => usize::MAX,
                "addition-overflow" => usize::MAX / mem::size_of::<RawFd>(),
                _ => u32::MAX as usize / mem::size_of::<RawFd>(),
            };
            assert!(matches!(
                receiver.receive_byte(expected, &fence),
                Err(ReceiveError::Fatal(_))
            ));
        }
        "short" | "extra" | "control-truncated" => {
            let count = match case.as_str() {
                "short" => 1,
                "extra" => 2,
                _ => 3,
            };
            let expected = if case == "extra" { 1 } else { 2 };
            let mut descriptors = vec![canary.as_raw_fd(); count];
            if case != "short" {
                // The last descriptor may be unreported on macOS. Keep it distinct
                // from the canary, whose reported copies must all close before return.
                descriptors[count - 1] = sender.as_raw_fd();
            }
            send_rights(sender.as_raw_fd(), &[MARKER], &descriptors);
            assert!(matches!(
                receiver.receive_marker(expected, MARKER, &fence),
                Err(ReceiveError::Fatal(_))
            ));
            drop(canary);
            assert_closed(peer);
        }
        "discard" => {
            // Even a zero-fd/header read must detect unsolicited ancillary data.
            send_rights(sender.as_raw_fd(), &[MARKER], &[canary.as_raw_fd()]);
            assert!(matches!(
                receiver.receive_byte(0, &fence),
                Err(ReceiveError::Fatal(_))
            ));
        }
        "payload-truncated" => {
            let (sender, socket) = UnixDatagram::pair().unwrap();
            send_rights(sender.as_raw_fd(), b"XX", &[canary.as_raw_fd()]);
            let mut datagram = DatagramReceiver::new(socket);
            assert!(matches!(
                datagram.receive(&mut [0], 1, &fence),
                Err(ReceiveError::Fatal(_))
            ));
            drop(canary);
            assert_closed(peer);
            assert!(datagram.fd().is_none());
            assert!(matches!(
                datagram.receive(&mut [0], 0, &fence),
                Err(ReceiveError::Fatal(_))
            ));
            println!("fatal returned; caller terminates");
            return;
        }
        "io" => {
            let result = receiver.0.receive_with(
                &mut [0],
                0,
                &fence,
                |_, _, _| Err(io::Error::from_raw_os_error(libc::EIO)),
                close_on_exec,
                control_buffer,
            );
            assert!(matches!(result, Err(ReceiveError::Fatal(_))));
        }
        #[cfg(not(target_os = "linux"))]
        "fcntl" => {
            send_rights(
                sender.as_raw_fd(),
                &[MARKER],
                &[canary.as_raw_fd(), canary.as_raw_fd()],
            );
            let mut calls = 0;
            let result = receiver.0.receive_with(
                &mut [0],
                2,
                &fence,
                system_receive,
                |fd| {
                    calls += 1;
                    if calls == 1 {
                        close_on_exec(fd)
                    } else {
                        Err(io::Error::from_raw_os_error(libc::EIO))
                    }
                },
                control_buffer,
            );
            assert_eq!(calls, 2);
            assert!(matches!(result, Err(ReceiveError::Fatal(_))));
            drop(canary);
            assert_closed(peer);
        }
        #[cfg(not(target_os = "linux"))]
        "poisoned-fence" => {
            let _ = std::panic::catch_unwind(|| {
                let _guard = fence.lock().unwrap();
                panic!("inject fence poison");
            });
            let result = receiver.0.receive_with(
                &mut [0],
                0,
                &fence,
                |_, _, _| panic!("poisoned fence must refuse before recvmsg"),
                close_on_exec,
                control_buffer,
            );
            assert!(matches!(result, Err(ReceiveError::Fatal(_))));
        }
        "allocation" => {
            let result = receiver.0.receive_with(
                &mut [0],
                0,
                &fence,
                |_, _, _| panic!("allocation failure must refuse before recvmsg"),
                close_on_exec,
                |_| Err(io::Error::other("injected allocation failure")),
            );
            assert!(matches!(result, Err(ReceiveError::Fatal(_))));
        }
        "reported-short" | "reported-control" | "reported-payload" | "reported-fcntl" => {
            reported_cleanup(&case);
            println!("fatal returned; caller terminates");
            return;
        }
        _ => panic!("unknown fatal case"),
    }
    assert!(receiver.fd().is_none(), "fatal receiver remained usable");
    assert!(matches!(
        receiver.receive_byte(0, &fence),
        Err(ReceiveError::Fatal(_))
    ));
    println!("fatal returned; caller terminates");
}

// Only called in an isolated, single-test subprocess. Observe the actual
// recvmsg output before the receiver owns it, without closing or duplicating it.
fn reported_cleanup(case: &str) {
    let (sender, socket) = UnixDatagram::pair().unwrap();
    let (source, peer) = UnixStream::pair().unwrap();
    let expected = if case == "reported-short" { 3 } else { 2 };
    let mut descriptors = vec![source.as_raw_fd(); 2];
    if case == "reported-control" {
        descriptors.push(sender.as_raw_fd()); // unreported macOS fd is distinct
    }
    send_rights(sender.as_raw_fd(), b"XX", &descriptors);
    let mut receiver = Receiver(Some(socket.into()));
    let mut reported = Vec::new();
    let mut set_calls = 0;
    let mut payload = [0; 2];
    let length = if case == "reported-payload" { 1 } else { 2 };
    let result = receiver.receive_with(
        &mut payload[..length],
        expected,
        &Mutex::new(()),
        |fd, message, flags| {
            let bytes = system_receive(fd, message, flags)?;
            // SAFETY: this fixture sends one real SCM_RIGHTS header. Bound the
            // observed prefix by actual storage, not macOS's untruncated length.
            unsafe {
                let header = libc::CMSG_FIRSTHDR(message);
                assert!(!header.is_null());
                assert_eq!((*header).cmsg_level, libc::SOL_SOCKET);
                assert_eq!((*header).cmsg_type, libc::SCM_RIGHTS);
                let length = ((*header).cmsg_len as usize).min(message.msg_controllen as usize);
                let count = (length - libc::CMSG_LEN(0) as usize) / mem::size_of::<RawFd>();
                assert_eq!(count, 2, "cleanup must cover both reported fds");
                for index in 0..count {
                    let descriptor = libc::CMSG_DATA(header)
                        .cast::<RawFd>()
                        .add(index)
                        .read_unaligned();
                    assert!(libc::fcntl(descriptor, libc::F_GETFD) >= 0);
                    reported.push(descriptor);
                }
            }
            Ok(bytes)
        },
        |fd| {
            set_calls += 1;
            if case == "reported-fcntl" && set_calls == 2 {
                Err(io::Error::from_raw_os_error(libc::EIO))
            } else {
                close_on_exec(fd)
            }
        },
        control_buffer,
    );
    assert!(matches!(result, Err(ReceiveError::Fatal(_))));
    assert_eq!(reported.len(), 2);
    for descriptor in reported {
        // SAFETY: inspect the reported number only; never wrap or close it twice.
        assert_eq!(unsafe { libc::fcntl(descriptor, libc::F_GETFD) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    }
    drop(source);
    assert_closed(peer);
    assert!(receiver.fd().is_none());
}
