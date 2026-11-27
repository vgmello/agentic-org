//! The real `agentic-org-office` binary, driven the way the host drives it: arguments, the launch
//! first line on stdin, the authenticated rendezvous (keys, events, lifetime) and the office
//! socket. stdout is untrusted and must stay empty. The host holds the office secrets key and hands it
//! over on the rendezvous (key custody), so there is no key store here and no test can touch a
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
use org_office_launch::{office_dir, OfficeFile};
use org_office_server::test_support::{test_admission, FakeWriter};
use org_proto::location::selected_office_socket_in;
use org_proto::{frame, OfficeRole, OfficeSecretsKey};
use tempfile::TempDir;

const OFFICE: &str = "019a8928-1337-7456-8456-0123456789ac";
const ORG: &str = "019a8928-1337-7123-8123-0123456789ab";
/// The owner member: the creator's person.
const OWNER: &str = "01900000-0000-7000-8000-0000000000ee";
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

/// A temporary office home, the organization's data roots, and the host's custody of the
/// office's writer (`FakeWriter`, which keeps its confirmed head across runs).
struct World {
    _dir: TempDir,
    home: PathBuf,
    /// `<dir>/<org-id>`: `durable/` and `runtime/` under it, as the host makes them.
    data: PathBuf,
    writer: FakeWriter,
}

impl World {
    fn new() -> Self {
        // A short path: unix socket paths are limited to ~100 bytes.
        let dir = tempfile::Builder::new().prefix("oo").tempdir().unwrap();
        let home = dir.path().join("h");
        Self::in_dir(dir, home)
    }

    fn in_dir(dir: TempDir, home: PathBuf) -> Self {
        std::fs::create_dir_all(home.join("offices")).unwrap();
        std::fs::create_dir_all(home.join("run")).unwrap();
        let data = dir.path().canonicalize().unwrap().join(ORG);
        for root in ["durable", "runtime"] {
            std::fs::create_dir_all(data.join(root)).unwrap();
        }
        let writer = FakeWriter::new(test_admission(
            activation::INSTANCE,
            ORG,
            OFFICE,
            OWNER,
            &data.join("durable"),
            &data.join("runtime"),
        ));
        Self {
            _dir: dir,
            home,
            data,
            writer,
        }
    }

