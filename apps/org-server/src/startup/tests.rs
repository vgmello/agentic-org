#![allow(unsafe_code)]

use super::*;
use org_office_launch::v3::{CreateLine, HandoffFd, OfficeLine};
use org_office_launch::{CreateOffice, DiscardOffice, StartOffice};
use org_proto::{AppToken, DbKey, GrantKey, OfficeRole, RecoveryCode};
use serde_json::{json, Value};
use std::fs::File;
use std::io::{BufReader, Read, Seek, Write};
use std::os::fd::RawFd;
use std::os::unix::net::{UnixDatagram, UnixListener};
use std::path::Path;
use std::time::Duration;

const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WAIT: Duration = Duration::from_secs(10);

fn first(path: &Path, op: FirstLine) -> HostFirstLine {
    let office = OfficeLine {
        instance_id: ID.into(),
        office_id: ID.into(),
        rendezvous: path.to_str().unwrap().into(),
    };
    match op {
        FirstLine::Create => HostFirstLine::Create(CreateLine {
            office,
            org_id: ID.into(),
            org_name: "Acme".into(),
            office_name: "HQ".into(),
            role: OfficeRole::Hq,
            owner_member_id: ID.into(),
        }),
        FirstLine::Start => HostFirstLine::Start(office),
        FirstLine::Discard => HostFirstLine::Discard(office),
    }
}

fn handoff(op: FirstLine) -> Handoff {
    match op {
        FirstLine::Create => Handoff::Create {
            db_key: DbKey::from_bytes([0xb3; 32]),
            grant_key: GrantKey::from_bytes([7; 32]),
            recovery_code: RecoveryCode::new("TEST-RECOVERY-SECRET".into()).unwrap(),
            fds: vec![HandoffFd::Transfer],
        },
        FirstLine::Start => Handoff::Start {
            db_key: DbKey::from_bytes([0xb3; 32]),
            grant_key: GrantKey::from_bytes([7; 32]),
            fds: vec![HandoffFd::Transfer],
        },
        FirstLine::Discard => Handoff::Discard {
            db_key: None,
            fds: vec![],
        },
    }
}

fn control(bytes: &[u8]) -> (Control, File) {
    control_with_fence(bytes, Arc::new(Mutex::new(())))
}

fn control_with_fence(bytes: &[u8], fence: Arc<Mutex<()>>) -> (Control, File) {
    crate::zeroed::take_secret_wipes();
    let mut from_host = tempfile::tempfile().unwrap();
    from_host.write_all(bytes).unwrap();
    from_host.rewind().unwrap();
    let output = tempfile::tempfile().unwrap();
    (
        Control::new(
            ControlPipe {
                from_host,
                to_host: output.try_clone().unwrap(),
            },
            fence,
        ),
        output,
    )
}

fn assert_header_wiped(had_data: bool) {
    assert_eq!(
        crate::zeroed::take_secret_wipes(),
        vec![crate::zeroed::SecretWipe {
            len: HANDOFF_HEADER_MAX_BYTES + 1,
            had_data,
            zeroed: true,
        }],
        "the actual handoff allocation must be wiped before it is freed"
    );
}

fn launch(line: &HostFirstLine) -> (Control, File) {
    launch_with_fence(line, Arc::new(Mutex::new(())))
}

fn launch_with_fence(line: &HostFirstLine, fence: Arc<Mutex<()>>) -> (Control, File) {
    let mut bytes = vec![];
    v3::write_first_line(&mut bytes, line).unwrap();
    // Subsequent pipe lines are never decoded, even if they look like secret commands.
    bytes.extend_from_slice(b"garbage\n{\"db_key\":\"unparsed\"}\n");
    control_with_fence(&bytes, fence)
}

fn listener() -> (tempfile::TempDir, UnixListener, std::path::PathBuf) {
    let dir = tempfile::Builder::new().prefix("ks").tempdir().unwrap();
    let path = dir.path().join("r");
    let listener = UnixListener::bind(&path).unwrap();
    (dir, listener, path)
}

fn accept(listener: UnixListener) -> (UnixStream, BufReader<UnixStream>) {
    let (stream, _) = listener.accept().unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    stream.set_write_timeout(Some(WAIT)).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    assert_eq!(
        event(&mut reader),
        json!({"v":3,"event":"ready_for_secrets"})
    );
    (stream, reader)
}

fn event(reader: &mut impl io::BufRead) -> Value {
    serde_json::from_slice(&frame::read_line(reader).unwrap().expect("event missing")).unwrap()
}

fn send_fds(stream: &UnixStream, byte: u8, fds: &[RawFd]) {
    try_send_fds(stream, byte, fds).unwrap();
}

