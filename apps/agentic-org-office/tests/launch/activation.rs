//! X-02 rev15 §5.1/5.2/5.4: a fake host drives the real worker independently of 2b.4.
use super::*;
use agentic_org_office::test_support::FakeWriter;
use org_office_launch::{CreateLine, Handoff, HandoffFd, HostFirstLine, OfficeLine};
use org_proto::{GrantKey, RecoveryCode};
use serde_json::{json, Value};
use std::io::BufRead;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixDatagram, UnixListener};

pub(super) const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607182";
/// The creation's launch operation, a ULID as the host makes it.
const OPERATION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";

/// The organization the host prepared: this file's office, organization and owner.
fn plan() -> org_proto::selected_bootstrap::PreparedV1 {
    org_proto::selected_bootstrap::PreparedV1 {
        plan: org_proto::selected_bootstrap::PlanV1 {
            version: 1,
            name: "Acme".into(),
            slug: "acme".into(),
            repository_remote_url: "https://example.test/org.git".into(),
            state_branch: "org/state".into(),
        },
        organization_id: ORG.into(),
        office_id: OFFICE.into(),
        repository_id: "019a8928-1337-7456-8456-0123456789ad".into(),
        home_team_id: TOP_TEAM.into(),
        hr_employee_id: "019a8928-1337-7456-8456-0123456789af".into(),
        creator: org_proto::selected_bootstrap::CreatorV1::from_login(OWNER.into(), "Ana"),
    }
}

fn office_line(path: &Path) -> OfficeLine {
    OfficeLine {
        id_profile: Some(org_office_launch::IdProfile::SelectedV1),
        instance_id: INSTANCE.into(),
        office_id: OFFICE.into(),
        rendezvous: path.to_str().unwrap().into(),
    }
}

fn create_line(path: &Path) -> HostFirstLine {
    HostFirstLine::Create(CreateLine {
        office: office_line(path),
        org_id: ORG.into(),
        org_name: "Acme".into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        owner_member_id: OWNER.into(),
        hq_creation: Some(org_office_launch::HqCreationV1 {
            version: org_office_launch::HqCreationV1::VERSION,
            operation_id: OPERATION.into(),
            escrow_profile: 1,
            bootstrap: Some(Box::new(plan())),
        }),
    })
}

/// The channel ID the fake host gives the writer control.
const WRITER_CHANNEL: &str = "initial-writer";

