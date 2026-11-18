//! The real `agentic-org-office` binary, driven the way the host drives it: arguments, the control
//! pipe on stdin and stdout, and the office socket. The host holds the database key and sends it
//! on the first line (key custody), so there is no key store here and no test can touch a
//! Keychain: the server has no Keychain code.

use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

#[path = "launch/joined_client.rs"]
mod joined_client;
#[path = "support/teardown.rs"]
mod test_teardown;
use joined_client::Client;
use org_office_launch::{
    office_dir, read_server_event, write_host_message, CreateOffice, DiscardOffice, FailCode,
    HostMessage, OfficeFile, ServerEvent, StartOffice,
};
use org_proto::location::office_socket_path;
use org_proto::{frame, AppToken, DbKey, OfficeRole};
use tempfile::TempDir;

const OFFICE: &str = "01OFFICE00000000000000HQ1";
const ORG: &str = "01ORG0000000000000000ACME1";
const WAIT: Duration = Duration::from_secs(30);

// Every launch case owns descriptors while it creates its own worker. Keep the
// whole case exclusive: taking a shared guard then upgrading during spawn would
// deadlock. This binary has its own process-wide fence, separate from lib tests.
static TEST_PROCESS_ISOLATION: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn process_isolation() -> std::sync::MutexGuard<'static, ()> {
    TEST_PROCESS_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

/// A temporary office home.
struct World {
    _dir: TempDir,
    home: PathBuf,
}

impl World {
    fn new() -> Self {
        // A short path: unix socket paths are limited to ~100 bytes.
        let dir = tempfile::Builder::new().prefix("oo").tempdir().unwrap();
        let home = dir.path().join("h");
        std::fs::create_dir_all(home.join("offices")).unwrap();
        std::fs::create_dir_all(home.join("run")).unwrap();
        Self { _dir: dir, home }
    }

    fn folder(&self, id: &str) -> PathBuf {
        office_dir(&self.home, id).unwrap()
    }

    fn spawn(&self, args: &[&str]) -> Run {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentic-org-office"))
            .args(args)
            .env("ORG_OFFICE_HOME", &self.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, events) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(event)) = read_server_event(&mut reader) {
                if tx.send(event).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        Run {
            child,
            stdin,
            events,
            reader: Some(reader),
        }
    }
}

