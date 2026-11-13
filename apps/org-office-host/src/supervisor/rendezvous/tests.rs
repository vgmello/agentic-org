//! The rendezvous, driven from this process: it plays the admitted child, so its own audit
//! token is the admitted one. macOS-only: the peer checks need `LOCAL_PEERTOKEN`.
#![cfg(target_os = "macos")]

use super::*;

use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;
use std::thread;

use org_office_launch::v3::{decode_handoff, HandoffFd};
use org_office_launch::DbKey;
use org_proto::GrantKey;

const WAIT: Duration = Duration::from_secs(5);
const READY: &[u8] = b"{\"v\":3,\"event\":\"ready_for_secrets\"}\n";

fn later() -> Instant {
    Instant::now() + WAIT
}

fn runtime() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("rv")
        .tempdir_in("/tmp")
        .unwrap()
}

/// This process's own token: the "admitted child" of these tests.
fn me() -> AuditToken {
    let (a, _b) = UnixStream::pair().unwrap();
    peer::audit_token(&a).unwrap()
}

fn unchanged() -> Result<(), String> {
    Ok(())
}

fn admitted(token: AuditToken) -> Admitted<'static> {
    Admitted {
        token,
        still: &unchanged,
    }
}

/// A connected pair through a real rendezvous: (the host's accepted end, the child's end).
fn connected(dir: &Path) -> (UnixStream, UnixStream) {
    let rendezvous = Rendezvous::bind(dir).unwrap();
    let child = UnixStream::connect(rendezvous.path()).unwrap();
    let host = rendezvous.accept_one(later(), || false).unwrap().unwrap();
    (host, child)
}

fn nothing_arrives(child: &UnixStream) {
    child.set_nonblocking(true).unwrap();
    let mut byte = [0u8; 1];
    let got = (&*child).read(&mut byte);
    assert!(
        matches!(&got, Err(e) if e.kind() == io::ErrorKind::WouldBlock) || matches!(got, Ok(0)),
        "the child got something: {got:?}"
    );
}

#[test]
fn the_listener_is_in_an_owner_only_folder_and_gone_after_one_accept() {
    let dir = runtime();
    let rendezvous = Rendezvous::bind(dir.path()).unwrap();
    let path = rendezvous.path().to_owned();
    assert_eq!(path.parent().unwrap(), dir.path().join("r"));
    let name = path.file_name().unwrap().to_str().unwrap();
    assert!(name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit()));
    let mode = fs::metadata(dir.path().join("r"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    let first = UnixStream::connect(&path).unwrap();
    let second = UnixStream::connect(&path).unwrap();
    let accepted = rendezvous.accept_one(later(), || false).unwrap();
    assert!(accepted.is_some());
    assert!(!path.exists(), "the listener outlived its accept");
    assert!(UnixStream::connect(&path).is_err());
    // The queued second connection was closed unanswered.
    // macOS refuses a timeout on a socket whose peer already closed (EINVAL): it reads EOF then.
    let _ = second.set_read_timeout(Some(WAIT));
    let mut byte = [0u8; 1];
    assert!(matches!((&second).read(&mut byte), Ok(0) | Err(_)));
    drop(first);
}

#[test]
fn a_folder_that_isnt_owner_only_is_refused_and_no_listener_is_made() {
    let dir = runtime();
    let r = dir.path().join("r");
    fs::create_dir(&r).unwrap();
    fs::set_permissions(&r, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Rendezvous::bind(dir.path()).is_err());
    assert_eq!(fs::read_dir(&r).unwrap().count(), 0);
    fs::remove_dir(&r).unwrap();
    std::os::unix::fs::symlink(dir.path(), &r).unwrap();
    assert!(Rendezvous::bind(dir.path()).is_err());
}

#[test]
fn an_unused_listener_is_unlinked_and_a_wait_ends() {
    let dir = runtime();
    let rendezvous = Rendezvous::bind(dir.path()).unwrap();
    let path = rendezvous.path().to_owned();
    // The child exited: the wait ends at once, with no connection.
    assert!(rendezvous.accept_one(later(), || true).unwrap().is_none());
    assert!(!path.exists());
    // No connection by the deadline: an error, promptly.
    let rendezvous = Rendezvous::bind(dir.path()).unwrap();
    let started = Instant::now();
    let deadline = started + Duration::from_millis(100);
    assert!(rendezvous.accept_one(deadline, || false).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    // Dropped unused.
    let rendezvous = Rendezvous::bind(dir.path()).unwrap();
    let path = rendezvous.path().to_owned();
    drop(rendezvous);
    assert!(!path.exists());
}

#[test]
fn the_admitted_peer_with_one_ready_line_passes() {
    let dir = runtime();
    let (host, mut child) = connected(dir.path());
    let writer = after_check(&mut child, READY);
    authenticate(&host, &admitted(me()), later()).unwrap();
    writer.join().unwrap();
}

#[test]
fn a_peer_with_another_token_is_refused_before_anything_is_read() {
    let dir = runtime();
    let (host, mut child) = connected(dir.path());
    let err = authenticate(&host, &admitted([9; 32]), later()).unwrap_err();
    assert!(err.contains("isn't the admitted server"), "{err}");
    // Nothing was read: a ready line written now is still all there.
    child.write_all(READY).unwrap();
    assert_eq!(peer::bytes_waiting(&host).unwrap(), READY.len() as u64);
    nothing_arrives(&child);
}

#[test]
fn a_child_whose_task_token_changed_is_refused() {
    let dir = runtime();
    let (host, mut child) = connected(dir.path());
    child.write_all(READY).unwrap();
    let changed = || Err::<(), _>("it changed (exec'ed) after it was checked".to_owned());
    let check = Admitted {
        token: me(),
        still: &changed,
    };
    assert!(authenticate(&host, &check, later())
        .unwrap_err()
        .contains("changed"));
    // Also after the line: the second re-check alone refuses it.
    let (host, mut child) = connected(dir.path());
    let writer = after_check(&mut child, READY);
    let calls = Mutex::new(0);
    let second_fails = || {
        let mut calls = lock(&calls);
        *calls += 1;
        if *calls > 1 {
            Err("it changed (exec'ed) after it was checked".to_owned())
        } else {
            Ok(())
        }
    };
    let check = Admitted {
        token: me(),
        still: &second_fails,
    };
    assert!(authenticate(&host, &check, later()).is_err());
    writer.join().unwrap();
    assert_eq!(*lock(&calls), 2);
}

/// The worker writes `ready_for_secrets` right after it connects, so the line is nearly always
/// waiting at the accept (198 of 200 in a probe): it is read, and checked as any other.
#[test]
fn a_ready_line_already_waiting_at_the_accept_is_read() {
    let dir = runtime();
    let rendezvous = Rendezvous::bind(dir.path()).unwrap();
    let mut child = UnixStream::connect(rendezvous.path()).unwrap();
    child.write_all(READY).unwrap();
    let host = rendezvous.accept_one(later(), || false).unwrap().unwrap();
    authenticate(&host, &admitted(me()), later()).unwrap();
}

/// The host checks with nothing buffered yet, so the line arrives after the accept's sample.
fn after_check(child: &mut UnixStream, bytes: &[u8]) -> thread::JoinHandle<()> {
    let mut child = child.try_clone().unwrap();
    let bytes = bytes.to_vec();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        child.write_all(&bytes).unwrap();
    })
}

