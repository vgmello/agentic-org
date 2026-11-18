//! The real `agentic-org serve` binary: signals, a killed host, and a second host. Kept in its
//! own test binary so its process spawns never overlap the in-process hosts' pipes.
//!
//! Each host serves a registered instance (`serve --name default`) that A2's setup fixture
//! (`org_host::setup_flow::fixture`) reserved under a temporary application base
//! (`ORG_DEV_ACCOUNT_BASE`), with one stored office the fake server runs. A host that starts
//! keeps its keys in a development key folder (`ORG_OFFICE_DEV_KEY_DIR`), which only a debug
//! build reads (a release host uses the Keychain, which tests never touch), so every test that
//! starts one runs in debug builds only.

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
use support::{alive, eventually, fake_server, hq, Client};

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
        let dir = tempfile::Builder::new()
            .prefix("ob")
            .tempdir_in("/tmp")
            .unwrap();
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
#[cfg(target_os = "macos")]
#[test]
fn sigterm_stops_the_servers_cleanly_and_exits_0() {
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

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn sigint_also_stops_it() {
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
#[cfg(target_os = "macos")]
#[test]
fn a_killed_host_takes_its_servers_with_it() {
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
#[cfg(target_os = "macos")]
#[test]
fn a_second_host_binary_on_the_same_home_exits_at_once() {
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
