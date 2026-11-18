//! X-02 rev15 §5.1/5.2/5.4: a fake host drives the real worker independently of 2b.4.
use super::*;
use org_office_launch::{CreateLine, Handoff, HandoffFd, HostFirstLine, OfficeLine};
use org_proto::{GrantKey, RecoveryCode};
use serde_json::{json, Value};
use std::io::BufRead;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixDatagram, UnixListener};

pub(super) const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607182";
const OWNER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

fn create_line(path: &Path) -> HostFirstLine {
    HostFirstLine::Create(CreateLine {
        office: OfficeLine {
            id_profile: None,
            selected_writer_required: false,
            instance_id: INSTANCE.into(),
            office_id: OFFICE.into(),
            rendezvous: path.to_str().unwrap().into(),
        },
        org_id: ORG.into(),
        org_name: "Acme".into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        owner_member_id: OWNER.into(),
        hq_creation: Some(hq_creation(path)),
    })
}

/// HQ creation-v1 without a selected bootstrap: the only create the worker accepts.
fn hq_creation(rendezvous: &Path) -> org_office_launch::HqCreationV1 {
    let public = rendezvous.parent().unwrap().join("state");
    std::fs::create_dir_all(&public).unwrap();
    org_office_launch::HqCreationV1 {
        version: 1,
        operation_id: "01ARZ3NDEKTSV4RRFFQ69G5FAX".into(),
        escrow_profile: 1,
        public_root: std::fs::canonicalize(public)
            .unwrap()
            .to_str()
            .unwrap()
            .into(),
        bootstrap: None,
    }
}

/// A recovery code in the form the host makes.
fn recovery_code() -> RecoveryCode {
    RecoveryCode::new("0001-0203-0405-0607-0809-0A0B-0C0D-0E0F-1011-1213".into()).unwrap()
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
    pass_fds(stream, org_office_launch::FD_MARKER, &[fd]);
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
fn launch_receives_keys_and_acknowledges_handoff_before_ready() {
    let _process = process_isolation();
    let world = World::new();
    let path = world.home.join("r.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let mut run = world.spawn(&["--create"]);
    org_office_launch::write_first_line(run.stdin.as_mut().unwrap(), &create_line(&path)).unwrap();
    let mut stream = accepted(&listener, &mut run);
    let mut reader = BufReader::with_capacity(1, stream.try_clone().unwrap());
    assert_eq!(
        line(&mut reader),
        json!({"v":1,"event":"ready_for_secrets","channel_profile":"office-channels/1"})
    );
    // The host may fill stdin before supplying secrets. Handoff must not gate draining.
    let mut stdin = run.stdin.take().unwrap();
    let (sent, complete) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        let result = (0..512).try_for_each(|_| stdin.write_all(&[b'x'; 4096]));
        let _ = sent.send((stdin, result));
    });
    let (stdin, result) = complete
        .recv_timeout(WAIT)
        .expect("stdin blocked before the secrets handoff");
    result.unwrap();
    writer.join().unwrap();
    run.stdin = Some(stdin);
    let (transfer, host_end) = UnixDatagram::pair().unwrap();
    transfer.shutdown(std::net::Shutdown::Write).unwrap();
    let handoff = Handoff::Create {
        office_secrets_key: office_secrets_key(),
        grant_key: GrantKey::from_bytes([7; 32]),
        recovery_code: Some(recovery_code()),
        fds: vec![HandoffFd::Transfer],
    };
    let mut header = org_office_launch::encode_handoff(&handoff).unwrap();
    stream.write_all(&header).unwrap();
    header.fill(0);
    pass_fd(&stream, transfer.as_raw_fd());
    assert_eq!(
        line(&mut reader),
        json!({"v":1,"event":"fds_received","of":"handoff"})
    );
    drop(transfer);
    let created = line(&mut reader);
    assert_eq!(created["event"], "created");
    assert_eq!(created["office"]["office_id"], OFFICE);
    assert_eq!(line(&mut reader)["event"], "ready");
    assert_eq!(hello(&world).organization_name, "Acme");
    // Sender closed its passed copy after ACK. The live worker must still own the datagram end.
    host_end.set_nonblocking(true).unwrap();
    let record =
        org_office_launch::encode_transfer_record(&org_office_launch::TransferRecord::Revoked {
            session_ids: vec![],
        })
        .unwrap();
    assert_eq!(host_end.send(&record).unwrap(), record.len());
    assert!(
        run.events.try_recv().is_err(),
        "launch authority events leaked onto stdout"
    );
    // Rendezvous EOF is fatal even though stdin remains open (X-02 §5.1 step9).
    stream.shutdown(std::net::Shutdown::Both).unwrap();
    drop(reader);
    drop(stream);
    assert!(run.wait().success());
    assert!(!socket(&world).exists());
    assert!(host_end.send(b"after shutdown").is_err());
    let logs = run.stderr();
    for secret in ["0707070707070707", "0001-0203-0405", SECRETS_KEY_HEX] {
        assert!(!logs.contains(secret));
    }
}

