//! X-02 rev15 §5.1/5.2/5.4: a fake host drives the real worker independently of 2b.4.
use super::*;
use org_office_launch::v3::{self, CreateLine, Handoff, HandoffFd, HostFirstLine, OfficeLine};
use org_proto::{GrantKey, RecoveryCode};
use serde_json::{json, Value};
use std::io::BufRead;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixDatagram, UnixListener};

const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const OWNER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

fn create_line(path: &Path) -> HostFirstLine {
    HostFirstLine::Create(CreateLine {
        office: OfficeLine {
            instance_id: INSTANCE.into(),
            office_id: OFFICE.into(),
            rendezvous: path.to_str().unwrap().into(),
        },
        org_id: ORG.into(),
        org_name: "Acme".into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        owner_member_id: OWNER.into(),
    })
}

fn accepted(listener: &UnixListener, run: &mut Run) -> UnixStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(WAIT)).unwrap();
                return stream;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("rendezvous accept: {e}"),
        }
        assert!(
            run.child.try_wait().unwrap().is_none(),
            "worker exited before rendezvous: {:?}",
            run.events.try_recv()
        );
        assert!(
            std::time::Instant::now() < deadline,
            "worker never connected to rendezvous"
        );
        org_socket::poll_readable(listener.as_raw_fd(), Duration::from_millis(10)).unwrap();
    }
}

fn line(reader: &mut impl BufRead) -> Value {
    let bytes = frame::read_line(reader)
        .unwrap()
        .expect("rendezvous ended before event");
    serde_json::from_slice(&bytes).unwrap()
}