fn try_send_fds(mut stream: &UnixStream, byte: u8, fds: &[RawFd]) -> io::Result<()> {
    if fds.is_empty() {
        return stream.write_all(&[byte]);
    }
    let mut byte = [byte];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // SAFETY: aligned control storage, valid iovec, and borrowed live descriptors.
    unsafe {
        let count = std::mem::size_of_val(fds);
        let space = if fds.is_empty() {
            0
        } else {
            libc::CMSG_SPACE(count as _) as usize
        };
        let mut storage = vec![0usize; space.div_ceil(std::mem::size_of::<usize>())];
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = storage.as_mut_ptr().cast();
        msg.msg_controllen = space as _;
        if !fds.is_empty() {
            let header = libc::CMSG_FIRSTHDR(&msg);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(count as _) as _;
            std::ptr::copy_nonoverlapping(
                fds.as_ptr().cast::<u8>(),
                libc::CMSG_DATA(header),
                count,
            );
        }
        if libc::sendmsg(stream.as_raw_fd(), &msg, 0) != 1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn stdout_is_empty(mut output: File) {
    output.rewind().unwrap();
    let mut bytes = vec![];
    output.read_to_end(&mut bytes).unwrap();
    assert!(bytes.is_empty(), "v3 wrote to stdout");
}

#[test]
fn v3_owns_cloexec_transfer_before_receipt_and_routes_events_without_overreading() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for op in [FirstLine::Create, FirstLine::Start] {
        let (_dir, listener, path) = listener();
        let (mut control, output) = launch(&first(&path, op));
        let (transfer, host_end) = UnixDatagram::pair().unwrap();
        let sender = std::thread::spawn(move || {
            let (mut stream, mut reader) = accept(listener);
            stream
                .write_all(&v3::encode_handoff(&handoff(op)).unwrap())
                .unwrap();
            // The complete header grants no receipt: ownership requires the marker/fd.
            assert!(
                !org_socket::poll_readable(stream.as_raw_fd(), Duration::from_millis(100)).unwrap()
            );
            send_fds(&stream, v3::FD_MARKER, &[transfer.as_raw_fd()]);
            assert_eq!(
                event(&mut reader),
                json!({"v":3,"event":"fds_received","of":"handoff"})
            );
            drop(transfer); // The real Mac topology: only the worker owns this end now.
            host_end.send(b"owned").unwrap();
            stream.write_all(b"Z").unwrap();
            assert_eq!(
                event(&mut reader),
                json!({"v":3,"event":"ready","socket":"/test","pid":123})
            );
        });
        let request = control.read(op).unwrap().unwrap();
        assert_header_wiped(true);
        match (request, op) {
            (
                Request::V3 {
                    line: HostFirstLine::Create(_),
                    handoff: Handoff::Create { .. },
                },
                FirstLine::Create,
            )
            | (
                Request::V3 {
                    line: HostFirstLine::Start(_),
                    handoff: Handoff::Start { .. },
                },
                FirstLine::Start,
            ) => {}
            _ => panic!("wrong typed startup"),
        }
        let received = control
            .transfer
            .take()
            .expect("retained transfer before receipt");
        assert!(crate::fdswap::is_cloexec(received.as_raw_fd()));
        let received = UnixDatagram::from(received);
        received.set_read_timeout(Some(WAIT)).unwrap();
        let mut bytes = [0; 5];
        assert_eq!(received.recv(&mut bytes).unwrap(), 5);
        assert_eq!(&bytes, b"owned");
        // The header reader stops before the marker, and the marker reader stops after it.
        control
            .rendezvous
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .read_exact(&mut bytes[..1])
            .unwrap();
        assert_eq!(bytes[0], b'Z');
        control
            .emit(&ServerEvent::Ready {
                socket: "/test".into(),
                pid: 123,
            })
            .unwrap();
        sender.join().unwrap();
        stdout_is_empty(output);
    }
}

#[test]
fn discard_gets_no_transfer_or_receipt_and_events_still_use_rendezvous() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for key in [false, true] {
        let (_dir, listener, path) = listener();
        let (mut control, output) = launch(&first(&path, FirstLine::Discard));
        let sender = std::thread::spawn(move || {
            let (mut stream, mut reader) = accept(listener);
            let header = Handoff::Discard {
                db_key: key.then(|| DbKey::from_bytes([0xb3; 32])),
                fds: vec![],
            };
            stream
                .write_all(&v3::encode_handoff(&header).unwrap())
                .unwrap();
            // No marker: requiring one deadlocks. No receipt: first event is discarded.
            assert_eq!(event(&mut reader), json!({"v":3,"event":"discarded"}));
        });
        assert!(
            matches!(control.read(FirstLine::Discard).unwrap(), Some(Request::V3 {
            handoff: Handoff::Discard { db_key, .. }, ..
        }) if db_key.is_some() == key)
        );
        assert_header_wiped(true);
        assert!(control.transfer.is_none());
        assert!(!control.failed_rights());
        control.emit(&ServerEvent::Discarded).unwrap();
        sender.join().unwrap();
        stdout_is_empty(output);
    }
}

