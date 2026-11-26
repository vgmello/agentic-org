use super::super::signal::Signal;
use super::*;
use crate::cloexec::SPAWN_LOCK;
use crate::sync::lock;
use org_office_launch::{ServerEvent, MAX_CHANNELS, MAX_WEB_CHANNELS};
use org_platform_host::admission::{Admission, Child};
use org_proto::{frame, OfficeSecretsKey};
use std::io;
use std::io::{BufReader, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// This test process stands in for a worker whose pid is `pid`: an admission that checks no
/// image (a debug build's) and a child handle that is never signalled or waited for. A system
/// without an admission adapter has neither, so nothing can be handed off there.
fn stand_in(pid: u32) -> io::Result<(Admission, Child)> {
    // A real program: an adapter that executes verified bytes reads it when the admission
    // is built.
    let admission = crate::server_check::ServerCheck::Skip
        .admission(&crate::test_programs::recording_worker())?;
    let child = org_platform_host::testing::admission::unowned_child(pid, None)?;
    Ok((admission, child))
}

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
) -> (org_office_launch::TransferRecord, Vec<OwnedFd>) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut buffer = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
    loop {
        match receiver.receive(&mut buffer, count, &SPAWN_LOCK) {
            Ok(received) => {
                return (
                    org_office_launch::decode_transfer_record(&buffer[..received.bytes]).unwrap(),
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
fn actual_launch_pair_handoff_transfers_owned_channel_then_revokes_and_closes_on_unknown_receipt() {
    let (host, mut worker) = UnixStream::pair().unwrap();
    host.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let worker_thread = std::thread::spawn(move || {
        frame::write_message(&mut worker, &org_office_launch::ReadyForSecrets).unwrap();
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
        let handoff = org_office_launch::decode_handoff(&header).unwrap();
        crate::secrets::zero(&mut header);
        assert!(matches!(
            handoff,
            org_office_launch::Handoff::Start {
                office_secrets_key: Some(_),
                ..
            }
        ));
        let (marker, mut fds) = byte(&mut receiver, 1);
        assert_eq!(marker, org_office_launch::FD_MARKER);
        let transfer = org_socket::rights::unix_datagram(fds.pop().unwrap()).unwrap();
        let mut transfer = org_socket::rights::DatagramReceiver::new(transfer);
        org_office_launch::write_rendezvous_event(
            &mut worker,
            &org_office_launch::RendezvousEvent::FdsReceived {
                of: "handoff".into(),
            },
        )
        .unwrap();
        let (record, mut fds) = datagram(&mut transfer, 1);
        let org_office_launch::TransferRecord::Channel {
            office_id,
            channel_id,
            ..
        } = record
        else {
            panic!("channel expected")
        };
        assert_eq!(office_id, "office");
        let channel = org_socket::rights::connected_unix_stream(fds.pop().unwrap()).unwrap();
        org_office_launch::write_rendezvous_event(
            &mut worker,
            &org_office_launch::RendezvousEvent::FdsReceived { of: channel_id },
        )
        .unwrap();
        let mut input = BufReader::new(channel.try_clone().unwrap());
        let value: serde_json::Value = frame::read_message(&mut input).unwrap().unwrap();
        assert_eq!(value, serde_json::json!({"through":"actual SCM_RIGHTS"}));
        let (record, fds) = datagram(&mut transfer, 0);
        assert!(fds.is_empty());
        assert!(
            matches!(record,org_office_launch::TransferRecord::Revoked {session_ids} if session_ids==["session"])
        );
        org_office_launch::write_rendezvous_event(
            &mut worker,
            &org_office_launch::RendezvousEvent::FdsReceived {
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
    let key = OfficeSecretsKey::from_bytes([71; 32]);
    let (admission, child) = stand_in(std::process::id()).unwrap();
    let endpoint = broker
        .handoff(
            host,
            "office",
            &admission,
            &child,
            HandoffSecrets::Start(Some(&key)),
            7,
            signals,
            Instant::now() + Duration::from_secs(3),
        )
        .unwrap();
    assert!(broker.endpoint("office").is_some());
    let (_id, mut channel, slot) = endpoint
        .open(org_office_launch::ChannelKind::Local)
        .unwrap();
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
    // Another pid than the reader's. Where no admission exists at all, nothing is handed off
    // either: the host's end closes without a byte written.
    let handed = stand_in(std::process::id() + 1).and_then(move |(admission, child)| {
        broker.handoff(
            host,
            "office",
            &admission,
            &child,
            HandoffSecrets::Start(Some(&OfficeSecretsKey::from_bytes([72; 32]))),
            1,
            signals,
            Instant::now() + Duration::from_secs(1),
        )
    });
    assert!(handed.is_err());
    assert!(frame::read_line(&mut BufReader::new(worker))
        .unwrap()
        .is_none());
}

/// The worker's side of a discard: `ready_for_secrets`, then the header bytes, one at a time,
/// with no descriptor on any of them.
pub(crate) fn discard_worker_reads_handoff(worker: &mut UnixStream) -> org_office_launch::Handoff {
    frame::write_message(worker, &org_office_launch::ReadyForSecrets).unwrap();
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
    let handoff = org_office_launch::decode_handoff(&header).unwrap();
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
        let org_office_launch::Handoff::Discard { fds } = discard_worker_reads_handoff(&mut worker)
        else {
            panic!("not a discard handoff");
        };
        assert!(fds.is_empty());
        // A discard gets no secret: `Handoff::Discard` has no field for one, and its decode
        // refuses any key field.
        // No marker and no transfer socket follow: the next bytes are the host's EOF once
        // the stream closes, never a descriptor. The worker reports on the stream.
        org_office_launch::write_rendezvous_event(
            &mut worker,
            &org_office_launch::RendezvousEvent::Server(ServerEvent::Discarded),
        )
        .unwrap();
    });
    let (signals, events) = mpsc::channel();
    let (admission, child) = stand_in(std::process::id()).unwrap();
    Broker::discard_handoff(
        host,
        &admission,
        &child,
        9,
        signals,
        Instant::now() + DEADLINE,
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
        org_office_launch::write_rendezvous_event(
            &mut worker,
            &org_office_launch::RendezvousEvent::FdsReceived {
                of: "handoff".into(),
            },
        )
        .unwrap();
        // The host may already have closed the stream at the receipt: a broken pipe here is fine.
        let _ = org_office_launch::write_rendezvous_event(
            &mut worker,
            &org_office_launch::RendezvousEvent::Server(ServerEvent::Discarded),
        );
        worker
    });
    let (signals, events) = mpsc::channel();
    let (admission, child) = stand_in(std::process::id()).unwrap();
    Broker::discard_handoff(
        host,
        &admission,
        &child,
        3,
        signals,
        Instant::now() + DEADLINE,
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
    // Another process than the one that holds the worker's end (this one): a real, living
    // process, so that the only thing wrong with the reader is who it is.
    let mut other = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let (admission, child) = stand_in(other.id()).unwrap();
    let refused = Broker::discard_handoff(
        host,
        &admission,
        &child,
        1,
        signals,
        Instant::now() + DEADLINE,
    );
    drop(child);
    other.kill().unwrap();
    other.wait().unwrap();
    assert!(refused.is_err());
    assert!(frame::read_line(&mut BufReader::new(worker))
        .unwrap()
        .is_none());
    assert!(events.try_recv().is_err());
}

#[test]
#[ignore = "root runs actual local listener; child sandbox forbids binding"]
fn accepted_stream_waits_for_delayed_ready_and_descriptor_receipt() {
    const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0";
    const OFFICE: &str = "01900000-0000-7000-8000-0000000000aa";
    let root =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    // Every start needs writer custody: a real one, with a reservation for this office.
    let custody = crate::writer::TestCustody::reserve(root.path(), INSTANCE, &[OFFICE]);
    let organization = custody.organization.clone();
    let mut broker = Broker::new(INSTANCE.into(), root.path().to_owned());
    broker.writers = Some(Arc::clone(&custody.writers));
    let rendezvous = broker.bind().unwrap();
    let path = rendezvous.path().to_owned();
    // Everything the host needs is ready before the worker connects: only `accept` itself
    // runs inside the worker's 150 ms delays.
    let (signals, _) = mpsc::channel();
    let (admission, mut child) = stand_in(std::process::id()).unwrap();
    // The stand-in child always reads as exited, so the host would take a worker that has
    // not connected yet for one that never will: the worker connects before `accept` runs.
    let (connected, on_connect) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(path).unwrap();
        connected.send(()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        // Ensure accept precedes READY. This failed immediately on macOS when
        // the accepted connected stream retained the listener's O_NONBLOCK.
        std::thread::sleep(Duration::from_millis(150));
        frame::write_message(&mut stream, &org_office_launch::ReadyForSecrets).unwrap();
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
        // The host has no key for this office: the start still goes ahead, with `null`.
        assert!(matches!(
            org_office_launch::decode_handoff(&header).unwrap(),
            org_office_launch::Handoff::Start {
                office_secrets_key: None,
                ..
            }
        ));
        crate::secrets::zero(&mut header);
        let (marker, mut fds) = byte(&mut receiver, 1);
        assert_eq!(marker, org_office_launch::FD_MARKER);
        let transfer = org_socket::rights::unix_datagram(fds.pop().unwrap()).unwrap();
        let mut transfer = org_socket::rights::DatagramReceiver::new(transfer);
        // A delayed receipt: the host's read of it must wait, not fail at once.
        std::thread::sleep(Duration::from_millis(150));
        org_office_launch::write_rendezvous_event(
            &mut stream,
            &org_office_launch::RendezvousEvent::FdsReceived {
                of: "handoff".into(),
            },
        )
        .unwrap();
        // The writer control comes next, as the real worker takes it: the record and its
        // one descriptor on the transfer socket, the host's admission first on that channel,
        // then the receipt.
        let (record, mut fds) = datagram(&mut transfer, 1);
        let org_office_launch::TransferRecord::WriterControl {
            office_id,
            channel_id,
        } = record
        else {
            panic!("writer control expected")
        };
        assert_eq!(office_id, OFFICE);
        let writer = org_socket::rights::connected_unix_stream(fds.pop().unwrap()).unwrap();
        writer.set_nonblocking(false).unwrap();
        writer
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut writer = BufReader::new(writer);
        let admission =
            org_office_launch::writer::read::<org_office_launch::writer::Admission>(&mut writer)
                .unwrap()
                .unwrap();
        assert!(admission.check());
        assert_eq!(admission.identity.instance_id, INSTANCE);
        assert_eq!(admission.identity.office_id, OFFICE);
        assert_eq!(admission.identity.organization_id, organization);
        org_office_launch::write_rendezvous_event(
            &mut stream,
            &org_office_launch::RendezvousEvent::FdsReceived { of: channel_id },
        )
        .unwrap();
        // The worker stays until the host ends the launch connection.
        assert!(frame::read_line(&mut BufReader::new(stream))
            .unwrap()
            .is_none());
        drop(writer);
    });
    on_connect.recv_timeout(Duration::from_secs(3)).unwrap();
    let endpoint = broker
        .accept(
            rendezvous,
            OFFICE,
            &admission,
            &mut child,
            HandoffSecrets::Start(None),
            1,
            signals,
        )
        .unwrap();
    assert!(endpoint.live.load(Ordering::SeqCst));
    endpoint.fail();
    worker.join().unwrap();
}

#[test]
fn a_worker_that_exits_without_connecting_ends_the_wait_at_once_as_a_crash() {
    let root =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let broker = Broker::new("instance".into(), root.path().to_owned());
    let rendezvous = broker.bind().unwrap();
    let started = Instant::now();
    let mut asked = 0;
    let error = broker
        .connect(rendezvous, started + Duration::from_secs(30), &mut || {
            asked += 1;
            true
        })
        .map(drop)
        .unwrap_err();
    // A crash (restart with backoff), not the listener's WouldBlock and not a 30 s stall.
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    assert_eq!(asked, 1);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_worker_that_connects_and_then_exits_is_still_accepted() {
    let root =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let broker = Broker::new("instance".into(), root.path().to_owned());
    let rendezvous = broker.bind().unwrap();
    let path = rendezvous.path().to_owned();
    let mut client = None;
    // The worker connects between the host's first accept and its exit check.
    let result = broker.connect(
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
    let root =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let broker = Broker::new("instance".into(), root.path().to_owned());
    let rendezvous = broker.bind().unwrap();
    let error = broker
        .connect(
            rendezvous,
            Instant::now() + Duration::from_millis(50),
            &mut || false,
        )
        .map(drop)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}

/// X5 review V2: a stopping host ends every launch handshake at once. A wait for the worker
/// to connect ends (before its own 30 s here), and so does a handshake whose worker has
/// connected: its connection is shut down, so the handshake's next read ends. After the
/// stop no handshake waits at all.
#[test]
fn a_stop_ends_the_launch_handshakes_under_way_and_every_later_one() {
    let root =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let broker = Broker::new("instance".into(), root.path().to_owned());
    let waiting = broker.bind().unwrap();
    let connected = broker.bind().unwrap();
    let path = connected.path().to_owned();
    let _worker = UnixStream::connect(&path).unwrap();
    let (stream, _deadline, handshake) = broker
        .connect(
            connected,
            Instant::now() + Duration::from_secs(30),
            &mut || false,
        )
        .unwrap();
    std::thread::scope(|scope| {
        let accepting = scope.spawn(|| {
            let started = Instant::now();
            let result = broker
                .connect(waiting, started + Duration::from_secs(30), &mut || false)
                .map(drop);
            (result, started.elapsed())
        });
        let reading = scope.spawn(|| {
            let mut byte = [0u8; 1];
            std::io::Read::read(&mut &stream, &mut byte)
        });
        std::thread::sleep(Duration::from_millis(100));
        broker.stop();
        let (result, took) = accepting.join().unwrap();
        assert_eq!(result.unwrap_err().to_string(), "the host is stopping");
        assert!(took < Duration::from_secs(5), "{took:?}");
        // The shut-down connection reads its end at once, not after its 30 s timeout.
        assert!(matches!(reading.join().unwrap(), Ok(0) | Err(_)));
    });
    drop(handshake);
    let later = broker.bind().unwrap();
    let started = Instant::now();
    let error = broker
        .connect(later, started + Duration::from_secs(30), &mut || false)
        .map(drop)
        .unwrap_err();
    assert_eq!(error.to_string(), "the host is stopping");
    assert!(started.elapsed() < Duration::from_secs(1));
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
    let (admission, child) = stand_in(std::process::id()).unwrap();
    let endpoint = broker
        .handoff(
            host,
            office,
            &admission,
            &child,
            HandoffSecrets::Start(Some(&OfficeSecretsKey::from_bytes([71; 32]))),
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
    frame::write_message(&mut worker, &org_office_launch::ReadyForSecrets).unwrap();
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
        org_office_launch::decode_handoff(&header).unwrap(),
        org_office_launch::Handoff::Start { .. }
    ));
    crate::secrets::zero(&mut header);
    let (_, mut fds) = byte(&mut receiver, 1);
    let transfer = org_socket::rights::DatagramReceiver::new(
        org_socket::rights::unix_datagram(fds.pop().unwrap()).unwrap(),
    );
    org_office_launch::write_rendezvous_event(
        &mut worker,
        &org_office_launch::RendezvousEvent::FdsReceived {
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
    let mut buffer = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
    while !stop.load(Ordering::SeqCst) {
        let received = match transfer.receive_up_to(&mut buffer, 1, &SPAWN_LOCK) {
            Ok(received) => received,
            Err(org_socket::rights::ReceiveError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(error) => panic!("{error}"),
        };
        let record = org_office_launch::decode_transfer_record(&buffer[..received.bytes]).unwrap();
        let mut fds = received.descriptors;
        let (id, recovery) = match record {
            org_office_launch::TransferRecord::Channel { channel_id, .. } => (channel_id, false),
            org_office_launch::TransferRecord::RecoveryPublicRead { channel_id, .. } => {
                (channel_id, true)
            }
            org_office_launch::TransferRecord::WriterControl { channel_id, .. } => {
                writers.push(fds.pop().unwrap());
                org_office_launch::write_rendezvous_event(
                    &mut *lock(&out),
                    &org_office_launch::RendezvousEvent::FdsReceived { of: channel_id },
                )
                .unwrap();
                continue;
            }
            org_office_launch::TransferRecord::Revoked { .. } => {
                assert!(fds.is_empty());
                continue;
            }
        };
        observed.fetch_add(1, Ordering::SeqCst);
        let stream = org_socket::rights::connected_unix_stream(fds.pop().unwrap()).unwrap();
        assert!(fds.is_empty());
        if refusing.swap(false, Ordering::SeqCst) {
            drop(stream);
            org_office_launch::write_rendezvous_event(
                &mut *lock(&out),
                &org_office_launch::RendezvousEvent::FdsRefused {
                    of: id,
                    reason: org_office_launch::RefusalReason::Capacity,
                },
            )
            .unwrap();
        } else {
            stream
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            org_office_launch::write_rendezvous_event(
                &mut *lock(&out),
                &org_office_launch::RendezvousEvent::FdsReceived { of: id.clone() },
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
    let _ = org_office_launch::write_rendezvous_event(
        &mut *lock(&out),
        &org_office_launch::RendezvousEvent::ChannelClosed { of: id },
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
fn actual_host_reserves_web_and_total_before_fd_send_and_keeps_held_fd_occupied() {
    use std::io::Read;
    let fixture = worker_fixture("office");
    let endpoint = &fixture.endpoint;
    let mut held = Vec::new();
    for _ in 0..MAX_WEB_CHANNELS {
        held.push(endpoint.open(org_office_launch::ChannelKind::Web).unwrap());
    }
    assert_eq!(fixture.transferred.load(Ordering::SeqCst), MAX_WEB_CHANNELS);
    assert!(endpoint.open(org_office_launch::ChannelKind::Web).is_err());
    assert_eq!(
        fixture.transferred.load(Ordering::SeqCst),
        MAX_WEB_CHANNELS,
        "refusal must precede FD send"
    );
    // Local callers take the ordinary slots the web allowance leaves, up to the total.
    let (_, mut local, local_slot) = endpoint
        .open(org_office_launch::ChannelKind::Local)
        .unwrap();
    let mut locals = Vec::new();
    for _ in MAX_WEB_CHANNELS + 1..MAX_CHANNELS {
        locals.push(
            endpoint
                .open(org_office_launch::ChannelKind::Local)
                .unwrap(),
        );
    }
    assert!(endpoint
        .open(org_office_launch::ChannelKind::Local)
        .is_err());
    assert_eq!(
        fixture.transferred.load(Ordering::SeqCst),
        MAX_CHANNELS,
        "refusal must precede FD send"
    );
    // Dropping permission witnesses must not free a descriptor still held by a caller.
    let (id, stream, slot) = held.pop().unwrap();
    let closed = slot.clone();
    drop((id, slot));
    assert_eq!(lock(&endpoint.channels).len(), MAX_CHANNELS);
    assert!(endpoint.open(org_office_launch::ChannelKind::Web).is_err());
    drop(stream);
    await_closed(&closed);
    let (_, replacement, witness) = endpoint.open(org_office_launch::ChannelKind::Web).unwrap();
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
    let (_, mut client, witness) = fixture
        .endpoint
        .open(org_office_launch::ChannelKind::Local)
        .unwrap();
    fixture.refuse.store(true, Ordering::SeqCst);
    assert!(fixture
        .endpoint
        .open(org_office_launch::ChannelKind::Local)
        .is_err());
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
    for _ in 1..MAX_CHANNELS {
        held.push(
            fixture
                .endpoint
                .open(org_office_launch::ChannelKind::Local)
                .unwrap(),
        );
    }
    let (host, worker) = UnixStream::pair().unwrap();
    fixture.endpoint.writer_control(worker).unwrap();
    assert_eq!(lock(&fixture.endpoint.channels).len(), MAX_CHANNELS);
    let request = org_office_launch::recovery::Request {
        version: 1,
        instance_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f607182".into(),
        office_id: "office".into(),
        organization_id: "org".into(),
        operation_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f607120".into(),
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
    while lock(&fixture.endpoint.channels).len() != MAX_CHANNELS - 1 {
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
        let mut ready = serde_json::json!({"v":1,"event":"ready_for_secrets"});
        if let Some(profile) = profile {
            ready["channel_profile"] = profile.into();
        }
        frame::write_message(&mut worker, &ready).unwrap();
        let (signals, _) = mpsc::channel();
        let (admission, child) = stand_in(std::process::id()).unwrap();
        assert!(Broker::new("instance".into(), PathBuf::new())
            .handoff(
                host,
                "office",
                &admission,
                &child,
                HandoffSecrets::Start(Some(&OfficeSecretsKey::from_bytes([71; 32]))),
                1,
                signals,
                Instant::now() + Duration::from_secs(1)
            )
            .is_err());
        assert_eq!(worker.read(&mut [0]).unwrap(), 0);
    }
}

#[test]
fn actual_concurrent_web_opens_reserve_before_send_without_overadmission_or_deadlock() {
    let fixture = worker_fixture("office");
    // At most MAX_PENDING transfers may be in flight, a cap independent of the web
    // allowance. Fill the allowance one by one down to the slots a single burst can contend
    // for without meeting that cap, so only the web cap can refuse below.
    let contested = MAX_WEB_CHANNELS.min(MAX_PENDING);
    let mut held: Vec<_> = (contested..MAX_WEB_CHANNELS)
        .map(|_| {
            fixture
                .endpoint
                .open(org_office_launch::ChannelKind::Web)
                .unwrap()
        })
        .collect();
    // More contenders than the slots left, so some must be refused.
    let contenders = contested + 8;
    let barrier = Arc::new(std::sync::Barrier::new(contenders + 1));
    let threads: Vec<_> = (0..contenders)
        .map(|_| {
            let endpoint = Arc::clone(&fixture.endpoint);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                endpoint.open(org_office_launch::ChannelKind::Web)
            })
        })
        .collect();
    barrier.wait();
    held.extend(
        threads
            .into_iter()
            .filter_map(|thread| thread.join().unwrap().ok()),
    );
    assert_eq!(held.len(), MAX_WEB_CHANNELS);
    assert_eq!(fixture.transferred.load(Ordering::SeqCst), MAX_WEB_CHANNELS);
    let local = fixture
        .endpoint
        .open(org_office_launch::ChannelKind::Local)
        .unwrap();
    assert_eq!(
        fixture.transferred.load(Ordering::SeqCst),
        MAX_WEB_CHANNELS + 1
    );
    assert_eq!(lock(&fixture.endpoint.channels).len(), MAX_WEB_CHANNELS + 1);
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
    let opener = std::thread::spawn(move || opening.open(org_office_launch::ChannelKind::Local));
    let mut receiver = org_socket::rights::DatagramReceiver::new(worker);
    let (record, mut fds) = datagram(&mut receiver, 1);
    let org_office_launch::TransferRecord::Channel { channel_id, .. } = record else {
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
    assert!(endpoint
        .open(org_office_launch::ChannelKind::Local)
        .is_err());
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
    assert!(endpoint
        .open(org_office_launch::ChannelKind::Local)
        .is_err());
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
        let (id, stream, witness) = fixture
            .endpoint
            .open(org_office_launch::ChannelKind::Local)
            .unwrap();
        let event = match violation {
            "duplicate-ack" => org_office_launch::RendezvousEvent::FdsReceived { of: id },
            "refusal-after-ack" => org_office_launch::RendezvousEvent::FdsRefused {
                of: id,
                reason: org_office_launch::RefusalReason::Capacity,
            },
            "duplicate-close" => {
                drop(stream);
                await_closed(&witness);
                org_office_launch::RendezvousEvent::ChannelClosed { of: id }
            }
            _ => org_office_launch::RendezvousEvent::ChannelClosed {
                of: "not-yet-acknowledged".into(),
            },
        };
        org_office_launch::write_rendezvous_event(&mut *lock(&fixture.events), &event).unwrap();
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

/// Darwin answers `EINVAL` to a timeout change once the peer has closed. A worker that reports
/// its failure and exits before the host changes the timeout must still have that report read,
/// then EOF, rather than the change failing as an I/O error (a crash and a restart).
#[test]
fn a_timeout_change_after_the_worker_closed_keeps_its_buffered_report() {
    let (host, mut worker) = UnixStream::pair().unwrap();
    org_office_launch::write_rendezvous_event(
        &mut worker,
        &org_office_launch::RendezvousEvent::Server(ServerEvent::Discarded),
    )
    .unwrap();
    drop(worker);
    set_timeout_unless_peer_closed(&host, Some(Duration::from_secs(3))).unwrap();
    set_timeout_unless_peer_closed(&host, None).unwrap();
    let mut reader = BufReader::new(host);
    assert!(matches!(
        org_office_launch::read_server_event(&mut reader).unwrap(),
        Some(org_office_launch::RendezvousEvent::Server(
            ServerEvent::Discarded
        ))
    ));
    assert!(org_office_launch::read_server_event(&mut reader)
        .unwrap()
        .is_none());
}

/// A worker sending one byte at a time, each well inside the socket's timeout, until `stop`.
fn trickle(mut worker: UnixStream, stop: Duration) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < stop {
            if worker.write_all(b" ").is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    })
}

/// The handoff and ready reads end at the launch's one deadline: a worker trickling bytes
/// (never a whole line) cannot hold the host past it by restarting the socket's timeout.
#[test]
fn a_trickled_receipt_still_ends_at_the_deadline() {
    let (host, worker) = UnixStream::pair().unwrap();
    // What `connect` leaves: a read timeout of what was left of the deadline.
    host.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let trickling = trickle(worker, Duration::from_secs(4));
    let (admission, child) = stand_in(std::process::id()).unwrap();
    let started = Instant::now();
    let error = Broker::ready(
        host,
        &admission,
        &child,
        started + Duration::from_millis(500),
    )
    .unwrap_err();
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}: {error}");
    trickling.join().unwrap();
}

/// The bounded reader serves what its buffer holds without a wait and leaves whatever follows
/// a line in that buffer, for the next read of the same stream.
#[test]
fn a_deadline_read_keeps_the_bytes_after_its_line() {
    let (host, mut worker) = UnixStream::pair().unwrap();
    worker.write_all(b"{\"a\":1}\n{\"b\":2}\n").unwrap();
    let mut reader = BufReader::new(host);
    let deadline = Instant::now() + Duration::from_secs(3);
    let first = frame::read_line(&mut ByDeadline::new(&mut reader, deadline)).unwrap();
    assert_eq!(first.as_deref(), Some(&b"{\"a\":1}"[..]));
    // Past the deadline, buffered bytes are still served: no read of the socket is needed.
    let second = frame::read_line(&mut ByDeadline::new(&mut reader, Instant::now())).unwrap();
    assert_eq!(second.as_deref(), Some(&b"{\"b\":2}"[..]));
    // And nothing more is read once it has passed.
    let late = frame::read_line(&mut ByDeadline::new(&mut reader, Instant::now()));
    assert!(
        matches!(late, Err(frame::FrameError::Io(ref e)) if e.kind() == io::ErrorKind::TimedOut),
        "{late:?}"
    );
}
