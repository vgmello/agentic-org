use super::*;
use org_office_launch::ServerEvent;

fn byte(receiver: &mut org_socket::rights::StreamReceiver, count: usize) -> (u8, Vec<OwnedFd>) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match receiver.receive_byte(count, &SPAWN_LOCK) {
            Ok(value) => return value,
            Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < deadline => {
                std::thread::yield_now()
            }
            Err(error) => panic!("{error}"),
        }
    }
}
fn datagram(
    receiver: &mut org_socket::rights::DatagramReceiver,
    count: usize,
) -> (v3::TransferRecord, Vec<OwnedFd>) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut buffer = [0; v3::TRANSFER_HEADER_MAX_BYTES];
    loop {
        match receiver.receive(&mut buffer, count, &SPAWN_LOCK) {
            Ok(received) => {
                return (
                    v3::decode_transfer_record(&buffer[..received.bytes]).unwrap(),
                    received.descriptors,
                );
            }
            Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < deadline => {
                std::thread::yield_now()
            }
            Err(error) => panic!("{error}"),
        }
    }
}

#[test]
fn actual_v3_pair_handoff_transfers_owned_channel_then_revokes_and_closes_on_unknown_receipt() {
    let (host, mut worker) = UnixStream::pair().unwrap();
    host.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let worker_thread = std::thread::spawn(move || {
        frame::write_message(&mut worker, &v3::ReadyForSecrets).unwrap();
        let mut receiver = org_socket::rights::StreamReceiver::new(worker.try_clone().unwrap());
        let mut header = Vec::new();
        loop {
            let (b, fds) = byte(&mut receiver, 0);
            assert!(fds.is_empty());
            if b == b'\n' {
                break;
            }
            header.push(b);
        }
        let handoff = v3::decode_handoff(&header).unwrap();
        crate::secrets::zero(&mut header);
        assert!(matches!(handoff, v3::Handoff::Start { .. }));
        let (marker, mut fds) = byte(&mut receiver, 1);
        assert_eq!(marker, v3::FD_MARKER);
        let transfer = org_socket::rights::unix_datagram(fds.pop().unwrap()).unwrap();
        let mut transfer = org_socket::rights::DatagramReceiver::new(transfer);
        v3::write_rendezvous_event(
            &mut worker,
            &v3::RendezvousEvent::FdsReceived {
                of: "handoff".into(),
            },
        )
        .unwrap();
        let (record, mut fds) = datagram(&mut transfer, 1);
        let v3::TransferRecord::Channel {
            office_id,
            channel_id,
            ..
        } = record
        else {
            panic!("channel expected")
        };
        assert_eq!(office_id, "office");
        let channel = org_socket::rights::connected_unix_stream(fds.pop().unwrap()).unwrap();
        v3::write_rendezvous_event(
            &mut worker,
            &v3::RendezvousEvent::FdsReceived { of: channel_id },
        )
        .unwrap();
        let mut input = BufReader::new(channel.try_clone().unwrap());
        let value: serde_json::Value = frame::read_message(&mut input).unwrap().unwrap();
        assert_eq!(value, serde_json::json!({"through":"actual SCM_RIGHTS"}));
        let (record, fds) = datagram(&mut transfer, 0);
        assert!(fds.is_empty());
        assert!(
            matches!(record,v3::TransferRecord::Revoked {session_ids} if session_ids==["session"])
        );
        v3::write_rendezvous_event(
            &mut worker,
            &v3::RendezvousEvent::FdsReceived {
                of: "not-pending".into(),
            },
        )
        .unwrap();
        // Keep worker alive until host closes the authenticated connection.
        let mut read = BufReader::new(worker);
        assert!(frame::read_line(&mut read).unwrap().is_none());
    });
    let broker = Broker::new("instance".into(), PathBuf::new());
    let (signals, events) = mpsc::channel();
    let key = DbKey::from_bytes([71; 32]);
    let endpoint = broker
        .handoff(
            host,
            "office",
            std::process::id(),
            None,
            &ServerCheck::Skip,
            &key,
            None,
            7,
            signals,
            Instant::now() + Duration::from_secs(3),
        )
        .unwrap();
    assert!(broker.endpoint("office").is_some());
    let (_id, mut channel, slot) = endpoint.open(v3::ChannelKind::Local).unwrap();
    frame::write_message(
        &mut channel,
        &serde_json::json!({"through":"actual SCM_RIGHTS"}),
    )
    .unwrap();
    endpoint.revoke("session").unwrap();
    assert!(matches!(
        events.recv_timeout(Duration::from_secs(3)).unwrap(),
        Signal::Closed { generation: 7 }
    ));
    assert!(broker.endpoint("office").is_none());
    drop(slot);
    assert_eq!(lock(&endpoint.channels).len(), 0);
    worker_thread.join().unwrap();
}

