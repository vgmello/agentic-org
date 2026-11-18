//! The real `agentic-org serve` binary: signals, a killed host, and a second host. Kept in its
//! own test binary so its process spawns never overlap the in-process hosts' pipes.
//!
//! Each host serves a registered instance (`serve --name default`) that A2's setup fixture
//! (`org_host::setup_flow::fixture`) reserved under a temporary application base
//! (`ORG_DEV_ACCOUNT_BASE`), with one stored office the fake server runs. A host that starts
//! keeps its keys in a development key folder (`ORG_OFFICE_DEV_KEY_DIR`), which only a debug
//! build reads (a release host uses the Keychain, which tests never touch), so every test that
//! starts one runs in debug builds only.

// Only the argument checks run in release builds: their helpers are unused there.
#![cfg_attr(not(debug_assertions), allow(unused_imports, dead_code))]

mod support;

use std::fs::{self, File};
use std::io::{Read, Write};
use std::ops::{Deref, DerefMut};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(debug_assertions)]
use org_host::setup_flow::fixture::{registered_account, RegisteredAccount};
use org_office_launch::OfficeFile;
#[cfg(not(debug_assertions))]
use support::TestHome;
use support::{alive, eventually, fake_server, hq, may_run_the_service, Client};

const WAIT: Duration = Duration::from_secs(10);

/// How long a stopping host gets to exit. The binary runs with the default timings, so it may
/// itself wait the full 10 s shutdown grace for a server before SIGKILLing it; on a loaded
/// machine a server can be that slow. A host still running after this is stuck.
const EXIT_WAIT: Duration = Duration::from_secs(30);
const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

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

/// A registered instance (setup's steps 2 and 3, through A2's fixture) under a short temporary
/// base `<tmp>/a`, its development keys under `<tmp>/k`, and one stored office the fake server
/// runs. `agentic-org serve --name default` serves it, as the account's service does after
/// setup.
#[cfg(debug_assertions)]
struct Registered {
    dir: tempfile::TempDir,
    account: RegisteredAccount,
}

