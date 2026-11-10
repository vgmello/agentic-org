//! The real `org-office-host` binary: signals, a killed host, and a second host. Kept in its
//! own test binary so its process spawns never overlap the in-process hosts' pipes.
//!
//! A host that starts keeps its offices' keys in a development key folder
//! (`ORG_OFFICE_DEV_KEY_DIR`), which only a debug build reads (a release host uses the Keychain,
//! which tests never touch), so every test that starts one runs in debug builds only.

// Only the argument checks run in release builds.
#![cfg_attr(not(debug_assertions), allow(unused_imports, dead_code))]

mod support;

use std::fs::{self, File};
use std::ops::{Deref, DerefMut};
use std::os::unix::net::UnixStream;
use std::path::Path;
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
    spawn_host_running(home, stderr_to, &fake_server())
}

/// A host whose office server program is `server_bin`.
fn spawn_host_running(home: &TestHome, stderr_to: &str, server_bin: &Path) -> HostProcess {
    let stderr = File::create(home.path().join(stderr_to)).unwrap();
    HostProcess(
        Command::new(env!("CARGO_BIN_EXE_org-office-host"))
            .arg("--home")
            .arg(home.path())
            .arg("--server-bin")
            .arg(server_bin)
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

#[cfg(debug_assertions)]
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
#[test]
fn sigint_also_stops_it() {
    let home = home_with_office();
    let mut host = spawn_host(&home, "host.err");
    wait_until_active(&home);
    signal(&host, libc::SIGINT);
    assert!(wait_for_exit(&mut host).success());
}

/// The early resumer's attack, end to end against the real (debug) host: the pre-exec image the
/// host spawned and resumed makes a socket without close-on-exec, forks a helper that keeps it,
/// and turns itself into the genuine server (`POSIX_SPAWN_SETEXEC`, suspended) with that socket
/// on fd 2 and `OBJC_HELP=YES`. The helper stops the host, connects the socket to the key socket,
/// shuts down its sending side and resumes the server, whose runtime writes its `OBJC_HELP` text
/// to fd 2 before `main`: the write fails, but makes the server the connection's last user, so
/// the host's peer rule (the pid, in debug builds) passes. The server then stops (standing in for
/// the attacker stopping it there), and the helper resumes the host. The peer-fd check refuses:
/// the admitted server holds the connection on fd 2. The helper gets nothing, the server is
/// killed, and the retry (a plain server) starts the office.
#[cfg(debug_assertions)]
#[test]
fn an_early_resumers_helper_gets_nothing_from_the_servers_pre_main_write() {
    let home = home_with_office();
    let attack = home.wrapper(
        "early-resumer",
        r#"if [ -e "$ORG_OFFICE_HOME/attacked" ]; then exec "$FAKE" "$@"; fi
: > "$ORG_OFFICE_HOME/attacked"
exec "$FAKE" --early-resumer "$@""#,
    );
    let mut host = spawn_host_running(&home, "host.err", &attack);
    eventually(WAIT, "the early resumer's helper to finish", || {
        home.log_count("early-resumer ") == 1
    });
    let record = home.log_lines("early-resumer ")[0].clone();
    let fields: Vec<&str> = record.split(' ').collect();
    assert_eq!(fields[1], A, "{record}");
    let server: u32 = fields[3].parse().unwrap();
    assert_eq!(
        fields[4], "0",
        "the early resumer's helper got the hand-off line"
    );
    wait_until_active(&home);
    assert!(!alive(server), "the refused server is still running");
    let stderr = fs::read_to_string(home.path().join("host.err")).unwrap();
    assert!(
        stderr.contains(&format!("office server {server}, key socket ")),
        "{stderr}"
    );
    // Refused by the fd check, not by the pid rule: the attack did make the server the peer.
    assert!(
        stderr.contains(
            "refused the connection to its key socket: it holds the connection on \
                         standard fd 2"
        ),
        "{stderr}"
    );
    assert!(!stderr.contains("another pid"), "{stderr}");
    // Only the retry took a key.
    assert_eq!(home.token_fingerprints(A).len(), 1);
    signal(&host, libc::SIGTERM);
    assert!(wait_for_exit(&mut host).success());
}

#[cfg(debug_assertions)]
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
    // Neither a token nor a key anywhere in the home (the host's stderr file included). The
    // keys of the stored office, the refused create and the good one stay in the key folder;
    // the failed create's went with its discard.
    home.assert_no_secret_in_home();
    assert_eq!(home.key_ids().len(), 3, "{:?}", home.key_ids());
}

#[cfg(debug_assertions)]
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

#[cfg(debug_assertions)]
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
    ];
    let runs = home.env_names(A);
    assert!(!runs.is_empty());
    // The host used its key folder (the server got the office's key on its hand-off)...
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