#[test]
fn v2_and_empty_first_lines_keep_the_legacy_transport() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for message in [
        HostMessage::Create(CreateOffice {
            org_id: ID.into(),
            org_name: "Acme".into(),
            office_id: ID.into(),
            office_name: "HQ".into(),
            role: OfficeRole::Hq,
            app_token: AppToken::from_bytes([7; 32]),
            db_key: DbKey::from_bytes([0xb3; 32]),
        }),
        HostMessage::Start(StartOffice {
            app_token: AppToken::from_bytes([7; 32]),
            db_key: DbKey::from_bytes([0xb3; 32]),
        }),
        HostMessage::Discard(DiscardOffice { db_key: None }),
    ] {
        let op = match &message {
            HostMessage::Create(_) => FirstLine::Create,
            HostMessage::Start(_) => FirstLine::Start,
            _ => FirstLine::Discard,
        };
        let mut bytes = vec![];
        org_office_launch::write_host_message(&mut bytes, &message).unwrap();
        bytes.extend_from_slice(b"garbage\n");
        let (mut control, mut output) = control(&bytes);
        assert!(
            matches!(control.read(op).unwrap(), Some(Request::V2(actual)) if actual == message)
        );
        assert!(crate::zeroed::take_secret_wipes().is_empty());
        assert!(control.rendezvous.is_none());
        assert!(control.transfer.is_none());
        assert!(!control.failed_rights());
        control.emit(&ServerEvent::Discarded).unwrap();
        output.rewind().unwrap();
        let mut output = BufReader::new(output);
        assert_eq!(
            org_office_launch::read_server_event(&mut output).unwrap(),
            Some(ServerEvent::Discarded)
        );
    }
    let (mut empty, _) = control(b"");
    assert!(empty.read(FirstLine::Start).unwrap().is_none());
}

