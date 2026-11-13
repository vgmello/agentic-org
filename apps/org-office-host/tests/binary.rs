//! The real `org-office-host` binary: signals, a killed host, and a second host. Kept in its
//! own test binary so its process spawns never overlap the in-process hosts' pipes.
//!
//! A host that starts keeps its offices' keys in a development key folder
//! (`ORG_OFFICE_DEV_KEY_DIR`), which only a debug build reads (a release host uses the Keychain,
//! which tests never touch), so every test that starts one runs in debug builds only.

// Only the argument checks run in release builds, and only macOS can spawn the office servers
// the other tests here start (BUG-187): in a release build or off macOS their helpers are unused.
// One attribute, because two identical `allow`s on the file are a clippy error (`duplicated_attributes`)
// in a release build off macOS, where both conditions hold.
#![cfg_attr(
    any(not(debug_assertions), not(target_os = "macos")),
    allow(unused_imports, dead_code)
)]

mod support;

use std::fs::{self, File};
use std::ops::{Deref, DerefMut};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use org_proto::{ErrorCode, OrganizationCreate};
use serde_json::json;
use support::{alive, eventually, fake_server, has_hex_run, hq, Client, TestHome};

const WAIT: Duration = Duration::from_secs(10);

/// How long a stopping host gets to exit. The binary runs with the default timings, so it may
/// itself wait the full 10 s shutdown grace for a server before SIGKILLing it; on a loaded
/// machine a server can be that slow. A host still running after this is stuck.
const EXIT_WAIT: Duration = Duration::from_secs(30);
const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

/// An environment variable every test host gets, to show it isn't passed on.
const CANARY: &str = "ORG_HOST_TEST_CANARY";

/// A host process that is killed (and reaped) when dropped, so a failed assertion never leaves
/// one running.
struct HostProcess(Child);

impl Deref for HostProcess {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl DerefMut for HostProcess {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn spawn_host(home: &TestHome, stderr_to: &str) -> HostProcess {
    let stderr = File::create(home.path().join(stderr_to)).unwrap();
    HostProcess(
        Command::new(env!("CARGO_BIN_EXE_org-office-host"))
            .arg("--home")
            .arg(home.path())
            .arg("--server-bin")
            .arg(fake_server())
            // Something of the login session's that must never reach a server.
            .env(CANARY, "from-the-host-environment")
            // The host's own development key folder: never passed on to a server.
            .env("ORG_OFFICE_DEV_KEY_DIR", home.keys_dir())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap(),
    )
}

#[cfg(debug_assertions)]
struct FixtureProcess {
    child: HostProcess,
    input: std::process::ChildStdin,
    output: std::io::BufReader<std::process::ChildStdout>,
}
#[cfg(debug_assertions)]
impl FixtureProcess {
    fn create(&mut self, name: &str) -> Result<org_proto::Organization, org_proto::ApiError> {
        use std::io::{BufRead, Write};
        serde_json::to_writer(&mut self.input, name).unwrap();
        self.input.write_all(b"\n").unwrap();
        self.input.flush().unwrap();
        let mut line = String::new();
        assert!(self.output.read_line(&mut line).unwrap() > 0);
        serde_json::from_str::<org_proto::Response>(&line)
            .unwrap()
            .into_result::<OrganizationCreate>()
            .map_err(|e| match e {
                org_proto::CallError::Api(e) => e,
                other => panic!("{other}"),
            })
    }
    fn stop(&mut self) -> ExitStatus {
        use std::io::Write;
        self.input.write_all(b"STOP\n").unwrap();
        self.input.flush().unwrap();
        wait_for_exit(&mut self.child)
    }
    fn id(&self) -> u32 {
        self.child.id()
    }
}
#[cfg(debug_assertions)]
fn spawn_fixture_host(home: &TestHome) -> FixtureProcess {
    let stderr = File::create(home.path().join("host.err")).unwrap();
    let mut child = HostProcess(
        Command::new(env!("CARGO_BIN_EXE_fixture-office-host"))
            .arg(home.fixture_base())
            .arg(fake_server())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .unwrap(),
    );
    let input = child.stdin.take().unwrap();
    let output = std::io::BufReader::new(child.stdout.take().unwrap());
    FixtureProcess {
        child,
        input,
        output,
    }
}

/// A home with one stored office.
fn home_with_office() -> TestHome {
    let home = TestHome::new();
    fs::create_dir_all(home.path()).unwrap();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home
}

fn wait_until_active(home: &TestHome) {
    let socket = home.path().join("host.sock");
    // host.sock can exist a moment before the host listens on it.
    eventually(WAIT, "host.sock", || UnixStream::connect(&socket).is_ok());
    let office_socket = org_proto::location::office_socket_path(&home.path(), A).unwrap();
    eventually(WAIT, "the supervised worker to answer hello", || {
        if UnixStream::connect(&office_socket).is_err() {
            return false;
        }
        let result = Client::connect(&office_socket)
            .call::<org_proto::Hello>(&org_proto::HelloParams::default())
            .unwrap();
        result.office.is_some_and(|office| office.office.id == A)
    });
}

fn wait_for_exit(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + EXIT_WAIT;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "the host didn't exit");
        thread::sleep(Duration::from_millis(10));
    }
}