use org_office_launch::FirstLine;
use org_proto::{
    msg::{MsgBind, MsgBindParams},
    ErrorCode,
};

/// A worker the fake host launched: its process and the authenticated rendezvous.
pub(super) struct Session {
    pub(super) run: Run,
    pub(super) stream: UnixStream,
    reader: BufReader<UnixStream>,
    transfer: Option<UnixDatagram>,
    // Sender copy of a descriptor in flight on `transfer`, kept until the worker answers
    // (as the host's pending registry does). Closing it while in flight lets the macOS
    // unix-socket GC flush the in-flight socket.
    in_flight: std::cell::RefCell<Option<UnixStream>>,
}
impl Session {
    /// The next launch event on the rendezvous.
    pub(super) fn event(&mut self) -> Value {
        let event = line(&mut self.reader);
        self.in_flight.get_mut().take();
        event
    }
    /// stdout ended with nothing on it (it is untrusted; the worker writes nothing there).
    pub(super) fn no_stdout(&self) {
        assert!(
            matches!(
                self.run.events.recv_timeout(WAIT),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
            ),
            "authority event on stdout"
        );
    }
    fn stop(mut self) {
        assert!(self.host_goes_away().success());
        self.no_stdout();
    }
    /// Ends the rendezvous, as a host that goes away does, and waits for the exit.
    pub(super) fn host_goes_away(&mut self) -> std::process::ExitStatus {
        self.stream.shutdown(std::net::Shutdown::Both).unwrap();
        self.run.wait()
    }
}

/// The launch first line for `op`, naming `rendezvous`, with this file's instance and office.
pub(super) fn first_line(op: FirstLine, rendezvous: &Path) -> HostFirstLine {
    let office = OfficeLine {
        id_profile: None,
        selected_writer_required: false,
        instance_id: INSTANCE.into(),
        office_id: OFFICE.into(),
        rendezvous: rendezvous.to_str().unwrap().into(),
    };
    match op {
        FirstLine::Create => create_line(rendezvous),
        FirstLine::Start => HostFirstLine::Start(office),
        FirstLine::Discard => HostFirstLine::Discard(office),
    }
}

pub(super) fn handshake(
    world: &World,
    op: FirstLine,
    instance: &str,
    key: Option<OfficeSecretsKey>,
) -> Session {
    handshake_with(world, op, instance, key, |_| {})
}

