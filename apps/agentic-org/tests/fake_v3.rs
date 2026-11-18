//! The fake office server speaks only launch v3, as the real one does: these tests drive it as the host does
//! (first line on stdin, then `ready_for_secrets`, the handoff header and, for create and start,
//! the transfer socket on its marker byte over the rendezvous socket), so the app's tests can
//! move to the rendezvous. Everything runs in a temporary home.

use std::fs;
use std::io::{BufReader, Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use org_office_launch::v3::{self, HostFirstLine, RendezvousEvent};
use org_office_launch::{FailCode, ServerEvent, CREATE_FLAG, DISCARD_FLAG, OFFICE_FLAG};
use org_proto::location::{OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::{DbKey, GrantKey, RecoveryCode};
use serde_json::json;

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0";
const OFFICE: &str = "01JD5Z0000000000000000OFFA";
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
        line["v"] = json!(3);
        if line.get("instance_id").is_none() {
            line["instance_id"] = json!(INSTANCE);
        }
        line["rendezvous"] = json!(rendezvous.to_str().unwrap());
        let stdin = child.stdin.as_mut().unwrap();
        if typed {
            // Through the typed writer, so the line is one the host could send.
            let typed: HostFirstLine = serde_json::from_value(line).unwrap();
            v3::write_first_line(stdin, &typed).unwrap();
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
    let _: v3::ReadyForSecrets = org_proto::frame::read_message(&mut reader)
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
        &[v3::FD_MARKER],
        &[worker.as_fd()],
        Instant::now() + WAIT,
    )
    .unwrap();
    assert_eq!(
        v3::read_server_event(reader).unwrap(),
        Some(RendezvousEvent::FdsReceived {
            of: "handoff".into()
        })
    );
    host
}

fn event(reader: &mut BufReader<UnixStream>) -> Option<RendezvousEvent> {
    v3::read_server_event(reader).unwrap()
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

fn create(home: &Home) {
    let (mut child, listener) = home.launch(
        &[CREATE_FLAG],
        &json!({
            "op": "create",
            "office_id": OFFICE,
            "org_id": "01JD5Z00000000000000000RGA",
            "org_name": "Acme",
            "office_name": "Headquarters",
            "role": "hq",
            "owner_member_id": "01JD5Z00000000000000000MBR",
        }),
    );
    let mut reader = accept_ready(&listener);
    let header = v3::encode_create_handoff(
        &DbKey::from_bytes([7; 32]),
        &GrantKey::from_bytes([9; 32]),
        &RecoveryCode::new("a recovery code".into()).unwrap(),
    )
    .unwrap();
    let _transfer = hand_off(&mut reader, header);
    match event(&mut reader) {
        Some(RendezvousEvent::Server(ServerEvent::Created { office })) => {
            assert_eq!(office.office_id, OFFICE)
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
    assert_eq!(stdout_of(&mut child), "", "v3 events never go to stdout");
    let pid = child.id();
    let log = home.log();
    for line in [
        format!("launch create {pid} v3"),
        format!("instance create {pid} {INSTANCE}"),
        format!("grant create {pid} "),
        format!("recovery create {pid} present"),
        format!("transfer create {pid} received"),
        format!("dbkey create {pid} "),
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
    let header =
        v3::encode_start_handoff(&DbKey::from_bytes([7; 32]), &GrantKey::from_bytes([10; 32]))
            .unwrap();
    let _transfer = hand_off(&mut reader, header);
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
    assert!(log_has(&log, &format!("launch {OFFICE} {pid} v3")), "{log}");
    assert!(
        log_has(&log, &format!("transfer {OFFICE} {pid} received")),
        "{log}"
    );
    assert!(!log_has(&log, &format!("recovery {OFFICE} {pid}")), "{log}");
}

#[test]
fn the_fake_discards_over_the_rendezvous_without_a_transfer_socket() {
    let home = Home::new();
    let (mut child, listener) = home.launch(
        &[DISCARD_FLAG, OFFICE],
        &json!({"op": "discard", "office_id": OFFICE}),
    );
    let mut reader = accept_ready(&listener);
    // An unfinished office's folder holding databases: a v3 discard removes it without a key.
    let dir = home
        .home
        .join(org_office_launch::OFFICES_DIR_NAME)
        .join(OFFICE);
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("db"), b"databases").unwrap();
    // The host never sends a key with a discard now.
    let header = v3::encode_discard_handoff(None).unwrap();
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
    assert!(log_has(&log, &format!("launch discard {pid} v3")), "{log}");
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
        &json!({"op": "start", "office_id": "01JD5Z0000000000000000OFFB"}),
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
fn the_fake_refuses_a_v3_line_naming_a_ulid_instance() {
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
    let create = v3::encode_create_handoff(
        &DbKey::from_bytes([7; 32]),
        &GrantKey::from_bytes([9; 32]),
        &RecoveryCode::new("a recovery code".into()).unwrap(),
    )
    .unwrap();
    let discard = v3::encode_discard_handoff(None).unwrap();
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
        assert_eq!(stdout_of(&mut child), "", "v3 events never go to stdout");
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
            .write_all(&vec![b'a'; v3::TRANSFER_HEADER_MAX_BYTES + 1])
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
        assert_eq!(stdout_of(&mut child), "", "v3 events never go to stdout");
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
