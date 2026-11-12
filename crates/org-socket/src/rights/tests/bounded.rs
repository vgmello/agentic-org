use super::*;

#[test]
fn bounded_receive_accepts_zero_and_one_on_the_same_socket() {
    let (sender, socket) = UnixDatagram::pair().unwrap();
    let mut receiver = DatagramReceiver::new(socket);
    let fence = Mutex::new(());
    let mut payload = [0; 1024];
    sender
        .send(br#"{"type":"revoked","session_ids":[]}"#)
        .unwrap();
    let received = receiver.receive_up_to(&mut payload, 1, &fence).unwrap();
    assert_eq!(
        &payload[..received.bytes],
        br#"{"type":"revoked","session_ids":[]}"#
    );
    assert!(received.descriptors.is_empty());
    let (source, mut peer) = UnixStream::pair().unwrap();
    peer.write_all(b"channel").unwrap();
    send_rights(sender.as_raw_fd(), b"channel-header", &[source.as_raw_fd()]);
    let received = receiver.receive_up_to(&mut payload, 1, &fence).unwrap();
    drop(source);
    assert_eq!(&payload[..received.bytes], b"channel-header");
    assert_eq!(received.descriptors.len(), 1);
    let descriptor = received.descriptors.into_iter().next().unwrap();
    assert_cloexec(&descriptor);
    let mut stream = UnixStream::from(descriptor);
    let mut bytes = [0; 7];
    stream.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"channel");
    drop(stream);
    assert_closed(peer);
    assert!(receiver.fd().is_some());
}

#[test]
fn count_policy_boundaries_preserve_exact_and_at_most_semantics() {
    for (capacity, actual, exact, bounded) in [
        (0, 0, true, true),
        (0, 1, false, false),
        (1, 0, false, true),
        (1, 1, true, true),
        (1, 2, false, false),
        (2, 1, false, true),
        (2, 2, true, true),
        (2, 3, false, false),
    ] {
        assert_eq!(DescriptorCount::Exact(capacity).capacity(), capacity);
        assert_eq!(DescriptorCount::AtMost(capacity).capacity(), capacity);
        assert_eq!(DescriptorCount::Exact(capacity).accepts(actual), exact);
        assert_eq!(DescriptorCount::AtMost(capacity).accepts(actual), bounded);
    }
}

#[test]
fn bounded_zero_empty_would_block_and_payload_boundary() {
    let (sender, socket) = UnixDatagram::pair().unwrap();
    let mut receiver = DatagramReceiver::new(socket);
    let fence = Mutex::new(());
    let mut payload = [0; 1024];
    assert!(matches!(
        receiver.receive_up_to(&mut payload, 1, &fence),
        Err(ReceiveError::WouldBlock)
    ));
    assert!(receiver.fd().is_some());
    sender.send(b"").unwrap();
    let empty = receiver.receive_up_to(&mut payload, 0, &fence).unwrap();
    assert_eq!(empty.bytes, 0);
    assert!(empty.descriptors.is_empty());
    sender.send(&[42; 1024]).unwrap();
    let full = receiver.receive_up_to(&mut payload, 0, &fence).unwrap();
    assert_eq!(full.bytes, 1024);
    assert!(full.descriptors.is_empty());
    assert_eq!(payload, [42; 1024]);
    sender.send(b"after").unwrap();
    assert_eq!(
        receiver
            .receive_up_to(&mut payload, 1, &fence)
            .unwrap()
            .bytes,
        5
    );
    assert_eq!(&payload[..5], b"after");
}