    /// The office on its store, opened in this process with the world's custody (the
    /// worker must not be running).
    fn office(&self) -> org_office::Office {
        org_office_sqlite::install();
        let (custody, roots) = self.writer.admitted();
        org_office::Office::open(
            self.folder(OFFICE),
            Some(org_office::Key::from_office_secrets_key(
                office_secrets_key(),
            )),
            activation::INSTANCE,
            roots,
            custody,
        )
        .unwrap()
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
        // stdout is untrusted and the worker writes nothing there: any line it carries is a
        // defect the cases see as an item on `events`.
        let (tx, events) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(line)) = frame::read_line(&mut reader) {
                if tx.send(line).is_err() {
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
    /// Lines the worker wrote on stdout (it must write none).
    events: Receiver<Vec<u8>>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Run {
    /// Closes stdin, as a host that goes away before the first line does, and waits for the exit.
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

/// The office secrets key the host would hold for the office.
fn office_secrets_key() -> OfficeSecretsKey {
    OfficeSecretsKey::from_bytes([0xB3; 32])
}

/// The key as the handoff carries it, to look for in logs.
const SECRETS_KEY_HEX: &str = "b3b3b3b3";

fn socket(world: &World) -> PathBuf {
    selected_office_socket_in(&world.home.join("run"), OFFICE).unwrap()
}

/// Creates the office through the worker, as the host does, then starts its server the way
/// the host does (start, handoff, writer control, ready on the rendezvous) and returns the
/// session.
fn started(world: &World) -> Session {
    seed(world);
    let mut session = handshake(
        world,
        FirstLine::Start,
        INSTANCE,
        Some(office_secrets_key()),
    );
    let ready = session.event();
    assert_eq!(ready["event"], "ready", "{ready}");
    assert_eq!(
        Path::new(ready["socket"].as_str().unwrap()),
        socket(world),
        "{ready}"
    );
    session
}

/// Creates the office through the worker's HQ creation, unless it is stored.
fn seed(world: &World) {
    if !world.folder(OFFICE).join("office.json").exists() {
        activation::ready_create(world).stop();
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

/// A failure the worker reported on the rendezvous: its code, after a failed exit with nothing
/// on stdout.
fn failed_code(mut session: Session) -> String {
    let failed = session.event();
    assert_eq!(failed["event"], "failed", "{failed}");
    assert!(!session.run.wait().success());
    session.no_stdout();
    failed["code"].as_str().unwrap().to_owned()
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

    let mut again = started(&world);
    assert_eq!(hello(&world).office.id, OFFICE);
    assert!(again.host_goes_away().success());
}

#[test]
fn the_first_line_must_match_the_flag() {
    let _process = process_isolation();
    let world = World::new();
    let rendezvous = world.home.parent().unwrap().join("flag");
    let listener = std::os::unix::net::UnixListener::bind(&rendezvous).unwrap();
    listener.set_nonblocking(true).unwrap();
    let line = |op: FirstLine| {
        let mut bytes = Vec::new();
        org_office_launch::write_first_line(&mut bytes, &activation::first_line(op, &rendezvous))
            .unwrap();
        bytes
    };
    // The worker refuses before it connects: no trusted channel exists, so nothing is reported
    // (stdout is untrusted), the exit fails and the rendezvous is never dialled.
    let refused = |args: &[&str], bytes: &[u8]| {
        let mut run = world.spawn(args);
        if let Some(stdin) = run.stdin.as_mut() {
            // A worker refusing its arguments may already be gone.
            let _ = stdin.write_all(bytes);
        }
        assert!(!run.wait().success(), "{args:?}");
        assert!(
            matches!(
                run.events.recv_timeout(WAIT),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            "{args:?} wrote on stdout"
        );
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "{args:?} dialled the rendezvous"
        );
    };
    // --create given a start line, --office a create line, and lines that aren't launch lines.
    refused(&["--create"], &line(FirstLine::Start));
    refused(&["--office", OFFICE], &line(FirstLine::Create));
    refused(&["--office", OFFICE], b"not json at all\n");
    refused(&["--office", OFFICE], b"{\"v\":2,\"op\":\"start\"}\n");

    // The discard line belongs to --discard alone, and --discard takes no other.
    refused(&["--create"], &line(FirstLine::Discard));
    refused(&["--office", OFFICE], &line(FirstLine::Discard));
    std::fs::create_dir_all(world.folder(OFFICE)).unwrap();
    for op in [FirstLine::Create, FirstLine::Start] {
        refused(&["--discard", OFFICE], &line(op));
    }
    assert!(world.folder(OFFICE).exists(), "a wrong line deleted");
    assert!(!world.folder(OFFICE).join("office.json").exists());

    // Wrong arguments.
    refused(&["--serve"], b"");
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
fn discard_without_office_json_finds_the_organization_and_deletes_nothing() {
    let _process = process_isolation();
    let world = World::new();
    let mut first = started(&world);
    assert!(first.host_goes_away().success());
    // Lose office.json, as a crash or a person might: the creation intent and the journal are
    // what is left.
    std::fs::remove_file(world.folder(OFFICE).join("office.json")).unwrap();

    // The discard handoff carries no secret, and a folder with a creation intent is never
    // discarded: the discard is refused and nothing is deleted.
    let mut session = handshake(&world, FirstLine::Discard, INSTANCE, None);
    let failed = session.event();
    assert_eq!(failed["code"], "already_exists", "{failed}");
    assert!(!session.run.wait().success());
    session.no_stdout();
    assert!(
        world.folder(OFFICE).join("hq-creation.v1.json").exists(),
        "the intent was deleted"
    );
    assert!(world.data.join("durable/recovery").is_dir());
}

#[test]
fn a_start_without_the_office_secrets_key_serves_the_office() {
    let _process = process_isolation();
    let world = World::new();
    let mut first = started(&world);
    assert!(first.host_goes_away().success());

    // The host has no key for the office: its start handoff carries `null` and the office
    // still opens and serves its plain databases; only the CA is unavailable.
    let mut session = handshake(&world, FirstLine::Start, INSTANCE, None);
    assert_eq!(session.event()["event"], "ready");
    assert_eq!(hello(&world).office.id, OFFICE);
    assert!(session.host_goes_away().success());
    assert!(
        !session.run.stderr().contains(SECRETS_KEY_HEX),
        "the key reached the log"
    );

    let mut again = started(&world);
    assert!(again.host_goes_away().success());
}

#[test]
fn a_wrong_secrets_key_still_opens_the_office_store_without_logging_keys() {
    let _process = process_isolation();
    let world = World::new();
    let mut first = started(&world);
    assert!(first.host_goes_away().success());

    // ADR-0016 decision 3: the key only wraps private keys; a wrong one does not stop the
    // office from serving its data.
    let mut session = handshake(
        &world,
        FirstLine::Start,
        INSTANCE,
        Some(OfficeSecretsKey::from_bytes([0xC4; 32])),
    );
    assert_eq!(session.event()["event"], "ready");
    assert!(session.host_goes_away().success());
    let log = session.run.stderr();
    assert!(!log.contains(SECRETS_KEY_HEX), "the key reached the log");
    assert!(!log.contains("c4c4c4c4"), "the wrong key reached the log");

    let mut again = started(&world);
    assert!(again.host_goes_away().success());
}

#[test]
fn a_server_given_no_key_line_does_nothing() {
    let _process = process_isolation();
    // The host going away before the first line is a clean exit for every mode, discard too.
    let world = World::new();
    std::fs::create_dir_all(world.folder(OFFICE)).unwrap();
    let mut run = world.spawn(&["--discard", OFFICE]);
    let status = run.host_goes_away();
    assert!(status.success(), "{status:?}");
    assert!(run.events.recv_timeout(Duration::from_millis(200)).is_err());
    assert!(world.folder(OFFICE).exists(), "it deleted without a line");
}

fn signal(run: &Run, signal: libc::c_int) {
    // Signalling a child process we started.
    let _ = org_platform::posix::kill(run.child.id() as libc::pid_t, signal);
}

#[test]
fn a_stop_signal_closes_the_office_and_exits_clean() {
    let _process = process_isolation();
    for stop in [libc::SIGTERM, libc::SIGINT] {
        let world = World::new();
        let mut session = started(&world);
        signal(&session.run, stop);
        let status = session.run.wait();
        assert_eq!(status.code(), Some(0), "signal {stop}: {status:?}");
        session.no_stdout();
        // The office can be served again right away: its lock was released.
        let mut again = started(&world);
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
    let mut again = started(&world);
    assert_eq!(hello(&world).office.id, OFFICE);
    assert!(again.host_goes_away().success());

    // A regular file is not ours to remove.
    std::fs::write(socket(&world), b"mine").unwrap();
    let refused = handshake(
        &world,
        FirstLine::Start,
        INSTANCE,
        Some(office_secrets_key()),
    );
    assert_eq!(failed_code(refused), "storage_error");
    assert_eq!(std::fs::read(socket(&world)).unwrap(), b"mine");
}

#[test]
fn what_the_host_writes_on_stdin_never_stops_the_server_and_rendezvous_eof_does() {
    let _process = process_isolation();
    let world = World::new();
    let mut session = started(&world);
    // Lines of any shape, and a lot of them: read and thrown away, unparsed. Then stdin ends:
    // in this contract that is not the host going away, the rendezvous is.
    let mut stdin = session.run.stdin.take().unwrap();
    std::io::Write::write_all(&mut stdin, b"garbage\n{\"op\":\"nonsense\"}\n").unwrap();
    std::io::Write::write_all(&mut stdin, &vec![b'x'; 3 * 1024 * 1024]).unwrap();
    drop(stdin);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        hello(&world).office.id,
        OFFICE,
        "the server stopped serving"
    );
    assert!(session.host_goes_away().success());
    session.no_stdout();
}

#[test]
fn the_keys_never_reach_the_log_and_fds_0_and_1_are_dev_null() {
    let _process = process_isolation();
    let world = World::new();
    let mut session = started(&world);
    let pid = session.run.child.id();
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
    assert!(session.host_goes_away().success());
    let log = session.run.stderr();
    // The handoff's grant key is [7; 32] (`activation::handshake`).
    assert!(!log.contains("07070707"), "the grant key reached the log");
    assert!(
        !log.contains(SECRETS_KEY_HEX),
        "the office secrets key reached the log"
    );
}

#[test]
fn a_folder_that_is_not_what_an_office_makes_is_storage_error_not_invalid_request() {
    let _process = process_isolation();
    // Disk state, not a bad argument: the host must be able to restart it once repaired.
    let world = World::new();
    std::fs::create_dir_all(world.folder(OFFICE)).unwrap();
    std::fs::write(world.folder(OFFICE).join("notes.txt"), b"not ours").unwrap();
    let mut session = handshake(&world, FirstLine::Discard, INSTANCE, None);
    let failed = session.event();
    assert_eq!(failed["code"], "storage_error", "{failed}");
    assert_eq!(session.run.wait().code(), Some(1));
    session.no_stdout();
    assert!(
        world.folder(OFFICE).join("notes.txt").exists(),
        "a foreign file was removed"
    );
}

#[test]
fn stopping_the_server_with_a_client_connected_closes_it_and_exits_clean() {
    let _process = process_isolation();
    let world = World::new();
    let mut session = started(&world);
    // A client that stays connected and silent: shutdown must close it, not wait for it.
    let idle = std::os::unix::net::UnixStream::connect(socket(&world)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let started = std::time::Instant::now();
    let status = session.host_goes_away();
    assert!(status.success(), "{status:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
    drop(idle);
}

/// A canonical lowercase UUIDv7 made now (the command IDs the office accepts), distinct for each `n`.
fn command_id(n: u8) -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let hi = (ms >> 16) as u32;
    let mid = (ms & 0xffff) as u16;
    format!("{hi:08x}-{mid:04x}-7000-8000-0000000000{n:02x}")
}

/// The creation's home team: the organization's top team.
const TOP_TEAM: &str = "019a8928-1337-7456-8456-0123456789ae";
/// The team the cases create under it.
const TEAM: &str = "019a8928-1337-7456-8456-0123456789b1";

fn team_create(
    client: &Client,
    command: &str,
    name: &str,
) -> Result<org_proto::org::CommandAccepted, org_client::ClientError> {
    client.call::<org_proto::org::TeamCreate>(&org_proto::org::TeamCreateParams {
        command_id: command.into(),
        team_id: TEAM.into(),
        name: name.into(),
        owner_team_id: Some(TOP_TEAM.into()),
    })
}

fn api_code(error: org_client::ClientError) -> org_proto::ErrorCode {
    match error {
        org_client::ClientError::Api(e) => e.code,
        other => panic!("{other:?}"),
    }
}

#[path = "launch/activation.rs"]
mod activation;
use activation::{handshake, Session, INSTANCE};
use org_office_launch::FirstLine;

#[path = "launch/inbox_hint.rs"]
mod inbox_hint;