#[cfg(debug_assertions)]
impl Registered {
    fn new() -> Self {
        let dir = support::short_tempdir("ob");
        let root = dir.path().canonicalize().unwrap();
        for name in ["a", "k"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(name))
                .unwrap();
        }
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let account = registered_account(&root.join("a"), None, port).unwrap();
        let registered = Self { dir, account };
        registered.store_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
        registered
    }

    fn base(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap()
    }

    /// The office home the registered host serves (and the fake server logs in).
    fn home(&self) -> &Path {
        &self.account.office_home
    }

    /// Stores an office as if the host had provisioned it: its `office.json` in the instance's
    /// office home and a key in the instance's development key folder (the layout
    /// `TestHome::write_office` uses for a registered home).
    fn store_office(&self, office: &OfficeFile) {
        let dir = self.home().join("offices").join(&office.office_id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("office.json"), office.to_vec().unwrap()).unwrap();
        let keys = self.base().join("k").join(&self.account.instance_id);
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&keys)
            .unwrap();
        let mut bytes = [0u8; 32];
        File::open("/dev/urandom")
            .unwrap()
            .read_exact(&mut bytes)
            .unwrap();
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(keys.join(format!("{}.key", office.office_id)))
            .unwrap()
            .write_all(hex.as_bytes())
            .unwrap();
    }

    fn host_socket(&self) -> PathBuf {
        self.account.host_socket()
    }

    /// Starts `agentic-org serve --name default` on the instance, with only the environment
    /// the account's service gives it plus the development base and key folder.
    fn spawn_host(&self, stderr_to: &str) -> HostProcess {
        let stderr = File::create(self.base().join(stderr_to)).unwrap();
        HostProcess(
            Command::new(env!("CARGO_BIN_EXE_agentic-org"))
                .args(["serve", "--name", "default", "--server-bin"])
                .arg(fake_server())
                .env_clear()
                .env("HOME", self.base())
                .env("ORG_DEV_ACCOUNT_BASE", self.base().join("a"))
                .env("ORG_OFFICE_DEV_KEY_DIR", self.base().join("k"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(stderr)
                .spawn()
                .unwrap(),
        )
    }

    fn stderr(&self, name: &str) -> String {
        fs::read_to_string(self.base().join(name)).unwrap()
    }

    /// The pids in the fake server's log lines of `kind` for `key` (see
    /// `testing/fake_office_server.rs`), in order.
    fn logged_pids(&self, kind: &str, key: &str) -> Vec<u32> {
        fs::read_to_string(self.home().join("fake.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| l.split(' ').collect::<Vec<_>>())
            .filter(|f| f[0] == kind && f[1] == key)
            .map(|f| f[2].parse().unwrap())
            .collect()
    }

    /// The pids of the fake's runs for `key`, in order.
    fn pids(&self, key: &str) -> Vec<u32> {
        self.logged_pids("start", key)
    }

    /// The pids that exited cleanly on stdin EOF.
    fn eof_exits(&self, key: &str) -> Vec<u32> {
        self.logged_pids("eof-exit", key)
    }

    /// How many times the fake ran for `key`.
    fn runs(&self, key: &str) -> u64 {
        fs::metadata(self.home().join("fake-runs").join(key))
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Waits until `host` listens on `host.sock` and its supervised worker for `A` answers hello
    /// on its office socket in the instance's runtime folder.
    fn wait_until_active(&self, host: &mut HostProcess) {
        let socket = self.host_socket();
        // host.sock can exist a moment before the host listens on it.
        eventually(WAIT, "host.sock", || {
            assert!(host.try_wait().unwrap().is_none(), "serve exited early");
            UnixStream::connect(&socket).is_ok()
        });
        let office_socket =
            org_proto::location::office_socket_in(&self.account.runtime, A).unwrap();
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
    if !may_run_the_service("sigterm_stops_the_servers_cleanly_and_exits_0") {
        return;
    }
    let instance = Registered::new();
    let mut host = instance.spawn_host("host.err");
    instance.wait_until_active(&mut host);
    signal(&host, libc::SIGTERM);
    let status = wait_for_exit(&mut host);
    assert!(
        status.success(),
        "{status}: {}",
        instance.stderr("host.err")
    );
    assert!(!instance.host_socket().exists());
    let pids = instance.pids(A);
    assert!(!pids.is_empty());
    assert_eq!(
        instance.eof_exits(A),
        pids,
        "the server didn't exit on stdin EOF"
    );
}

/// X5 review V2: the stop is bounded whatever a worker does. A worker that ignores the
/// end of its input (as one that is stopped or hung does) is killed when the grace ends,
/// and the service still exits 0 inside the 3 s of Docker Desktop's default grace (Linux
/// host design, section 3.4).
#[cfg(debug_assertions)]
#[test]
fn a_worker_that_ignores_the_stop_is_killed_and_the_service_exits_0_within_3_seconds() {
    const NAME: &str =
        "a_worker_that_ignores_the_stop_is_killed_and_the_service_exits_0_within_3_seconds";
    if !may_run_the_service(NAME) {
        return;
    }
    let instance = Registered::new();
    fs::write(
        instance.home().join("fake-script.json"),
        format!(r#"{{"{A}": {{"do": "ready", "ignore_eof": true}}}}"#),
    )
    .unwrap();
    let mut host = instance.spawn_host("host.err");
    instance.wait_until_active(&mut host);
    let pids = instance.pids(A);
    assert_eq!(pids.len(), 1);
    let alive = |pid: u32| unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
    assert!(alive(pids[0]));
    let asked = Instant::now();
    signal(&host, libc::SIGTERM);
    let status = wait_for_exit(&mut host);
    let took = asked.elapsed();
    println!("the service stopped {took:?} after SIGTERM with a worker that ignores it");
    assert!(
        status.success(),
        "{status}: {}",
        instance.stderr("host.err")
    );
    assert!(took < Duration::from_secs(3), "{took:?}");
    // It was given the grace first: the stop did not simply kill at once.
    assert!(took >= Duration::from_millis(1400), "{took:?}");
    assert!(
        instance.eof_exits(A).is_empty(),
        "the worker exited by itself"
    );
    eventually(Duration::from_secs(2), "the worker to be gone", || {
        !alive(pids[0])
    });
    assert!(!instance.host_socket().exists());
}

#[cfg(debug_assertions)]
#[test]
fn sigint_also_stops_it() {
    if !may_run_the_service("sigint_also_stops_it") {
        return;
    }
    let instance = Registered::new();
    let mut host = instance.spawn_host("host.err");
    instance.wait_until_active(&mut host);
    signal(&host, libc::SIGINT);
    let status = wait_for_exit(&mut host);
    assert!(
        status.success(),
        "{status}: {}",
        instance.stderr("host.err")
    );
    assert!(!instance.host_socket().exists());
}

#[cfg(debug_assertions)]
#[test]
fn a_killed_host_takes_its_servers_with_it() {
    if !may_run_the_service("a_killed_host_takes_its_servers_with_it") {
        return;
    }
    let instance = Registered::new();
    let mut host = instance.spawn_host("host.err");
    instance.wait_until_active(&mut host);
    let server = instance.pids(A)[0];
    assert!(alive(server));
    signal(&host, libc::SIGKILL);
    wait_for_exit(&mut host);
    // The server's stdin closes with the host, so it exits by itself.
    eventually(WAIT, "the server to exit on stdin EOF", || {
        instance.eof_exits(A).contains(&server)
    });
    eventually(WAIT, "the server process to be gone", || !alive(server));
}

#[cfg(debug_assertions)]
#[test]
fn a_second_host_binary_on_the_same_home_exits_at_once() {
    if !may_run_the_service("a_second_host_binary_on_the_same_home_exits_at_once") {
        return;
    }
    let instance = Registered::new();
    let mut first = instance.spawn_host("first.err");
    instance.wait_until_active(&mut first);
    let started = Instant::now();
    let mut second = instance.spawn_host("second.err");
    let status = wait_for_exit(&mut second);
    assert!(!status.success());
    assert!(started.elapsed() < Duration::from_secs(5));
    // A registered host holds its instance's state lock while it runs: the second one is
    // refused there, before `host.lock`, custody or the registry.
    let stderr = instance.stderr("second.err");
    assert!(
        stderr.contains(
            "reopening the reserved instance: instance default is in use by another agentic-org \
             process (a running service, setup or admin reset); stop it first"
        ),
        "{stderr}"
    );
    // The second host started nothing, and the first still serves.
    assert_eq!(instance.runs(A), 1);
    assert!(first.try_wait().unwrap().is_none());
    assert!(UnixStream::connect(instance.host_socket()).is_ok());
    signal(&first, libc::SIGTERM);
    assert!(wait_for_exit(&mut first).success());
}

/// The service is an ordinary user's on every system: started as root, `serve` refuses before
/// it opens or creates anything, saying to run it as the service user. Only root can show
/// this, so the test does its work where the tests run as root (a development container)
/// and says so elsewhere; the rule itself is unit-tested for every user in `org-host`.
#[cfg(debug_assertions)]
#[test]
fn serve_started_as_root_refuses_before_it_makes_anything() {
    // SAFETY: no arguments and no side effects; it cannot fail.
    if unsafe { libc::geteuid() } != 0 {
        eprintln!(
            "serve_started_as_root_refuses_before_it_makes_anything: SKIPPED: only root can \
             start the service as root"
        );
        return;
    }
    let dir = support::short_tempdir("or");
    let base = dir.path().canonicalize().unwrap();
    let listing = |path: &Path| -> Vec<PathBuf> {
        fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    };
    for arguments in [
        vec!["serve", "--name", "default"],
        vec!["serve", "--home", base.join("h").to_str().unwrap()],
        vec!["serve"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
            .args(&arguments)
            .env_clear()
            .env("HOME", &base)
            .env("ORG_DEV_ACCOUNT_BASE", base.join("a"))
            .env("ORG_OFFICE_DEV_KEY_DIR", base.join("k"))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{arguments:?}: {stderr}");
        assert!(
            stderr.contains("does not run as root") && stderr.contains("service user"),
            "{arguments:?}: {stderr}"
        );
        assert_eq!(listing(&base), Vec::<PathBuf>::new(), "{arguments:?}");
    }
    // Asking for the version starts no service and is answered to anyone.
    let version = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
        .args(["serve", "--version"])
        .output()
        .unwrap();
    assert!(version.status.success());
}

/// `setup` reserves an instance and registers a service that runs as the user who ran it,
/// and that service refuses root: started as root, `setup` refuses before it reads or makes
/// anything (no instance, no custody, no service definition). Only root can show this with
/// the real program, so the test does its work where the tests run as root and says so
/// elsewhere; the order (refused before any option is parsed) is unit-tested without
/// privilege in `src/setup.rs`, and the rule for every user in `org-host`.
#[cfg(debug_assertions)]
#[test]
fn setup_started_as_root_refuses_before_it_makes_anything() {
    // SAFETY: no arguments and no side effects; it cannot fail.
    if unsafe { libc::geteuid() } != 0 {
        eprintln!(
            "setup_started_as_root_refuses_before_it_makes_anything: SKIPPED: only root can \
             start setup as root"
        );
        return;
    }
    let dir = support::short_tempdir("os");
    let base = dir.path().canonicalize().unwrap();
    let listing = |path: &Path| -> Vec<PathBuf> {
        fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    };
    for arguments in [
        vec!["setup"],
        vec!["setup", "--name", "lab"],
        vec!["setup", "--port", "7420"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
            .args(&arguments)
            .env_clear()
            .env("HOME", &base)
            .env("PATH", "/usr/bin:/bin")
            .env("ORG_DEV_ACCOUNT_BASE", base.join("app"))
            .env("ORG_OFFICE_DEV_KEY_DIR", base.join("keys"))
            .env("ORG_DEV_SERVICE_DIR", base.join("svc"))
            .env("ORG_DEV_DISK_ENCRYPTION", "on")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{arguments:?}: {stderr}");
        assert!(
            stderr.contains("does not run as root") && stderr.contains("service user"),
            "{arguments:?}: {stderr}"
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "",
            "{arguments:?}: it printed something"
        );
        assert_eq!(listing(&base), Vec::<PathBuf>::new(), "{arguments:?}");
    }
    // Help is not setup: answered to anyone.
    let help = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
        .args(["setup", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
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
