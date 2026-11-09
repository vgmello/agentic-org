//! The real `org-office-server` binary, driven the way the host drives it: arguments, the control
//! pipe on stdin and stdout, and the office socket. The host holds the database key and sends it
//! on the first line (key custody), so there is no key store here and no test can touch a
//! Keychain: the server has no Keychain code.

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use org_client::Client;
use org_office_launch::{
    office_dir, read_server_event, write_host_message, CreateOffice, DiscardOffice, FailCode,
    HostMessage, OfficeFile, ServerEvent, StartOffice,
};
use org_proto::location::office_socket_path;
use org_proto::{AppToken, DbKey, OfficeRole};
use tempfile::TempDir;

const OFFICE: &str = "01OFFICE00000000000000HQ1";
const ORG: &str = "01ORG0000000000000000ACME1";
const WAIT: Duration = Duration::from_secs(30);

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
        let mut child = Command::new(env!("CARGO_BIN_EXE_org-office-server"))
            .args(args)
            .env("ORG_OFFICE_HOME", &self.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
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
        }
    }
}

struct Run {
    child: Child,
    stdin: Option<ChildStdin>,
    events: Receiver<ServerEvent>,
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

/// Creates the office and returns the running server.
fn create(world: &World) -> Run {
    let mut run = world.spawn(&["--create"]);
    run.send(&create_message());
    let ServerEvent::Created { office } = run.event() else {
        panic!("expected created");
    };
    assert_eq!(office.office_id, OFFICE);
    let ServerEvent::Ready {
        socket: reported, ..
    } = run.event()
    else {
        panic!("expected ready");
    };
    assert_eq!(Path::new(&reported), socket(world));
    run
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
fn create_stores_the_office_serves_hello_and_exits_clean_when_the_host_goes() {
    let world = World::new();
    let mut run = create(&world);

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

    let status = run.host_goes_away();
    assert!(status.success(), "{status:?}");
    assert!(!socket(&world).exists(), "the socket file was left behind");
}

#[test]
fn a_restart_with_the_same_key_serves_the_same_office() {
    let world = World::new();
    let mut first = create(&world);
    assert!(first.host_goes_away().success());

    let mut again = world.spawn(&["--office", OFFICE]);
    again.send(&start_message());
    assert!(matches!(again.event(), ServerEvent::Ready { .. }));
    assert_eq!(hello(&world).office.id, OFFICE);
    assert!(again.host_goes_away().success());
}

#[test]
fn creating_an_office_that_exists_is_already_exists_and_changes_nothing() {
    let world = World::new();
    let mut first = create(&world);
    assert!(first.host_goes_away().success());
    let org_before = std::fs::read(world.folder(OFFICE).join("org.db")).unwrap();

    let mut second = world.spawn(&["--create"]);
    second.send(&create_message());
    assert_eq!(failed_code(second.event()), FailCode::AlreadyExists);
    assert!(!second.wait().success());
    assert_eq!(
        std::fs::read(world.folder(OFFICE).join("org.db")).unwrap(),
        org_before
    );
    assert!(world.folder(OFFICE).join("office.json").exists());
}

#[test]
fn a_bad_name_is_invalid_name_and_leaves_nothing() {
    let world = World::new();
    let mut run = world.spawn(&["--create"]);
    let HostMessage::Create(mut create) = create_message() else {
        unreachable!()
    };
    create.org_name = "zero\u{200B}width".into();
    run.send(&HostMessage::Create(create));
    assert_eq!(failed_code(run.event()), FailCode::InvalidName);
    assert!(!run.wait().success());
    assert!(!world.folder(OFFICE).join("office.json").exists());
}

#[test]
fn the_first_line_must_match_the_flag() {
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
    let world = World::new();
    let mut run = world.spawn(&["--create"]);
    let status = run.host_goes_away();
    assert!(status.success(), "{status:?}");
    assert!(run.events.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn discard_removes_an_unfinished_create_and_only_that() {
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
    let mut first = create(&world);
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
fn discard_that_cannot_check_the_databases_is_storage_error_and_deletes_nothing() {
    let world = World::new();
    let mut first = create(&world);
    assert!(first.host_goes_away().success());
    // Lose office.json, as a crash or a person might: the databases are all that is left.
    std::fs::remove_file(world.folder(OFFICE).join("office.json")).unwrap();

    // No key: the databases cannot be looked into. A key that does not open them: the same,
    // reported as `storage_error` for a discard (never `key_missing`, which the host would
    // read as its own missing item). With the right key the organization is found: refused.
    for (key, code) in [
        (None, FailCode::StorageError),
        (Some(DbKey::from_bytes([0xC4; 32])), FailCode::StorageError),
        (Some(db_key()), FailCode::AlreadyExists),
    ] {
        let mut run = world.spawn(&["--discard", OFFICE]);
        run.send(&discard_message(key));
        assert_eq!(failed_code(run.event()), code);
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
fn a_key_that_does_not_open_the_databases_is_key_missing_and_nothing_changes() {
    let world = World::new();
    let mut first = create(&world);
    assert!(first.host_goes_away().success());
    let org_before = std::fs::read(world.folder(OFFICE).join("org.db")).unwrap();
    let office_before = std::fs::read(world.folder(OFFICE).join("office.db")).unwrap();

    let mut run = world.spawn(&["--office", OFFICE]);
    run.send(&start_message_with(DbKey::from_bytes([0xC4; 32])));
    assert_eq!(failed_code(run.event()), FailCode::KeyMissing);
    assert!(!run.wait().success());
    let log = run.stderr();
    assert!(!log.contains(DB_KEY_HEX), "the key reached the log");
    assert!(!log.contains("c4c4c4c4"), "the wrong key reached the log");

    assert_eq!(
        std::fs::read(world.folder(OFFICE).join("org.db")).unwrap(),
        org_before
    );
    assert_eq!(
        std::fs::read(world.folder(OFFICE).join("office.db")).unwrap(),
        office_before
    );
    // The right key still opens it: the refusal changed nothing.
    let mut again = world.spawn(&["--office", OFFICE]);
    again.send(&start_message());
    assert!(matches!(again.event(), ServerEvent::Ready { .. }));
    assert!(again.host_goes_away().success());
}

#[test]
fn a_server_given_no_key_line_does_nothing() {
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
    for stop in [libc::SIGTERM, libc::SIGINT] {
        let world = World::new();
        let mut run = create(&world);
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
    let world = World::new();
    let mut run = world.spawn(&["--create"]);
    std::io::Write::write_all(run.stdin.as_mut().unwrap(), b"{\"op\":\"crea").unwrap();
    let status = run.host_goes_away();
    assert_eq!(status.code(), Some(0), "{status:?}");
    assert!(run.events.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn the_socket_is_owner_only_and_other_methods_are_unknown() {
    use std::os::unix::fs::PermissionsExt;
    let world = World::new();
    let mut run = create(&world);
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
    let world = World::new();
    let mut first = create(&world);
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
    let world = World::new();
    let mut run = create(&world);
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
fn a_create_whose_socket_path_cannot_exist_fails_before_anything_is_stored() {
    let dir = tempfile::Builder::new().prefix("oo").tempdir().unwrap();
    let long = dir.path().join("x".repeat(90));
    std::fs::create_dir_all(long.join("offices")).unwrap();
    std::fs::create_dir_all(long.join("run")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_org-office-server"))
        .arg("--create")
        .env("ORG_OFFICE_HOME", &long)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    write_host_message(&mut stdin, &create_message()).unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let first = read_server_event(&mut reader).unwrap().unwrap();
    assert_eq!(failed_code(first), FailCode::InvalidRequest);
    assert_eq!(child.wait().unwrap().code(), Some(1));
    assert!(
        !office_dir(&long, OFFICE).unwrap().exists(),
        "an office was stored"
    );
}

#[test]
fn the_token_and_the_key_never_reach_the_log_and_fds_0_and_1_are_dev_null() {
    let world = World::new();
    let mut run = create(&world);
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
fn a_name_that_is_not_trimmed_is_refused_not_quietly_trimmed() {
    let world = World::new();
    for (org_name, office_name) in [(" Acme", "HQ"), ("Acme", "HQ ")] {
        let mut run = world.spawn(&["--create"]);
        let HostMessage::Create(mut create) = create_message() else {
            unreachable!()
        };
        create.org_name = org_name.into();
        create.office_name = office_name.into();
        run.send(&HostMessage::Create(create));
        assert_eq!(failed_code(run.event()), FailCode::InvalidName);
        assert!(!run.wait().success());
    }
}

#[test]
fn the_snapshot_has_the_owner_at_position_1_1_and_survives_a_restart() {
    let world = World::new();
    let mut first = create(&world);
    let client = Client::connect(socket(&world)).unwrap();
    // The organization is read by a bound connection only.
    assert_eq!(
        api_code(
            client
                .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
                .unwrap_err()
        ),
        org_proto::ErrorCode::NotAuthorized
    );
    bind(&client, token()).unwrap();
    let snapshot = client
        .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
        .unwrap();
    assert_eq!(
        snapshot.position,
        org_proto::org::Position { epoch: 1, seq: 1 }
    );
    let [owner] = &snapshot.members[..] else {
        panic!("{:?}", snapshot.members)
    };
    assert!(owner.owner && org_proto::is_valid_member_id(&owner.id));
    assert!(snapshot.teams.is_empty() && snapshot.memberships.is_empty());
    drop(client);
    assert!(first.host_goes_away().success());

    let mut again = world.spawn(&["--office", OFFICE]);
    again.send(&start_message());
    assert!(matches!(again.event(), ServerEvent::Ready { .. }));
    let client = Client::connect(socket(&world)).unwrap();
    bind(&client, token()).unwrap();
    let after = client
        .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
        .unwrap();
    assert_eq!(after, snapshot, "the organization changed across a restart");
    drop(client);
    assert!(again.host_goes_away().success());
}

#[test]
fn stopping_the_server_with_a_client_connected_closes_it_and_exits_clean() {
    let world = World::new();
    let mut run = create(&world);
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

fn bind(client: &Client, app_token: AppToken) -> Result<(), org_client::ClientError> {
    let bound = client.bind_app(app_token)?;
    assert_eq!(bound.office_id, OFFICE);
    assert_eq!(bound.member_id, None);
    Ok(())
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
fn only_the_bound_app_changes_the_organization_and_what_it_changed_survives_a_restart() {
    let world = World::new();
    let mut first = create(&world);
    let command = command_id(1);
    let client = Client::connect(socket(&world)).unwrap();

    // Anonymous, and bound with the wrong token: refused, nothing changes. A garbled command is
    // refused the same way (not told apart from a good one).
    let refused = team_create(&client, &command, "Platform").unwrap_err();
    assert_eq!(api_code(refused), org_proto::ErrorCode::NotAuthorized);
    let garbled = client
        .call::<org_proto::org::TeamCreate>(&org_proto::org::TeamCreateParams {
            command_id: "not a ulid".into(),
            team_id: String::new(),
            name: String::new(),
            owner_team_id: None,
        })
        .unwrap_err();
    assert_eq!(api_code(garbled), org_proto::ErrorCode::NotAuthorized);
    let wrong = AppToken::from_bytes([0xA8; 32]);
    assert_eq!(
        api_code(bind(&client, wrong).unwrap_err()),
        org_proto::ErrorCode::NotAuthorized
    );
    assert_eq!(
        api_code(team_create(&client, &command, "Platform").unwrap_err()),
        org_proto::ErrorCode::NotAuthorized
    );
    let snapshot = |client: &Client| {
        client
            .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
            .unwrap()
    };
    // The organization is read by a bound connection only.
    bind(&client, token()).unwrap();
    assert_eq!(
        snapshot(&client).position,
        org_proto::org::Position { epoch: 1, seq: 1 }
    );

    // Bound as the app: the command commits, and a retry is the same answer.
    let accepted = team_create(&client, &command, "Platform").unwrap();
    assert_eq!(
        accepted.position,
        Some(org_proto::org::Position { epoch: 1, seq: 2 })
    );
    assert_eq!(
        team_create(&client, &command, "Platform").unwrap(),
        accepted
    );
    assert_eq!(
        api_code(team_create(&client, &command, "Other").unwrap_err()),
        org_proto::ErrorCode::InvalidRequest,
        "the same id with other params"
    );
    let after = snapshot(&client);
    assert_eq!(after.teams.len(), 1);
    assert_eq!(after.teams[0].name, "Platform");
    drop(client);
    assert!(first.host_goes_away().success());

    // After a restart the team is there, and the old command id still gets its answer.
    let mut again = world.spawn(&["--office", OFFICE]);
    again.send(&start_message());
    assert!(matches!(again.event(), ServerEvent::Ready { .. }));
    let client = Client::connect(socket(&world)).unwrap();
    bind(&client, token()).unwrap();
    assert_eq!(snapshot(&client), after);
    assert_eq!(
        team_create(&client, &command, "Platform").unwrap(),
        accepted
    );
    assert_eq!(snapshot(&client), after);
    drop(client);
    assert!(again.host_goes_away().success());
}

fn changed(event: &org_proto::Event) -> org_proto::org::OrgChangedData {
    assert_eq!(event.event, "org.changed");
    serde_json::from_value(event.data.clone()).unwrap()
}

fn next_event(sub: &org_client::Subscription) -> org_proto::Event {
    sub.recv_timeout(WAIT).expect("no event")
}

#[test]
fn a_subscriber_follows_the_organization_and_resumes_from_where_it_was_after_a_restart() {
    let world = World::new();
    let mut first = create(&world);
    let app = Client::connect(socket(&world)).unwrap();
    let watcher = Client::connect(socket(&world)).unwrap();

    // Only a bound connection subscribes.
    assert_eq!(
        api_code(watcher.subscribe().err().unwrap()),
        org_proto::ErrorCode::NotAuthorized
    );
    bind(&app, token()).unwrap();
    bind(&watcher, token()).unwrap();
    let snapshot = app
        .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
        .unwrap();

    // Live: another connection's command arrives as `org.changed`, with its command id.
    let sub = watcher.subscribe_from(Some(snapshot.position)).unwrap();
    let command = command_id(1);
    let accepted = team_create(&app, &command, "Platform").unwrap();
    let event = next_event(&sub);
    assert_eq!(event.seq, 1);
    let data = changed(&event);
    assert_eq!(Some(data.position), accepted.position);
    assert_eq!(data.command_id, command);
    assert!(matches!(
        &data.changes[..],
        [org_proto::org::OrgChange::TeamUpserted { team }] if team.name == "Platform"
    ));
    // A refused command is not an event, and a retry of a committed one is not either.
    let _ = team_create(&app, &command_id(2), " ").unwrap_err();
    team_create(&app, &command, "Platform").unwrap();
    assert!(sub.recv_timeout(Duration::from_millis(500)).is_err());

    // A position the log does not have: take a snapshot.
    let beyond = org_proto::org::Position { epoch: 1, seq: 99 };
    let probe = Client::connect(socket(&world)).unwrap();
    bind(&probe, token()).unwrap();
    assert_eq!(
        api_code(probe.subscribe_from(Some(beyond)).err().unwrap()),
        org_proto::ErrorCode::SnapshotRequired
    );
    drop((sub, app, watcher, probe));
    assert!(first.host_goes_away().success());

    // After a restart a client resumes from the position it held and misses nothing.
    let mut again = world.spawn(&["--office", OFFICE]);
    again.send(&start_message());
    assert!(matches!(again.event(), ServerEvent::Ready { .. }));
    let app = Client::connect(socket(&world)).unwrap();
    bind(&app, token()).unwrap();
    let sub = app.subscribe_from(Some(snapshot.position)).unwrap();
    let backlog = changed(&next_event(&sub));
    assert_eq!(Some(backlog.position), accepted.position);
    assert_eq!(backlog.command_id, command);
    // ...and then follows live from there.
    let other = Client::connect(socket(&world)).unwrap();
    bind(&other, token()).unwrap();
    let rename = org_proto::org::TeamRenameParams {
        command_id: command_id(3),
        team_id: "T1".into(),
        expected_version: 1,
        name: "Platform team".into(),
    };
    let renamed = other.call::<org_proto::org::TeamRename>(&rename).unwrap();
    let live = changed(&next_event(&sub));
    assert_eq!(Some(live.position), renamed.position);
    assert_eq!(live.command_id, rename.command_id);
    drop((sub, app, other));
    assert!(again.host_goes_away().success());
}

#[test]
fn the_app_hires_an_agent_into_the_office_and_it_survives_a_restart() {
    use org_proto::org::{
        AgentProfile, AgentRuntime, AgentStatus, MemberAddAgent, MemberAddAgentParams,
        MemberOffboard, MemberOffboardParams,
    };
    let world = World::new();
    let mut first = create(&world);
    let client = Client::connect(socket(&world)).unwrap();
    bind(&client, token()).unwrap();
    let member_id = "AGENT00000000000000000000A".to_owned();
    let hire = |office: &str, command: &str, handle: &str| MemberAddAgentParams {
        command_id: command.into(),
        member_id: member_id.clone(),
        name: "Pixel".into(),
        office_id: office.into(),
        profile: AgentProfile {
            role: "engineer".into(),
            title: "Engineer".into(),
            focus: None,
            runtime: AgentRuntime::ClaudeCode,
            model: None,
            run_mode: None,
            schedule: None,
            home: None,
            messaging_name: handle.into(),
            status: AgentStatus::Active,
            offboarded_at: None,
        },
    };

    // An office this organization does not have: not_found, nothing added.
    let nowhere = client
        .call::<MemberAddAgent>(&hire("NOSUCHOFFICE", &command_id(1), "pixel"))
        .unwrap_err();
    assert_eq!(api_code(nowhere), org_proto::ErrorCode::NotFound);
    // Its own office: hired.
    let hired = client
        .call::<MemberAddAgent>(&hire(OFFICE, &command_id(2), "pixel"))
        .unwrap();
    assert_eq!(
        hired.position,
        Some(org_proto::org::Position { epoch: 1, seq: 2 })
    );
    // Its handle is taken, in any case.
    let again = client
        .call::<MemberAddAgent>(&hire(OFFICE, &command_id(3), "PIXEL"))
        .unwrap_err();
    assert_eq!(api_code(again), org_proto::ErrorCode::AlreadyExists);
    let snapshot = client
        .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
        .unwrap();
    let agent = snapshot.members.iter().find(|m| m.id == member_id).unwrap();
    assert_eq!(agent.office_id.as_deref(), Some(OFFICE));
    drop(client);
    assert!(first.host_goes_away().success());

    // After a restart: still there; offboarded, and its handle stays taken.
    let mut again = world.spawn(&["--office", OFFICE]);
    again.send(&start_message());
    assert!(matches!(again.event(), ServerEvent::Ready { .. }));
    let client = Client::connect(socket(&world)).unwrap();
    bind(&client, token()).unwrap();
    let offboarded = client
        .call::<MemberOffboard>(&MemberOffboardParams {
            command_id: command_id(4),
            member_id: member_id.clone(),
            expected_version: 1,
        })
        .unwrap();
    assert_eq!(
        offboarded.position,
        Some(org_proto::org::Position { epoch: 1, seq: 3 })
    );
    let clash = client
        .call::<MemberAddAgent>(&MemberAddAgentParams {
            member_id: "AGENT00000000000000000000B".into(),
            ..hire(OFFICE, &command_id(5), "pixel")
        })
        .unwrap_err();
    assert_eq!(api_code(clash), org_proto::ErrorCode::AlreadyExists);
    let after = client
        .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
        .unwrap();
    let agent = after.members.iter().find(|m| m.id == member_id).unwrap();
    let profile = agent.agent.as_ref().unwrap();
    assert_eq!(profile.status, AgentStatus::Offboarded);
    assert!(profile.offboarded_at.is_some());
    drop(client);
    assert!(again.host_goes_away().success());
}

#[test]
fn who_lists_the_directory_with_cards_for_the_app_and_not_for_an_unbound_connection() {
    use org_proto::directory::{Who, WhoParams};
    use org_proto::org::{
        AgentProfile, AgentRuntime, AgentStatus, MemberAddAgent, MemberAddAgentParams, MemberKind,
        MemberOffboard, MemberOffboardParams, TeamAddMember, TeamAddMemberParams,
    };
    let world = World::new();
    let mut run = create(&world);
    let client = Client::connect(socket(&world)).unwrap();

    // Unbound: not_bound, whatever the params.
    let unbound = client.call::<Who>(&WhoParams::default()).unwrap_err();
    assert_eq!(api_code(unbound), org_proto::ErrorCode::NotBound);
    bind(&client, token()).unwrap();

    // The owner alone to begin with: their handle is `owner`, not their name.
    let first = client.call::<Who>(&WhoParams::default()).unwrap();
    let [owner] = &first.members[..] else {
        panic!("{:?}", first.members)
    };
    assert!(owner.owner);
    assert_eq!(owner.handle.as_deref(), Some("owner"));

    // A team, an agent in it (placed in this office), and what the card then says.
    team_create(&client, &command_id(1), "Platform").unwrap();
    let agent_id = "AGENT00000000000000000000A".to_owned();
    client
        .call::<MemberAddAgent>(&MemberAddAgentParams {
            command_id: command_id(2),
            member_id: agent_id.clone(),
            name: "Pixel the engineer".into(),
            office_id: OFFICE.into(),
            profile: AgentProfile {
                role: "engineer".into(),
                title: "Senior Engineer".into(),
                focus: None,
                runtime: AgentRuntime::ClaudeCode,
                model: None,
                run_mode: None,
                schedule: None,
                home: None,
                messaging_name: "pixel".into(),
                status: AgentStatus::Active,
                offboarded_at: None,
            },
        })
        .unwrap();
    client
        .call::<TeamAddMember>(&TeamAddMemberParams {
            command_id: command_id(3),
            membership_id: "m1".into(),
            team_id: "T1".into(),
            member_id: agent_id.clone(),
            title_in_team: Some("Lead".into()),
        })
        .unwrap();

    let by_handle = client
        .call::<Who>(&WhoParams {
            member: Some("PIXEL".into()),
            ..WhoParams::default()
        })
        .unwrap();
    let [pixel] = &by_handle.members[..] else {
        panic!("{:?}", by_handle.members)
    };
    assert_eq!(pixel.id, agent_id);
    assert_eq!(pixel.kind, MemberKind::Agent);
    assert_eq!(pixel.role_title.as_deref(), Some("Senior Engineer"));
    assert_eq!(pixel.office.as_ref().map(|o| o.name.as_str()), Some("HQ"));
    assert_eq!(pixel.teams[0].name, "Platform");
    assert_eq!(pixel.teams[0].title_in_team.as_deref(), Some("Lead"));
    // Filters: by team name, and by kind.
    let in_team = client
        .call::<Who>(&WhoParams {
            team: Some("platform".into()),
            ..WhoParams::default()
        })
        .unwrap();
    assert_eq!(in_team.members.len(), 1);
    let people = client
        .call::<Who>(&WhoParams {
            kind: Some(MemberKind::Person),
            ..WhoParams::default()
        })
        .unwrap();
    assert!(people.members.iter().all(|c| c.kind == MemberKind::Person));
    let nobody = client
        .call::<Who>(&WhoParams {
            member: Some("nobody".into()),
            ..WhoParams::default()
        })
        .unwrap_err();
    assert_eq!(api_code(nobody), org_proto::ErrorCode::NotFound);

    // Offboarded: gone from the directory, and naming it is not_found.
    client
        .call::<MemberOffboard>(&MemberOffboardParams {
            command_id: command_id(4),
            member_id: agent_id.clone(),
            expected_version: 1,
        })
        .unwrap();
    let after = client.call::<Who>(&WhoParams::default()).unwrap();
    assert!(after.members.iter().all(|c| c.id != agent_id));
    let gone = client
        .call::<Who>(&WhoParams {
            member: Some("pixel".into()),
            ..WhoParams::default()
        })
        .unwrap_err();
    assert_eq!(api_code(gone), org_proto::ErrorCode::NotFound);
    drop(client);
    assert!(run.host_goes_away().success());
}

#[test]
fn an_agent_and_the_owner_message_each_other_through_the_real_office_and_it_survives_a_restart() {
    use org_proto::msg::{
        MsgBind, MsgBindParams, MsgInbox, MsgInboxParams, MsgRead, MsgReadParams, MsgSend,
        MsgSendParams, MsgWait, MsgWaitParams, KIND_MSG,
    };
    use org_proto::org::{
        AgentProfile, AgentRuntime, AgentStatus, MemberAddAgent, MemberAddAgentParams,
    };
    let world = World::new();
    let mut first = create(&world);
    let app = Client::connect(socket(&world)).unwrap();
    bind(&app, token()).unwrap();
    let agent_id = "AGENT00000000000000000000A".to_owned();
    app.call::<MemberAddAgent>(&MemberAddAgentParams {
        command_id: command_id(1),
        member_id: agent_id.clone(),
        name: "Pixel".into(),
        office_id: OFFICE.into(),
        profile: AgentProfile {
            role: "engineer".into(),
            title: "Engineer".into(),
            focus: None,
            runtime: AgentRuntime::ClaudeCode,
            model: None,
            run_mode: None,
            schedule: None,
            home: None,
            messaging_name: "pixel".into(),
            status: AgentStatus::Active,
            offboarded_at: None,
        },
    })
    .unwrap();

    // The agent binds with no token, the owner with the app's.
    let bind_as = |client: &Client, member: &str, app_token: Option<AppToken>| {
        client
            .call::<MsgBind>(&MsgBindParams {
                member: Some(member.into()),
                app_token,
            })
            .unwrap()
    };
    let agent = Client::connect(socket(&world)).unwrap();
    assert_eq!(
        bind_as(&agent, "pixel", None).member_id,
        Some(agent_id.clone())
    );
    let owner = Client::connect(socket(&world)).unwrap();
    let owner_id = bind_as(&owner, "owner", Some(token())).member_id.unwrap();

    let send = |client: &Client, n: u8, to: &str, body: &str| {
        client
            .call::<MsgSend>(&MsgSendParams {
                id: command_id(n),
                to: to.into(),
                kind: KIND_MSG.into(),
                priority: None,
                subject: Some("hello".into()),
                body: body.into(),
                meta: Default::default(),
                reply_to: None,
            })
            .unwrap()
    };
    let inbox = MsgInboxParams {
        include_all: true,
        from: None,
        thread_id: None,
        cursor: None,
        limit: None,
    };
    send(&agent, 10, "owner", "from the agent");
    let seen = owner.call::<MsgInbox>(&inbox).unwrap();
    assert_eq!(seen.headers.len(), 1);
    assert_eq!(seen.headers[0].from_member_id, agent_id);
    assert_eq!(seen.headers[0].to_member_id, owner_id);
    let read = owner.call::<MsgRead>(&MsgReadParams { id: None }).unwrap();
    assert_eq!(read.message.unwrap().body, "from the agent");

    // The owner's message wakes the agent's held wait.
    let waiting = {
        let agent = Client::connect(socket(&world)).unwrap();
        bind_as(&agent, "pixel", None);
        std::thread::spawn(move || {
            agent
                .call::<MsgWait>(&MsgWaitParams {
                    timeout_ms: 30_000,
                    from: None,
                    urgent_only: false,
                })
                .unwrap()
        })
    };
    std::thread::sleep(Duration::from_millis(300));
    send(&owner, 11, "pixel", "from the owner");
    let woken = waiting.join().unwrap();
    assert_eq!(woken.message.unwrap().body, "from the owner");

    // A restart keeps both messages, and the agent binds again.
    drop((app, agent, owner));
    assert!(first.host_goes_away().success());
    let mut again = world.spawn(&["--office", OFFICE]);
    again.send(&start_message());
    assert!(matches!(again.event(), ServerEvent::Ready { .. }));
    let agent = Client::connect(socket(&world)).unwrap();
    bind_as(&agent, "pixel", None);
    let all = agent.call::<MsgInbox>(&inbox).unwrap();
    assert_eq!(all.headers.len(), 1);
    assert_eq!(all.headers[0].from_member_id, owner_id);
    assert!(again.host_goes_away().success());
}
