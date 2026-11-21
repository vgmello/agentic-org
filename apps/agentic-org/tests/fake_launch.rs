//! The fake office server speaks only the launch contract, as the real one does: these tests drive it as the host does
//! (first line on stdin, then `ready_for_secrets`, the handoff header and, for create and start,
//! the transfer socket on its marker byte over the rendezvous socket, then the writer control
//! on the transfer socket with the host's admission), so the app's tests can move to the
//! rendezvous. Offices, organizations and instances are named by canonical UUIDv7, as every
//! launch line names them. Everything runs in a temporary home.

use std::fs;
use std::io::{BufReader, Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use org_office_launch::writer::{Admission, Identity};
use org_office_launch::{FailCode, ServerEvent, CREATE_FLAG, DISCARD_FLAG, OFFICE_FLAG};
use org_office_launch::{HostFirstLine, RendezvousEvent, TransferRecord};
use org_proto::location::{OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::{GrantKey, OfficeSecretsKey, RecoveryCode};
use serde_json::json;

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0";
const OFFICE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1";
const ORGANIZATION: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b1";
const OWNER: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c1";
const WAIT: Duration = Duration::from_secs(20);

fn fake_server() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-office-server"))
}

struct Home {
    _dir: tempfile::TempDir,
    home: PathBuf,
    runtime: PathBuf,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let (home, runtime) = (base.join("h"), base.join("r"));
        fs::create_dir(&home).unwrap();
        // The host makes the offices folder; the fake makes only an office's own.
        fs::create_dir(home.join(org_office_launch::OFFICES_DIR_NAME)).unwrap();
        fs::create_dir(&runtime).unwrap();
        Home {
            _dir: dir,
            home,
            runtime,
        }
    }

    fn log(&self) -> String {
        fs::read_to_string(self.home.join("fake.log")).unwrap_or_default()
    }

    /// Spawns the fake with `args`, writes `line` as its first stdin line and returns it with
    /// the rendezvous listener the line names.
    fn launch(&self, args: &[&str], line: &serde_json::Value) -> (Child, UnixListener) {
        self.spawn(args, line, true)
    }

    /// As [`Home::launch`], but writes `line` as raw JSON (its `instance_id` kept when present),
    /// so a line the typed writer would refuse still reaches the fake.
    fn launch_raw(&self, args: &[&str], line: &serde_json::Value) -> (Child, UnixListener) {
        self.spawn(args, line, false)
    }

    fn spawn(&self, args: &[&str], line: &serde_json::Value, typed: bool) -> (Child, UnixListener) {
        let rendezvous = self.runtime.join("rv");
        let _ = fs::remove_file(&rendezvous);
        let listener = UnixListener::bind(&rendezvous).unwrap();
        let mut child = Command::new(fake_server())
            .args(args)
            .env_clear()
            .env(OFFICE_HOME_ENV, &self.home)
            .env(OFFICE_RUNTIME_DIR_ENV, &self.runtime)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = line.clone();
        line["v"] = json!(org_office_launch::LAUNCH_VERSION);
        if line.get("instance_id").is_none() {
            line["instance_id"] = json!(INSTANCE);
        }
        line["rendezvous"] = json!(rendezvous.to_str().unwrap());
        let stdin = child.stdin.as_mut().unwrap();
        if typed {
            // Through the typed writer, so the line is one the host could send.
            let typed: HostFirstLine = serde_json::from_value(line).unwrap();
            org_office_launch::write_first_line(stdin, &typed).unwrap();
        } else {
            stdin.write_all(format!("{line}\n").as_bytes()).unwrap();
        }
        (child, listener)
    }
}

/// Accepts the fake's connection and reads its `ready_for_secrets`.
fn accept_ready(listener: &UnixListener) -> BufReader<UnixStream> {
    let (stream, _) = listener.accept().unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    let mut reader = BufReader::new(stream);
    let _: org_office_launch::ReadyForSecrets = org_proto::frame::read_message(&mut reader)
        .unwrap()
        .expect("ready_for_secrets");
    reader
}