#[test]
fn a_reader_from_another_process_identity_gets_no_key_bytes() {
    let (host, worker) = UnixStream::pair().unwrap();
    worker
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let broker = Broker::new("instance".into(), PathBuf::new());
    let (signals, _) = mpsc::channel();
    assert!(broker
        .handoff(
            host,
            "office",
            std::process::id() + 1,
            None,
            &ServerCheck::Skip,
            &DbKey::from_bytes([72; 32]),
            None,
            1,
            signals,
            Instant::now() + Duration::from_secs(1)
        )
        .is_err());
    assert!(frame::read_line(&mut BufReader::new(worker))
        .unwrap()
        .is_none());
}

/// The worker's side of a discard: `ready_for_secrets`, then the header bytes, one at a time,
/// with no descriptor on any of them.
pub(crate) fn discard_worker_reads_handoff(worker: &mut UnixStream) -> v3::Handoff {
    frame::write_message(worker, &v3::ReadyForSecrets).unwrap();
    let mut receiver = org_socket::rights::StreamReceiver::new(worker.try_clone().unwrap());
    let mut header = Vec::new();
    loop {
        let (b, fds) = byte(&mut receiver, 0);
        assert!(fds.is_empty());
        if b == b'\n' {
            break;
        }
        header.push(b);
    }
    let handoff = v3::decode_handoff(&header).unwrap();
    crate::secrets::zero(&mut header);
    handoff
}

#[test]
fn a_discard_hands_off_no_key_and_no_descriptors_and_forwards_its_report() {
    let (host, mut worker) = UnixStream::pair().unwrap();
    host.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    worker
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let worker_thread = std::thread::spawn(move || {
        let v3::Handoff::Discard { db_key, fds } = discard_worker_reads_handoff(&mut worker) else {
            panic!("not a discard handoff");
        };
        assert!(fds.is_empty());
        // A discard gets no secret: the header's key is `null`.
        assert!(db_key.is_none(), "a discard handoff carried a key");
        // No marker and no transfer socket follow: the next bytes are the host's EOF once
        // the stream closes, never a descriptor. The worker reports on the stream.
        v3::write_rendezvous_event(
            &mut worker,
            &v3::RendezvousEvent::Server(ServerEvent::Discarded),
        )
        .unwrap();
    });
    let (signals, events) = mpsc::channel();
    Broker::discard_handoff(
        host,
        std::process::id(),
        None,
        &ServerCheck::Skip,
        9,
        signals,
    )
    .unwrap();
    worker_thread.join().unwrap();
    assert!(matches!(
        events.recv_timeout(Duration::from_secs(3)).unwrap(),
        Signal::Event {
            generation: 9,
            event: ServerEvent::Discarded
        }
    ));
    assert!(matches!(
        events.recv_timeout(Duration::from_secs(3)).unwrap(),
        Signal::Closed { generation: 9 }
    ));
}