// Rights faults may install unreported Mac descriptors: they must be exercised in a process
// which exits on any startup error, never in a process that continues serving other work.
#[test]
fn handoff_fault_child() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let Ok(case) = std::env::var("KAI_STARTUP_FAULT") else {
        return;
    };
    let (_dir, listener, path) = listener();
    let fence = Arc::new(Mutex::new(()));
    let (mut control, output) =
        launch_with_fence(&first(&path, FirstLine::Create), Arc::clone(&fence));
    let expected_case = case.clone();
    let (decoded, header_consumed) = std::sync::mpsc::sync_channel(0);
    let (resume, marker_queued) = std::sync::mpsc::sync_channel(0);
    if case == "poison_fence_marker" {
        // Observe the actual post-header boundary, so the header receive cannot mask
        // a replacement fence at the separate fd-bearing marker site.
        control.before_marker = Some(Box::new(move || {
            decoded.send(()).unwrap();
            marker_queued.recv_timeout(WAIT).unwrap();
        }));
    }
    let sender = std::thread::spawn(move || {
        let (mut stream, _reader) = accept(listener);
        let (fd, _other) = UnixDatagram::pair().unwrap();
        let mut header = v3::encode_handoff(&handoff(FirstLine::Create)).unwrap();
        match case.as_str() {
            "poison_fence" => {
                // Configure the host and receive Ready before poisoning. No header byte is
                // sent until the shared fence is poisoned, irrespective of worker scheduling.
                #[cfg(target_os = "macos")]
                let _ = std::panic::catch_unwind(|| {
                    let _guard = fence.lock().unwrap();
                    panic!("synthetic poisoned receive/spawn fence");
                });
                let _ = stream.write_all(&header);
                let _ = try_send_fds(&stream, v3::FD_MARKER, &[fd.as_raw_fd()]);
            }
            "poison_fence_marker" => {
                stream.write_all(&header).unwrap();
                header_consumed.recv_timeout(WAIT).unwrap();
                #[cfg(target_os = "macos")]
                let _ = std::panic::catch_unwind(|| {
                    let _guard = fence.lock().unwrap();
                    panic!("poison original caller fence only after complete header");
                });
                send_fds(&stream, v3::FD_MARKER, &[fd.as_raw_fd()]);
                resume.send(()).unwrap();
            }
            "early_fd" => {
                send_fds(&stream, header[0], &[fd.as_raw_fd()]);
                let _ = stream.write_all(&header[1..]);
                // A later correct marker makes silent loss of the early fd observable:
                // an ordinary-read header mutant would otherwise fail only at marker EOF.
                let _ = try_send_fds(&stream, v3::FD_MARKER, &[fd.as_raw_fd()]);
            }
            "ctrunc_header" => {
                stream.write_all(&header[..16]).unwrap();
                send_fds(&stream, header[16], &[fd.as_raw_fd(); 8]);
                let _ = stream.write_all(&header[17..]);
            }
            "eof_header" => {
                stream.write_all(b"{\"v\":3").unwrap();
            }
            "oversize" => {
                let _ = stream.write_all(&vec![b'x'; HANDOFF_HEADER_MAX_BYTES + 1]);
            }
            "wrong_op" => {
                stream
                    .write_all(&v3::encode_handoff(&handoff(FirstLine::Start)).unwrap())
                    .unwrap();
                let _ = try_send_fds(&stream, v3::FD_MARKER, &[fd.as_raw_fd()]);
            }
            "bad_version" => {
                header[5] = b'4';
                stream.write_all(&header).unwrap();
            }
            "unknown_field" => {
                header.pop();
                header.pop();
                header.extend_from_slice(b",\"extra\":true}\n");
                stream.write_all(&header).unwrap();
            }
            "wrong_fds" => {
                let mut value: Value = serde_json::from_slice(&header).unwrap();
                value["fds"] = json!([]);
                writeln!(stream, "{value}").unwrap();
            }
            "missing_key" => {
                let mut value: Value = serde_json::from_slice(&header).unwrap();
                value.as_object_mut().unwrap().remove("grant_key");
                writeln!(stream, "{value}").unwrap();
            }
            "secret_in_invalid_op" => {
                let mut value: Value = serde_json::from_slice(&header).unwrap();
                value["op"] = json!("TEST-RECOVERY-SECRET");
                writeln!(stream, "{value}").unwrap();
            }
            _ => {
                stream.write_all(&header).unwrap();
                match case.as_str() {
                    "eof_marker" => {}
                    "zero_fds" => send_fds(&stream, v3::FD_MARKER, &[]),
                    "extra_fds" => {
                        send_fds(&stream, v3::FD_MARKER, &[fd.as_raw_fd(), fd.as_raw_fd()])
                    }
                    "ctrunc" => send_fds(&stream, v3::FD_MARKER, &[fd.as_raw_fd(); 8]),
                    "wrong_marker" => send_fds(&stream, 9, &[fd.as_raw_fd()]),
                    _ => panic!("unknown test case"),
                }
            }
        }
        if let Err(error) = stream.shutdown(std::net::Shutdown::Write) {
            // A poisoned fence can refuse before the host finishes writing.
            assert_eq!(error.kind(), io::ErrorKind::NotConnected);
        }
        // Keep every sender copy until the worker ends the connection on refusal.
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(0) => {}
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
            other => panic!("startup refusal emitted bytes or failed to close: {other:?}"),
        }
    });
    let result = control.read(FirstLine::Create);
    // Early fd/header-fence refusals observe Drop only: no secret bytes arrived.
    // Every later case, including marker-fence refusal, observes an actual populated buffer.
    assert_header_wiped(!matches!(
        expected_case.as_str(),
        "early_fd" | "poison_fence"
    ));
    assert!(result.is_err(), "accepted malformed/faulted handoff");
    assert_eq!(
        control.failed_rights(),
        matches!(
            expected_case.as_str(),
            "poison_fence"
                | "poison_fence_marker"
                | "early_fd"
                | "ctrunc_header"
                | "eof_header"
                | "eof_marker"
                | "zero_fds"
                | "extra_fds"
                | "ctrunc"
                | "wrong_marker"
        ),
        "fatal receiver classification: {expected_case}"
    );
    let error = result.err().unwrap();
    if expected_case == "bad_version" {
        assert!(matches!(
            error,
            LaunchError::UnsupportedVersion { found: 4, .. }
        ));
    }
    let error = error.to_string();
    for secret in ["TEST-RECOVERY-SECRET", "b3b3b3b3", "070707070707"] {
        assert!(!error.contains(secret), "secret-bearing decoder diagnostic");
    }
    stdout_is_empty(output);
    drop(control);
    sender.join().unwrap();
    std::process::exit(0);
}