/// Sends `header`, then the transfer socket on its marker, and reads the handoff receipt. The
/// returned end is the host's half of the transfer pair.
fn hand_off(reader: &mut BufReader<UnixStream>, header: Vec<u8>) -> UnixDatagram {
    reader.get_mut().write_all(&header).unwrap();
    let (host, worker) = UnixDatagram::pair().unwrap();
    org_socket::rights::send(
        reader.get_ref().as_fd(),
        &[org_office_launch::FD_MARKER],
        &[worker.as_fd()],
        Instant::now() + WAIT,
    )
    .unwrap();
    assert_eq!(
        org_office_launch::read_server_event(reader).unwrap(),
        Some(RendezvousEvent::FdsReceived {
            of: "handoff".into()
        })
    );
    host
}

/// The host's admission for `office` of `organization` on this instance, as a reservation
/// mints it.
fn admission(office: &str, organization: &str) -> Admission {
    Admission {
        version: org_office_launch::writer::VERSION,
        identity: Identity {
            instance_id: INSTANCE.into(),
            organization_id: organization.into(),
            office_id: office.into(),
            journal_incarnation: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071d1".into(),
            writer_epoch: 1,
            machine_id: "ab".repeat(32),
        },
        durable_root: format!("/instance/{organization}/durable"),
        runtime_root: format!("/instance/{organization}/runtime"),
        owner_account: "01JD5Z00000000000000000ACC".into(),
        owner_member: OWNER.into(),
    }
}

/// Sends the writer control the host sends right after the handoff: the `writer_control`
/// record naming `office` with the custody channel on the transfer socket `transfer`, and
/// `admission` first on that channel. Returns both ends of the channel: the host's, and the
/// sender's copy of the one it sent, which (as the host does) the caller keeps until the
/// fake has answered, so the descriptor is never only in flight when it is collected.
fn send_writer_control(
    transfer: &UnixDatagram,
    office: &str,
    admission: &Admission,
) -> (UnixStream, UnixStream) {
    let (host, worker) = UnixStream::pair().unwrap();
    let header = org_office_launch::encode_transfer_record(&TransferRecord::WriterControl {
        office_id: office.into(),
        channel_id: "01JD5Z0000000000000000CH01".into(),
    })
    .unwrap();
    org_socket::rights::send(
        transfer.as_fd(),
        &header,
        &[worker.as_fd()],
        Instant::now() + WAIT,
    )
    .unwrap();
    org_office_launch::writer::write(&mut &host, admission).unwrap();
    (host, worker)
}

/// The writer control as a valid host sends it, and its receipt; returns the channel's host
/// end, which the fake holds for its life.
fn writer_control(
    reader: &mut BufReader<UnixStream>,
    transfer: &UnixDatagram,
    organization: &str,
) -> UnixStream {
    let (channel, sent) = send_writer_control(transfer, OFFICE, &admission(OFFICE, organization));
    assert_eq!(
        event(reader),
        Some(RendezvousEvent::FdsReceived {
            of: "01JD5Z0000000000000000CH01".into()
        })
    );
    drop(sent);
    channel
}

fn event(reader: &mut BufReader<UnixStream>) -> Option<RendezvousEvent> {
    org_office_launch::read_server_event(reader).unwrap()
}

fn wait_exit(child: &mut Child) -> i32 {
    let until = Instant::now() + WAIT;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code().unwrap_or(-1);
        }
        assert!(Instant::now() < until, "the fake didn't exit");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn stdout_of(child: &mut Child) -> String {
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    out
}

fn log_has(log: &str, prefix: &str) -> bool {
    log.lines().any(|line| line.starts_with(prefix))
}