fn signal(child: &Child, signal: i32) {
    // SAFETY: sends a signal to our own child.
    assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, signal) }, 0);
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn sigterm_stops_the_servers_cleanly_and_exits_0() {
    let home = home_with_office();
    let mut host = spawn_host(&home, "host.err");
    wait_until_active(&home);
    signal(&host, libc::SIGTERM);
    let status = wait_for_exit(&mut host);
    assert!(status.success(), "{status}");
    assert!(!home.path().join("host.sock").exists());
    let pids = home.pids(A);
    assert_eq!(
        home.eof_exits(A),
        pids,
        "the server didn't exit on stdin EOF"
    );
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn sigint_also_stops_it() {
    let home = home_with_office();
    let mut host = spawn_host(&home, "host.err");
    wait_until_active(&home);
    signal(&host, libc::SIGINT);
    assert!(wait_for_exit(&mut host).success());
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn a_killed_host_takes_its_servers_with_it() {
    let home = home_with_office();
    let mut host = spawn_host(&home, "host.err");
    wait_until_active(&home);
    let server = home.pids(A)[0];
    signal(&host, libc::SIGKILL);
    wait_for_exit(&mut host);
    // The server's stdin closes with the host, so it exits by itself.
    eventually(WAIT, "the server to exit on stdin EOF", || {
        home.eof_exits(A).contains(&server)
    });
    eventually(WAIT, "the server process to be gone", || !alive(server));
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn a_second_host_binary_on_the_same_home_exits_at_once() {
    let home = home_with_office();
    let mut first = spawn_host(&home, "first.err");
    wait_until_active(&home);
    let started = Instant::now();
    let mut second = spawn_host(&home, "second.err");
    let status = wait_for_exit(&mut second);
    assert!(!status.success());
    assert!(started.elapsed() < Duration::from_secs(5));
    let stderr = fs::read_to_string(home.path().join("second.err")).unwrap();
    assert!(stderr.contains("already running"), "{stderr}");
    // The second host started nothing.
    assert_eq!(home.runs(A), 1);
    signal(&first, libc::SIGTERM);
    assert!(wait_for_exit(&mut first).success());
}

#[test]
fn bad_arguments_are_refused() {
    let status = Command::new(env!("CARGO_BIN_EXE_org-office-host"))
        .arg("--frobnicate")
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
    let status = Command::new(env!("CARGO_BIN_EXE_org-office-host"))
        .args(["--home", "relative/home", "--server-bin", "/nonexistent"])
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
}

/// A release build trusts only what it was built with: it refuses every argument that would
/// change that, before it starts anything.
#[cfg(not(debug_assertions))]
#[test]
fn a_release_host_refuses_the_arguments_that_change_what_it_trusts() {
    let home = TestHome::new();
    for flag in ["--home", "--server-bin", "--app-team-id"] {
        let out = Command::new(env!("CARGO_BIN_EXE_org-office-host"))
            .args([flag, home.path().to_str().unwrap()])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!out.status.success(), "{flag}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(flag) && stderr.contains("release build"),
            "{flag}: {stderr}"
        );
        assert!(!home.path().exists(), "{flag}: it made the home");
    }
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn the_host_never_logs_a_token() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    // A crash restart, a refused create, a failed create (with its discard) and a good one:
    // every path that logs, each with a token in play.
    home.script(json!({
        A: [{"do": "crash"}, {"do": "ready"}],
        "create": [
            {"do": "ready", "report": {"org_name": "Other"}},
            {"do": "fail", "code": "storage_error"},
            {"do": "ready"}
        ]
    }));
    let mut host = spawn_fixture_host(&home);
    eventually(WAIT, "the stored office ready", || home.runs(A) >= 2);
    let codes: Vec<_> = ["One", "Two", "Three"]
        .into_iter()
        .map(|name| host.create(name).map(|_| ()).map_err(|e| e.code))
        .collect();
    assert_eq!(
        codes,
        [
            Err(ErrorCode::StorageError), // refused, moved aside
            Err(ErrorCode::StorageError), // failed, discarded
            Ok(()),
        ]
    );
    assert_eq!(home.runs("create"), 3);
    assert_eq!(home.runs("discard"), 1);
    assert!(host.stop().success());
    let log = fs::read_to_string(home.path().join("host.err")).unwrap();
    assert!(
        log.contains("org-office-host:"),
        "the host logged nothing: {log}"
    );
    assert!(
        !has_hex_run(&log, 64),
        "the host's log holds 64 hex digits:\n{log}"
    );
    // Each create got a recovery code (X-02 §6.4), and none reached a log in its shape.
    assert_eq!(home.recovery_codes("create"), 3);
    for (path, text) in home
        .logs()
        .into_iter()
        .chain([(home.path().join("host.err"), log)])
    {
        assert!(
            !has_recovery_code(&text),
            "{} holds a recovery code",
            path.display()
        );
    }
    // Neither a token nor a key anywhere in the home (the host's stderr file included). The
    // keys of the stored office, the refused create and the good one stay in the key folder;
    // the failed create's went with its discard.
    home.assert_no_secret_in_home();
    assert_eq!(home.key_ids().len(), 3, "{:?}", home.key_ids());
}

/// Whether `text` holds a recovery code's shape: 8 groups of 4 Crockford Base32 characters.
fn has_recovery_code(text: &str) -> bool {
    let code = |c: u8| c.is_ascii_digit() || c.is_ascii_uppercase();
    text.as_bytes().windows(39).any(|w| {
        w.iter()
            .enumerate()
            .all(|(i, &c)| if i % 5 == 4 { c == b'-' } else { code(c) })
    })
}

#[test]
fn the_recovery_code_check_finds_one() {
    assert!(has_recovery_code(
        "x 0123-4567-89AB-CDEF-GHJK-MNPQ-RSTV-WXYZ y"
    ));
    assert!(!has_recovery_code("0123-4567-89AB-CDEF-GHJK-MNPQ-RSTV"));
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn servers_inherit_no_host_fds_while_clients_connect() {
    let home = TestHome::registered();
    fs::create_dir_all(home.path()).unwrap();
    // Every create fails at once and is discarded: two short-lived servers per create.
    home.script(json!({"create": {"do": "fail", "code": "invalid_name"}}));
    let mut host = spawn_fixture_host(&home);
    let socket = home.runtime().join("host.sock");
    eventually(WAIT, "host.sock", || UnixStream::connect(&socket).is_ok());
    // Clients connecting and leaving all the while, so the host keeps accepting.
    let stop = Arc::new(AtomicBool::new(false));
    let churn: Vec<_> = (0..4)
        .map(|_| {
            let (socket, stop) = (socket.clone(), Arc::clone(&stop));
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    drop(UnixStream::connect(&socket));
                }
            })
        })
        .collect();
    for i in 0..100 {
        let _ = host.create(&format!("Org {i}"));
    }
    stop.store(true, Ordering::Relaxed);
    for thread in churn {
        thread.join().unwrap();
    }
    // The host keeps no office log open once nothing writes it: 200 finished runs must not
    // leave 200 files open.
    if let Ok(out) = Command::new("/usr/sbin/lsof")
        .args(["-n", "-P", "-Fn", "-p", &host.id().to_string()])
        .output()
    {
        // lsof ran and listed the host, so a count of 0 means something.
        assert!(out.status.success(), "lsof failed: {out:?}");
        let listed = format!("p{}", host.id());
        assert!(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|l| l == listed),
            "lsof didn't list the host"
        );
        let open_logs = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.starts_with('n') && l.contains("/logs/"))
            .count();
        assert!(
            open_logs <= 4,
            "the host holds {open_logs} office logs open"
        );
    }
    assert!(host.stop().success());

    let log = fs::read_to_string(home.path().join("fake.log")).unwrap();
    let reports: Vec<&str> = log.lines().filter(|l| l.starts_with("fds ")).collect();
    assert_eq!(reports.len(), 200, "every server reports its fds");
    let leaked: Vec<&&str> = reports.iter().filter(|l| !l.ends_with(" -")).collect();
    assert!(leaked.is_empty(), "servers inherited host fds: {leaked:?}");
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn servers_get_only_the_allowed_environment() {
    let home = home_with_office();
    let mut host = spawn_host(&home, "host.err");
    wait_until_active(&home);
    signal(&host, libc::SIGTERM);
    assert!(wait_for_exit(&mut host).success());
    let allowed = [
        "HOME",
        "USER",
        "LOGNAME",
        "PATH",
        "TMPDIR",
        "LANG",
        "ORG_OFFICE_HOME",
        "ORG_OFFICE_RUNTIME_DIR",
    ];
    let runs = home.env_names(A);
    assert!(!runs.is_empty());
    // The host used its key folder (the server got the office's key on its first line)...
    assert_eq!(
        home.db_key_fingerprints(A),
        [home.stored_key_fingerprint(A).unwrap()]
    );
    for names in runs {
        assert!(names.iter().any(|n| n == "ORG_OFFICE_HOME"), "{names:?}");
        assert!(!names.iter().any(|n| n == CANARY), "the canary got through");
        // ...but never passes the folder on: servers never touch a key store.
        assert!(
            !names.iter().any(|n| n == "ORG_OFFICE_DEV_KEY_DIR"),
            "the dev key folder was passed on: {names:?}"
        );
        for name in names.iter().filter(|n| !n.is_empty()) {
            assert!(allowed.contains(&name.as_str()), "{name} was passed on");
        }
    }
}
