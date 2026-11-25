//! The real `agentic-org serve --setup <file>` binary stopped while it starts (X5 review,
//! third round, P1): the stop signal is taken at once, not when the start ends.
//!
//! Setup is held at its first line of output: the binary's standard output is a pipe this
//! test filled, so that line waits until the test reads the pipe. A start that waits like
//! that (as one waiting for a slow volume does) ends at the stop's bound; one that the test
//! lets finish ends without starting the host. Both exit 0: the stop was asked for.
//!
//! Each instance is set up under a temporary application base (`ORG_DEV_ACCOUNT_BASE`),
//! which only a debug build reads, so every test runs in debug builds only.

#![cfg_attr(
    not(debug_assertions),
    allow(
        unused_imports,
        dead_code,
        reason = "serve_stop tests run in debug builds only; helpers are unused in release builds"
    )
)]

mod support;

use std::fs::{self, File};
use std::io::Read;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::DirBuilderExt;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use support::{eventually, fake_server, may_run_the_service};

const WAIT: Duration = Duration::from_secs(30);

/// The tests of this file one at a time: a pipe is inheritable while it is made (it has
/// no close-on-exec flag until `full_pipe` sets it), and a `serve` started by the other
/// test meanwhile would hold its write end open.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A started `serve`, killed (and reaped) when dropped so a failed assertion never leaves one.
struct Serve(Child);

impl Drop for Serve {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// A temporary folder with a foreground setup file in it, for a loopback address on a free
/// port. The file names a credential directory only where the system keeps a host's secrets
/// in one (setup's own reading of the file says which): `needs_volume` then, since such a
/// directory must be a mounted volume, which a test cannot make.
struct Foreground {
    dir: tempfile::TempDir,
    file: PathBuf,
    needs_volume: bool,
}

impl Foreground {
    fn new() -> Self {
        let dir = support::short_tempdir("os");
        let base = dir.path().canonicalize().unwrap();
        for name in ["a", "k"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(base.join(name))
                .unwrap();
        }
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let file = base.join("setup.json");
        let write = |credentials: Option<&Path>| {
            let mut text = serde_json::json!({
                "version": 1,
                "service": "foreground",
                "root": base.join("data"),
                "listen": {
                    "address": "127.0.0.1", "port": port, "scheme": "http_loopback",
                    "origin": format!("http://127.0.0.1:{port}"), "hsts": "off"
                }
            });
            if let Some(credentials) = credentials {
                text["credentials"] = serde_json::json!(credentials);
            }
            fs::write(&file, text.to_string()).unwrap();
        };
        write(None);
        let needs_volume = match org_host::setup_flow::configuration(&file) {
            Ok(_) => false,
            Err(refused) if refused.contains("needs \"credentials\"") => {
                write(Some(&base.join("secrets")));
                org_host::setup_flow::configuration(&file).unwrap();
                true
            }
            Err(refused) => panic!("{refused}"),
        };
        Self {
            dir,
            file,
            needs_volume,
        }
    }

    fn base(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap()
    }

    /// Starts `serve --setup <file>` with `stdout` as its standard output and its standard
    /// error in `stderr.log`, and returns once `serve` watches for its stop: setup has
    /// made the folders of its application base (`<base>/a/temp`), which it does after
    /// `serve` blocked the stop signals and before setup's first line.
    fn spawn(&self, stdout: OwnedFd) -> Serve {
        let stderr = File::create(self.base().join("stderr.log")).unwrap();
        let mut serve = Serve(
            Command::new(env!("CARGO_BIN_EXE_agentic-org"))
                .arg("serve")
                .arg("--setup")
                .arg(&self.file)
                .arg("--server-bin")
                .arg(fake_server())
                .env_clear()
                .env("HOME", self.base())
                .env("ORG_DEV_ACCOUNT_BASE", self.base().join("a"))
                .env("ORG_OFFICE_DEV_KEY_DIR", self.base().join("k"))
                .stdin(Stdio::null())
                .stdout(Stdio::from(stdout))
                .stderr(stderr)
                .spawn()
                .unwrap(),
        );
        let watching = self.base().join("a").join("temp");
        eventually(WAIT, "serve to reach setup", || {
            assert!(
                serve.0.try_wait().unwrap().is_none(),
                "serve exited early: {}",
                self.stderr()
            );
            watching.is_dir()
        });
        serve
    }