/// The create line a host sends for `OFFICE`: every create is a protected HQ creation with
/// its bootstrap.
fn create_line() -> serde_json::Value {
    use org_proto::selected_bootstrap::{CreatorV1, PlanV1, PreparedV1};
    let prepared = PreparedV1 {
        plan: PlanV1 {
            version: 1,
            name: "Acme".into(),
            slug: "acme".into(),
            repository_remote_url: "https://example.test/org.git".into(),
            state_branch: "org/state".into(),
        },
        organization_id: ORGANIZATION.into(),
        office_id: OFFICE.into(),
        repository_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071e1".into(),
        home_team_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071e2".into(),
        hr_employee_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071e3".into(),
        creator: CreatorV1::from_login(OWNER.into(), "Ana"),
    };
    let line = HostFirstLine::Create(org_office_launch::CreateLine {
        office: org_office_launch::OfficeLine {
            instance_id: INSTANCE.into(),
            office_id: OFFICE.into(),
            rendezvous: String::new(),
        },
        org_id: ORGANIZATION.into(),
        org_name: "Acme".into(),
        office_name: "Headquarters".into(),
        role: org_proto::OfficeRole::Hq,
        owner_member_id: OWNER.into(),
        hq_creation: org_office_launch::HqCreationV1 {
            version: org_office_launch::HqCreationV1::VERSION,
            operation_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071f1".into(),
            escrow_profile: 1,
            bootstrap: Box::new(prepared),
        },
    });
    serde_json::to_value(&line).unwrap()
}

fn create(home: &Home) {
    let (mut child, listener) = home.launch(&[CREATE_FLAG], &create_line());
    let mut reader = accept_ready(&listener);
    let header = org_office_launch::encode_create_handoff(
        &OfficeSecretsKey::from_bytes([7; 32]),
        &GrantKey::from_bytes([9; 32]),
        &RecoveryCode::new("a recovery code".into()).unwrap(),
    )
    .unwrap();
    let transfer = hand_off(&mut reader, header);
    let _channel = writer_control(&mut reader, &transfer, ORGANIZATION);
    match event(&mut reader) {
        Some(RendezvousEvent::Server(ServerEvent::Created { office })) => {
            assert_eq!(office.office_id, OFFICE);
            assert_eq!(office.org_id, ORGANIZATION);
            assert_eq!(
                office.version(),
                org_office_launch::SELECTED_OFFICE_FILE_VERSION
            );
        }
        other => panic!("expected created, got {other:?}"),
    }
    assert!(matches!(
        event(&mut reader),
        Some(RendezvousEvent::Server(ServerEvent::Ready { .. }))
    ));
    // The rendezvous stream's end is the host's end too: stdin stays open.
    drop(reader);
    assert_eq!(wait_exit(&mut child), 0);
    assert_eq!(
        stdout_of(&mut child),
        "",
        "launch events never go to stdout"
    );
    let pid = child.id();
    let log = home.log();
    for line in [
        format!("launch create {pid} v1"),
        format!("instance create {pid} {INSTANCE}"),
        format!("grant create {pid} "),
        format!("recovery create {pid} present"),
        format!("transfer create {pid} received"),
        format!("writer create {pid} {ORGANIZATION}"),
        format!("secrets-key create {pid} "),
    ] {
        assert!(log_has(&log, &line), "no `{line}` in:\n{log}");
    }
    assert!(!log.contains("a recovery code"), "the fake logged a secret");
}

#[test]
fn the_fake_creates_and_starts_an_office_over_the_rendezvous() {
    let home = Home::new();
    create(&home);

    let (mut child, listener) = home.launch(
        &[OFFICE_FLAG, OFFICE],
        &json!({"op": "start", "office_id": OFFICE}),
    );
    let mut reader = accept_ready(&listener);
    let header = org_office_launch::encode_start_handoff(
        Some(&OfficeSecretsKey::from_bytes([7; 32])),
        &GrantKey::from_bytes([10; 32]),
    )
    .unwrap();
    let transfer = hand_off(&mut reader, header);
    let _channel = writer_control(&mut reader, &transfer, ORGANIZATION);
    match event(&mut reader) {
        Some(RendezvousEvent::Server(ServerEvent::Ready { socket, .. })) => {
            assert!(
                UnixStream::connect(socket).is_ok(),
                "the office socket answers"
            )
        }
        other => panic!("expected ready, got {other:?}"),
    }
    // Here stdin's end ends it, the rendezvous stream still open.
    drop(child.stdin.take());
    assert_eq!(wait_exit(&mut child), 0);
    drop(reader);
    let pid = child.id();
    let log = home.log();
    assert!(log_has(&log, &format!("launch {OFFICE} {pid} v1")), "{log}");
    assert!(
        log_has(&log, &format!("transfer {OFFICE} {pid} received")),
        "{log}"
    );
    assert!(
        log_has(&log, &format!("writer {OFFICE} {pid} {ORGANIZATION}")),
        "{log}"
    );
    assert!(!log_has(&log, &format!("recovery {OFFICE} {pid}")), "{log}");
}