#[test]
fn malformed_headers_and_descriptor_faults_end_startup_without_events() {
    for case in [
        #[cfg(target_os = "macos")]
        "poison_fence",
        #[cfg(target_os = "macos")]
        "poison_fence_marker",
        "early_fd",
        "ctrunc_header",
        "eof_header",
        "oversize",
        "wrong_op",
        "bad_version",
        "unknown_field",
        "wrong_fds",
        "missing_key",
        "secret_in_invalid_op",
        "eof_marker",
        "zero_fds",
        "extra_fds",
        "ctrunc",
        "wrong_marker",
    ] {
        let mut child = crate::transfer::tests::spawn_process(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "startup::tests::handoff_fault_child",
                    "--nocapture",
                ])
                .env("KAI_STARTUP_FAULT", case)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped()),
        )
        .unwrap();
        let start = std::time::Instant::now();
        let _spawn = crate::transfer::tests::without_process_spawn();
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if start.elapsed() > WAIT * 2 {
                child.kill().unwrap();
                panic!("startup fault did not terminate: {case}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{case}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn handoff_header_accepts_exact_cap_and_refuses_one_byte_more() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    assert_eq!(HANDOFF_HEADER_MAX_BYTES, 1024);
    for size in [1024, 1025] {
        let (_dir, listener, path) = listener();
        let (mut control, output) = launch(&first(&path, FirstLine::Discard));
        let sender = std::thread::spawn(move || {
            let (mut stream, mut reader) = accept(listener);
            let mut header = v3::encode_handoff(&handoff(FirstLine::Discard)).unwrap();
            header.pop();
            // Trailing JSON whitespace is legal; the only difference is the frame size.
            header.resize(size, b' ');
            header.push(b'\n');
            let _ = stream.write_all(&header);
            if size == 1024 {
                assert_eq!(event(&mut reader), json!({"v":3,"event":"discarded"}));
            }
        });
        let result = control.read(FirstLine::Discard);
        assert_header_wiped(true);
        if size == 1024 {
            assert!(matches!(
                result,
                Ok(Some(Request::V3 {
                    handoff: Handoff::Discard { .. },
                    ..
                }))
            ));
            control.emit(&ServerEvent::Discarded).unwrap();
        } else {
            assert!(matches!(
                result,
                Err(LaunchError::Frame(FrameError::TooLarge))
            ));
        }
        drop(control);
        sender.join().unwrap();
        stdout_is_empty(output);
    }
}

#[test]
fn first_line_refusal_precedes_connect_and_connection_failures_are_propagated() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (_dir, listener, path) = listener();
    drop(listener);
    let (mut control, _) = launch(&first(&path, FirstLine::Start));
    assert!(matches!(
        control.read(FirstLine::Discard),
        Err(LaunchError::Invalid(_))
    ));
    assert!(control.rendezvous.is_none());
    let (mut control, _) = launch(&first(&path, FirstLine::Start));
    assert!(matches!(
        control.read(FirstLine::Start),
        Err(LaunchError::Frame(FrameError::Io(_)))
    ));
    assert!(control.rendezvous.is_none());
}

#[test]
fn receive_waits_after_would_block_and_fatal_diagnostics_are_fixed() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (stream, mut peer) = UnixStream::pair().unwrap();
    peer.write_all(b"ready").unwrap();
    let mut calls = 0;
    let answer = receive(&stream, || {
        calls += 1;
        if calls == 1 {
            Err(ReceiveError::WouldBlock)
        } else {
            Ok(7)
        }
    })
    .unwrap();
    assert_eq!(answer, 7);
    assert_eq!(calls, 2);
    let error = receive::<()>(&stream, || {
        Err(ReceiveError::Fatal(io::Error::other("SECRET")))
    })
    .unwrap_err();
    assert_eq!(error.to_string(), "invalid descriptor handoff");
}

#[test]
fn event_writer_errors_are_propagated_on_both_transports() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let input = tempfile::tempfile().unwrap();
    let output = tempfile::NamedTempFile::new().unwrap();
    let mut control = Control::new(
        ControlPipe {
            from_host: input,
            to_host: File::open(output.path()).unwrap(),
        },
        Arc::new(Mutex::new(())),
    );
    assert!(matches!(
        control.emit(&ServerEvent::Discarded),
        Err(LaunchError::Frame(FrameError::Io(_)))
    ));
    let (stream, peer) = UnixStream::pair().unwrap();
    // A concurrent fork may briefly retain peer despite CLOEXEC. Shutdown is shared
    // by inherited copies and deterministically forces this writer's error path.
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    drop(peer);
    control.rendezvous = Some(Arc::new(Mutex::new(stream)));
    assert!(matches!(
        control.emit(&ServerEvent::Discarded),
        Err(LaunchError::Frame(FrameError::Io(_)))
    ));
}

#[test]
fn interrupted_polls_retry_but_other_poll_errors_end_reception() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let calls = std::cell::Cell::new(0);
    let polls = std::cell::Cell::new(0);
    assert_eq!(
        receive_with_poll(
            || {
                calls.set(calls.get() + 1);
                if calls.get() < 4 {
                    Err(ReceiveError::WouldBlock)
                } else {
                    Ok(7)
                }
            },
            || {
                polls.set(polls.get() + 1);
                match polls.get() {
                    1 => Err(io::Error::from(io::ErrorKind::Interrupted)),
                    2 => Ok(false),
                    _ => Ok(true),
                }
            }
        )
        .unwrap(),
        7
    );
    assert_eq!((calls.get(), polls.get()), (4, 3));
    let mut calls = 0;
    let error = receive_with_poll(
        || {
            calls += 1;
            if calls == 1 {
                Err(ReceiveError::WouldBlock)
            } else {
                Ok(7)
            }
        },
        || Err(io::Error::from(io::ErrorKind::PermissionDenied)),
    )
    .unwrap_err();
    assert!(
        matches!(error, LaunchError::Frame(FrameError::Io(ref e)) if e.kind() == io::ErrorKind::PermissionDenied)
    );
    assert_eq!(calls, 1);
}