#[test]
fn a_discard_stream_ends_at_a_non_server_event() {
    let (host, mut worker) = UnixStream::pair().unwrap();
    host.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    worker
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let worker_thread = std::thread::spawn(move || {
        discard_worker_reads_handoff(&mut worker);
        // A discard has no descriptor to acknowledge: a receipt is a protocol error.
        v3::write_rendezvous_event(
            &mut worker,
            &v3::RendezvousEvent::FdsReceived {
                of: "handoff".into(),
            },
        )
        .unwrap();
        v3::write_rendezvous_event(
            &mut worker,
            &v3::RendezvousEvent::Server(ServerEvent::Discarded),
        )
        .unwrap();
        worker
    });
    let (signals, events) = mpsc::channel();
    Broker::discard_handoff(
        host,
        std::process::id(),
        None,
        &ServerCheck::Skip,
        3,
        signals,
    )
    .unwrap();
    let _worker = worker_thread.join().unwrap();
    // Only the stream's end: the `discarded` after the receipt is never forwarded.
    assert!(matches!(
        events.recv_timeout(Duration::from_secs(3)).unwrap(),
        Signal::Closed { generation: 3 }
    ));
    assert!(events.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn a_discard_reader_from_another_process_identity_gets_no_header() {
    let (host, worker) = UnixStream::pair().unwrap();
    worker
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let (signals, events) = mpsc::channel();
    assert!(Broker::discard_handoff(
        host,
        std::process::id() + 1,
        None,
        &ServerCheck::Skip,
        1,
        signals,
    )
    .is_err());
    assert!(frame::read_line(&mut BufReader::new(worker))
        .unwrap()
        .is_none());
    assert!(events.try_recv().is_err());
}

#[test]
#[ignore = "root runs actual local listener; child sandbox forbids binding"]
fn accepted_stream_waits_for_delayed_ready_and_descriptor_receipt() {
    let root = tempfile::tempdir_in("/private/tmp").unwrap();
    let broker = Broker::new("instance".into(), root.path().to_owned());
    let rendezvous = broker.bind().unwrap();
    let path = rendezvous.path().to_owned();
    let worker = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        // Ensure accept precedes READY. This failed immediately on macOS when
        // the accepted connected stream retained the listener's O_NONBLOCK.
        std::thread::sleep(Duration::from_millis(150));
        frame::write_message(&mut stream, &v3::ReadyForSecrets).unwrap();
        let mut receiver = org_socket::rights::StreamReceiver::new(stream.try_clone().unwrap());
        let mut header = Vec::new();
        loop {
            let (b, fds) = byte(&mut receiver, 0);
            assert!(fds.is_empty());
            if b == b'\n' {
                break;
            }
            header.push(b);
        }
        assert!(matches!(
            v3::decode_handoff(&header).unwrap(),
            v3::Handoff::Start { .. }
        ));
        crate::secrets::zero(&mut header);
        let (marker, mut fds) = byte(&mut receiver, 1);
        assert_eq!(marker, v3::FD_MARKER);
        let _transfer = org_socket::rights::unix_datagram(fds.pop().unwrap()).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        v3::write_rendezvous_event(
            &mut stream,
            &v3::RendezvousEvent::FdsReceived {
                of: "handoff".into(),
            },
        )
        .unwrap();
        assert!(frame::read_line(&mut BufReader::new(stream))
            .unwrap()
            .is_none());
    });
    let (signals, _) = mpsc::channel();
    let endpoint = broker
        .accept(
            rendezvous,
            "office",
            std::process::id(),
            None,
            &ServerCheck::Skip,
            &DbKey::from_bytes([73; 32]),
            None,
            1,
            signals,
            &mut || false,
        )
        .unwrap();
    assert!(endpoint.live.load(Ordering::SeqCst));
    endpoint.fail();
    worker.join().unwrap();
}