fn pass_fd(stream: &UnixStream, fd: RawFd) {
    pass_fds(stream, v3::FD_MARKER, &[fd]);
}
fn pass_fds(stream: &UnixStream, byte: u8, fds: &[RawFd]) {
    let mut marker = [byte];
    let mut iov = libc::iovec {
        iov_base: marker.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // SAFETY: usize-aligned ancillary storage and live pointers sized for the fd slice.
    unsafe {
        let bytes = std::mem::size_of_val(fds);
        let space = libc::CMSG_SPACE(bytes as _) as usize;
        let mut control = vec![0usize; space.div_ceil(std::mem::size_of::<usize>())];
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = space as _;
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(bytes as _) as _;
        std::ptr::copy_nonoverlapping(fds.as_ptr().cast::<u8>(), libc::CMSG_DATA(header), bytes);
        assert_eq!(libc::sendmsg(stream.as_raw_fd(), &message, 0), 1);
    }
}

#[test]
fn v3_launch_receives_keys_and_acknowledges_handoff_before_ready() {
    let world = World::new();
    let path = world.home.join("r.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let mut run = world.spawn(&["--create"]);
    v3::write_first_line(run.stdin.as_mut().unwrap(), &create_line(&path)).unwrap();
    let mut stream = accepted(&listener, &mut run);
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    assert_eq!(
        line(&mut reader),
        json!({"v":3,"event":"ready_for_secrets"})
    );
    let (transfer, host_end) = UnixDatagram::pair().unwrap();
    transfer.shutdown(std::net::Shutdown::Write).unwrap();
    let handoff = Handoff::Create {
        db_key: db_key(),
        grant_key: GrantKey::from_bytes([7; 32]),
        recovery_code: RecoveryCode::new("TEST-RECOVERY-SECRET".into()).unwrap(),
        fds: vec![HandoffFd::Transfer],
    };
    let mut header = v3::encode_handoff(&handoff).unwrap();
    stream.write_all(&header).unwrap();
    header.fill(0);
    pass_fd(&stream, transfer.as_raw_fd());
    assert_eq!(
        line(&mut reader),
        json!({"v":3,"event":"fds_received","of":"handoff"})
    );
    drop(transfer);
    let created = line(&mut reader);
    assert_eq!(created["event"], "created");
    assert_eq!(created["office"]["office_id"], OFFICE);
    assert_eq!(line(&mut reader)["event"], "ready");
    assert_eq!(hello(&world).organization_name, "Acme");
    // Sender closed its passed copy after ACK. The live worker must still own the datagram end.
    host_end.set_nonblocking(true).unwrap();
    assert_eq!(host_end.send(b"probe").unwrap(), 5);
    assert!(
        run.events.try_recv().is_err(),
        "v3 authority events leaked onto stdout"
    );
    // Rendezvous EOF is fatal even though stdin remains open (X-02 §5.1 step9).
    stream.shutdown(std::net::Shutdown::Both).unwrap();
    drop(reader);
    drop(stream);
    assert!(run.wait().success());
    assert!(!socket(&world).exists());
    assert!(host_end.send(b"after shutdown").is_err());
    let logs = run.stderr();
    for secret in ["0707070707070707", "TEST-RECOVERY-SECRET", DB_KEY_HEX] {
        assert!(!logs.contains(secret));
    }
}

use org_office_launch::FirstLine;
use org_proto::{
    msg::{MsgBind, MsgBindParams},
    ErrorCode,
};

struct Session {
    run: Run,
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}
impl Session {
    fn event(&mut self) -> Value {
        line(&mut self.reader)
    }
    fn no_stdout(&self) {
        assert!(
            matches!(
                self.run.events.recv_timeout(WAIT),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
            ),
            "authority event on stdout"
        );
    }
    fn stop(mut self) {
        self.stream.shutdown(std::net::Shutdown::Both).unwrap();
        assert!(self.run.wait().success());
        self.no_stdout();
    }
}

fn handshake(world: &World, op: FirstLine, instance: &str, key: Option<DbKey>) -> Session {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = world.home.join(format!(
        "r{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let (args, first) = match op {
        FirstLine::Create => {
            let HostFirstLine::Create(mut create) = create_line(&path) else {
                unreachable!()
            };
            create.office.instance_id = instance.into();
            (vec!["--create"], HostFirstLine::Create(create))
        }
        FirstLine::Start => (
            vec!["--office", OFFICE],
            HostFirstLine::Start(OfficeLine {
                instance_id: instance.into(),
                office_id: OFFICE.into(),
                rendezvous: path.to_str().unwrap().into(),
            }),
        ),
        FirstLine::Discard => (
            vec!["--discard", OFFICE],
            HostFirstLine::Discard(OfficeLine {
                instance_id: instance.into(),
                office_id: OFFICE.into(),
                rendezvous: path.to_str().unwrap().into(),
            }),
        ),
    };
    let mut run = world.spawn(&args);
    v3::write_first_line(run.stdin.as_mut().unwrap(), &first).unwrap();
    let mut stream = accepted(&listener, &mut run);
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    assert_eq!(
        line(&mut reader),
        json!({"v":3,"event":"ready_for_secrets"})
    );
    let header = match op {
        FirstLine::Create => Handoff::Create {
            db_key: key.unwrap(),
            grant_key: GrantKey::from_bytes([7; 32]),
            recovery_code: RecoveryCode::new("TEST-RECOVERY-SECRET".into()).unwrap(),
            fds: vec![HandoffFd::Transfer],
        },
        FirstLine::Start => Handoff::Start {
            db_key: key.unwrap(),
            grant_key: GrantKey::from_bytes([7; 32]),
            fds: vec![HandoffFd::Transfer],
        },
        FirstLine::Discard => Handoff::Discard {
            db_key: key,
            fds: vec![],
        },
    };
    let mut bytes = v3::encode_handoff(&header).unwrap();
    stream.write_all(&bytes).unwrap();
    bytes.fill(0);
    if op != FirstLine::Discard {
        let (transfer, _host_end) = UnixDatagram::pair().unwrap();
        transfer.shutdown(std::net::Shutdown::Write).unwrap();
        pass_fd(&stream, transfer.as_raw_fd());
        assert_eq!(
            line(&mut reader),
            json!({"v":3,"event":"fds_received","of":"handoff"})
        );
        drop(transfer);
    }
    Session {
        run,
        stream,
        reader,
    }
}
fn ready_create(world: &World) -> Session {
    let mut session = handshake(world, FirstLine::Create, INSTANCE, Some(db_key()));
    assert_eq!(session.event()["event"], "created");
    assert_eq!(session.event()["event"], "ready");
    session
}
fn assert_reopens(world: &World) {
    assert!(matches!(
        org_office::Office::open_for_instance(
            world.folder(OFFICE),
            org_office::Key::from_db_key(db_key()),
            "01ARZ3NDEKTSV4RRFFQ69G5FAZ"
        ),
        Err(org_office::OfficeError::Invalid(_))
    ));
    let office = org_office::Office::open_for_instance(
        world.folder(OFFICE),
        org_office::Key::from_db_key(db_key()),
        INSTANCE,
    )
    .unwrap();
    assert!(office.org_snapshot().is_ok());
    drop(office);
}

#[test]
fn rendezvous_eof_closes_all_databases_and_persists_the_create_instance() {
    let world = World::new();
    let session = ready_create(&world);
    // stdin is still open. Only rendezvous EOF asks the worker to close its office.
    assert!(session.run.stdin.is_some());
    session.stop();
    assert!(!socket(&world).exists());
    assert_reopens(&world);
}

#[test]
fn stdin_is_unparsed_and_non_authoritative_while_v3_binds_fail_closed() {
    let world = World::new();
    let mut session = ready_create(&world);
    let mut stdin = session.run.stdin.take().unwrap();
    let (sent, complete) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        // Exceeds the kernel pipe buffer; success requires the worker's independent drain.
        let result = (0..512).try_for_each(|_| stdin.write_all(&[b'x'; 4096]));
        drop(stdin);
        let _ = sent.send(result);
    });
    complete
        .recv_timeout(WAIT)
        .expect("stdin was not drained")
        .unwrap();
    writer.join().unwrap();
    let client = Client::connect(socket(&world)).unwrap();
    assert_eq!(client.hello().unwrap().api_version, 1);
    for (member, app_token) in [
        (None, None),
        (None, Some(token())),
        (None, Some(AppToken::from_bytes([0; 32]))),
        (Some("owner".into()), Some(token())),
        (Some("ada".into()), None),
    ] {
        assert!(
            matches!(client.call::<MsgBind>(&MsgBindParams{member,app_token}),Err(org_client::ClientError::Api(error)) if error.code==ErrorCode::NotAuthorized)
        );
    }
    assert_eq!(client.hello().unwrap().api_version, 1);
    // This distinguishes a rendezvous watcher from a pipe-EOF watcher without a sleep:
    // the undefined control byte must produce exit1; a pipe-authoritative worker exits0.
    session.stream.write_all(b"!").unwrap();
    assert!(!session.run.wait().success());
    session.no_stdout();
}

fn database_bytes(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut result = std::collections::BTreeMap::new();
    for item in std::fs::read_dir(dir).unwrap() {
        let path = item.unwrap().path();
        if path.is_dir() {
            result.extend(database_bytes(&path));
        } else if path.extension().is_some_and(|ext| ext == "db") {
            result.insert(path.clone(), std::fs::read(path).unwrap());
        }
    }
    result
}
#[test]
fn v3_start_adopts_legacy_then_matches_and_refuses_other_instance_without_writes() {
    let world = World::new();
    let mut legacy = create(&world);
    assert!(legacy.host_goes_away().success());
    for _ in 0..2 {
        let mut session = handshake(&world, FirstLine::Start, INSTANCE, Some(db_key()));
        assert_eq!(session.event()["event"], "ready");
        assert_eq!(hello(&world).office.id, OFFICE);
        session.stop();
    }
    assert_reopens(&world);
    let before = database_bytes(&world.home);
    assert!(!before.is_empty());
    let mut wrong = handshake(
        &world,
        FirstLine::Start,
        "01ARZ3NDEKTSV4RRFFQ69G5FAZ",
        Some(db_key()),
    );
    let failed = wrong.event();
    assert_eq!(failed["event"], "failed");
    assert_eq!(failed["code"], "storage_error");
    assert!(!wrong.run.wait().success());
    wrong.no_stdout();
    assert_eq!(database_bytes(&world.home), before);
    let mut wrong_key = handshake(
        &world,
        FirstLine::Start,
        INSTANCE,
        Some(DbKey::from_bytes([0; 32])),
    );
    assert_eq!(wrong_key.event()["code"], "key_missing");
    assert!(!wrong_key.run.wait().success());
    wrong_key.no_stdout();
    assert_eq!(database_bytes(&world.home), before);
}

#[test]
fn v3_discard_uses_zero_fds_and_no_receipt_with_null_or_present_key() {
    let world = World::new();
    for key in [None, Some(db_key())] {
        std::fs::create_dir_all(world.folder(OFFICE)).unwrap();
        let mut session = handshake(&world, FirstLine::Discard, INSTANCE, key);
        assert_eq!(session.event(), json!({"v":3,"event":"discarded"}));
        assert!(session.run.wait().success());
        session.no_stdout();
        assert!(!world.folder(OFFICE).exists());
    }
    ready_create(&world).stop();
    for key in [None, Some(db_key())] {
        let mut session = handshake(&world, FirstLine::Discard, INSTANCE, key);
        let failed = session.event();
        assert_eq!(failed["event"], "failed");
        assert_eq!(failed["code"], "already_exists");
        assert!(!session.run.wait().success());
        session.no_stdout();
        assert!(world.folder(OFFICE).join("office.json").exists());
    }
}

#[test]
fn v3_cli_office_mismatch_refuses_before_connect_for_start_and_discard() {
    for op in [FirstLine::Start, FirstLine::Discard] {
        let world = World::new();
        let path = world.home.join("r.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut run = world.spawn(&[
            if op == FirstLine::Start {
                "--office"
            } else {
                "--discard"
            },
            OFFICE,
        ]);
        let office = OfficeLine {
            instance_id: INSTANCE.into(),
            office_id: "OTHEROFFICE".into(),
            rendezvous: path.to_str().unwrap().into(),
        };
        v3::write_first_line(
            run.stdin.as_mut().unwrap(),
            &if op == FirstLine::Start {
                HostFirstLine::Start(office)
            } else {
                HostFirstLine::Discard(office)
            },
        )
        .unwrap();
        assert!(matches!(
            run.event(),
            ServerEvent::Failed {
                code: FailCode::InvalidRequest,
                ..
            }
        ));
        assert!(!run.wait().success());
        assert!(
            matches!(listener.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock)
        );
        assert!(!world.folder(OFFICE).exists());
    }
}

