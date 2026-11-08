//! The real `org-office-host` binary: signals, a killed host, and a second host. Kept in its
//! own test binary so its process spawns never overlap the in-process hosts' pipes.

mod support;

use std::fs::{self, File};
use std::ops::{Deref, DerefMut};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use org_proto::{
    Empty, ErrorCode, OfficeList, OfficeStatus, OrganizationCreate, OrganizationCreateParams,
};
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
            // A dev-keys server's key folder: passed on in debug builds only.
            .env("ORG_OFFICE_DEV_KEY_DIR", "/tmp/org-dev-keys-test")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap(),
    )
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
    eventually(WAIT, "the office to be active", || {
        Client::connect(&socket)
            .call::<OfficeList>(&Empty {})
            .unwrap()
            .offices
            .iter()
            .any(|o| o.id == A && o.status == OfficeStatus::Active)
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

#[test]
fn sigint_also_stops_it() {
    let home = home_with_office();
    let mut host = spawn_host(&home, "host.err");
    wait_until_active(&home);
    signal(&host, libc::SIGINT);
    assert!(wait_for_exit(&mut host).success());
}

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

#[test]
fn the_host_never_logs_a_token() {
    let home = home_with_office();
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
    let mut host = spawn_host(&home, "host.err");
    wait_until_active(&home);
    let mut client = Client::connect(&home.path().join("host.sock"));
    let codes: Vec<_> = ["One", "Two", "Three"]
        .into_iter()
        .map(|name| {
            client
                .call::<OrganizationCreate>(&OrganizationCreateParams { name: name.into() })
                .map(|_| ())
                .map_err(|e| e.code)
        })
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
    signal(&host, libc::SIGTERM);
    assert!(wait_for_exit(&mut host).success());
    let log = fs::read_to_string(home.path().join("host.err")).unwrap();
    assert!(
        log.contains("org-office-host:"),
        "the host logged nothing: {log}"
    );
    assert!(
        !has_hex_run(&log, 64),
        "the host's log holds 64 hex digits:\n{log}"
    );
    home.assert_no_token_in_logs();
}

#[test]
fn servers_inherit_no_host_fds_while_clients_connect() {
    let home = TestHome::new();
    fs::create_dir_all(home.path()).unwrap();
    // Every create fails at once and is discarded: two short-lived servers per create.
    home.script(json!({"create": {"do": "fail", "code": "invalid_name"}}));
    let mut host = spawn_host(&home, "host.err");
    let socket = home.path().join("host.sock");
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
    let mut client = Client::connect(&socket);
    for i in 0..100 {
        let _ = client.call::<OrganizationCreate>(&OrganizationCreateParams {
            name: format!("Org {i}"),
        });
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
    signal(&host, libc::SIGTERM);
    assert!(wait_for_exit(&mut host).success());

    let log = fs::read_to_string(home.path().join("fake.log")).unwrap();
    let reports: Vec<&str> = log.lines().filter(|l| l.starts_with("fds ")).collect();
    assert_eq!(reports.len(), 200, "every server reports its fds");
    let leaked: Vec<&&str> = reports.iter().filter(|l| !l.ends_with(" -")).collect();
    assert!(leaked.is_empty(), "servers inherited host fds: {leaked:?}");
}

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
        // Debug builds only (checked below).
        "ORG_OFFICE_DEV_KEY_DIR",
    ];
    let runs = home.env_names(A);
    assert!(!runs.is_empty());
    for names in runs {
        assert!(names.iter().any(|n| n == "ORG_OFFICE_HOME"), "{names:?}");
        assert!(!names.iter().any(|n| n == CANARY), "the canary got through");
        // The test host is given a dev key folder (see `spawn_host`). A debug host passes it on
        // to a dev-keys server; a release host never does.
        let dev_keys = names.iter().any(|n| n == "ORG_OFFICE_DEV_KEY_DIR");
        if cfg!(debug_assertions) {
            assert!(dev_keys, "the dev key folder wasn't passed on: {names:?}");
        } else {
            assert!(
                !dev_keys,
                "a release host passed the dev key folder on: {names:?}"
            );
        }
        for name in names.iter().filter(|n| !n.is_empty()) {
            assert!(allowed.contains(&name.as_str()), "{name} was passed on");
        }
    }
}