/// Sends the host's writer control on `transfer`, as the host does right after the handoff:
/// the worker's end of a new custody channel that `writer` serves. Returns that end, which
/// the host keeps until the worker answers.
fn send_writer(writer: &FakeWriter, transfer: &UnixDatagram) -> UnixStream {
    use std::os::fd::AsFd;
    let (worker, _thread) = writer.channel();
    let record = org_office_launch::encode_transfer_record(
        &org_office_launch::TransferRecord::WriterControl {
            office_id: OFFICE.into(),
            channel_id: WRITER_CHANNEL.into(),
        },
    )
    .unwrap();
    // A worker that already refused its launch is gone: what it says is the case's to check.
    let _ = org_socket::rights::send(
        transfer.as_fd(),
        &record,
        &[worker.as_fd()],
        std::time::Instant::now() + WAIT,
    );
    worker
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
    // The writer control comes next, before anything is created.
    let writer = send_writer(&world.writer, &host_end);
    assert_eq!(
        line(&mut reader),
        json!({"v":1,"event":"fds_received","of":WRITER_CHANNEL})
    );
    drop(writer);
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
    /// The host's copy of the writer control's descriptor, until the worker acknowledges it.
    writer: std::cell::RefCell<Option<UnixStream>>,
}
impl Session {
    /// The next launch event on the rendezvous.
    /// The writer control's receipt is taken here: it is the host's bookkeeping.
    pub(super) fn event(&mut self) -> Value {
        let mut event = line(&mut self.reader);
        if event == json!({"v":1,"event":"fds_received","of":WRITER_CHANNEL}) {
            self.writer.get_mut().take();
            event = line(&mut self.reader);
        }
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
    pub(super) fn stop(mut self) {
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
    let office = office_line(rendezvous);
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

/// `handshake`, with the create line changed first.
fn handshake_with(
    world: &World,
    op: FirstLine,
    instance: &str,
    key: Option<OfficeSecretsKey>,
    edit: impl FnOnce(&mut CreateLine),
) -> Session {
    handshake_as(world, op, instance, key, edit, Some(&world.writer))
}

/// The host's side of a launch: the first line, the handoff and, for a create or a start,
/// the writer control from `writer` (none: the host sends none). The rendezvous sits beside
/// the home, so a home whose own paths are too long can still be reached.
fn handshake_as(
    world: &World,
    op: FirstLine,
    instance: &str,
    key: Option<OfficeSecretsKey>,
    edit: impl FnOnce(&mut CreateLine),
    writer: Option<&FakeWriter>,
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
    let writer = match (&transfer_host, writer) {
        (Some(transfer), Some(writer)) => Some(send_writer(writer, transfer)),
        _ => None,
    };
    Session {
        run,
        stream,
        reader,
        transfer: transfer_host,
        in_flight: Default::default(),
        writer: std::cell::RefCell::new(writer),
    }
}
pub(super) fn ready_create(world: &World) -> Session {
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
/// The office reopens in process for its own instance and custody only.
fn assert_reopens(world: &World) {
    let (custody, roots) = world.writer.admitted();
    assert!(matches!(
        org_office::journal_office::Office::open(
            world.folder(OFFICE),
            Some(org_office::Key::from_office_secrets_key(
                office_secrets_key()
            )),
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f607184",
            roots,
            custody,
        ),
        Err(org_office::OfficeError::Invalid(_))
    ));
    let office = world.office();
    assert!(office.org_snapshot().is_ok());
    office.close().unwrap();
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
    let before = stored_bytes(&world);
    // The same operation id resumes its own creation (ADR-0011). Another operation is another
    // request than the frozen intent, so it is refused, not a new office.
    let code = refused_create(&world, |create| {
        create.hq_creation.as_mut().unwrap().operation_id = "01ARZ3NDEKTSV4RRFFQ79G5FAY".into();
    });
    assert_eq!(code, "already_exists");
    assert_eq!(stored_bytes(&world), before);
    assert!(world.folder(OFFICE).join("office.json").exists());
}

#[test]
fn hq_creation_with_a_bad_or_untrimmed_name_is_invalid_name_and_leaves_nothing() {
    let _process = process_isolation();
    let world = World::new();
    // The organization's name is the prepared plan's, which the launch contract checks before
    // the line is written; the office's name is the line's alone.
    for office_name in ["zero\u{200B}width", " HQ", "HQ "] {
        let code = refused_create(&world, |create| {
            create.office_name = office_name.into();
        });
        assert_eq!(code, "invalid_name", "{office_name:?}");
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
    let world = World::in_dir(dir, home);
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
    // The worker closed its office in order: the store's clean-stop marker is there.
    assert!(
        world.office().stopped_cleanly(),
        "the worker did not close its office"
    );
    assert_reopens(&world);
}

#[test]
fn stdin_is_unparsed_while_launch_allows_agent_binds_and_refuses_app_person_binds() {
    let _process = process_isolation();
    let world = World::new();
    // The creation's HR lead is an agent with no profile and no office yet (agents are hired
    // through HR from B2 on): no agent of this office can bind until then.
    let hr = plan().hr_employee_id;
    let mut session = started(&world);
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
    // No member is not a bind at all (the app-wide bind is gone). The retired `app_token` is
    // refused on presence, before the member is looked at. A person is refused the same way
    // as an unknown member.
    for (params, expected) in [
        (serde_json::json!({}), ErrorCode::InvalidRequest),
        (
            serde_json::json!({"app_token": "a7".repeat(32)}),
            ErrorCode::InvalidRequest,
        ),
        (
            serde_json::json!({"member": "owner", "app_token": "a7".repeat(32)}),
            ErrorCode::InvalidRequest,
        ),
        (
            serde_json::json!({"member": hr, "app_token": null}),
            ErrorCode::InvalidRequest,
        ),
        (
            serde_json::json!({"member": "owner"}),
            ErrorCode::NotAuthorized,
        ),
        (
            serde_json::json!({ "member": hr }),
            ErrorCode::NotAuthorized,
        ),
    ] {
        assert!(
            matches!(client.call::<RawBind>(&params), Err(org_client::ClientError::Api(error)) if error.code == expected),
            "{params}"
        );
    }
    // The connection survives its refusals.
    assert_eq!(client.hello().unwrap().api_version, 1);
    // This distinguishes a rendezvous watcher from a pipe-EOF watcher without a sleep:
    // the undefined control byte must produce exit1; a pipe-authoritative worker exits0.
    session.stream.write_all(b"!").unwrap();
    assert_eq!(session.run.wait().code(), Some(1));
    session.no_stdout();
}

/// Every file the office keeps: its folder and the organization's data roots.
fn stored_bytes(world: &World) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn walk(dir: &Path, found: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for item in std::fs::read_dir(dir).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                walk(&path, found);
            } else {
                found.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
    }
    let mut found = std::collections::BTreeMap::new();
    walk(&world.home.join("offices"), &mut found);
    walk(&world.data, &mut found);
    found
}

/// A start for another instance is refused before the office is opened, and writes
/// nothing; a wrong secrets key still serves the office (it only wraps private keys).
#[test]
fn start_matches_its_instance_and_refuses_another_without_writes() {
    let _process = process_isolation();
    let world = World::new();
    for _ in 0..2 {
        let mut session = started(&world);
        assert_eq!(hello(&world).office.id, OFFICE);
        assert!(session.host_goes_away().success());
    }
    assert_reopens(&world);
    let before = stored_bytes(&world);
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
    assert_eq!(stored_bytes(&world), before);
    let mut wrong_key = handshake(
        &world,
        FirstLine::Start,
        INSTANCE,
        Some(OfficeSecretsKey::from_bytes([0; 32])),
    );
    // ADR-0016 decision 3: the key only wraps private keys.
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

/// A folder the host reserved writer custody in is never discarded, whether it holds the
/// marker or only a staging file of it (a host killed while installing it), even with nothing
/// else there: the refusal is the fixed storage error, not "an office exists".
#[test]
fn discard_refuses_a_folder_holding_the_hosts_custody_marker_or_its_staging() {
    let _process = process_isolation();
    for name in [
        "selected-writer.required",
        ".selected-writer-01ARZ3NDEK.tmp",
    ] {
        let world = World::new();
        std::fs::create_dir_all(world.folder(OFFICE)).unwrap();
        let marker = world.folder(OFFICE).join(name);
        std::fs::write(&marker, b"selected").unwrap();
        let mut session = handshake(&world, FirstLine::Discard, INSTANCE, None);
        let failed = session.event();
        assert_eq!(failed["event"], "failed", "{name}: {failed}");
        assert_eq!(failed["code"], "storage_error", "{name}: {failed}");
        assert!(!session.run.wait().success());
        session.no_stdout();
        assert_eq!(std::fs::read(&marker).unwrap(), b"selected", "{name}");
    }
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
            office_id: "019a8928-1337-7456-8456-0123456789ff".into(),
            ..office_line(&path)
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

/// The CA package the creation published.
fn package(world: &World) -> Vec<u8> {
    std::fs::read(world.data.join("durable/state/security/ca-escrow.v1.bin")).unwrap()
}

/// A worker create retried after its worker was killed, at the writer control's receipt or
/// after `created`, completes over the same host reservation (the same custody) with one CA:
/// the retry resumes the frozen intent, and every later start serves the same CA.
#[test]
fn a_create_retried_after_its_worker_was_killed_completes_with_one_ca() {
    let _process = process_isolation();
    for after_created in [false, true] {
        let world = World::new();
        let mut interrupted = handshake(
            &world,
            FirstLine::Create,
            INSTANCE,
            Some(office_secrets_key()),
        );
        if after_created {
            assert_eq!(interrupted.event()["event"], "created");
        } else {
            // The writer control's receipt: the creation is under way.
            let receipt = line(&mut interrupted.reader);
            assert_eq!(receipt["of"], WRITER_CHANNEL, "{receipt}");
        }
        interrupted.run.child.kill().unwrap();
        assert!(!interrupted.run.wait().success());
        drop(interrupted);
        let retry = ready_create(&world);
        let created = package(&world);
        let head = world.writer.head().expect("the creation was confirmed");
        assert!(head.ordinal >= 1, "{head:?}");
        retry.stop();
        // The same request again, on a finished office: the same CA, never a second.
        ready_create(&world).stop();
        assert_eq!(package(&world), created, "after_created={after_created}");
        let mut start = started(&world);
        assert_eq!(hello(&world).office.id, OFFICE);
        assert!(start.host_goes_away().success());
        assert_eq!(package(&world), created);
        let office = world.office();
        let record = office.hq_creation_record().unwrap().unwrap();
        assert_eq!(record.package, created);
        // The records name the store operation the launch's ULID maps to (M7).
        assert!(org_proto::is_uuid_v7(&record.operation_id));
        office.close().unwrap();
    }
}

/// A launch whose writer control is missing or whose admission the worker refuses never
/// becomes ready and binds no socket: every office needs the host's custody.
#[test]
fn a_launch_without_valid_writer_custody_is_refused_and_never_ready() {
    let _process = process_isolation();
    // A create checks the admission's owner against its line; a start checks the roots
    // against the intent its creation froze, and the owner against the office's owner.
    for (op, modes) in [
        (
            FirstLine::Create,
            &["missing", "scope", "account", "owner"][..],
        ),
        (
            FirstLine::Start,
            &["missing", "scope", "account", "roots", "owner"][..],
        ),
    ] {
        for &mode in modes {
            let world = World::new();
            if op == FirstLine::Start {
                seed(&world);
            }
            let before = stored_bytes(&world);
            let mut admission = world.writer.admission().clone();
            match mode {
                "scope" => {
                    admission.identity.office_id = "019a8928-1337-7456-8456-0123456789ff".into()
                }
                "account" => admission.owner_account = "test-account".into(),
                "owner" => admission.owner_member = "01900000-0000-7000-8000-0000000000ef".into(),
                "roots" => admission.runtime_root = world.home.join("run").to_str().unwrap().into(),
                _ => {}
            }
            let writer = FakeWriter::new(admission);
            // The host's custody of the seeded office: the head its creation confirmed, so
            // only the changed admission field can refuse the start.
            writer.set_head(world.writer.head());
            let mut session = handshake_as(
                &world,
                op,
                INSTANCE,
                Some(office_secrets_key()),
                |_| {},
                (mode != "missing").then_some(&writer),
            );
            let failed = session.event();
            assert_eq!(failed["event"], "failed", "{op:?} {mode}: {failed}");
            assert_eq!(failed["code"], "storage_error", "{op:?} {mode}");
            if mode != "roots" {
                assert_eq!(
                    failed["message"], "protected writer admission requires recovery",
                    "{op:?} {mode}"
                );
            }
            assert!(!session.run.wait().success(), "{op:?} {mode}");
            session.no_stdout();
            assert!(!socket(&world).exists(), "{op:?} {mode}");
            if op == FirstLine::Create {
                assert!(
                    !world.folder(OFFICE).exists(),
                    "{op:?} {mode}: a create without custody stored something"
                );
            } else if mode == "owner" {
                // The owner is known only once the office is open: opening it may rewrite
                // SQLite's shared-memory index of the runtime projection, and the clean-stop
                // marker it consumes is written back unchanged by the orderly stop.
                let without_shm = |bytes: std::collections::BTreeMap<PathBuf, Vec<u8>>| {
                    bytes
                        .into_iter()
                        .filter(|(path, _)| !path.to_string_lossy().ends_with("-shm"))
                        .collect::<std::collections::BTreeMap<_, _>>()
                };
                assert_eq!(
                    without_shm(stored_bytes(&world)),
                    without_shm(before),
                    "{op:?} {mode}"
                );
            } else {
                assert_eq!(stored_bytes(&world), before, "{op:?} {mode}");
            }
        }
    }
}

/// A finished office whose creation intent is gone is not this build's: never ready.
#[test]
fn a_start_without_the_creation_intent_is_refused_and_never_ready() {
    let _process = process_isolation();
    let world = World::new();
    seed(&world);
    std::fs::remove_file(world.folder(OFFICE).join("hq-creation.v1.json")).unwrap();
    let session = handshake(
        &world,
        FirstLine::Start,
        INSTANCE,
        Some(office_secrets_key()),
    );
    assert_eq!(failed_code(session), "storage_error");
    assert!(!socket(&world).exists());
}

/// `msg.bind` with params written by hand, to send what the typed params cannot (the retired
/// `app_token`).
struct RawBind;

impl org_proto::Call for RawBind {
    const METHOD: &'static str = "msg.bind";
    type Params = serde_json::Value;
    type Result = serde_json::Value;
}
