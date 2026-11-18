//! The real `org-host` binary: signals, a killed host, and a second host. Kept in its
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
use std::thread;
use std::time::{Duration, Instant};

use support::{alive, eventually, fake_server, hq, Client, TestHome};

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
        Command::new(env!("CARGO_BIN_EXE_agentic-org"))
            .arg("serve")
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
#[ignore = "needs a registered `agentic-org serve` (A2 B3): serve.rs starts Host::start, which starts no office on an unregistered home"]
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
#[ignore = "needs a registered `agentic-org serve` (A2 B3): serve.rs starts Host::start, which starts no office on an unregistered home"]
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
#[ignore = "needs a registered `agentic-org serve` (A2 B3): serve.rs starts Host::start, which starts no office on an unregistered home"]
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
#[ignore = "needs a registered `agentic-org serve` (A2 B3): serve.rs starts Host::start, which starts no office on an unregistered home"]
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
    let status = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
        .arg("serve")
        .arg("--frobnicate")
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
    let status = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
        .arg("serve")
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
        let out = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
            .arg("serve")
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