#[test]
fn handoff_framing_uses_the_move_one_header_cap_vector() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let vectors: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/org-office-launch/test-vectors/v3/transfer-records.json"
    )))
    .unwrap();
    assert_eq!(vectors["header_max_bytes"], HANDOFF_HEADER_MAX_BYTES);
    for (section, size) in [("records", 1024), ("refused", 1025)] {
        let wire = vectors[section]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["bytes"] == size)
            .unwrap()["wire"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(wire.len(), size);
        let (_dir, listener, path) = listener();
        let (mut control, output) = launch(&first(&path, FirstLine::Discard));
        let sender = std::thread::spawn(move || {
            let (mut stream, _) = accept(listener);
            writeln!(stream, "{wire}").unwrap();
        });
        let result = control.read(FirstLine::Discard);
        assert_header_wiped(true);
        if size == 1024 {
            // The record fits framing; it is not a handoff and is refused by its decoder.
            assert!(matches!(
                result,
                Err(LaunchError::Invalid("invalid secret handoff"))
            ));
        } else {
            assert!(matches!(
                result,
                Err(LaunchError::Frame(FrameError::TooLarge))
            ));
        }
        sender.join().unwrap();
        stdout_is_empty(output);
    }
}

#[test]
fn unparsed_drain_retries_interrupts_and_stops_at_eof_or_error() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    struct Script {
        steps: std::collections::VecDeque<io::Result<usize>>,
        reads: usize,
    }
    impl Read for Script {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            self.reads += 1;
            self.steps
                .pop_front()
                .expect("drain read after its terminal result")
        }
    }
    for last in [Ok(0), Err(io::Error::from(io::ErrorKind::PermissionDenied))] {
        let mut script = Script {
            steps: std::collections::VecDeque::from([
                Err(io::Error::from(io::ErrorKind::Interrupted)),
                Ok(7),
                last,
            ]),
            reads: 0,
        };
        drain(&mut script);
        assert_eq!(script.reads, 3);
        assert!(script.steps.is_empty());
    }
}

#[test]
fn lifetime_probe_child() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let Ok(case) = std::env::var("KAI_LIFETIME_PROBE") else {
        return;
    };
    let (mut stream, mut peer) = UnixStream::pair().unwrap();
    let fence = Arc::new(Mutex::new(()));
    let mut control = Control::new(
        ControlPipe {
            from_host: tempfile::tempfile().unwrap(),
            to_host: tempfile::tempfile().unwrap(),
        },
        Arc::clone(&fence),
    );
    control.rendezvous = Some(Arc::new(Mutex::new(stream.try_clone().unwrap())));
    control.receiver = Some(StreamReceiver::new(stream.try_clone().unwrap()));
    let mut lifetime = control.lifetime().unwrap();
    if case == "empty" {
        peer.write_all(b"X").unwrap();
        assert_eq!(lifetime.read(&mut []).unwrap(), 0);
        drop(lifetime);
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [b'X']);
    } else if case == "poison" {
        #[cfg(target_os = "macos")]
        {
            let original = Arc::clone(&fence);
            assert!(std::thread::spawn(move || {
                let _held = original.lock().unwrap();
                panic!("poison original caller fence after handoff");
            })
            .join()
            .is_err());
            peer.shutdown(std::net::Shutdown::Write).unwrap();
            let _ = lifetime.read(&mut [0]);
            // A replacement fence would accept EOF and incorrectly reach exit0 below.
        }
        #[cfg(not(target_os = "macos"))]
        panic!("poisoned receive/spawn fence is a macOS-only probe");
    } else {
        assert_eq!(case, "drop");
        drop(control);
        let _ = peer.write_all(b"X");
        assert_eq!(lifetime.read(&mut [0]).unwrap(), 0);
    }
    std::process::exit(0);
}