/// As the real worker: an office opens only under the custody the host admits it to. An
/// admission for another office fails the start on the stream before it is ready, with the
/// worker's custody refusal (`storage_error`, which the host retries).
#[test]
fn the_fake_refuses_a_start_whose_writer_admission_names_another_office() {
    let home = Home::new();
    create(&home);
    let other = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a2";
    let (mut child, listener) = home.launch(
        &[OFFICE_FLAG, OFFICE],
        &json!({"op": "start", "office_id": OFFICE}),
    );
    let mut reader = accept_ready(&listener);
    let header = org_office_launch::encode_start_handoff(
        Some(&OfficeSecretsKey::from_bytes([7; 32])),
        &GrantKey::from_bytes([10; 32]),
    )
    .unwrap();
    let transfer = hand_off(&mut reader, header);
    let _channel = send_writer_control(&transfer, OFFICE, &admission(other, ORGANIZATION));
    // Both ends are held until the refusal: the fake refused the admission, not a channel
    // that went away.
    assert_eq!(failed_then_end(&mut reader), FailCode::StorageError);
    assert_eq!(wait_exit(&mut child), 1);
    let pid = child.id();
    assert!(!log_has(&home.log(), &format!("writer {OFFICE} {pid}")));
}

/// As the real worker: a start takes the writer control (its receipt comes first), then
/// refuses an admission for another organization than its stored office's with the custody
/// refusal, before it is ready.
#[test]
fn the_fake_refuses_a_start_whose_admission_names_another_organization() {
    let home = Home::new();
    create(&home);
    let (mut child, listener) = home.launch(
        &[OFFICE_FLAG, OFFICE],
        &json!({"op": "start", "office_id": OFFICE}),
    );
    let mut reader = accept_ready(&listener);
    let header = org_office_launch::encode_start_handoff(
        Some(&OfficeSecretsKey::from_bytes([7; 32])),
        &GrantKey::from_bytes([10; 32]),
    )
    .unwrap();
    let transfer = hand_off(&mut reader, header);
    let other = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b2";
    let _channel = send_writer_control(&transfer, OFFICE, &admission(OFFICE, other));
    assert_eq!(
        event(&mut reader),
        Some(RendezvousEvent::FdsReceived {
            of: "01JD5Z0000000000000000CH01".into()
        })
    );
    assert_eq!(failed_then_end(&mut reader), FailCode::StorageError);
    assert_eq!(wait_exit(&mut child), 1);
}

/// As the real worker: a create takes the writer control (its receipt comes first), then
/// refuses an admission for another owner of its organization with the custody refusal, and
/// stores nothing.
#[test]
fn the_fake_refuses_a_create_whose_admission_names_another_owner() {
    let home = Home::new();
    let (mut child, listener) = home.launch(&[CREATE_FLAG], &create_line());
    let mut reader = accept_ready(&listener);
    let header = org_office_launch::encode_create_handoff(
        &OfficeSecretsKey::from_bytes([7; 32]),
        &GrantKey::from_bytes([9; 32]),
        &RecoveryCode::new("a recovery code".into()).unwrap(),
    )
    .unwrap();
    let transfer = hand_off(&mut reader, header);
    let mut other_owner = admission(OFFICE, ORGANIZATION);
    other_owner.owner_member = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c2".into();
    let _channel = send_writer_control(&transfer, OFFICE, &other_owner);
    assert_eq!(
        event(&mut reader),
        Some(RendezvousEvent::FdsReceived {
            of: "01JD5Z0000000000000000CH01".into()
        })
    );
    assert_eq!(failed_then_end(&mut reader), FailCode::StorageError);
    assert_eq!(wait_exit(&mut child), 1);
    let office = home
        .home
        .join(org_office_launch::OFFICES_DIR_NAME)
        .join(OFFICE);
    assert!(
        !office.join("office.json").exists(),
        "the refused create stored an office"
    );
}

