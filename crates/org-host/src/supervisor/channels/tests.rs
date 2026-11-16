use super::*;

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
                )
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
    assert_eq!(*lock(&endpoint.channels), 0);
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
        )
        .unwrap();
    assert!(endpoint.live.load(Ordering::SeqCst));
    endpoint.fail();
    worker.join().unwrap();
}