#[test]
fn a_worker_that_exits_without_connecting_ends_the_wait_at_once_as_a_crash() {
    let root = tempfile::tempdir_in("/private/tmp").unwrap();
    let broker = Broker::new("instance".into(), root.path().to_owned());
    let rendezvous = broker.bind().unwrap();
    let started = Instant::now();
    let mut asked = 0;
    let error = Broker::connect(rendezvous, started + Duration::from_secs(30), &mut || {
        asked += 1;
        true
    })
    .unwrap_err();
    // A crash (restart with backoff), not the listener's WouldBlock and not a 30 s stall.
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    assert_eq!(asked, 1);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_worker_that_connects_and_then_exits_is_still_accepted() {
    let root = tempfile::tempdir_in("/private/tmp").unwrap();
    let broker = Broker::new("instance".into(), root.path().to_owned());
    let rendezvous = broker.bind().unwrap();
    let path = rendezvous.path().to_owned();
    let mut client = None;
    // The worker connects between the host's first accept and its exit check.
    let result = Broker::connect(
        rendezvous,
        Instant::now() + Duration::from_secs(30),
        &mut || {
            client = Some(UnixStream::connect(&path).unwrap());
            true
        },
    );
    assert!(result.is_ok());
    assert!(client.is_some());
}

#[test]
fn no_connection_by_the_deadline_is_a_timeout_not_would_block() {
    let root = tempfile::tempdir_in("/private/tmp").unwrap();
    let broker = Broker::new("instance".into(), root.path().to_owned());
    let rendezvous = broker.bind().unwrap();
    let error = Broker::connect(
        rendezvous,
        Instant::now() + Duration::from_millis(50),
        &mut || false,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}

/// Real host handoff/SCM_RIGHTS/event receiver with a bounded synthetic worker.
/// No image admission, domain authority or actual office implementation is claimed.
pub(crate) struct WorkerFixture {
    pub endpoint: Arc<Endpoint>,
    pub transferred: Arc<std::sync::atomic::AtomicUsize>,
    pub refuse: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    events: Arc<Mutex<UnixStream>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Drop for WorkerFixture {
    fn drop(&mut self) {
        self.endpoint.fail();
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}
pub(crate) fn worker_fixture(office: &str) -> WorkerFixture {
    let (host, worker) = UnixStream::pair().unwrap();
    host.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let stopping = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&stopping);
    let transferred = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&transferred);
    let refuse = Arc::new(AtomicBool::new(false));
    let refusing = Arc::clone(&refuse);
    let (event_sender, event_receiver) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let (transfer, out) = synthetic_worker_handoff(worker);
        event_sender.send(Arc::clone(&out)).unwrap();
        synthetic_worker_loop(transfer, out, stop, observed, refusing);
    });
    let broker = Broker::new("instance".into(), PathBuf::new());
    let (signals, _) = mpsc::channel();
    let endpoint = broker
        .handoff(
            host,
            office,
            std::process::id(),
            None,
            &ServerCheck::Skip,
            &DbKey::from_bytes([71; 32]),
            None,
            7,
            signals,
            Instant::now() + Duration::from_secs(3),
        )
        .unwrap();
    WorkerFixture {
        endpoint,
        transferred,
        refuse,
        stop: stopping,
        events: event_receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
        worker: Some(thread),
    }
}
fn synthetic_worker_handoff(
    mut worker: UnixStream,
) -> (org_socket::rights::DatagramReceiver, Arc<Mutex<UnixStream>>) {
    frame::write_message(&mut worker, &v3::ReadyForSecrets).unwrap();
    let mut receiver = org_socket::rights::StreamReceiver::new(worker.try_clone().unwrap());
    let mut header = Vec::new();
    loop {
        let (b, fds) = byte(&mut receiver, 0);
        assert!(fds.is_empty());
        if b == b'\n' {
            break;
        }
        header.push(b);
    }
    assert!(matches!(
        v3::decode_handoff(&header).unwrap(),
        v3::Handoff::Start { .. }
    ));
    crate::secrets::zero(&mut header);
    let (_, mut fds) = byte(&mut receiver, 1);
    let transfer = org_socket::rights::DatagramReceiver::new(
        org_socket::rights::unix_datagram(fds.pop().unwrap()).unwrap(),
    );
    v3::write_rendezvous_event(
        &mut worker,
        &v3::RendezvousEvent::FdsReceived {
            of: "handoff".into(),
        },
    )
    .unwrap();
    (transfer, Arc::new(Mutex::new(worker)))
}
fn synthetic_worker_loop(
    mut transfer: org_socket::rights::DatagramReceiver,
    out: Arc<Mutex<UnixStream>>,
    stop: Arc<AtomicBool>,
    observed: Arc<std::sync::atomic::AtomicUsize>,
    refusing: Arc<AtomicBool>,
) {
    let mut threads = Vec::new();
    let mut writers = Vec::new();
    let mut buffer = [0; v3::TRANSFER_HEADER_MAX_BYTES];
    while !stop.load(Ordering::SeqCst) {
        let received = match transfer.receive_up_to(&mut buffer, 1, &SPAWN_LOCK) {
            Ok(received) => received,
            Err(org_socket::rights::ReceiveError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(error) => panic!("{error}"),
        };
        let record = v3::decode_transfer_record(&buffer[..received.bytes]).unwrap();
        let mut fds = received.descriptors;
        let (id, recovery) = match record {
            v3::TransferRecord::Channel { channel_id, .. } => (channel_id, false),
            v3::TransferRecord::RecoveryPublicRead { channel_id, .. } => (channel_id, true),
            v3::TransferRecord::WriterControl { channel_id, .. } => {
                writers.push(fds.pop().unwrap());
                v3::write_rendezvous_event(
                    &mut *lock(&out),
                    &v3::RendezvousEvent::FdsReceived { of: channel_id },
                )
                .unwrap();
                continue;
            }
            v3::TransferRecord::Revoked { .. } => {
                assert!(fds.is_empty());
                continue;
            }
        };
        observed.fetch_add(1, Ordering::SeqCst);
        let stream = org_socket::rights::connected_unix_stream(fds.pop().unwrap()).unwrap();
        assert!(fds.is_empty());
        if refusing.swap(false, Ordering::SeqCst) {
            drop(stream);
            v3::write_rendezvous_event(
                &mut *lock(&out),
                &v3::RendezvousEvent::FdsRefused {
                    of: id,
                    reason: v3::RefusalReason::Capacity,
                },
            )
            .unwrap();
        } else {
            stream
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            v3::write_rendezvous_event(
                &mut *lock(&out),
                &v3::RendezvousEvent::FdsReceived { of: id.clone() },
            )
            .unwrap();
            let out = Arc::clone(&out);
            let stop = Arc::clone(&stop);
            threads.push(std::thread::spawn(move || {
                synthetic_channel(stream, recovery, id, out, stop)
            }));
        }
    }
    drop(writers);
    for thread in threads {
        thread.join().unwrap();
    }
}
fn synthetic_channel(
    mut stream: UnixStream,
    recovery: bool,
    id: String,
    out: Arc<Mutex<UnixStream>>,
    stop: Arc<AtomicBool>,
) {
    use std::io::Read;
    let mut bytes = [0; 512];
    while !stop.load(Ordering::SeqCst) {
        match stream.read(&mut bytes) {
            Ok(0) => break,
            Ok(n) if &bytes[..n] == b"not-json\n" => break,
            Ok(n) if !recovery => stream.write_all(&bytes[..n]).unwrap(),
            Ok(_) => {} // Public-recovery error fixture: consume until EOF.
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(_) => break,
        }
    }
    drop(stream);
    let _ = v3::write_rendezvous_event(
        &mut *lock(&out),
        &v3::RendezvousEvent::ChannelClosed { of: id },
    );
}
pub(crate) fn await_closed(slot: &ChannelSlot) {
    let until = Instant::now() + Duration::from_secs(3);
    while slot.is_live() {
        assert!(
            Instant::now() < until,
            "authenticated closure was not observed"
        );
        std::thread::yield_now();
    }
}
#[test]
fn actual_host_reserves_gateway_and_total_before_fd_send_and_keeps_held_fd_occupied() {
    use std::io::Read;
    let fixture = worker_fixture("office");
    let endpoint = &fixture.endpoint;
    let mut held = Vec::new();
    for _ in 0..7 {
        held.push(endpoint.open(v3::ChannelKind::Gateway).unwrap());
    }
    assert_eq!(fixture.transferred.load(Ordering::SeqCst), 7);
    assert!(endpoint.open(v3::ChannelKind::Gateway).is_err());
    assert_eq!(
        fixture.transferred.load(Ordering::SeqCst),
        7,
        "refusal must precede FD send"
    );
    let (_, mut local, local_slot) = endpoint.open(v3::ChannelKind::Local).unwrap();
    assert!(endpoint.open(v3::ChannelKind::Local).is_err());
    // Dropping permission witnesses must not free a descriptor still held by a caller.
    let (id, stream, slot) = held.pop().unwrap();
    let closed = slot.clone();
    drop((id, slot));
    assert_eq!(lock(&endpoint.channels).len(), 8);
    assert!(endpoint.open(v3::ChannelKind::Gateway).is_err());
    drop(stream);
    await_closed(&closed);
    let (_, replacement, witness) = endpoint.open(v3::ChannelKind::Gateway).unwrap();
    local.write_all(b"existing RPC").unwrap();
    local
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut result = [0; 12];
    local.read_exact(&mut result).unwrap();
    assert_eq!(&result, b"existing RPC");
    drop(replacement);
    await_closed(&witness);
    drop(local);
    await_closed(&local_slot);
    assert!(endpoint.is_live());
}
#[test]
fn actual_host_negative_ack_keeps_endpoint_and_existing_rpc_live_and_writer_independent() {
    use std::io::Read;
    let fixture = worker_fixture("office");
    let (_, mut client, witness) = fixture.endpoint.open(v3::ChannelKind::Local).unwrap();
    fixture.refuse.store(true, Ordering::SeqCst);
    assert!(fixture.endpoint.open(v3::ChannelKind::Local).is_err());
    assert_eq!(lock(&fixture.endpoint.channels).len(), 1);
    assert!(fixture.endpoint.is_live());
    client.write_all(b"still live").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = [0; 10];
    client.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"still live");
    let mut held = Vec::new();
    for _ in 1..8 {
        held.push(fixture.endpoint.open(v3::ChannelKind::Local).unwrap());
    }
    let (host, worker) = UnixStream::pair().unwrap();
    fixture.endpoint.writer_control(worker).unwrap();
    assert_eq!(lock(&fixture.endpoint.channels).len(), 8);
    let request = org_office_launch::recovery::Request {
        version: 1,
        instance_id: "instance".into(),
        office_id: "office".into(),
        organization_id: "org".into(),
        operation_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
    };
    let count = fixture.transferred.load(Ordering::SeqCst);
    assert!(fixture.endpoint.recovery_public(&request).is_err());
    assert_eq!(fixture.transferred.load(Ordering::SeqCst), count);
    drop(client);
    await_closed(&witness);
    assert!(
        fixture.endpoint.recovery_public(&request).is_err(),
        "synthetic worker supplies no package"
    );
    assert_eq!(fixture.transferred.load(Ordering::SeqCst), count + 1);
    let until = Instant::now() + Duration::from_secs(3);
    while lock(&fixture.endpoint.channels).len() != 7 {
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    assert!(fixture.endpoint.is_live());
    drop(host);
}
#[test]
fn incompatible_profile_gets_no_secret_or_descriptor() {
    use std::io::Read;
    for profile in [None, Some("other")] {
        let (host, mut worker) = UnixStream::pair().unwrap();
        worker
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut ready = serde_json::json!({"v":3,"event":"ready_for_secrets"});
        if let Some(profile) = profile {
            ready["channel_profile"] = profile.into();
        }
        frame::write_message(&mut worker, &ready).unwrap();
        let (signals, _) = mpsc::channel();
        assert!(Broker::new("instance".into(), PathBuf::new())
            .handoff(
                host,
                "office",
                std::process::id(),
                None,
                &ServerCheck::Skip,
                &DbKey::from_bytes([71; 32]),
                None,
                1,
                signals,
                Instant::now() + Duration::from_secs(1)
            )
            .is_err());
        assert_eq!(worker.read(&mut [0]).unwrap(), 0);
    }
}

#[test]
fn actual_concurrent_gateway_opens_reserve_before_send_without_overadmission_or_deadlock() {
    let fixture = worker_fixture("office");
    let barrier = Arc::new(std::sync::Barrier::new(17));
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let endpoint = Arc::clone(&fixture.endpoint);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                endpoint.open(v3::ChannelKind::Gateway)
            })
        })
        .collect();
    barrier.wait();
    let held: Vec<_> = threads
        .into_iter()
        .filter_map(|thread| thread.join().unwrap().ok())
        .collect();
    assert_eq!(held.len(), 7);
    assert_eq!(fixture.transferred.load(Ordering::SeqCst), 7);
    let local = fixture.endpoint.open(v3::ChannelKind::Local).unwrap();
    assert_eq!(fixture.transferred.load(Ordering::SeqCst), 8);
    assert_eq!(lock(&fixture.endpoint.channels).len(), 8);
    let witnesses: Vec<_> = held
        .iter()
        .map(|(_, _, slot)| slot.clone())
        .chain(std::iter::once(local.2.clone()))
        .collect();
    drop((held, local));
    for witness in witnesses {
        await_closed(&witness);
    }
    assert_eq!(lock(&fixture.endpoint.channels).len(), 0);
    assert!(fixture.endpoint.is_live());
}
#[test]
fn endpoint_failure_releases_pending_sender_copy_and_invalidates_all_witnesses() {
    use std::io::Read;
    let (endpoint, first, worker, _control) = Endpoint::for_grant_test();
    let opening = Arc::clone(&endpoint);
    let opener = std::thread::spawn(move || opening.open(v3::ChannelKind::Local));
    let mut receiver = org_socket::rights::DatagramReceiver::new(worker);
    let (record, mut fds) = datagram(&mut receiver, 1);
    let v3::TransferRecord::Channel { channel_id, .. } = record else {
        panic!("channel")
    };
    let mut owned = org_socket::rights::connected_unix_stream(fds.pop().unwrap()).unwrap();
    owned
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    assert!(lock(&endpoint.pending).contains_key(&channel_id));
    endpoint.fail();
    assert!(opener.join().unwrap().is_err());
    assert!(!first.is_live());
    assert!(lock(&endpoint.pending).is_empty());
    assert_eq!(lock(&endpoint.channels).len(), 0);
    assert_eq!(owned.read(&mut [0]).unwrap(), 0);
    assert!(endpoint.open(v3::ChannelKind::Local).is_err());
}
#[test]
fn pretransfer_pending_limit_rolls_back_physical_reservation_without_failing_endpoint() {
    let (endpoint, slot, worker, _control) = Endpoint::for_grant_test();
    let mut peers = Vec::new();
    let mut waiters = Vec::new();
    for n in 0..MAX_PENDING {
        let (descriptor, peer) = UnixStream::pair().unwrap();
        let (receipt, waiter) = mpsc::channel();
        lock(&endpoint.pending).insert(
            format!("pending-{n}"),
            Pending {
                descriptor: Arc::new(descriptor.into()),
                receipt,
                ordinary: false,
            },
        );
        peers.push(peer);
        waiters.push(waiter);
    }
    assert!(endpoint.open(v3::ChannelKind::Local).is_err());
    assert!(slot.is_live());
    assert_eq!(lock(&endpoint.channels).len(), 1);
    assert!(endpoint.is_live());
    use std::os::fd::AsRawFd;
    assert!(!org_socket::poll_readable(worker.as_raw_fd(), Duration::from_millis(10)).unwrap());
    drop((peers, waiters));
}
#[test]
fn actual_duplicate_misordered_or_post_close_lifecycle_receipts_fail_endpoint() {
    for violation in [
        "duplicate-ack",
        "refusal-after-ack",
        "duplicate-close",
        "close-before-ack",
    ] {
        let fixture = worker_fixture("office");
        let (id, stream, witness) = fixture.endpoint.open(v3::ChannelKind::Local).unwrap();
        let event = match violation {
            "duplicate-ack" => v3::RendezvousEvent::FdsReceived { of: id },
            "refusal-after-ack" => v3::RendezvousEvent::FdsRefused {
                of: id,
                reason: v3::RefusalReason::Capacity,
            },
            "duplicate-close" => {
                drop(stream);
                await_closed(&witness);
                v3::RendezvousEvent::ChannelClosed { of: id }
            }
            _ => v3::RendezvousEvent::ChannelClosed {
                of: "not-yet-acknowledged".into(),
            },
        };
        v3::write_rendezvous_event(&mut *lock(&fixture.events), &event).unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while fixture.endpoint.is_live() {
            assert!(Instant::now() < until, "{violation} did not fail Endpoint");
            std::thread::yield_now();
        }
        // fail() publishes endpoint death before completing registry invalidation.
        await_closed(&witness);
        assert!(!witness.is_live());
    }
}