/// As the real worker: a discard never removes a folder the host reserved writer custody in
/// (its marker), whatever else it holds: that is the reservation's, not an unfinished create.
#[test]
fn the_fake_refuses_to_discard_a_folder_holding_the_custody_marker() {
    let home = Home::new();
    let (mut child, listener) = home.launch(
        &[DISCARD_FLAG, OFFICE],
        &json!({"op": "discard", "office_id": OFFICE}),
    );
    let mut reader = accept_ready(&listener);
    let dir = home
        .home
        .join(org_office_launch::OFFICES_DIR_NAME)
        .join(OFFICE);
    fs::create_dir(&dir).unwrap();
    fs::write(
        dir.join(org_office_launch::writer::marker::MARKER),
        b"selected-writer/1\n",
    )
    .unwrap();
    fs::write(dir.join("office.json"), b"{}").unwrap();
    let header = org_office_launch::encode_discard_handoff().unwrap();
    reader.get_mut().write_all(&header).unwrap();
    assert_eq!(failed_then_end(&mut reader), FailCode::StorageError);
    assert_eq!(wait_exit(&mut child), 1);
    assert!(dir.exists(), "the discard removed a reserved folder");
}

#[test]
fn the_fake_discards_over_the_rendezvous_without_a_transfer_socket() {
    let home = Home::new();
    let (mut child, listener) = home.launch(
        &[DISCARD_FLAG, OFFICE],
        &json!({"op": "discard", "office_id": OFFICE}),
    );
    let mut reader = accept_ready(&listener);
    // An unfinished office's folder holding databases: a launch discard removes it without a key.
    let dir = home
        .home
        .join(org_office_launch::OFFICES_DIR_NAME)
        .join(OFFICE);
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("db"), b"databases").unwrap();
    // The host never sends a key with a discard now.
    let header = org_office_launch::encode_discard_handoff().unwrap();
    reader.get_mut().write_all(&header).unwrap();
    // No marker is sent and no receipt comes: the first event is the report.
    assert_eq!(
        event(&mut reader),
        Some(RendezvousEvent::Server(ServerEvent::Discarded))
    );
    assert_eq!(wait_exit(&mut child), 0);
    assert!(!dir.exists(), "the discard left the office folder");
    let pid = child.id();
    let log = home.log();
    assert!(log_has(&log, &format!("launch discard {pid} v1")), "{log}");
    assert!(!log.contains("transfer discard"), "{log}");
}

/// Reads the rendezvous stream to its end: the one `failed` it carries, then nothing.
fn failed_then_end(reader: &mut BufReader<UnixStream>) -> FailCode {
    let code = match event(reader) {
        Some(RendezvousEvent::Server(ServerEvent::Failed { code, .. })) => code,
        other => panic!("expected failed, got {other:?}"),
    };
    assert_eq!(event(reader), None, "nothing follows `failed`");
    code
}

#[test]
fn the_fake_refuses_a_start_line_naming_another_office_before_connecting() {
    let home = Home::new();
    let (mut child, listener) = home.launch(
        &[OFFICE_FLAG, OFFICE],
        &json!({"op": "start", "office_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a2"}),
    );
    assert_eq!(wait_exit(&mut child), 1);
    listener.set_nonblocking(true).unwrap();
    assert!(
        matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "the fake connected to the rendezvous"
    );
    // Not connected, so the refusal reaches no one: stdout carries nothing.
    assert_eq!(stdout_of(&mut child), "");
    let pid = child.id();
    let log = home.log();
    assert!(
        log_has(&log, &format!("first-line-refused {OFFICE} {pid}")),
        "{log}"
    );
    assert!(!log_has(&log, &format!("transfer {OFFICE} {pid}")), "{log}");
}

/// The instance id is canonical lowercase UUIDv7 text. A ULID (the earlier form) or uppercase
/// UUIDv7 is refused before connecting, as the real server refuses it.
#[test]
fn the_fake_refuses_a_launch_line_naming_a_ulid_instance() {
    for instance in [
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "0190A5D2-7C3E-7A01-8B2C-3D4E5F6071C0",
    ] {
        let home = Home::new();
        let (mut child, listener) = home.launch_raw(
            &[OFFICE_FLAG, OFFICE],
            &json!({"op": "start", "office_id": OFFICE, "instance_id": instance}),
        );
        assert_eq!(wait_exit(&mut child), 1, "{instance}");
        listener.set_nonblocking(true).unwrap();
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "the fake connected to the rendezvous for {instance}"
        );
        assert_eq!(stdout_of(&mut child), "");
        let pid = child.id();
        let log = home.log();
        assert!(
            log_has(&log, &format!("first-line-refused {OFFICE} {pid}")),
            "{log}"
        );
        assert!(!log.contains("instance "), "{log}");
    }
}