#[test]
fn lifetime_empty_reads_preserve_bytes_and_control_drop_wakes_the_reader() {
    {
        let _spawn = crate::transfer::tests::without_process_spawn();
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"X").unwrap();
        file.rewind().unwrap();
        let mut pipe = Lifetime::Pipe(file);
        assert_eq!(pipe.read(&mut []).unwrap(), 0);
        let mut byte = [0];
        assert_eq!(pipe.read(&mut byte).unwrap(), 1);
        assert_eq!(byte, [b'X']);
    }
    for case in [
        ("empty", 0),
        ("drop", 0),
        #[cfg(target_os = "macos")]
        ("poison", 1),
    ] {
        let mut child = crate::transfer::tests::spawn_process(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "startup::tests::lifetime_probe_child",
                    "--nocapture",
                ])
                .env("KAI_LIFETIME_PROBE", case.0)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped()),
        )
        .unwrap();
        let start = std::time::Instant::now();
        let _spawn = crate::transfer::tests::without_process_spawn();
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if start.elapsed() > WAIT {
                let _ = child.kill();
                let _ = child.wait();
                panic!("lifetime probe hung: {}", case.0);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.code() == Some(case.1),
            "{}: {:?} {}",
            case.0,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn only_eof_and_connection_reset_are_host_disappearance_not_protocol_or_other_io_faults() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for kind in [
        io::ErrorKind::UnexpectedEof,
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::InvalidData,
        io::ErrorKind::BrokenPipe,
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::NotConnected,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::Interrupted,
        io::ErrorKind::WouldBlock,
        io::ErrorKind::Other,
    ] {
        assert_eq!(
            host_disappeared(kind),
            matches!(
                kind,
                io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
            ),
            "{kind:?}"
        );
    }
}

#[test]
fn transfer_is_taken_once_with_the_same_owned_fd_and_spawn_fence() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (host, child) = std::os::unix::net::UnixDatagram::pair().unwrap();
    let raw = child.as_raw_fd();
    let fence = Arc::new(Mutex::new(()));
    let (mut control, _events) = control(b"");
    control.transfer = Some(child.into());
    control.fence = Arc::clone(&fence);
    let transferred = control.transfer().unwrap();
    assert!(control.transfer().is_none());
    assert_eq!(transferred.receiver.fd().unwrap().as_raw_fd(), raw);
    assert!(Arc::ptr_eq(&transferred.fence, &fence));
    drop(transferred);
    assert!(host.send(b"closed").is_err());
}

#[test]
fn real_rendezvous_eof_and_unread_peer_close_have_measured_platform_outcomes() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for unread in [false, true] {
        let (host, mut worker) = UnixStream::pair().unwrap();
        if unread {
            worker.write_all(b"unread worker event").unwrap();
        }
        drop(host);
        let mut receiver = StreamReceiver::new(worker);
        let deadline = std::time::Instant::now() + WAIT;
        let error = loop {
            match receiver.receive_byte(0, &Mutex::new(())) {
                Err(ReceiveError::Fatal(error)) => break error,
                Err(ReceiveError::WouldBlock) => {
                    assert!(std::time::Instant::now() < deadline);
                    org_socket::poll_readable(
                        receiver.fd().unwrap().as_raw_fd(),
                        Duration::from_millis(10),
                    )
                    .unwrap();
                }
                Ok(_) => panic!("peer close produced unexpected data"),
            }
        };
        let reset = unread && cfg!(target_os = "linux");
        let expected = if reset {
            io::ErrorKind::ConnectionReset
        } else {
            io::ErrorKind::UnexpectedEof
        };
        assert_eq!(error.kind(), expected, "unread={unread}");
        assert_eq!(error.raw_os_error(), reset.then_some(libc::ECONNRESET));
        assert!(host_disappeared(error.kind()));
        assert!(receiver.fd().is_none());
        eprintln!(
            "measured rendezvous unread={unread}: kind={:?} errno={:?}",
            error.kind(),
            error.raw_os_error()
        );
    }
}

