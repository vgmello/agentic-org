//! The real `org-office-server` binary, driven the way the host drives it: arguments, the control
//! pipe on stdin and stdout, and the office socket. Built with `dev-keys`, so keys are files in a
//! temporary folder and no test ever touches a real Keychain.

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use org_client::Client;
use org_office_launch::{
    office_dir, read_server_event, write_host_message, CreateOffice, FailCode, HostMessage,
    OfficeFile, ServerEvent, StartOffice,
};
use org_proto::location::office_socket_path;
use org_proto::{AppToken, OfficeRole};
use tempfile::TempDir;

const OFFICE: &str = "01OFFICE00000000000000HQ1";
const ORG: &str = "01ORG0000000000000000ACME1";
const WAIT: Duration = Duration::from_secs(30);

/// A temporary office home and key folder.
struct World {
    _dir: TempDir,
    home: PathBuf,
    keys: PathBuf,
}

impl World {
    fn new() -> Self {
        // A short path: unix socket paths are limited to ~100 bytes.
        let dir = tempfile::Builder::new().prefix("oo").tempdir().unwrap();
        let home = dir.path().join("h");
        let keys = dir.path().join("k");
        std::fs::create_dir_all(home.join("offices")).unwrap();
        std::fs::create_dir_all(home.join("run")).unwrap();
        Self {
            _dir: dir,
            home,
            keys,
        }
    }

    fn folder(&self, id: &str) -> PathBuf {
        office_dir(&self.home, id).unwrap()
    }

    fn key_file(&self, id: &str) -> PathBuf {
        self.keys.join(format!("{id}.key"))
    }

    fn spawn(&self, args: &[&str]) -> Run {
        let mut child = Command::new(env!("CARGO_BIN_EXE_org-office-server"))
            .args(args)
            .env("ORG_OFFICE_HOME", &self.home)
            .env("ORG_OFFICE_DEV_KEY_DIR", &self.keys)
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

fn create_message() -> HostMessage {
    HostMessage::Create(CreateOffice {
        org_id: ORG.into(),
        org_name: "Acme".into(),
        office_id: OFFICE.into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        app_token: token(),
    })
}

fn start_message() -> HostMessage {
    HostMessage::Start(StartOffice { app_token: token() })
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
    assert!(world.key_file(OFFICE).exists(), "the key was made");

    let status = run.host_goes_away();
    assert!(status.success(), "{status:?}");
    assert!(!socket(&world).exists(), "the socket file was left behind");
}

#[test]
fn a_restart_serves_the_same_office_with_its_key() {
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
    let key_before = std::fs::read(world.key_file(OFFICE)).unwrap();

    let mut second = world.spawn(&["--create"]);
    second.send(&create_message());
    assert_eq!(failed_code(second.event()), FailCode::AlreadyExists);
    assert!(!second.wait().success());
    assert_eq!(std::fs::read(world.key_file(OFFICE)).unwrap(), key_before);
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
    assert!(!world.key_file(OFFICE).exists());
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
fn discard_removes_an_unfinished_create_and_its_key_and_only_that() {
    let world = World::new();
    // An unfinished create: a folder with the lock file and a key, no office.json.
    std::fs::create_dir_all(world.folder("UNFINISHED1")).unwrap();
    std::fs::create_dir_all(&world.keys).unwrap();
    std::fs::write(world.key_file("UNFINISHED1"), "11".repeat(32)).unwrap();

    let mut run = world.spawn(&["--discard", "UNFINISHED1"]);
    assert!(matches!(run.event(), ServerEvent::Discarded));
    assert!(run.wait().success());
    assert!(!world.folder("UNFINISHED1").exists());
    assert!(!world.key_file("UNFINISHED1").exists());

    // A finished office is refused and untouched.
    let mut first = create(&world);
    assert!(first.host_goes_away().success());
    let mut run = world.spawn(&["--discard", OFFICE]);
    assert_eq!(failed_code(run.event()), FailCode::AlreadyExists);
    assert!(!run.wait().success());
    assert!(world.folder(OFFICE).join("office.json").exists());
    assert!(world.key_file(OFFICE).exists());
}

#[test]
fn a_lost_key_is_key_missing_and_nothing_is_made() {
    let world = World::new();
    let mut first = create(&world);
    assert!(first.host_goes_away().success());
    std::fs::remove_file(world.key_file(OFFICE)).unwrap();

    let mut run = world.spawn(&["--office", OFFICE]);
    run.send(&start_message());
    assert_eq!(failed_code(run.event()), FailCode::KeyMissing);
    assert!(!run.wait().success());
    assert!(!world.key_file(OFFICE).exists(), "a key was made");
}

#[test]
fn a_stop_signal_closes_the_office_and_exits_clean() {
    let world = World::new();
    let mut run = create(&world);
    // SAFETY: signalling a child process we started.
    #[allow(unsafe_code)]
    unsafe {
        libc::kill(run.child.id() as libc::pid_t, libc::SIGTERM);
    }
    let status = run.wait();
    assert!(status.success(), "{status:?}");
    // The office can be served again right away: its lock was released.
    let mut again = world.spawn(&["--office", OFFICE]);
    again.send(&start_message());
    assert!(matches!(again.event(), ServerEvent::Ready { .. }));
    assert!(again.host_goes_away().success());
}

#[test]
fn the_token_never_reaches_the_log_and_fds_0_and_1_are_dev_null() {
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
}