#[test]
fn anything_after_the_ready_line_is_refused() {
    let dir = runtime();
    let (host, mut child) = connected(dir.path());
    let writer = after_check(&mut child, &[READY, b"{}\n"].concat());
    let err = authenticate(&host, &admitted(me()), later()).unwrap_err();
    writer.join().unwrap();
    assert!(err.contains("followed"), "{err}");
}

#[test]
fn another_first_line_or_none_in_time_is_refused() {
    let dir = runtime();
    let (host, mut child) = connected(dir.path());
    let writer = after_check(&mut child, b"{\"v\":3,\"event\":\"ready\"}\n");
    assert!(authenticate(&host, &admitted(me()), later()).is_err());
    writer.join().unwrap();
    let (host, mut child) = connected(dir.path());
    let writer = after_check(&mut child, &[b'x'; READY_MAX_BYTES + 1]);
    assert!(authenticate(&host, &admitted(me()), later()).is_err());
    writer.join().unwrap();
    let (host, _child) = connected(dir.path());
    let started = Instant::now();
    let deadline = started + Duration::from_millis(100);
    assert!(authenticate(&host, &admitted(me()), deadline).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
}

fn start_handoff() -> Handoff {
    Handoff::Start {
        db_key: DbKey::from_bytes([0xd1; 32]),
        grant_key: GrantKey::from_bytes([0x9a; 32]),
        fds: vec![HandoffFd::Transfer],
    }
}

/// The child's side, as the worker reads it: the header a byte at a time with `recvmsg`, then
/// the marker with exactly `fds` descriptors.
fn receive(child: &UnixStream, fds: usize) -> (Handoff, Vec<OwnedFd>) {
    let fence = Mutex::new(());
    let mut receiver = org_socket::rights::StreamReceiver::new(child.try_clone().unwrap());
    let mut line = Vec::new();
    loop {
        org_socket::poll_readable(std::os::fd::AsRawFd::as_raw_fd(child), WAIT).unwrap();
        match receiver.receive_byte(0, &fence) {
            Ok((b'\n', _)) => break,
            Ok((byte, _)) => line.push(byte),
            Err(org_socket::rights::ReceiveError::WouldBlock) => {}
            Err(e) => panic!("{e}"),
        }
    }
    let handoff = decode_handoff(&line).unwrap();
    if fds == 0 {
        return (handoff, Vec::new());
    }
    org_socket::poll_readable(std::os::fd::AsRawFd::as_raw_fd(child), WAIT).unwrap();
    let passed = receiver.receive_marker(fds, FD_MARKER, &fence).unwrap();
    (handoff, passed)
}

#[test]
fn a_start_gets_its_keys_then_exactly_one_transfer_end_and_waits_for_the_receipt() {
    let dir = runtime();
    let (host, mut child) = connected(dir.path());
    let sending = thread::spawn(move || hand_off(host, start_handoff(), later()));
    let (handoff, passed) = receive(&child, 1);
    let Handoff::Start {
        db_key, grant_key, ..
    } = handoff
    else {
        panic!("not a start");
    };
    assert!(db_key == DbKey::from_bytes([0xd1; 32]));
    assert!(*grant_key.expose_bytes() == [0x9a; 32]);
    assert_eq!(passed.len(), 1);
    assert!(
        !sending.is_finished(),
        "the host didn't wait for fds_received"
    );
    child
        .write_all(b"{\"v\":3,\"event\":\"fds_received\",\"of\":\"handoff\"}\n")
        .unwrap();
    let handed = sending.join().unwrap().unwrap();
    // The host's end reaches the worker's, one way only.
    let host_end = handed.transfer.unwrap();
    let worker_end = UnixDatagram::from(passed.into_iter().next().unwrap());
    host_end.send(b"record").unwrap();
    let mut got = [0u8; 16];
    assert_eq!(worker_end.recv(&mut got).unwrap(), 6);
    assert!(worker_end.send(b"back").is_err());
    // Later events reach the returned reader.
    child
        .write_all(b"{\"v\":3,\"event\":\"discarded\"}\n")
        .unwrap();
    let mut events = handed.events;
    assert!(matches!(
        v3::read_server_event(&mut events),
        Ok(Some(RendezvousEvent::Server(
            org_office_launch::ServerEvent::Discarded
        )))
    ));
}

/// The read timeouts used while authenticating never outlive it: a discard's reader waits as
/// long as its worker takes (a contended office lock is 10 s).
#[test]
fn an_authenticated_discard_leaves_no_read_timeout() {
    let dir = runtime();
    let (host, mut child) = connected(dir.path());
    let writer = after_check(&mut child, READY);
    authenticate(&host, &admitted(me()), later()).unwrap();
    writer.join().unwrap();
    let handoff = Handoff::Discard {
        db_key: None,
        fds: Vec::new(),
    };
    let handed = hand_off(host, handoff, later()).unwrap();
    assert_eq!(handed.events.get_ref().read_timeout().unwrap(), None);
}

/// Rule 14: the receipt wait is one 5 s deadline, however slowly a worker trickles bytes.
#[test]
fn a_trickled_receipt_still_ends_at_the_deadline() {
    let dir = runtime();
    let (host, mut child) = connected(dir.path());
    let started = Instant::now();
    let sending = thread::spawn(move || hand_off(host, start_handoff(), later()));
    let _ = receive(&child, 1);
    let trickle = thread::spawn(move || {
        for _ in 0..40 {
            if child.write_all(b" ").is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }
    });
    assert!(sending.join().unwrap().is_err());
    let took = started.elapsed();
    assert!(
        took >= ACK_TIMEOUT && took < ACK_TIMEOUT + Duration::from_secs(2),
        "{took:?}"
    );
    trickle.join().unwrap();
}

#[test]
fn a_discard_gets_its_key_and_no_descriptor_or_marker() {
    let dir = runtime();
    let (host, child) = connected(dir.path());
    let handoff = Handoff::Discard {
        db_key: None,
        fds: Vec::new(),
    };
    let handed = hand_off(host, handoff, later()).unwrap();
    assert!(handed.transfer.is_none());
    let (handoff, _) = receive(&child, 0);
    assert!(matches!(handoff, Handoff::Discard { db_key: None, .. }));
    nothing_arrives(&child);
}

/// Rule 14: a worker that never acknowledges is given up on after `ACK_TIMEOUT`, observed.
#[test]
fn no_receipt_in_time_or_another_one_fails_the_handoff() {
    let dir = runtime();
    let (host, child) = connected(dir.path());
    let started = Instant::now();
    let sending = thread::spawn(move || hand_off(host, start_handoff(), later()));
    let _ = receive(&child, 1);
    let err = sending.join().unwrap().err().unwrap();
    assert!(err.contains("fds_received"), "{err}");
    let took = started.elapsed();
    assert!(took >= ACK_TIMEOUT && took < ACK_TIMEOUT * 2, "{took:?}");

    let (host, mut child) = connected(dir.path());
    let sending = thread::spawn(move || hand_off(host, start_handoff(), later()));
    let _ = receive(&child, 1);
    child
        .write_all(b"{\"v\":3,\"event\":\"fds_received\",\"of\":\"01CHANNEL\"}\n")
        .unwrap();
    assert!(sending.join().unwrap().is_err());
}

#[test]
fn errors_and_debug_never_carry_a_key() {
    assert!(!format!("{:?}", start_handoff()).contains("9a9a"));
    assert!(!format!("{:?}", start_handoff()).contains("d1d1"));
    let dir = runtime();
    let (host, child) = connected(dir.path());
    drop(child);
    if let Err(err) = hand_off(host, start_handoff(), later()) {
        assert!(!err.contains("9a9a") && !err.contains("d1d1"), "{err}");
    }
}