#[test]
fn the_fake_refuses_a_handoff_whose_op_is_not_the_start_lines() {
    let create = org_office_launch::encode_create_handoff(
        &OfficeSecretsKey::from_bytes([7; 32]),
        &GrantKey::from_bytes([9; 32]),
        &RecoveryCode::new("a recovery code".into()).unwrap(),
    )
    .unwrap();
    let discard = org_office_launch::encode_discard_handoff().unwrap();
    for header in [create, discard] {
        let home = Home::new();
        let (mut child, listener) = home.launch(
            &[OFFICE_FLAG, OFFICE],
            &json!({"op": "start", "office_id": OFFICE}),
        );
        let mut reader = accept_ready(&listener);
        reader.get_mut().write_all(&header).unwrap();
        // The first event is the refusal: no `fds_received` precedes it.
        assert_eq!(failed_then_end(&mut reader), FailCode::InvalidRequest);
        assert_eq!(wait_exit(&mut child), 1);
        assert_eq!(
            stdout_of(&mut child),
            "",
            "launch events never go to stdout"
        );
        let pid = child.id();
        let log = home.log();
        assert!(
            log_has(&log, &format!("handoff-op-refused {OFFICE} {pid}")),
            "{log}"
        );
        assert!(!log_has(&log, &format!("transfer {OFFICE} {pid}")), "{log}");
    }
}

#[test]
fn the_fake_fails_a_bad_header_on_the_stream_not_stdout() {
    // An oversized header (no newline within the limit), then a header byte carrying an fd.
    type Send = fn(&UnixStream);
    let oversized: Send = |stream| {
        let mut stream = stream;
        stream
            .write_all(&vec![
                b'a';
                org_office_launch::TRANSFER_HEADER_MAX_BYTES + 1
            ])
            .unwrap();
    };
    let fd_on_header: Send = |stream| {
        let (_spare, carried) = UnixDatagram::pair().unwrap();
        org_socket::rights::send(
            stream.as_fd(),
            b"{",
            &[carried.as_fd()],
            Instant::now() + WAIT,
        )
        .unwrap();
    };
    for send in [oversized, fd_on_header] {
        let home = Home::new();
        let (mut child, listener) = home.launch(
            &[OFFICE_FLAG, OFFICE],
            &json!({"op": "start", "office_id": OFFICE}),
        );
        let mut reader = accept_ready(&listener);
        send(reader.get_ref());
        // A whole `failed` event: `bail` made the stream blocking again before writing it.
        assert_eq!(failed_then_end(&mut reader), FailCode::InvalidRequest);
        assert_eq!(wait_exit(&mut child), 1);
        assert_eq!(
            stdout_of(&mut child),
            "",
            "launch events never go to stdout"
        );
        let pid = child.id();
        assert!(!log_has(&home.log(), &format!("transfer {OFFICE} {pid}")));
    }
}

/// A retired v2 line (here a discard, which once needed no rendezvous) is refused by its version
/// before anything else, as the real server refuses it: nothing is removed or reported.
#[test]
fn the_fake_refuses_a_v2_line() {
    let home = Home::new();
    let mut child = Command::new(fake_server())
        .args([DISCARD_FLAG, OFFICE])
        .env_clear()
        .env(OFFICE_HOME_ENV, &home.home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let line = json!({"v": 2, "op": "discard", "db_key": null});
    let stdin = child.stdin.as_mut().unwrap();
    stdin.write_all(format!("{line}\n").as_bytes()).unwrap();
    assert_eq!(wait_exit(&mut child), 1);
    assert_eq!(stdout_of(&mut child), "");
    let pid = child.id();
    let log = home.log();
    assert!(
        log_has(&log, &format!("first-line-refused discard {pid}")),
        "{log}"
    );
    assert!(!log.contains("launch discard"), "{log}");
}