#[test]
fn bounded_capacity_order_cloexec_and_retry_use_the_shared_receiver() {
    let (sender, socket) = UnixDatagram::pair().unwrap();
    let mut originals = Vec::new();
    let mut peers = Vec::new();
    for tag in *b"AB" {
        let (source, mut peer) = UnixStream::pair().unwrap();
        peer.write_all(&[tag]).unwrap();
        originals.push(source);
        peers.push(peer);
    }
    let raw: Vec<_> = originals.iter().map(AsRawFd::as_raw_fd).collect();
    send_rights(sender.as_raw_fd(), b"!", &raw);
    let mut receiver = Receiver(Some(socket.into()));
    let fence = Mutex::new(());
    let mut calls = 0;
    let mut set_calls = 0;
    let received = receiver
        .receive_with(
            &mut [0],
            DescriptorCount::AtMost(2),
            &fence,
            |fd, message, flags| {
                calls += 1;
                assert_eq!(control_len(message), control_space(2).unwrap());
                assert_ne!(flags & libc::MSG_DONTWAIT, 0);
                #[cfg(target_os = "linux")]
                {
                    assert_ne!(flags & libc::MSG_CMSG_CLOEXEC, 0);
                    assert!(fence.try_lock().is_ok(), "Linux must not acquire the fence");
                }
                #[cfg(not(target_os = "linux"))]
                assert!(matches!(
                    fence.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                ));
                if calls == 1 {
                    message.msg_controllen = 0;
                    Err(io::Error::from_raw_os_error(libc::EINTR))
                } else {
                    system_receive(fd, message, flags)
                }
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
    drop(originals);
    assert_eq!(calls, 2);
    assert_eq!(set_calls, if cfg!(target_os = "linux") { 0 } else { 2 });
    assert!(fence.try_lock().is_ok());
    assert_eq!(received.descriptors.len(), 2);
    for (descriptor, tag) in received.descriptors.into_iter().zip(*b"AB") {
        assert_cloexec(&descriptor);
        let mut byte = [0];
        UnixStream::from(descriptor).read_exact(&mut byte).unwrap();
        assert_eq!(byte, [tag]);
    }
    for peer in peers {
        assert_closed(peer);
    }
}

// Fatal cases run in the existing isolated subprocess harness: macOS may install
// unreported fds on CTRUNC, so a fatal receiver must never continue in the parent.
pub(super) fn fatal_case(case: &str) {
    let (sender, socket) = UnixDatagram::pair().unwrap();
    let (source, peer) = UnixStream::pair().unwrap();
    let mut receiver = DatagramReceiver::new(socket);
    let mut payload = [0; 1024];
    let fence = Mutex::new(());
    let result = match case {
        "bounded-poisoned-fence" => {
            let _ = std::panic::catch_unwind(|| {
                let _guard = fence.lock().unwrap();
                panic!("inject fence poison");
            });
            let error = receiver.receive_up_to(&mut payload, 1, &fence);
            assert!(matches!(error, Err(ReceiveError::Fatal(_))));
            assert!(receiver.fd().is_none());
            return;
        }
        "bounded-zero-reported" => {
            // Exercise the zero-count guard without the earlier CTRUNC guard masking it.
            // Allocate and expose room for the actual kernel-received canary, but retain
            // the caller's zero policy. This checks reported-fd cleanup before returning.
            send_rights(sender.as_raw_fd(), b"x", &[source.as_raw_fd()]);
            receiver.0.receive_with(
                &mut payload,
                DescriptorCount::AtMost(0),
                &fence,
                |fd, message, flags| {
                    assert_eq!(control_len(message), control_space(0).unwrap());
                    message.msg_controllen = control_space(1).unwrap() as _;
                    system_receive(fd, message, flags)
                },
                close_on_exec,
                |_| control_buffer(control_space(1).unwrap()),
            )
        }
        "bounded-overflow" => receiver.receive_up_to(&mut payload, usize::MAX, &fence),
        "bounded-exact-short" => {
            sender.send(b"no-rights").unwrap();
            // An exact call must still reject fewer descriptors.
            receiver.receive(&mut payload, 1, &fence)
        }
        _ => {
            let (body, count, maximum): (&[u8], usize, usize) = match case {
                // Linux CMSG_SPACE(1 fd) fits two via padding; macOS instead
                // reaches CTRUNC. Keep this case for the Linux count guard.
                "bounded-excess" => (b"x", 2, 1),
                "bounded-control" => (b"x", 3, 1),
                "bounded-payload" => (&[42; 1025], 1, 1),
                "bounded-zero" => (b"x", 1, 0),
                _ => panic!("unknown bounded fatal case"),
            };
            let mut fds = vec![sender.as_raw_fd(); count];
            // bounded-zero proves Fatal/receiver poisoning only: macOS can
            // install an unreported fd, so no before-return closure is claimed.
            fds[0] = source.as_raw_fd();
            send_rights(sender.as_raw_fd(), body, &fds);
            receiver.receive_up_to(&mut payload, maximum, &fence)
        }
    };
    assert!(
        matches!(result, Err(ReceiveError::Fatal(ref e)) if e.kind() == io::ErrorKind::InvalidData),
        "{case}: {result:?}"
    );
    drop(source);
    if case == "bounded-zero" {
        drop(peer);
    } else {
        assert_closed(peer);
    }
    assert!(receiver.fd().is_none());
    assert!(matches!(
        receiver.receive_up_to(&mut payload, 1, &fence),
        Err(ReceiveError::Fatal(_))
    ));
}

#[cfg(not(target_os = "linux"))]
#[test]
fn bounded_public_api_uses_the_original_caller_fence() {
    run_fatal_child("bounded-poisoned-fence");
}

#[cfg(target_os = "linux")]
#[test]
fn bounded_linux_atomic_cloexec_does_not_acquire_a_poisoned_fence() {
    let fence = Mutex::new(());
    let _ = std::panic::catch_unwind(|| {
        let _guard = fence.lock().unwrap();
        panic!("inject fence poison");
    });
    let (sender, socket) = UnixDatagram::pair().unwrap();
    let (source, peer) = UnixStream::pair().unwrap();
    send_rights(sender.as_raw_fd(), b"!", &[source.as_raw_fd()]);
    let received = DatagramReceiver::new(socket)
        .receive_up_to(&mut [0], 1, &fence)
        .unwrap();
    drop(source);
    assert_eq!(received.descriptors.len(), 1);
    assert_cloexec(&received.descriptors[0]);
    drop(received);
    assert_closed(peer);
}