#[test]
fn malformed_launch_json_never_echoes_input_bytes() {
    let mut legacy = serde_json::to_value(create_message()).unwrap();
    legacy["v"] = json!(2);
    legacy["org_name"] = json!(314159265358979323u64);
    let world = World::new();
    let mut v3 = serde_json::to_value(create_line(&world.home.join("unused"))).unwrap();
    v3["v"] = json!(3);
    v3.as_object_mut()
        .unwrap()
        .insert("TEST-RECOVERY-SECRET".into(), json!(true));
    for (value, secret) in [(legacy, "314159265358979323"), (v3, "TEST-RECOVERY-SECRET")] {
        let wire = format!("{value}\n");
        // Prove this exact malformed input reaches a decoder error that quotes its secret.
        let error = v3::read_first_line_any(
            &mut std::io::Cursor::new(wire.as_bytes()),
            FirstLine::Create,
        )
        .unwrap_err();
        assert!(matches!(error, org_office_launch::LaunchError::Json(_)));
        assert!(
            error.to_string().contains(secret),
            "fixture never reached a secret-bearing decoder error: {error}"
        );
        let mut run = world.spawn(&["--create"]);
        run.stdin
            .as_mut()
            .unwrap()
            .write_all(wire.as_bytes())
            .unwrap();
        match run.event() {
            ServerEvent::Failed { code, message } => {
                assert_eq!(code, FailCode::InvalidRequest);
                assert_eq!(message, "invalid launch message");
                assert!(!message.contains(secret));
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(!run.wait().success());
        assert!(!run.stderr().contains(secret));
        assert!(!world.folder(OFFICE).exists());
    }
}

#[test]
fn undefined_lifetime_bytes_and_rights_exit_the_real_worker_and_close_reported_fd() {
    for rights in [false, true] {
        let world = World::new();
        let mut session = ready_create(&world);
        let (passed, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(WAIT)).unwrap();
        if rights {
            pass_fd(&session.stream, passed.as_raw_fd());
        } else {
            session.stream.write_all(b"!").unwrap();
        }
        assert!(!session.run.wait().success());
        session.no_stdout();
        drop(passed);
        use std::io::Read;
        let mut byte = [0];
        assert_eq!(peer.read(&mut byte).unwrap(), 0);
    }
}

#[test]
fn startup_rights_faults_exit_before_any_failure_or_office_event() {
    for case in [
        "early_fd",
        "zero_fds",
        "extra_fds",
        "ctrunc",
        "wrong_marker",
        "eof_header",
    ] {
        let world = World::new();
        let path = world.home.join("r.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let mut run = world.spawn(&["--create"]);
        v3::write_first_line(run.stdin.as_mut().unwrap(), &create_line(&path)).unwrap();
        let mut stream = accepted(&listener, &mut run);
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        assert_eq!(line(&mut reader)["event"], "ready_for_secrets");
        let header = v3::encode_handoff(&Handoff::Create {
            db_key: db_key(),
            grant_key: GrantKey::from_bytes([7; 32]),
            recovery_code: RecoveryCode::new("TEST-RECOVERY-SECRET".into()).unwrap(),
            fds: vec![HandoffFd::Transfer],
        })
        .unwrap();
        let (passed, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(WAIT)).unwrap();
        match case {
            "early_fd" => {
                pass_fds(&stream, header[0], &[passed.as_raw_fd()]);
                let _ = stream.write_all(&header[1..]);
            }
            "eof_header" => {
                stream.write_all(&header[..16]).unwrap();
                stream.shutdown(std::net::Shutdown::Write).unwrap();
            }
            _ => {
                stream.write_all(&header).unwrap();
                match case {
                    "zero_fds" => stream.write_all(&[v3::FD_MARKER]).unwrap(),
                    "extra_fds" => pass_fds(&stream, v3::FD_MARKER, &[passed.as_raw_fd(); 2]),
                    "ctrunc" => pass_fds(&stream, v3::FD_MARKER, &[passed.as_raw_fd(); 8]),
                    "wrong_marker" => pass_fds(&stream, 9, &[passed.as_raw_fd()]),
                    _ => unreachable!(),
                }
            }
        }
        assert!(!run.wait().success(), "{case}");
        assert!(!world.folder(OFFICE).exists(), "{case} made an office");
        match frame::read_line(&mut reader) {
            Ok(None) => {}
            Err(frame::FrameError::Io(e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
            other => panic!("{case} emitted after a rights fault: {other:?}"),
        }
        assert!(matches!(
            run.events.recv_timeout(WAIT),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        ));
        drop(passed);
        use std::io::Read;
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    }
}
