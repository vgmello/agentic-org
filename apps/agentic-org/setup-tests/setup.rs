//! Real-process tests of `agentic-org setup`, `admin` and `service` (A2 B4). The binary runs
//! with a development application base in a temporary folder and the test adapters
//! (`ORG_DEV_SERVICE_DIR`: the service is a detached child, never launchd; disk encryption is a
//! fixed answer, never `fdesetup`). Development key files replace the Keychain. No test opens a
//! real organization root.
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_agentic-org");

struct Account {
    base: PathBuf,
    _temp: tempfile::TempDir,
}

impl Account {
    fn new() -> Self {
        // Short, so socket paths stay under 103 bytes.
        let temp = tempfile::Builder::new()
            .prefix("a2s")
            .tempdir_in("/private/tmp")
            .unwrap();
        let base = temp.path().canonicalize().unwrap();
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
        for dir in ["keys", "svc"] {
            std::fs::create_dir(base.join(dir)).unwrap();
            std::fs::set_permissions(base.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        Self { base, _temp: temp }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(BIN);
        command
            .args(args)
            .env_clear()
            .env("HOME", &self.base)
            .env("PATH", "/usr/bin:/bin")
            .env("ORG_DEV_ACCOUNT_BASE", self.base.join("app"))
            .env("ORG_OFFICE_DEV_KEY_DIR", self.base.join("keys"))
            .env("ORG_DEV_SERVICE_DIR", self.base.join("svc"))
            .env("ORG_DEV_DISK_ENCRYPTION", "on");
        command
    }

    /// Runs with stdout and stderr piped: not a terminal.
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// Runs with stdout on a pseudo-terminal, as at a person's terminal (stdin stays
    /// closed, so setup asks no question). Returns success, stdout and stderr.
    fn run_in_terminal(&self, args: &[&str]) -> (bool, String, String) {
        let (mut master, mut slave) = (-1, -1);
        // SAFETY: openpty only writes the two descriptors; no name, termios or size.
        let opened = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(opened, 0, "openpty: {}", std::io::Error::last_os_error());
        // SAFETY: both descriptors are new and owned only here.
        let (master, slave) =
            unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        for fd in [&master, &slave] {
            // SAFETY: plain flag change on a descriptor this function owns. Children of
            // parallel tests must not inherit the terminal, or its end would never be seen.
            unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
        }
        let mut command = self.command(args);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(slave))
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        drop(command);
        let (send, receive) = std::sync::mpsc::channel();
        let mut reader = std::fs::File::from(master);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(n @ 1..) = reader.read(&mut buffer) {
                if send.send(buffer[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut err = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut err)
            .unwrap();
        let status = child.wait().unwrap();
        let mut out = Vec::new();
        while let Ok(chunk) = receive.recv_timeout(Duration::from_millis(500)) {
            out.extend(chunk);
        }
        (
            status.success(),
            String::from_utf8_lossy(&out).replace("\r\n", "\n"),
            err,
        )
    }

    /// The frozen setup record of `name`, as JSON.
    fn record(&self, name: &str) -> serde_json::Value {
        let path = self.base.join(format!("app/setup/{name}/setup.v1.json"));
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn address(out: &str) -> u16 {
        out.lines()
            .find_map(|l| l.trim().strip_prefix("Address: http://127.0.0.1:"))
            .and_then(|p| p.trim_end_matches('/').parse().ok())
            .unwrap_or_else(|| panic!("no address in:\n{out}"))
    }

    fn service_pids(&self) -> Vec<(String, u32)> {
        let mut pids = Vec::new();
        for entry in std::fs::read_dir(self.base.join("svc")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "pid") {
                let id = path.file_stem().unwrap().to_string_lossy().into_owned();
                let pid = std::fs::read_to_string(&path)
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                pids.push((id, pid));
            }
        }
        pids.sort();
        pids
    }

    fn file(&self, id: &str, ext: &str) -> Vec<u8> {
        std::fs::read(self.base.join("svc").join(format!("{id}.{ext}"))).unwrap_or_default()
    }
}

impl Drop for Account {
    fn drop(&mut self) {
        for (_, pid) in self.service_pids() {
            // SAFETY: plain signal to a child this test's setup started.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
}

fn text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn link_token(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .find_map(|l| l.trim().split_once("/register#").map(|(_, t)| t.to_owned()))
}

#[test]
fn setup_starts_the_service_prints_one_link_and_a_second_run_changes_nothing() {
    let account = Account::new();
    let port = free_port().to_string();
    let (ok, out, err) = account.run_in_terminal(&["setup", "--port", &port]);
    assert!(ok, "stdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("Setting up instance \"default\"."), "{out}");
    assert!(
        out.contains(&format!("Address: http://127.0.0.1:{port}")),
        "{out}"
    );
    // ENC-3: the injected encryption answer, the modes as read from disk, the isolation mode.
    assert!(out.contains("Disk encryption (FileVault): on."), "{out}");
    assert!(out.contains("(folders 700, files 600)"), "{out}");
    assert!(out.contains("same-user mode"), "{out}");
    assert!(out.contains("Service running."), "{out}");
    assert!(out.contains("It has no administrator yet."), "{out}");
    let token = link_token(&out).expect("the first run in a terminal prints the link");
    assert!(token.len() >= 40, "{out}");
    assert_eq!(account.record("default")["initial_link_issued"], true);

    let pids = account.service_pids();
    assert_eq!(pids.len(), 1, "one service child");
    let (id, pid) = pids[0].clone();
    assert!(alive(pid));
    let unit = account.file(&id, "unit");
    let unit_text = String::from_utf8_lossy(&unit);
    assert!(
        unit_text.contains("a:serve\0a:--name\0a:default"),
        "{unit_text:?}"
    );
    // The token never reaches the service's definition or its log.
    assert!(!unit_text.contains(&token));
    assert!(!String::from_utf8_lossy(&account.file(&id, "log")).contains(&token));

    // SRV-2/SRV-6: the same instance, address and child; no new link; nothing redefined.
    let second = account.run(&["setup"]);
    let (out2, err2) = text(&second);
    assert!(second.status.success(), "stdout:\n{out2}\nstderr:\n{err2}");
    assert!(
        out2.contains(&format!("Address: http://127.0.0.1:{port}")),
        "{out2}"
    );
    assert!(link_token(&out2).is_none(), "{out2}");
    assert!(
        out2.contains("For a new link, run: agentic-org admin reset"),
        "{out2}"
    );
    assert_eq!(
        account.service_pids(),
        pids,
        "the running service is not restarted"
    );
    assert_eq!(account.file(&id, "unit"), unit);

    // A different port on a later run is refused, and the saved address is unchanged.
    let other = account.run(&["setup", "--port", &free_port().to_string()]);
    assert!(!other.status.success());
    assert!(text(&other).1.contains("does not change on a later run"));

    // admin reset never prints a link to a pipe or file: refused before the host is asked.
    let piped = account.run(&["admin", "reset"]);
    assert!(!piped.status.success());
    assert!(
        text(&piped).1.contains("Run this in a terminal."),
        "{:?}",
        text(&piped)
    );
    assert!(link_token(&text(&piped).0).is_none());

    // service status and stop go through the same adapter.
    let status = account.run(&["service", "status"]);
    assert!(
        text(&status).0.contains(&format!("running (pid {pid})")),
        "{:?}",
        text(&status)
    );
    let stop = account.run(&["service", "stop"]);
    assert!(stop.status.success(), "{:?}", text(&stop));
    assert!(!alive(pid));
    // With the service stopped, admin reset names the recovery.
    let (ok, reset_out, reset_err) = account.run_in_terminal(&["admin", "reset"]);
    assert!(!ok, "{reset_out}");
    assert!(
        reset_err.contains("agentic-org service start"),
        "{reset_err}"
    );

    // While the service is stopped, another program takes the saved port: setup names the
    // port and the way out instead of starting a service that cannot serve it.
    let busy = std::net::TcpListener::bind(("127.0.0.1", port.parse::<u16>().unwrap())).unwrap();
    let held = account.run(&["setup"]);
    let (held_out, held_err) = text(&held);
    assert!(!held.status.success(), "{held_out}");
    assert!(
        held_err.contains(&format!("port {port} on 127.0.0.1"))
            && held_err.contains("another program")
            && held_err.contains("then run agentic-org setup"),
        "{held_err}"
    );
    assert!(account.service_pids().iter().all(|(_, pid)| !alive(*pid)));
    drop(busy);
}

/// ADR-0022 / UX §4a.2: noninteractive setup asks for no link and prints none; the link
/// stays unissued, so a later run at a terminal still gets the initial one.
#[test]
fn piped_setup_prints_no_link_and_leaves_it_unissued() {
    let account = Account::new();
    let port = free_port().to_string();
    let piped = account.run(&["setup", "--port", &port]);
    let (out, err) = text(&piped);
    assert!(piped.status.success(), "stdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("It has no administrator yet."), "{out}");
    assert!(
        out.contains("For a new link, run: agentic-org admin reset"),
        "{out}"
    );
    assert!(link_token(&out).is_none(), "{out}");
    assert!(!out.contains("/register"), "{out}");
    assert_eq!(account.record("default")["initial_link_issued"], false);

    let (ok, again, err) = account.run_in_terminal(&["setup"]);
    assert!(ok, "stdout:\n{again}\nstderr:\n{err}");
    assert!(link_token(&again).is_some(), "{again}");
    assert_eq!(account.record("default")["initial_link_issued"], true);
}

/// A second named instance never takes the port the first one saved, even while the first
/// one's service is stopped and nothing listens there.
#[test]
fn a_named_instance_skips_ports_other_instances_saved() {
    let account = Account::new();
    let first = account.run(&["setup", "--name", "one"]);
    let (out, err) = text(&first);
    assert!(first.status.success(), "stdout:\n{out}\nstderr:\n{err}");
    let saved = Account::address(&out);
    let stop = account.run(&["service", "stop", "--name", "one"]);
    assert!(stop.status.success(), "{:?}", text(&stop));
    let second = account.run(&["setup", "--name", "two"]);
    let (out2, err2) = text(&second);
    assert!(second.status.success(), "stdout:\n{out2}\nstderr:\n{err2}");
    assert_ne!(Account::address(&out2), saved, "{out2}");
    // An explicit request for the saved port is refused by name.
    let third = account.run(&["setup", "--name", "three", "--port", &saved.to_string()]);
    assert!(!third.status.success());
    assert!(
        text(&third).1.contains("saved address of instance \"one\""),
        "{:?}",
        text(&third)
    );
}

/// A named instance without `--port` takes the first free port from 7421 (7420 is
/// `default`'s). The test holds 7421 when it can; if it cannot, another program (or a
/// parallel test) holds it. Either way the named instance skips it, so the test asserts
/// only a lower bound, never a fixed port. Residual: a holder that releases 7421 between
/// this test's bind attempt and setup's choice would let setup take 7421.
#[test]
fn a_named_instance_takes_the_first_free_port_from_7421() {
    let account = Account::new();
    let held = std::net::TcpListener::bind(("127.0.0.1", 7421)).ok();
    let output = account.run(&["setup", "--name", "lab"]);
    let (out, err) = text(&output);
    assert!(output.status.success(), "stdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("Setting up instance \"lab\"."), "{out}");
    let port = Account::address(&out);
    if held.is_some() {
        assert!(port >= 7422, "7421 is held by the test: {port}");
    } else {
        assert!(port >= 7421, "{port}");
    }
    assert_ne!(port, 7420);
    drop(held);
    // The unit serves the named instance, and a rerun keeps the frozen port even though
    // 7421 may be free now.
    let pids = account.service_pids();
    assert_eq!(pids.len(), 1);
    let unit = account.file(&pids[0].0, "unit");
    assert!(
        String::from_utf8_lossy(&unit).contains("a:serve\0a:--name\0a:lab"),
        "{unit:?}"
    );
    let again = account.run(&["setup", "--name", "lab"]);
    let (out2, err2) = text(&again);
    assert!(again.status.success(), "stdout:\n{out2}\nstderr:\n{err2}");
    assert!(
        out2.contains(&format!("Address: http://127.0.0.1:{port}")),
        "{out2}"
    );
    assert!(link_token(&out2).is_none(), "{out2}");
    assert_eq!(account.service_pids(), pids);
}

#[test]
fn an_occupied_port_is_refused_and_nothing_is_saved() {
    let account = Account::new();
    let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = busy.local_addr().unwrap().port().to_string();
    let output = account.run(&["setup", "--name", "lab", "--port", &port]);
    let (out, err) = text(&output);
    assert!(!output.status.success(), "{out}");
    assert!(err.contains("is in use") && err.contains("--port"), "{err}");
    assert!(!account.base.join("app/setup/lab/setup.v1.json").exists());
    assert!(account.service_pids().is_empty());
}

#[test]
fn satellite_flags_and_unknown_arguments_are_refused_before_any_change() {
    let account = Account::new();
    for args in [
        &["setup", "--satellite"][..],
        &["setup", "--hq=x"],
        &["setup", "--invitation", "x"],
        &["setup", "--bogus", "1"],
        &["setup", "--port", "0"],
        &["setup", "--name", "a", "--name", "b"],
        &["admin", "rotate"],
        &["service", "restart"],
    ] {
        let output = account.run(args);
        assert!(!output.status.success(), "{args:?}");
        if args[1].starts_with("--hq")
            || args[1] == "--satellite"
            || args[1].starts_with("--invitation")
        {
            assert!(
                text(&output).1.contains("setup creates HQ only"),
                "{args:?}: {:?}",
                text(&output)
            );
        }
    }
    assert!(!account.base.join("app").exists(), "nothing was created");
}

#[test]
fn a_release_style_base_must_be_absolute() {
    let account = Account::new();
    let output = Command::new(BIN)
        .args(["setup"])
        .env_clear()
        .env("ORG_DEV_ACCOUNT_BASE", "relative/base")
        .env("ORG_DEV_SERVICE_DIR", account.base.join("svc"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(text(&output).1.contains("absolute"), "{:?}", text(&output));
}

/// The registered-home fixture A1's binary `serve` tests use: `serve --name` starts the
/// fixture's instance from a temporary base and stops cleanly on SIGTERM.
#[test]
fn the_registered_account_fixture_serves_with_serve_name() {
    let account = Account::new();
    let app = account.base.join("app");
    std::fs::create_dir(&app).unwrap();
    std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o700)).unwrap();
    let registered =
        org_host::setup_flow::fixture::registered_account(&app, None, free_port()).unwrap();
    assert!(registered.root.starts_with(&app));
    let mut child = Command::new(BIN)
        .args(["serve", "--name", "default"])
        .env_clear()
        .env("HOME", &account.base)
        .env("ORG_DEV_ACCOUNT_BASE", &app)
        .env("ORG_OFFICE_DEV_KEY_DIR", account.base.join("keys"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !registered.host_socket().exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "host.sock never appeared"
        );
        assert!(child.try_wait().unwrap().is_none(), "serve exited early");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(registered.office_home.join("host-registry.db").exists());
    // SAFETY: plain signal to our child.
    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    assert!(child.wait().unwrap().success());
}