    fn stderr(&self) -> String {
        fs::read_to_string(self.base().join("stderr.log")).unwrap()
    }
}

/// A pipe whose buffer is full: a write to its write end waits until the read end is read.
fn full_pipe() -> (File, OwnedFd) {
    let mut fds = [0; 2];
    // SAFETY: `fds` has room for the two descriptors pipe makes, which only these handles own.
    let (read, write) = unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
        (File::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))
    };
    use std::os::fd::AsRawFd;
    for fd in [read.as_raw_fd(), write.as_raw_fd()] {
        // SAFETY: fcntl on a descriptor this function owns. The child's standard output is
        // a copy of the write end, which does not keep the flag.
        unsafe { assert_ne!(libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC), -1) };
    }
    let fd = write.as_raw_fd();
    // SAFETY: plain fcntl and write calls on a descriptor this function owns.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        assert_ne!(libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK), -1);
        let chunk = [b'.'; 512];
        while libc::write(fd, chunk.as_ptr().cast(), chunk.len()) > 0 {}
        while libc::write(fd, chunk.as_ptr().cast(), 1) > 0 {}
        // Blocking again for the child: the flag belongs to the pipe's one open file.
        assert_ne!(libc::fcntl(fd, libc::F_SETFL, flags), -1);
    }
    (read, write)
}

/// Sends SIGTERM to `serve` and returns its status once it exits (within 30 s), with how
/// long after the signal it did.
fn stop(serve: &mut Serve) -> (ExitStatus, Duration) {
    let sent = Instant::now();
    // SAFETY: sends SIGTERM to this test's own live child process.
    let rc = unsafe { libc::kill(serve.0.id() as libc::pid_t, libc::SIGTERM) };
    assert_eq!(rc, 0);
    loop {
        if let Some(status) = serve.0.try_wait().unwrap() {
            return (status, sent.elapsed());
        }
        assert!(
            sent.elapsed() < Duration::from_secs(30),
            "serve did not exit after SIGTERM"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// A start that has not ended when the stop's bound passes (here setup, waiting to write
/// its first line) ends there, 2 s after the signal, with a line that says why, within the
/// design's 2.5 s, and with status 0: the stop was asked for. Before X5's third round the
/// signal stayed pending until setup and the host's start had ended, which here is never.
#[cfg(debug_assertions)]
#[test]
fn a_start_that_does_not_end_is_ended_at_the_stop_s_bound() {
    if !may_run_the_service("a_start_that_does_not_end_is_ended_at_the_stop_s_bound") {
        return;
    }
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let instance = Foreground::new();
    let (output, write) = full_pipe();
    let mut serve = instance.spawn(write);
    // X5 review F10: whatever mask it was started with (this test's), the service makes
    // what is its user's alone. Read where the system shows a process's mask.
    let shown = format!("/proc/{}/status", serve.0.id());
    match fs::read_to_string(&shown) {
        Ok(status) => assert!(
            status.lines().any(|line| line == "Umask:\t0077"),
            "{status}"
        ),
        Err(_) => println!("the file-creation mask is not checked: {shown} is not readable"),
    }
    let (status, took) = stop(&mut serve);
    println!("serve exited {took:?} after SIGTERM, held in setup: {status}");
    assert_eq!(status.code(), Some(0), "{status}: {}", instance.stderr());
    let stderr = instance.stderr();
    assert!(
        stderr.contains("stopped by signal 15 while starting, which had not ended"),
        "{stderr}"
    );
    assert!(took >= Duration::from_secs(2), "{took:?}");
    assert!(took < Duration::from_millis(2500), "{took:?}");
    drop(output);
}

/// A stop that comes while setup runs ends `serve` when setup returns: the instance is set
/// up, the host is never started, and the status is 0. Before X5's third round the host
/// started first and then stopped.
#[cfg(debug_assertions)]
#[test]
fn a_stop_during_setup_ends_serve_with_0_once_setup_ends_and_starts_no_host() {
    if !may_run_the_service("a_stop_during_setup_ends_serve_with_0") {
        return;
    }
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let instance = Foreground::new();
    if instance.needs_volume {
        println!(
            "a_stop_during_setup_ends_serve_with_0: SKIPPED: this system keeps a foreground \
             host's secrets in a mounted volume, which a test cannot mount"
        );
        return;
    }
    let (mut output, write) = full_pipe();
    let mut serve = instance.spawn(write);
    // The signal comes while setup waits to write its first line; then setup goes on.
    let reading = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        let mut text = Vec::new();
        output.read_to_end(&mut text).unwrap();
        String::from_utf8_lossy(&text).into_owned()
    });
    let (status, took) = stop(&mut serve);
    let printed = reading.join().unwrap();
    let stderr = instance.stderr();
    println!("serve exited {took:?} after SIGTERM, which came during setup: {status}");
    assert_eq!(status.code(), Some(0), "{status}: {stderr}");
    assert!(
        printed.contains("Service: none is registered (foreground mode)."),
        "setup did not finish: {printed}"
    );
    assert!(stderr.contains("stopped before serving"), "{stderr}");
    assert!(
        instance.base().join("data/office-state").is_dir(),
        "setup did not make the instance's layout"
    );
}