struct Run {
    child: Child,
    stdin: Option<ChildStdin>,
    events: Receiver<ServerEvent>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Run {
    fn send(&mut self, message: &HostMessage) {
        let stdin = self.stdin.as_mut().unwrap();
        write_host_message(stdin, message).unwrap();
    }

    fn event(&self) -> ServerEvent {
        self.events.recv_timeout(WAIT).expect("no control line")
    }

    /// Closes stdin, as a host that goes away does, and waits for the exit.
    fn host_goes_away(&mut self) -> std::process::ExitStatus {
        self.stdin = None;
        self.wait()
    }

    fn wait(&mut self) -> std::process::ExitStatus {
        let started = std::time::Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(started.elapsed() < WAIT, "the server did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn stderr(&mut self) -> String {
        let mut text = String::new();
        if let Some(mut stderr) = self.child.stderr.take() {
            std::io::Read::read_to_string(&mut stderr, &mut text).unwrap();
        }
        text
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The case's process fence must outlive the pipe owned by this thread.
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap_or_else(|panic| {
                if !std::thread::panicking() {
                    std::panic::resume_unwind(panic);
                }
            });
        }
    }
}

fn token() -> AppToken {
    AppToken::from_bytes([0xA7; 32])
}

/// The database key the host would hold for the office.
fn db_key() -> DbKey {
    DbKey::from_bytes([0xB3; 32])
}

/// The key as the line carries it, to look for in logs.
const DB_KEY_HEX: &str = "b3b3b3b3";

fn create_message() -> HostMessage {
    HostMessage::Create(CreateOffice {
        org_id: ORG.into(),
        org_name: "Acme".into(),
        office_id: OFFICE.into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        app_token: token(),
        db_key: db_key(),
    })
}

fn start_message() -> HostMessage {
    start_message_with(db_key())
}

fn start_message_with(db_key: DbKey) -> HostMessage {
    HostMessage::Start(StartOffice {
        app_token: token(),
        db_key,
    })
}

fn discard_message(db_key: Option<DbKey>) -> HostMessage {
    HostMessage::Discard(DiscardOffice { db_key })
}

fn socket(world: &World) -> PathBuf {
    office_socket_path(&world.home, OFFICE).unwrap()
}

/// Stores the office with the library, as an HQ creation leaves it, then starts its server the
/// way the host does and returns it. The worker creates offices only through HQ creation-v1
/// over the rendezvous (`v3_activation`); every other case starts an existing office.
fn started(world: &World) -> Run {
    seed(world);
    let mut run = world.spawn(&["--office", OFFICE]);
    run.send(&start_message());
    let ServerEvent::Ready {
        socket: reported, ..
    } = run.event()
    else {
        panic!("expected ready");
    };
    assert_eq!(Path::new(&reported), socket(world));
    run
}

/// Stores the office with the library, as an HQ creation leaves it, unless it is stored.
fn seed(world: &World) {
    if !world.folder(OFFICE).join("office.json").exists() {
        let new = org_office::NewOffice {
            office_id: OFFICE.into(),
            office_name: "HQ".into(),
            role: org_office::OfficeRole::Hq,
            org_id: ORG.into(),
            org_name: "Acme".into(),
        };
        drop(
            org_office::Office::create(
                world.folder(OFFICE),
                &new,
                org_office::Key::from_db_key(db_key()),
            )
            .unwrap(),
        );
    }
}

fn hello(world: &World) -> org_proto::OfficeHello {
    let client = Client::connect(socket(world)).unwrap();
    client
        .hello()
        .unwrap()
        .office
        .expect("an office answers hello with its identity")
}

fn failed_code(event: ServerEvent) -> FailCode {
    match event {
        ServerEvent::Failed { code, .. } => code,
        other => panic!("expected failed, got {other:?}"),
    }
}

#[test]
fn srv_32_unsupported_range_closes_before_a_pipelined_call() {
    let _process = process_isolation();
    // X-02 §3: "unsupported_version and close" before the next request; this
    // B1-07.1 phase is V1-only until instance-aware launch can enable V2.
    let world = World::new();
    let mut run = started(&world);
    let mut peer = UnixStream::connect(socket(&world)).unwrap();
    peer.set_read_timeout(Some(WAIT)).unwrap();
    peer.set_write_timeout(Some(WAIT)).unwrap();
    // Use the wire format so an older worker cannot silently turn this into hello {}.
    peer.write_all(
        concat!(
            "{\"id\":1,\"method\":\"hello\",\"params\":{\"api\":{\"min\":99,\"max\":99}}}\n",
            "{\"id\":2,\"method\":\"hello\",\"params\":{}}\n",
        )
        .as_bytes(),
    )
    .unwrap();

    let mut reader = BufReader::new(peer);
    assert_unsupported_version_and_closed(&mut reader, 1);

    assert_eq!(hello(&world).office.id, OFFICE);
    assert!(run.host_goes_away().success());
}

#[test]
fn srv_32_legacy_first_call_locks_the_socket_to_version_one() {
    let _process = process_isolation();
    // X-02 §3: "A first request that isn't hello ... fixes the connection at v1".
    let world = World::new();
    let mut run = started(&world);
    let mut peer = UnixStream::connect(socket(&world)).unwrap();
    peer.set_read_timeout(Some(WAIT)).unwrap();
    peer.set_write_timeout(Some(WAIT)).unwrap();
    peer.write_all(
        concat!(
            "{\"id\":1,\"method\":\"org.snapshot\",\"params\":{}}\n",
            "{\"id\":2,\"method\":\"hello\",\"params\":{\"api\":{\"min\":1,\"max\":2}}}\n",
            "{\"id\":3,\"method\":\"hello\",\"params\":{\"api\":{\"min\":2,\"max\":2}}}\n",
            "{\"id\":4,\"method\":\"hello\",\"params\":{}}\n",
        )
        .as_bytes(),
    )
    .unwrap();

    let mut reader = BufReader::new(peer);
    let first = frame::read_message::<serde_json::Value>(&mut reader)
        .unwrap()
        .expect("the legacy request must reach normal authorization");
    assert_eq!(first["id"], 1);
    assert_eq!(first["error"]["code"], "not_authorized");
    let compatible = frame::read_message::<serde_json::Value>(&mut reader)
        .unwrap()
        .expect("the next hello still has a common version");
    assert_eq!(compatible["id"], 2);
    assert_eq!(compatible["result"]["api_version"], 1);
    assert_unsupported_version_and_closed(&mut reader, 3);

    assert_eq!(hello(&world).office.id, OFFICE);
    assert!(run.host_goes_away().success());
}

fn assert_unsupported_version_and_closed(reader: &mut BufReader<UnixStream>, request_id: u64) {
    let refusal = frame::read_message::<serde_json::Value>(reader)
        .unwrap()
        .expect("version refusal must precede closure");
    assert_eq!(refusal["id"], request_id);
    assert_eq!(refusal["error"]["code"], "unsupported_version");
    assert!(refusal.get("result").is_none());
    match frame::read_message::<serde_json::Value>(reader) {
        Ok(None) => {}
        Err(frame::FrameError::Io(error))
            if error.kind() == std::io::ErrorKind::ConnectionReset => {}
        other => panic!("expected closure after the refusal, got {other:?}"),
    }
}

#[test]
fn x02_s3_malformed_hello_is_invalid_request_before_normal_dispatch() {
    let _process = process_isolation();
    // X-02 §3: malformed api/non-string client is "invalid_request, never
    // answered as v1". The strict shared HelloParams decoder owns these shapes.
    let world = World::new();
    let mut run = started(&world);
    let cases = [
        serde_json::json!([]),
        serde_json::json!(true),
        serde_json::json!(7),
        serde_json::json!("not an object"),
        serde_json::json!({"api": []}),
        serde_json::json!({"api": "2"}),
        serde_json::json!({"api": {"min": 0, "max": 1}}),
        serde_json::json!({"api": {"min": 2, "max": 1}}),
        serde_json::json!({"api": {"min": 1.5, "max": 2}}),
        serde_json::json!({"api": {"min": -1, "max": 2}}),
        serde_json::json!({"api": {"min": 1, "max": 4294967296_u64}}),
        serde_json::json!({"api": {"min": 1}}),
        serde_json::json!({"api": {"max": 1}}),
        serde_json::json!({"api": {"min": 1, "max": 1, "extra": true}}),
        serde_json::json!({"client": null}),
        serde_json::json!({"client": 7}),
    ];
    for params in cases {
        let mut peer = UnixStream::connect(socket(&world)).unwrap();
        peer.set_read_timeout(Some(WAIT)).unwrap();
        peer.set_write_timeout(Some(WAIT)).unwrap();
        frame::write_message(
            &mut peer,
            &serde_json::json!({"id": 1, "method": "hello", "params": params}),
        )
        .unwrap();
        let mut reader = BufReader::new(peer);
        let refusal = frame::read_message::<serde_json::Value>(&mut reader)
            .unwrap()
            .unwrap();
        assert_eq!(refusal["id"], 1);
        assert_eq!(refusal["error"]["code"], "invalid_request", "{params}");
        assert!(refusal.get("result").is_none(), "{params}");
        // A malformed offer doesn't poison the service or replace a valid legacy hello.
        frame::write_message(
            reader.get_mut(),
            &serde_json::json!({"id": 2, "method": "hello", "params": {}}),
        )
        .unwrap();
        let next = frame::read_message::<serde_json::Value>(&mut reader)
            .unwrap()
            .unwrap();
        assert_eq!(next["id"], 2);
        assert_eq!(next["result"]["api_version"], 1);
    }
    assert!(run.host_goes_away().success());
}

#[test]
fn x02_s3_null_api_and_unknown_outer_fields_preserve_legacy_hello() {
    let _process = process_isolation();
    // X-02 §3: "api: null is the same as an absent api"; unknown outer hints
    // remain additive, while the nested api range has exactly min/max.
    let world = World::new();
    let mut run = started(&world);
    let peer = UnixStream::connect(socket(&world)).unwrap();
    peer.set_read_timeout(Some(WAIT)).unwrap();
    peer.set_write_timeout(Some(WAIT)).unwrap();
    let mut reader = BufReader::new(peer);
    for (id, params) in [
        (1, serde_json::json!({"api": null, "future_hint": true})),
        (
            2,
            serde_json::json!({"api": {"min": 1, "max": 2}, "client": "test"}),
        ),
        (3, serde_json::json!({})),
        // Request::params treats a missing/null params object as {}.
        (4, serde_json::Value::Null),
    ] {
        frame::write_message(
            reader.get_mut(),
            &serde_json::json!({"id": id, "method": "hello", "params": params}),
        )
        .unwrap();
        let reply = frame::read_message::<serde_json::Value>(&mut reader)
            .unwrap()
            .unwrap();
        assert_eq!(reply["id"], id);
        assert_eq!(reply["result"]["api_version"], 1);
    }
    frame::write_message(
        reader.get_mut(),
        &serde_json::json!({"id": 5, "method": "hello", "params": {"api": {"min": 2, "max": 2}}}),
    )
    .unwrap();
    assert_unsupported_version_and_closed(&mut reader, 5);
    assert!(run.host_goes_away().success());
}

#[test]
fn a_restart_with_the_same_key_serves_the_same_office() {
    let _process = process_isolation();
    let world = World::new();
    let mut first = started(&world);
    assert!(first.host_goes_away().success());

    let mut again = world.spawn(&["--office", OFFICE]);
    again.send(&start_message());
    assert!(matches!(again.event(), ServerEvent::Ready { .. }));
    assert_eq!(hello(&world).office.id, OFFICE);
    assert!(again.host_goes_away().success());
}

#[test]
fn the_first_line_must_match_the_flag() {
    let _process = process_isolation();
    let world = World::new();
    // --create given a start message.
    let mut run = world.spawn(&["--create"]);
    run.send(&start_message());
    assert_eq!(failed_code(run.event()), FailCode::InvalidRequest);
    assert!(!run.wait().success());

    // --office given a create message, and garbage.
    let mut run = world.spawn(&["--office", OFFICE]);
    run.send(&create_message());
    assert_eq!(failed_code(run.event()), FailCode::InvalidRequest);
    assert!(!run.wait().success());
    let mut run = world.spawn(&["--office", OFFICE]);
    std::io::Write::write_all(run.stdin.as_mut().unwrap(), b"not json at all\n").unwrap();
    assert_eq!(failed_code(run.event()), FailCode::InvalidRequest);
    assert!(!run.wait().success());

    // The discard line belongs to --discard alone, and --discard takes no other.
    let mut run = world.spawn(&["--create"]);
    run.send(&discard_message(None));
    assert_eq!(failed_code(run.event()), FailCode::InvalidRequest);
    assert!(!run.wait().success());
    let mut run = world.spawn(&["--office", OFFICE]);
    run.send(&discard_message(Some(db_key())));
    assert_eq!(failed_code(run.event()), FailCode::InvalidRequest);
    assert!(!run.wait().success());
    std::fs::create_dir_all(world.folder("UNFINISHED1")).unwrap();
    for message in [create_message(), start_message()] {
        let mut run = world.spawn(&["--discard", "UNFINISHED1"]);
        run.send(&message);
        assert_eq!(failed_code(run.event()), FailCode::InvalidRequest);
        assert!(!run.wait().success());
    }
    assert!(world.folder("UNFINISHED1").exists(), "a wrong line deleted");

    // Wrong arguments.
    let mut run = world.spawn(&["--serve"]);
    assert_eq!(failed_code(run.event()), FailCode::InvalidRequest);
    assert!(!run.wait().success());
}

#[test]
fn the_host_going_away_before_the_first_line_is_a_clean_exit_with_no_line() {
    let _process = process_isolation();
    let world = World::new();
    let mut run = world.spawn(&["--create"]);
    let status = run.host_goes_away();
    assert!(status.success(), "{status:?}");
    assert!(run.events.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn discard_removes_an_unfinished_create_and_only_that() {
    let _process = process_isolation();
    let world = World::new();
    // An unfinished create: a folder with the lock file, no office.json, no database. The host
    // may hold a key for it or not; either way nothing is there to check.
    for key in [None, Some(db_key())] {
        std::fs::create_dir_all(world.folder("UNFINISHED1")).unwrap();
        let mut run = world.spawn(&["--discard", "UNFINISHED1"]);
        run.send(&discard_message(key));
        assert!(matches!(run.event(), ServerEvent::Discarded));
        assert!(run.wait().success());
        assert!(!world.folder("UNFINISHED1").exists());
    }

    // A finished office is refused and untouched, with a key or without.
    let mut first = started(&world);
    assert!(first.host_goes_away().success());
    for key in [None, Some(db_key())] {
        let mut run = world.spawn(&["--discard", OFFICE]);
        run.send(&discard_message(key));
        assert_eq!(failed_code(run.event()), FailCode::AlreadyExists);
        assert!(!run.wait().success());
        assert!(world.folder(OFFICE).join("office.json").exists());
    }
}

#[test]
fn discard_without_office_json_finds_the_organization_with_any_or_no_key_and_deletes_nothing() {
    let _process = process_isolation();
    let world = World::new();
    let mut first = started(&world);
    assert!(first.host_goes_away().success());
    // Lose office.json, as a crash or a person might: the databases are all that is left.
    std::fs::remove_file(world.folder(OFFICE).join("office.json")).unwrap();

    // The databases are plain SQLite (ADR-0016): with no key, a wrong key or the right one the
    // organization is found, so the discard is refused and nothing is deleted.
    for key in [None, Some(DbKey::from_bytes([0xC4; 32])), Some(db_key())] {
        let mut run = world.spawn(&["--discard", OFFICE]);
        run.send(&discard_message(key));
        assert_eq!(failed_code(run.event()), FailCode::AlreadyExists);
        assert!(!run.wait().success());
        assert!(
            world.folder(OFFICE).join("org.db").exists(),
            "a database was deleted"
        );
        assert!(
            !run.stderr().contains(DB_KEY_HEX),
            "the key reached the log of a failed discard"
        );
    }
}

#[test]
fn a_wrong_secrets_key_still_opens_the_plain_databases_without_logging_keys() {
    let _process = process_isolation();
    let world = World::new();
    let mut first = started(&world);
    assert!(first.host_goes_away().success());

    // ADR-0016 decision 3: the key only wraps private keys; a wrong one does not stop the
    // office from serving its data.
    let mut run = world.spawn(&["--office", OFFICE]);
    run.send(&start_message_with(DbKey::from_bytes([0xC4; 32])));
    assert!(matches!(run.event(), ServerEvent::Ready { .. }));
    assert!(run.host_goes_away().success());
    let log = run.stderr();
    assert!(!log.contains(DB_KEY_HEX), "the key reached the log");
    assert!(!log.contains("c4c4c4c4"), "the wrong key reached the log");

    let mut again = world.spawn(&["--office", OFFICE]);
    again.send(&start_message());
    assert!(matches!(again.event(), ServerEvent::Ready { .. }));
    assert!(again.host_goes_away().success());
}

#[test]
fn a_server_given_no_key_line_does_nothing() {
    let _process = process_isolation();
    // The host going away before the first line is a clean exit for every mode, discard too.
    let world = World::new();
    std::fs::create_dir_all(world.folder("UNFINISHED1")).unwrap();
    let mut run = world.spawn(&["--discard", "UNFINISHED1"]);
    let status = run.host_goes_away();
    assert!(status.success(), "{status:?}");
    assert!(run.events.recv_timeout(Duration::from_millis(200)).is_err());
    assert!(
        world.folder("UNFINISHED1").exists(),
        "it deleted without a line"
    );
}

fn signal(run: &Run, signal: libc::c_int) {
    // SAFETY: signalling a child process we started.
    #[allow(unsafe_code)]
    unsafe {
        libc::kill(run.child.id() as libc::pid_t, signal);
    }
}

#[test]
fn a_stop_signal_closes_the_office_and_exits_clean() {
    let _process = process_isolation();
    for stop in [libc::SIGTERM, libc::SIGINT] {
        let world = World::new();
        let mut run = started(&world);
        signal(&run, stop);
        let status = run.wait();
        assert_eq!(status.code(), Some(0), "signal {stop}: {status:?}");
        // The office can be served again right away: its lock was released.
        let mut again = world.spawn(&["--office", OFFICE]);
        again.send(&start_message());
        assert!(matches!(again.event(), ServerEvent::Ready { .. }));
        assert!(again.host_goes_away().success());
    }
}

#[test]
fn a_server_still_waiting_for_its_first_line_can_be_terminated() {
    let _process = process_isolation();
    let world = World::new();
    let mut run = world.spawn(&["--create"]);
    // Give it time to reach the read, then ask it to stop until it does. A server that handled
    // the signal before reading (and so ignored it) would never stop.
    std::thread::sleep(Duration::from_millis(1500));
    let started = std::time::Instant::now();
    let status = loop {
        signal(&run, libc::SIGTERM);
        if let Some(status) = run.child.try_wait().unwrap() {
            break status;
        }
        assert!(started.elapsed() < WAIT, "the server ignored SIGTERM");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(!status.success(), "{status:?}");
}

#[test]
fn the_host_going_away_in_the_middle_of_the_first_line_is_a_clean_exit_with_no_line() {
    let _process = process_isolation();
    let world = World::new();
    let mut run = world.spawn(&["--create"]);
    std::io::Write::write_all(run.stdin.as_mut().unwrap(), b"{\"op\":\"crea").unwrap();
    let status = run.host_goes_away();
    assert_eq!(status.code(), Some(0), "{status:?}");
    assert!(run.events.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn the_socket_is_owner_only_and_other_methods_are_unknown() {
    let _process = process_isolation();
    use std::os::unix::fs::PermissionsExt;
    let world = World::new();
    let mut run = started(&world);
    let mode = std::fs::metadata(socket(&world))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    let client = Client::connect(socket(&world)).unwrap();
    let error = client
        .call::<org_proto::OrganizationList>(&org_proto::Empty {})
        .unwrap_err();
    assert!(
        matches!(&error, org_client::ClientError::Api(e) if e.code == org_proto::ErrorCode::UnknownMethod),
        "{error:?}"
    );
    drop(client);
    assert!(run.host_goes_away().success());
}

#[test]
fn a_stale_socket_file_is_replaced_but_anything_else_there_is_not() {
    let _process = process_isolation();
    let world = World::new();
    let mut first = started(&world);
    assert!(first.host_goes_away().success());

    // A crashed server leaves its socket file behind.
    drop(std::os::unix::net::UnixListener::bind(socket(&world)).unwrap());
    assert!(socket(&world).exists());
    let mut again = world.spawn(&["--office", OFFICE]);
    again.send(&start_message());
    assert!(matches!(again.event(), ServerEvent::Ready { .. }));
    assert_eq!(hello(&world).office.id, OFFICE);
    assert!(again.host_goes_away().success());

    // A regular file is not ours to remove.
    std::fs::write(socket(&world), b"mine").unwrap();
    let mut refused = world.spawn(&["--office", OFFICE]);
    refused.send(&start_message());
    assert_eq!(failed_code(refused.event()), FailCode::StorageError);
    assert!(!refused.wait().success());
    assert_eq!(std::fs::read(socket(&world)).unwrap(), b"mine");
}

#[test]
fn what_the_host_writes_after_the_first_line_never_stops_the_server_and_eof_does() {
    let _process = process_isolation();
    let world = World::new();
    let mut run = started(&world);
    // Lines of any shape, and a lot of them: read and thrown away, unparsed.
    let stdin = run.stdin.as_mut().unwrap();
    std::io::Write::write_all(stdin, b"garbage\n{\"op\":\"nonsense\"}\n").unwrap();
    std::io::Write::write_all(stdin, &vec![b'x'; 3 * 1024 * 1024]).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        hello(&world).office.id,
        OFFICE,
        "the server stopped serving"
    );
    assert!(run.host_goes_away().success());
}

#[test]
fn the_token_and_the_key_never_reach_the_log_and_fds_0_and_1_are_dev_null() {
    let _process = process_isolation();
    let world = World::new();
    let mut run = started(&world);
    let pid = run.child.id();
    // fds 0 and 1 of the server are /dev/null (the control pipe was moved), checked with lsof.
    if let Ok(out) = Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "0,1", "-Fn"])
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        if !text.trim().is_empty() {
            assert!(
                text.matches("/dev/null").count() >= 2,
                "fds 0 and 1 are not /dev/null:\n{text}"
            );
        }
    }
    assert!(run.host_goes_away().success());
    let log = run.stderr();
    assert!(!log.contains("a7a7a7a7"), "the app token reached the log");
    assert!(
        !log.contains(DB_KEY_HEX),
        "the database key reached the log"
    );
}

#[test]
fn a_folder_that_is_not_what_an_office_makes_is_storage_error_not_invalid_request() {
    let _process = process_isolation();
    // Disk state, not a bad argument: the host must be able to restart it once repaired.
    let world = World::new();
    std::fs::create_dir_all(world.folder("UNFINISHED1")).unwrap();
    std::fs::write(world.folder("UNFINISHED1").join("notes.txt"), b"not ours").unwrap();
    let mut run = world.spawn(&["--discard", "UNFINISHED1"]);
    run.send(&discard_message(None));
    assert_eq!(failed_code(run.event()), FailCode::StorageError);
    assert_eq!(run.wait().code(), Some(1));
    assert!(
        world.folder("UNFINISHED1").join("notes.txt").exists(),
        "a foreign file was removed"
    );
}

#[test]
fn stopping_the_server_with_a_client_connected_closes_it_and_exits_clean() {
    let _process = process_isolation();
    let world = World::new();
    let mut run = started(&world);
    // A client that stays connected and silent: shutdown must close it, not wait for it.
    let idle = std::os::unix::net::UnixStream::connect(socket(&world)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let started = std::time::Instant::now();
    let status = run.host_goes_away();
    assert!(status.success(), "{status:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
    drop(idle);
}

/// A ULID made now (the command ids the office accepts), distinct for each `n`.
fn command_id(n: u8) -> String {
    const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let mut value = (ms << 80) | u128::from(n);
    let mut out = [0u8; 26];
    for slot in out.iter_mut().rev() {
        *slot = CROCKFORD[(value & 0x1F) as usize];
        value >>= 5;
    }
    String::from_utf8(out.to_vec()).unwrap()
}

fn team_create(
    client: &Client,
    command: &str,
    name: &str,
) -> Result<org_proto::org::CommandAccepted, org_client::ClientError> {
    client.call::<org_proto::org::TeamCreate>(&org_proto::org::TeamCreateParams {
        command_id: command.into(),
        team_id: "T1".into(),
        name: name.into(),
        owner_team_id: None,
    })
}

fn api_code(error: org_client::ClientError) -> org_proto::ErrorCode {
    match error {
        org_client::ClientError::Api(e) => e.code,
        other => panic!("{other:?}"),
    }
}

#[test]
#[ignore = "rewrite on A3's grant-bound host-channel subscribe (/events): no office-socket subscriber exists"]
/// Must be rewritten on A3's grant-bound host-channel subscribe (/events): since A1.5 nobody
/// binds or subscribes as the owner on the office socket, and host channels refuse `subscribe`
/// until A3. Scenario to keep: an unbound connection's subscribe is `not_authorized`; a
/// subscriber from a snapshot position gets another caller's committed `team.create` as
/// `org.changed` (seq 1, same position and command id); a refused command and a retry of a
/// committed one are not events; a position beyond the log is `snapshot_required`; after a host
/// restart a subscriber resuming from the old position gets the backlog, then live changes.
fn a_subscriber_follows_the_organization_and_resumes_from_where_it_was_after_a_restart() {
    panic!("rewrite on A3's grant-bound host-channel subscribe; see the doc comment");
}

#[test]
#[ignore = "rewrite on A3's grant-bound host-channel subscribe (/events): no office-socket subscriber exists"]
/// Must be rewritten on A3's grant-bound host-channel subscribe (/events): since A1.5 nobody
/// binds or subscribes as the owner on the office socket, and host channels refuse `subscribe`
/// until A3. Scenario to keep: the owner's subscriber gets an agent's `msg.send` to the owner as
/// `message.received` (header id, never the body), then the agent's `status.set` as
/// `member.presence_changed` (member id, status text) with the next seq on the same connection.
fn the_app_is_told_live_of_a_message_and_of_a_status_line_through_the_real_office() {
    panic!("rewrite on A3's grant-bound host-channel subscribe; see the doc comment");
}

#[path = "launch/v3_activation.rs"]
mod v3_activation;

#[path = "launch/inbox_hint.rs"]
mod inbox_hint;