/// `handshake`, with the create line changed first. The rendezvous sits beside the home, so a
/// home whose own paths are too long can still be reached.
fn handshake_with(
    world: &World,
    op: FirstLine,
    instance: &str,
    key: Option<OfficeSecretsKey>,
    edit: impl FnOnce(&mut CreateLine),
) -> Session {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = world.home.parent().unwrap().join(format!(
        "r{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let mut first = first_line(op, &path);
    match &mut first {
        HostFirstLine::Create(create) => {
            create.office.instance_id = instance.into();
            edit(create);
        }
        HostFirstLine::Start(office) | HostFirstLine::Discard(office) => {
            office.instance_id = instance.into();
        }
        _ => unreachable!(),
    }
    let args = match op {
        FirstLine::Create => vec!["--create"],
        FirstLine::Start => vec!["--office", OFFICE],
        FirstLine::Discard => vec!["--discard", OFFICE],
    };
    let mut run = world.spawn(&args);
    org_office_launch::write_first_line(run.stdin.as_mut().unwrap(), &first).unwrap();
    let mut stream = accepted(&listener, &mut run);
    let mut reader = BufReader::with_capacity(1, stream.try_clone().unwrap());
    assert_eq!(
        line(&mut reader),
        json!({"v":1,"event":"ready_for_secrets","channel_profile":"office-channels/1"})
    );
    let header = match op {
        FirstLine::Create => Handoff::Create {
            office_secrets_key: key.unwrap(),
            grant_key: GrantKey::from_bytes([7; 32]),
            recovery_code: Some(recovery_code()),
            fds: vec![HandoffFd::Transfer],
        },
        FirstLine::Start => Handoff::Start {
            office_secrets_key: key,
            grant_key: GrantKey::from_bytes([7; 32]),
            fds: vec![HandoffFd::Transfer],
        },
        FirstLine::Discard => {
            assert!(key.is_none(), "a discard handoff carries no secret");
            Handoff::Discard { fds: vec![] }
        }
    };
    let mut bytes = org_office_launch::encode_handoff(&header).unwrap();
    stream.write_all(&bytes).unwrap();
    bytes.fill(0);
    let transfer_host = if op != FirstLine::Discard {
        let (transfer, host_end) = UnixDatagram::pair().unwrap();
        transfer.shutdown(std::net::Shutdown::Write).unwrap();
        pass_fd(&stream, transfer.as_raw_fd());
        assert_eq!(
            line(&mut reader),
            json!({"v":1,"event":"fds_received","of":"handoff"})
        );
        drop(transfer);
        Some(host_end)
    } else {
        None
    };
    Session {
        run,
        stream,
        reader,
        transfer: transfer_host,
        in_flight: Default::default(),
    }
}
fn ready_create(world: &World) -> Session {
    let mut session = handshake(
        world,
        FirstLine::Create,
        INSTANCE,
        Some(office_secrets_key()),
    );
    assert_eq!(session.event()["event"], "created");
    assert_eq!(session.event()["event"], "ready");
    session
}
fn assert_reopens(world: &World) {
    assert!(matches!(
        org_office::Office::open_for_instance(
            world.folder(OFFICE),
            org_office::Key::from_office_secrets_key(office_secrets_key()),
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f607184"
        ),
        Err(org_office::OfficeError::Invalid(_))
    ));
    let office = org_office::Office::open_for_instance(
        world.folder(OFFICE),
        org_office::Key::from_office_secrets_key(office_secrets_key()),
        INSTANCE,
    )
    .unwrap();
    assert!(office.org_snapshot().is_ok());
    drop(office);
}

/// A create the worker refuses: the reason, a failed exit and no stdout.
fn refused_create(world: &World, edit: impl FnOnce(&mut CreateLine)) -> Value {
    let mut session = handshake_with(
        world,
        FirstLine::Create,
        INSTANCE,
        Some(office_secrets_key()),
        edit,
    );
    let failed = session.event();
    assert_eq!(failed["event"], "failed", "{failed}");
    assert!(!session.run.wait().success());
    session.no_stdout();
    failed["code"].clone()
}

#[test]
fn hq_creation_stores_the_office_serves_hello_and_closes_on_rendezvous_eof() {
    let _process = process_isolation();
    let world = World::new();
    let session = ready_create(&world);
    let identity = hello(&world);
    assert_eq!(identity.organization_name, "Acme");
    assert_eq!(identity.office.id, OFFICE);
    assert_eq!(identity.office.organization_id, ORG);
    assert_eq!(identity.office.name, "HQ");
    assert_eq!(identity.office.role, OfficeRole::Hq);
    // What the host reads is there, valid, and names this office.
    let json = std::fs::read(world.folder(OFFICE).join("office.json")).unwrap();
    let stored = OfficeFile::from_slice(&json).unwrap();
    assert_eq!(
        (stored.office_id.as_str(), stored.org_name.as_str()),
        (OFFICE, "Acme")
    );
    session.stop();
    assert!(!socket(&world).exists(), "the socket file was left behind");
}

#[test]
fn hq_creation_of_an_existing_office_by_another_operation_is_refused_and_changes_nothing() {
    let _process = process_isolation();
    let world = World::new();
    ready_create(&world).stop();
    let before = database_bytes(&world.home);
    // The same operation id resumes its own creation (ADR-0011). Another operation does not
    // match the retained creation record, so it is refused as stored state, not a new office.
    let code = refused_create(&world, |create| {
        create.hq_creation.as_mut().unwrap().operation_id = "01ARZ3NDEKTSV4RRFFQ69G5FAY".into();
    });
    assert_eq!(code, "storage_error");
    assert_eq!(database_bytes(&world.home), before);
    assert!(world.folder(OFFICE).join("office.json").exists());
}

#[test]
fn hq_creation_with_a_bad_or_untrimmed_name_is_invalid_name_and_leaves_nothing() {
    let _process = process_isolation();
    let world = World::new();
    for (org_name, office_name) in [
        ("zero\u{200B}width", "HQ"),
        (" Acme", "HQ"),
        ("Acme", "HQ "),
    ] {
        let code = refused_create(&world, |create| {
            create.org_name = org_name.into();
            create.office_name = office_name.into();
        });
        assert_eq!(code, "invalid_name", "{org_name:?} / {office_name:?}");
        assert!(!world.folder(OFFICE).join("office.json").exists());
    }
}

#[test]
fn hq_creation_whose_socket_path_cannot_exist_fails_before_anything_is_stored() {
    let _process = process_isolation();
    let dir = tempfile::Builder::new().prefix("oo").tempdir().unwrap();
    let home = dir.path().join("x".repeat(90));
    std::fs::create_dir_all(home.join("offices")).unwrap();
    std::fs::create_dir_all(home.join("run")).unwrap();
    let world = World { _dir: dir, home };
    assert_eq!(refused_create(&world, |_| {}), "invalid_request");
    assert!(
        !office_dir(&world.home, OFFICE).unwrap().exists(),
        "an office was stored"
    );
}

#[test]
fn rendezvous_eof_closes_all_databases_and_persists_the_create_instance() {
    let _process = process_isolation();
    let world = World::new();
    let session = ready_create(&world);
    // stdin is still open. Only rendezvous EOF asks the worker to close its office.
    assert!(session.run.stdin.is_some());
    session.stop();
    assert!(!socket(&world).exists());
    assert_reopens(&world);
}

#[test]
fn stdin_is_unparsed_while_launch_allows_agent_binds_and_refuses_app_person_binds() {
    let _process = process_isolation();
    use org_proto::org::MemberAddAgent;
    let world = World::new();
    // Seed through the library, then start it as the host does and hire through the owner's grant.
    seed(&world);
    let owner_id = {
        let office = org_office::Office::open(
            world.folder(OFFICE),
            org_office::Key::from_office_secrets_key(office_secrets_key()),
        )
        .unwrap();
        let owner = office
            .org_snapshot()
            .unwrap()
            .members
            .into_iter()
            .find(|m| m.owner)
            .unwrap()
            .id;
        drop(office);
        owner
    };
    let member_id = "AGENT00000000000000000000A";
    let mut live = owner::live(&world);
    live.caller(Some(&owner_id))
        .call::<MemberAddAgent>(&owner::hire(6, member_id, "Ada", "ada"))
        .unwrap();
    let mut session = live.session;
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
    // No member is not a bind at all (the app-wide bind is gone); a person, with or without
    // a token, is refused the same way as an unknown member.
    for (member, app_token, expected) in [
        (None, None, ErrorCode::InvalidRequest),
        (None, Some(token()), ErrorCode::InvalidRequest),
        (
            None,
            Some(AppToken::from_bytes([7; 32])),
            ErrorCode::InvalidRequest,
        ),
        (
            Some("owner".into()),
            Some(token()),
            ErrorCode::NotAuthorized,
        ),
        (Some("owner".into()), None, ErrorCode::NotAuthorized),
    ] {
        assert!(
            matches!(client.call::<MsgBind>(&MsgBindParams{member,app_token}),Err(org_client::ClientError::Api(error)) if error.code==expected)
        );
    }
    let bound = client
        .call::<MsgBind>(&MsgBindParams {
            member: Some("ada".into()),
            app_token: None,
        })
        .unwrap();
    assert_eq!(bound.member_id.as_deref(), Some(member_id));
    client
        .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
        .unwrap();
    assert_eq!(client.hello().unwrap().api_version, 1);
    // This distinguishes a rendezvous watcher from a pipe-EOF watcher without a sleep:
    // the undefined control byte must produce exit1; a pipe-authoritative worker exits0.
    session.stream.write_all(b"!").unwrap();
    assert_eq!(session.run.wait().code(), Some(1));
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
fn start_adopts_legacy_then_matches_and_refuses_other_instance_without_writes() {
    let _process = process_isolation();
    let world = World::new();
    let mut legacy = started(&world);
    assert!(legacy.host_goes_away().success());
    for _ in 0..2 {
        let mut session = handshake(
            &world,
            FirstLine::Start,
            INSTANCE,
            Some(office_secrets_key()),
        );
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
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f607184",
        Some(office_secrets_key()),
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
        Some(OfficeSecretsKey::from_bytes([0; 32])),
    );
    // ADR-0016 decision 3: the key only wraps private keys. The plain databases open
    // without it, so a wrong key still serves the office's data.
    assert_eq!(wrong_key.event()["event"], "ready");
    assert_eq!(hello(&world).office.id, OFFICE);
    wrong_key.stop();
    assert_reopens(&world);
}

#[test]
fn discard_uses_zero_fds_no_receipt_and_no_secret() {
    let _process = process_isolation();
    let world = World::new();
    std::fs::create_dir_all(world.folder(OFFICE)).unwrap();
    let mut session = handshake(&world, FirstLine::Discard, INSTANCE, None);
    assert_eq!(session.event(), json!({"v":1,"event":"discarded"}));
    assert!(session.run.wait().success());
    session.no_stdout();
    assert!(!world.folder(OFFICE).exists());
    ready_create(&world).stop();
    let mut session = handshake(&world, FirstLine::Discard, INSTANCE, None);
    let failed = session.event();
    assert_eq!(failed["event"], "failed");
    assert_eq!(failed["code"], "already_exists");
    assert!(!session.run.wait().success());
    session.no_stdout();
    assert!(world.folder(OFFICE).join("office.json").exists());
}

#[test]
fn cli_office_mismatch_refuses_before_connect_for_start_and_discard() {
    let _process = process_isolation();
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
            id_profile: None,
            selected_writer_required: false,
            instance_id: INSTANCE.into(),
            office_id: "OTHEROFFICE".into(),
            rendezvous: path.to_str().unwrap().into(),
        };
        org_office_launch::write_first_line(
            run.stdin.as_mut().unwrap(),
            &if op == FirstLine::Start {
                HostFirstLine::Start(office)
            } else {
                HostFirstLine::Discard(office)
            },
        )
        .unwrap();
        // Refused before any trusted channel exists: nothing reported, nothing on stdout.
        assert!(!run.wait().success());
        assert!(matches!(
            run.events.recv_timeout(WAIT),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        ));
        assert!(
            matches!(listener.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock)
        );
        assert!(!world.folder(OFFICE).exists());
    }
}

#[test]
fn malformed_launch_json_never_echoes_input_bytes() {
    let _process = process_isolation();
    let world = World::new();
    let mut line = serde_json::to_value(create_line(&world.home.join("unused"))).unwrap();
    line["v"] = json!(org_office_launch::LAUNCH_VERSION);
    line.as_object_mut()
        .unwrap()
        .insert("TEST-RECOVERY-SECRET".into(), json!(true));
    let secret = "TEST-RECOVERY-SECRET";
    let wire = format!("{line}\n");
    // Prove this exact malformed input reaches a decoder error that quotes its secret.
    let error = org_office_launch::read_first_line(
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
    // Refused before any trusted channel exists: no event at all, so none can echo it.
    assert!(!run.wait().success());
    assert!(matches!(
        run.events.recv_timeout(WAIT),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    ));
    assert!(!run.stderr().contains(secret));
    assert!(!world.folder(OFFICE).exists());
}

#[test]
fn undefined_lifetime_bytes_and_rights_exit_the_real_worker_and_close_reported_fd() {
    let _process = process_isolation();
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
    let _process = process_isolation();
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
        org_office_launch::write_first_line(run.stdin.as_mut().unwrap(), &create_line(&path))
            .unwrap();
        let mut stream = accepted(&listener, &mut run);
        let mut reader = BufReader::with_capacity(1, stream.try_clone().unwrap());
        assert_eq!(line(&mut reader)["event"], "ready_for_secrets");
        let header = org_office_launch::encode_handoff(&Handoff::Create {
            office_secrets_key: office_secrets_key(),
            grant_key: GrantKey::from_bytes([7; 32]),
            recovery_code: Some(recovery_code()),
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
                    "zero_fds" => stream.write_all(&[org_office_launch::FD_MARKER]).unwrap(),
                    "extra_fds" => pass_fds(
                        &stream,
                        org_office_launch::FD_MARKER,
                        &[passed.as_raw_fd(); 2],
                    ),
                    "ctrunc" => pass_fds(
                        &stream,
                        org_office_launch::FD_MARKER,
                        &[passed.as_raw_fd(); 8],
                    ),
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

#[path = "activation/transfer.rs"]
mod transfer;

#[path = "activation/owner.rs"]
mod owner;

fn selected_plan() -> org_proto::selected_bootstrap::PreparedV1 {
    let id = |n| format!("01900000-0000-7000-8000-{n:012x}");
    org_proto::selected_bootstrap::PreparedV1 {
        plan: org_proto::selected_bootstrap::PlanV1 {
            version: 1,
            name: "Acme".into(),
            slug: "acme".into(),
            repository_remote_url: "https://example.test/org.git".into(),
            state_branch: "org/state".into(),
            home_team_name: "Core".into(),
        },
        organization_id: id(1),
        office_id: id(2),
        repository_id: id(3),
        home_team_id: id(4),
        hr_employee_id: id(5),
    }
}
fn selected_handshake(world: &World, start: bool) -> Session {
    let prepared = selected_plan();
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = world.home.join(format!(
        "selected-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let office = OfficeLine {
        id_profile: Some(org_office_launch::IdProfile::SelectedV1),
        selected_writer_required: true,
        instance_id: INSTANCE.into(),
        office_id: prepared.office_id.clone(),
        rendezvous: path.to_str().unwrap().into(),
    };
    let first = if start {
        HostFirstLine::Start(office)
    } else {
        let public = world.home.join("state");
        std::fs::create_dir_all(&public).unwrap();
        HostFirstLine::Create(CreateLine {
            office,
            org_id: prepared.organization_id.clone(),
            org_name: prepared.plan.name.clone(),
            office_name: "HQ".into(),
            role: OfficeRole::Hq,
            owner_member_id: OWNER.into(),
            hq_creation: Some(org_office_launch::HqCreationV1 {
                version: 1,
                operation_id: "01ARZ3NDEKTSV4RRFFQ69G5FAX".into(),
                escrow_profile: 1,
                public_root: std::fs::canonicalize(public)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .into(),
                bootstrap: Some(Box::new(prepared.clone())),
            }),
        })
    };
    let mut run = world.spawn(&if start {
        vec!["--office", &prepared.office_id]
    } else {
        vec!["--create"]
    });
    org_office_launch::write_first_line(run.stdin.as_mut().unwrap(), &first).unwrap();
    let mut stream = accepted(&listener, &mut run);
    let mut reader = BufReader::with_capacity(1, stream.try_clone().unwrap());
    assert_eq!(line(&mut reader)["event"], "ready_for_secrets");
    let handoff = if start {
        Handoff::Start {
            office_secrets_key: Some(office_secrets_key()),
            grant_key: GrantKey::from_bytes([7; 32]),
            fds: vec![HandoffFd::Transfer],
        }
    } else {
        Handoff::Create {
            office_secrets_key: office_secrets_key(),
            grant_key: GrantKey::from_bytes([7; 32]),
            recovery_code: Some(
                RecoveryCode::new("0001-0203-0405-0607-0809-0A0B-0C0D-0E0F-1011-1213".into())
                    .unwrap(),
            ),
            fds: vec![HandoffFd::Transfer],
        }
    };
    let mut header = org_office_launch::encode_handoff(&handoff).unwrap();
    stream.write_all(&header).unwrap();
    header.fill(0);
    let (worker, host) = UnixDatagram::pair().unwrap();
    worker.shutdown(std::net::Shutdown::Write).unwrap();
    pass_fd(&stream, worker.as_raw_fd());
    assert_eq!(
        line(&mut reader),
        json!({"v":1,"event":"fds_received","of":"handoff"})
    );
    drop(worker);
    let mut session = Session {
        run,
        stream,
        reader,
        transfer: Some(host),
        in_flight: Default::default(),
    };
    if !start {
        assert_eq!(session.event()["event"], "created");
    }
    let socket = org_proto::location::selected_office_socket_in(
        &world.home.join("run"),
        &prepared.office_id,
    )
    .unwrap();
    assert!(!socket.exists(), "Created must not bind before admission");
    session
}
fn selected_admission(world: &World) -> org_office_launch::writer::Admission {
    use org_office_launch::writer::{self as wire, Admission};
    let p = selected_plan();
    let identity = org_proto::work::WriterIdentity {
        instance_id: INSTANCE.into(),
        organization_id: p.organization_id,
        office_id: p.office_id.clone(),
        machine_id: "test-machine".into(),
        writer_epoch: 1,
        journal_incarnation: "test-journal".into(),
    };
    Admission {
        version: wire::VERSION,
        identity,
        protected_root: world.home.join("protected").to_str().unwrap().into(),
        public_root: std::fs::canonicalize(world.home.join("state"))
            .unwrap()
            .to_str()
            .unwrap()
            .into(),
        owner_account: "test-account".into(),
        owner_member: OWNER.into(),
    }
}
fn selected_writer(
    world: &World,
    session: &Session,
    position: std::sync::Arc<std::sync::Mutex<Option<org_proto::work::JournalPosition>>>,
    mode: &'static str,
) -> std::thread::JoinHandle<()> {
    let mut admission = selected_admission(world);
    if mode == "scope" {
        admission.identity.office_id = "foreign".into();
    }
    let (thread, sent) = selected_writer_attempt(session, position, mode, admission);
    sent.unwrap();
    thread
}

// Expected startup refusal may close transfer before the initial descriptor send.
// Only refusal tests inspect this result; the ordinary path above stays strict.
fn selected_writer_attempt(
    session: &Session,
    position: std::sync::Arc<std::sync::Mutex<Option<org_proto::work::JournalPosition>>>,
    mode: &'static str,
    admission: org_office_launch::writer::Admission,
) -> (std::thread::JoinHandle<()>, Result<(), String>) {
    use org_office_launch::writer::{self as wire, Request, Response};
    use std::os::fd::AsFd;
    let (worker, host) = UnixStream::pair().unwrap();
    host.set_read_timeout(Some(WAIT)).unwrap();
    let thread = std::thread::spawn(move || {
        if mode == "eof" {
            return;
        }
        let mut reader = BufReader::new(host);
        if wire::write(reader.get_mut(), &admission).is_err() {
            return;
        }
        while let Ok(Some(request)) = wire::read::<Request>(&mut reader) {
            let response = match request {
                Request::Checkpoint { identity } if identity == admission.identity => {
                    Response::Checkpoint {
                        position: position.lock().unwrap().clone(),
                    }
                }
                Request::Confirm {
                    identity,
                    before,
                    after,
                } if identity == admission.identity => {
                    let mut p = position.lock().unwrap();
                    if *p != before
                        || !after.valid()
                        || before
                            .as_ref()
                            .is_some_and(|prior| prior != &after && !after.follows(prior))
                    {
                        Response::Denied
                    } else {
                        *p = Some(after);
                        Response::Confirmed
                    }
                }
                Request::Owner { identity }
                    if identity == admission.identity && mode == "allow" =>
                {
                    Response::Owner {
                        account: admission.owner_account.clone(),
                        member: admission.owner_member.clone(),
                    }
                }
                Request::Owner { .. } if mode == "account" => Response::Owner {
                    account: "different-account".into(),
                    member: OWNER.into(),
                },
                _ => Response::Denied,
            };
            if wire::write(reader.get_mut(), &response).is_err() {
                break;
            }
        }
    });
    let record = org_office_launch::encode_transfer_record(
        &org_office_launch::TransferRecord::WriterControl {
            office_id: selected_plan().office_id,
            channel_id: "initial-writer".into(),
        },
    )
    .unwrap();
    let sent = org_socket::rights::send(
        session.transfer.as_ref().unwrap().as_fd(),
        &record,
        &[worker.as_fd()],
        std::time::Instant::now() + WAIT,
    )
    .map(|_| ())
    .map_err(|error| error.to_string());
    *session.in_flight.borrow_mut() = Some(worker);
    (thread, sent)
}
#[test]
fn selected_provider_actual_create_admit_reopen_preserves_ca_and_journal() {
    let _process = process_isolation();
    let world = World::new();
    let p = selected_plan();
    let checkpoint = std::sync::Arc::new(std::sync::Mutex::new(None));
    let package_path = world.home.join("state/security/ca-escrow.v1.bin");
    // Kill after the stored Created marker, before any writer/owner admission.
    // Exact same-operation create must resume the original CA/package.
    let mut interrupted = selected_handshake(&world, false);
    let package = std::fs::read(&package_path).unwrap();
    interrupted.run.child.kill().unwrap();
    assert!(!interrupted.run.wait().success());
    drop(interrupted);
    let mut create = selected_handshake(&world, false);
    assert_eq!(std::fs::read(&package_path).unwrap(), package);
    let thread = selected_writer(&world, &create, checkpoint.clone(), "allow");
    assert_eq!(
        create.event(),
        json!({"v":1,"event":"fds_received","of":"initial-writer"})
    );
    assert_eq!(create.event()["event"], "ready");
    assert_eq!(checkpoint.lock().unwrap().as_ref().unwrap().ordinal, 2);
    let socket =
        org_proto::location::selected_office_socket_in(&world.home.join("run"), &p.office_id)
            .unwrap();
    assert!(socket.exists());
    create.stop();
    thread.join().unwrap();
    assert!(!socket.exists());
    let retained = checkpoint.lock().unwrap().clone();
    let mut reopen = selected_handshake(&world, true);
    let thread = selected_writer(&world, &reopen, checkpoint.clone(), "allow");
    assert_eq!(reopen.event()["event"], "fds_received");
    assert_eq!(reopen.event()["event"], "ready");
    assert_eq!(*checkpoint.lock().unwrap(), retained);
    assert_eq!(std::fs::read(package_path).unwrap(), package);
    reopen.stop();
    thread.join().unwrap();
    let office = org_office::Office::open_for_instance(
        world.folder(&p.office_id),
        org_office::Key::from_office_secrets_key(office_secrets_key()),
        INSTANCE,
    )
    .unwrap();
    let record = office.hq_creation_record().unwrap().unwrap();
    assert_eq!(record.package, package);
    fn bytes(root: &Path) -> Vec<u8> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                out.extend(bytes(&path))
            } else {
                out.extend(std::fs::read(path).unwrap())
            }
        }
        out
    }
    // The unwrapped CA DER is the secret itself; the wrapped record must not
    // leak either (ENC-5).
    let ca_der = org_pki::wrap::unwrap(
        office_secrets_key().expose_bytes(),
        &org_pki::wrap::WrapBinding {
            organization_id: &record.organization_id,
            office_id: &p.office_id,
            purpose: org_pki::wrap::WrapPurpose::OrganizationCa,
        },
        record.wrapped_ca_key(),
    )
    .unwrap();
    assert_eq!(
        org_pki::SigningKey::from_local_der(&ca_der)
            .unwrap()
            .public_key(),
        record.public_key
    );
    for root in [world.home.join("state"), world.home.join("protected")] {
        let published = bytes(&root);
        for secret in [record.wrapped_ca_key(), &ca_der[..]] {
            assert!(!published.windows(secret.len()).any(|s| s == secret));
        }
    }
}
#[test]
fn selected_provider_actual_denied_scope_account_owner_and_eof_never_ready() {
    let _process = process_isolation();
    for mode in ["scope", "account", "revoked", "eof", "missing"] {
        let world = World::new();
        let mut session = selected_handshake(&world, false);
        let thread = (mode != "missing")
            .then(|| selected_writer(&world, &session, std::sync::Arc::default(), mode));
        assert_eq!(session.event()["event"], "failed", "{mode}");
        assert!(!session.run.wait().success(), "{mode}");
        let p = selected_plan();
        assert!(!org_proto::location::selected_office_socket_in(
            &world.home.join("run"),
            &p.office_id
        )
        .unwrap()
        .exists());
        drop(session);
        if let Some(t) = thread {
            t.join().unwrap();
        }
        assert!(world.folder(&p.office_id).join("office.json").exists());
    }
}

struct SelectedRetainedMaterial {
    intent: Vec<u8>,
    marker: Vec<u8>,
    record: org_office::hq_creation::LocalRecord,
}

fn selected_interrupted_material(world: &World) -> SelectedRetainedMaterial {
    let p = selected_plan();
    let mut created = selected_handshake(world, false);
    created.run.child.kill().unwrap();
    assert!(!created.run.wait().success());
    drop(created);
    let folder = world.folder(&p.office_id);
    let office = org_office::Office::open_for_instance(
        &folder,
        org_office::Key::from_office_secrets_key(office_secrets_key()),
        INSTANCE,
    )
    .unwrap();
    SelectedRetainedMaterial {
        intent: std::fs::read(folder.join("hq-creation.v1.json")).unwrap(),
        marker: std::fs::read(folder.join("office.json")).unwrap(),
        record: office.hq_creation_record().unwrap().unwrap(),
    }
}

fn assert_selected_retained_material(world: &World, retained: &SelectedRetainedMaterial) {
    let folder = world.folder(&selected_plan().office_id);
    assert_eq!(
        std::fs::read(folder.join("office.json")).unwrap(),
        retained.marker
    );
    assert_eq!(
        std::fs::read(world.home.join("state/security/ca-escrow.v1.bin")).unwrap(),
        retained.record.package
    );
    // Restore only this deliberately damaged test input after the worker exits,
    // so strict production open can verify that local cryptographic material survived.
    std::fs::write(folder.join("hq-creation.v1.json"), &retained.intent).unwrap();
    let office = org_office::Office::open_for_instance(
        &folder,
        org_office::Key::from_office_secrets_key(office_secrets_key()),
        INSTANCE,
    )
    .unwrap();
    let record = office.hq_creation_record().unwrap().unwrap();
    assert_eq!(record.organization_id, retained.record.organization_id);
    assert_eq!(record.operation_id, retained.record.operation_id);
    assert_eq!(record.public_key, retained.record.public_key);
    assert_eq!(record.package, retained.record.package);
    assert!(record.wrapped_ca_key() == retained.record.wrapped_ca_key());
}

fn selected_expect_start_refusal(
    world: &World,
    retained: &SelectedRetainedMaterial,
    admission: org_office_launch::writer::Admission,
) {
    let mut start = selected_handshake(world, true);
    let (thread, sent) =
        selected_writer_attempt(&start, std::sync::Arc::default(), "allow", admission);
    let first = start.event();
    let outcome = if first["event"] == "fds_received" {
        assert_eq!(
            first,
            json!({"v":1,"event":"fds_received","of":"initial-writer"})
        );
        assert!(
            sent.is_ok(),
            "acknowledged failed descriptor delivery: {sent:?}"
        );
        start.event()
    } else {
        first
    };
    // A broken baseline may emit Ready and stay running. Reap it explicitly so
    // each independent regression fails promptly without leaving a live worker.
    if outcome["event"] != "failed" {
        start.run.child.kill().unwrap();
    }
    let status = start.run.wait();
    start.no_stdout();
    drop(start);
    thread.join().unwrap();
    assert_selected_retained_material(world, retained);
    assert_eq!(
        outcome["event"], "failed",
        "selected startup falsely reached Ready: {outcome}"
    );
    assert!(!status.success());
    let socket = org_proto::location::selected_office_socket_in(
        &world.home.join("run"),
        &selected_plan().office_id,
    )
    .unwrap();
    assert!(!socket.exists(), "refused selected startup bound a socket");
    assert!(
        !world.home.join("protected").exists(),
        "refused creation context installed protected acceptance/owner state"
    );
    let public: Vec<_> = std::fs::read_dir(world.home.join("state"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(public, [std::ffi::OsString::from("security")]);
}

#[test]
fn selected_provider_actual_missing_intent_restart_never_ready() {
    let _process = process_isolation();
    let world = World::new();
    let retained = selected_interrupted_material(&world);
    std::fs::remove_file(
        world
            .folder(&selected_plan().office_id)
            .join("hq-creation.v1.json"),
    )
    .unwrap();
    selected_expect_start_refusal(&world, &retained, selected_admission(&world));
}

#[test]
fn selected_provider_actual_null_bootstrap_restart_never_ready() {
    let _process = process_isolation();
    let world = World::new();
    let retained = selected_interrupted_material(&world);
    let mut intent: Value = serde_json::from_slice(&retained.intent).unwrap();
    intent["bootstrap"] = Value::Null;
    std::fs::write(
        world
            .folder(&selected_plan().office_id)
            .join("hq-creation.v1.json"),
        serde_json::to_vec(&intent).unwrap(),
    )
    .unwrap();
    selected_expect_start_refusal(&world, &retained, selected_admission(&world));
}

#[test]
fn selected_provider_actual_disjoint_public_root_never_ready() {
    let _process = process_isolation();
    let world = World::new();
    let retained = selected_interrupted_material(&world);
    let foreign = tempfile::tempdir().unwrap();
    let mut admission = selected_admission(&world);
    admission.public_root = std::fs::canonicalize(foreign.path())
        .unwrap()
        .to_str()
        .unwrap()
        .into();
    selected_expect_start_refusal(&world, &retained, admission);
    assert_eq!(
        std::fs::read_dir(foreign.path()).unwrap().count(),
        0,
        "mismatched admission published canonical artifacts to the foreign root"
    );
}

#[test]
fn selected_provider_actual_different_creation_owner_never_ready() {
    let _process = process_isolation();
    let world = World::new();
    let retained = selected_interrupted_material(&world);
    let mut admission = selected_admission(&world);
    admission.owner_member = "01ARZ3NDEKTSV4RRFFQ69G5FAY".into();
    assert_ne!(admission.owner_member, OWNER);
    assert!(org_proto::is_valid_member_id(&admission.owner_member));
    selected_expect_start_refusal(&world, &retained, admission);
}