#[test]
fn rendezvous_writer_poison_refuses_without_writing_an_event() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (socket, peer) = UnixStream::pair().unwrap();
    let writer = Arc::new(Mutex::new(socket));
    let poisoned = Arc::clone(&writer);
    assert!(std::thread::spawn(move || {
        let _guard = poisoned.lock().unwrap();
        panic!("synthetic poisoned event writer");
    })
    .join()
    .is_err());
    assert!(matches!(
        write_event(
            &writer,
            &RendezvousEvent::FdsReceived { of: "owned".into() }
        ),
        Err(LaunchError::Invalid("rendezvous writer poisoned"))
    ));
    assert!(!org_socket::poll_readable(peer.as_raw_fd(), Duration::from_millis(10)).unwrap());
}
#[test]
fn concurrent_receipts_and_server_events_share_one_writer_and_never_interleave_frames() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (socket, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(WAIT)).unwrap();
    let (mut control, mut stdout) = control(b"");
    control.rendezvous = Some(Arc::new(Mutex::new(socket)));
    let writer = control.writer().unwrap();
    assert!(Arc::ptr_eq(&writer, control.rendezvous.as_ref().unwrap()));
    let reader = std::thread::spawn(move || {
        let mut peer = BufReader::new(peer);
        let mut values = Vec::new();
        for _ in 0..33 {
            let value: Value = frame::read_message(&mut peer).unwrap().unwrap();
            assert_eq!(value["v"], 3);
            values.push(value);
        }
        values
    });
    let mut writers = Vec::new();
    let barrier = Arc::new(std::sync::Barrier::new(5));
    for lane in 0..4 {
        let writer = Arc::clone(&writer);
        let barrier = Arc::clone(&barrier);
        writers.push(std::thread::spawn(move || {
            barrier.wait();
            for n in 0..8 {
                write_event(
                    &writer,
                    &RendezvousEvent::FdsReceived {
                        of: format!("{lane}:{n}:{}", "x".repeat(64 * 1024)),
                    },
                )
                .unwrap();
            }
        }));
    }
    barrier.wait();
    control.emit(&ServerEvent::Discarded).unwrap();
    for writer in writers {
        writer.join().unwrap();
    }
    let values = reader.join().unwrap();
    assert_eq!(
        values.iter().filter(|v| v["event"] == "discarded").count(),
        1
    );
    let ids: std::collections::HashSet<_> = values
        .iter()
        .filter_map(|v| v["of"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(ids.len(), 32);
    for lane in 0..4 {
        for n in 0..8 {
            assert!(ids.contains(&format!("{lane}:{n}:{}", "x".repeat(64 * 1024))));
        }
    }
    stdout.rewind().unwrap();
    assert_eq!(stdout.metadata().unwrap().len(), 0);
}

#[test]
fn event_writer_holds_the_shared_lock_until_the_whole_backpressured_frame_finishes() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (socket, peer) = UnixStream::pair().unwrap();
    let small: libc::c_int = 4096;
    // SAFETY: this test owns the live socket and the correctly sized int buffer.
    assert_eq!(
        unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&small as *const libc::c_int).cast(),
                std::mem::size_of_val(&small) as _,
            )
        },
        0
    );
    let writer = Arc::new(Mutex::new(socket));
    let owned = Arc::clone(&writer);
    let writing = std::thread::spawn(move || {
        write_event(
            &owned,
            &RendezvousEvent::FdsReceived {
                of: "x".repeat(org_proto::MAX_FRAME_BYTES / 2),
            },
        )
    });
    assert!(org_socket::poll_readable(peer.as_raw_fd(), Duration::from_secs(2)).unwrap());
    // A response larger than the tiny send buffer is now blocked; the entire
    // event must still own the shared lock, not just a temporary clone operation.
    let held_for_frame = writer.try_lock().is_err();
    drop(peer);
    assert!(writing.join().unwrap().is_err());
    assert!(
        held_for_frame,
        "writer released serialization before completing its frame"
    );
}
#[test]
fn rendezvous_event_write_has_one_five_second_whole_frame_deadline() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (socket, peer) = UnixStream::pair().unwrap();
    let small: libc::c_int = 4096;
    // SAFETY: owned live socket, correctly sized int.
    assert_eq!(
        unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&small as *const libc::c_int).cast(),
                std::mem::size_of_val(&small) as _,
            )
        },
        0
    );
    let writer = Arc::new(Mutex::new(socket));
    let (cancel, cancelled) = std::sync::mpsc::channel();
    let watchdog = std::thread::spawn(move || {
        // A broken deadline is released by closing the peer after seven seconds.
        // The elapsed-time assertion catches it without an external harness timeout.
        let _ = cancelled.recv_timeout(Duration::from_secs(7));
        drop(peer);
    });
    let start = std::time::Instant::now();
    let error = write_event(
        &writer,
        &RendezvousEvent::FdsReceived {
            of: "x".repeat(org_proto::MAX_FRAME_BYTES / 2),
        },
    )
    .unwrap_err();
    let elapsed = start.elapsed();
    let _ = cancel.send(());
    watchdog.join().unwrap();
    assert!(
        matches!(error, LaunchError::Frame(FrameError::Io(ref error)) if matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)),
        "unexpected bounded write outcome: {error}"
    );
    assert!(
        elapsed >= Duration::from_secs(4) && elapsed < Duration::from_millis(6500),
        "write deadline was not five seconds: {elapsed:?}"
    );
}

#[test]
fn writer_is_available_only_for_the_authenticated_rendezvous_transport() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (mut control, _) = control(b"");
    assert!(control.writer().is_none());
    let (socket, _peer) = UnixStream::pair().unwrap();
    let writer = Arc::new(Mutex::new(socket));
    control.rendezvous = Some(Arc::clone(&writer));
    assert!(Arc::ptr_eq(&control.writer().unwrap(), &writer));
}
