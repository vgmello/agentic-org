//! Real-process tests of `agentic-org setup`, `admin` and `service` (A2 B4). The binary runs
//! with a development application base in a temporary folder and the test adapters
//! (`ORG_DEV_SERVICE_DIR`: the service is a detached child, never launchd; disk encryption is a
//! fixed answer, never `fdesetup`). Development key files replace the Keychain. No test opens a
//! real organization root.
//!
//! `systemd` (below) runs the same commands through the Linux service adapter itself, with
//! stand-ins for `systemctl` and `loginctl` (`ORG_DEV_SERVICE_PROGRAMS`).
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_agentic-org");

/// On macOS a new socket or terminal descriptor gets close-on-exec in a second step after
/// it is created, so a child spawned by a parallel test in between inherits it. A probe
/// listener inherited by a long-lived service child keeps its port held, and the setup that
/// asked for that port then refuses it as in use. Every descriptor this process creates
/// without atomic close-on-exec, and every spawn, happens under this lock.
static DESCRIPTORS: Mutex<()> = Mutex::new(());

fn descriptors() -> MutexGuard<'static, ()> {
    DESCRIPTORS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Spawns under the descriptor lock; the caller waits outside it.
fn spawn(command: &mut Command) -> Child {
    let _held = descriptors();
    command.spawn().unwrap()
}

/// `Command::output`, spawned under the descriptor lock. Unlike `output`, it always sets
/// stdin to null and pipes stdout and stderr, overriding any stdio the caller set.
fn output(command: &mut Command) -> Output {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spawn(command).wait_with_output().unwrap()
}

/// A loopback listener, created under the descriptor lock so no child inherits it.
fn listen(port: u16) -> std::io::Result<std::net::TcpListener> {
    let _held = descriptors();
    std::net::TcpListener::bind(("127.0.0.1", port))
}

/// Where this account's named instances start their port search. Production starts at
/// 7421, which real instances and other test runs on the machine also use; the
/// test-support build reads `ORG_DEV_NAMED_PORT_BASE` instead. The base is derived from
/// this process's id, and each account in the process takes its own ten ports, so no two
/// tests and (except for an id collision modulo 150) no two test processes search the
/// same range.
fn named_port_base() -> u16 {
    static NEXT: AtomicU16 = AtomicU16::new(0);
    let process = u16::try_from(std::process::id() % 150).unwrap();
    20_000 + process * 100 + (NEXT.fetch_add(1, Ordering::SeqCst) % 10) * 10
}

struct Account {
    base: PathBuf,
    /// This account's `agentic-org`, with the office and web programs beside it and a UI
    /// folder for the web process (`installed`): setup is ready only once the instance's
    /// browser service runs (X5 review, fourth round, P2-1).
    bin: PathBuf,
    /// Set: the system's own service adapter with stand-in programs from this folder,
    /// instead of the detached-child adapter.
    programs: Option<PathBuf>,
    /// The first port a named instance of this account tries without `--port`.
    port_base: u16,
    _temp: tempfile::TempDir,
}

impl Account {
    fn new() -> Self {
        // Short, so socket paths stay under the system's limit, and on a disk: an instance
        // root is refused on a memory filesystem.
        let temp = tempfile::Builder::new()
            .prefix("a2s")
            .tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap())
            .unwrap();
        let base = temp.path().canonicalize().unwrap();
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
        for dir in ["keys", "svc"] {
            std::fs::create_dir(base.join(dir)).unwrap();
            std::fs::set_permissions(base.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        Self {
            bin: installed(&base),
            base,
            programs: None,
            port_base: named_port_base(),
            _temp: temp,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.bin);
        command
            .args(args)
            .env_clear()
            .env("HOME", &self.base)
            .env("PATH", "/usr/bin:/bin")
            .env("ORG_DEV_ACCOUNT_BASE", self.base.join("app"))
            .env("ORG_OFFICE_DEV_KEY_DIR", self.base.join("keys"))
            .env("ORG_DEV_DISK_ENCRYPTION", "on")
            .env("ORG_DEV_NAMED_PORT_BASE", self.port_base.to_string());
        match &self.programs {
            None => command.env("ORG_DEV_SERVICE_DIR", self.base.join("svc")),
            Some(programs) => command
                .env("ORG_DEV_SERVICE_PROGRAMS", programs)
                .env("XDG_RUNTIME_DIR", self.base.join("run")),
        };
        command
    }

    /// Runs with stdout and stderr piped: not a terminal.
    fn run(&self, args: &[&str]) -> Output {
        output(&mut self.command(args))
    }

    /// Runs with stdout on a pseudo-terminal, as at a person's terminal (stdin stays
    /// closed, so setup asks no question). Returns success, stdout and stderr.
    fn run_in_terminal(&self, args: &[&str]) -> (bool, String, String) {
        // Held from openpty until the spawn, so no other child inherits the terminal.
        let held = descriptors();
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
        drop(held);
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
        let stand_in = self.programs.as_ref().and_then(|programs| {
            let pid = std::fs::read_to_string(programs.join("pid")).ok()?;
            Some((String::new(), pid.trim().parse().ok()?))
        });
        for (_, pid) in self.service_pids().into_iter().chain(stand_in) {
            // SAFETY: plain signal to a child this test's setup started.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
}

/// The built `agentic-org`, `agentic-org-office` and `agentic-org-web` in `<base>/bin`,
/// with a UI folder beside them as an installation has one, so that the service's web
/// process starts. Returns the `agentic-org` there.
fn installed(base: &std::path::Path) -> PathBuf {
    let built = std::path::Path::new(BIN).parent().unwrap();
    let bin = base.join("bin");
    std::fs::create_dir_all(bin.join("ui")).unwrap();
    // The web process refuses an entry without its inline bootstrap, as for a real build.
    std::fs::write(
        bin.join("ui/index.html"),
        "<html><body>setup test<script>/* bootstrap */</script></body></html>",
    )
    .unwrap();
    for name in ["agentic-org", "agentic-org-office", "agentic-org-web"] {
        let from = built.join(name);
        assert!(
            from.is_file(),
            "{} is not built: build agentic-org-office and agentic-org-web first",
            from.display()
        );
        // A copy, not a link: a program is known by its one path (the CLI accepts its
        // own executable as the development host).
        std::fs::copy(&from, bin.join(name)).unwrap();
    }
    for dir in [bin.clone(), bin.join("ui")] {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    bin.join("agentic-org")
}

fn text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn free_port() -> u16 {
    listen(0).unwrap().local_addr().unwrap().port()
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
    let encryption = org_platform::disk::wording().name;
    assert!(
        out.contains(&format!("Disk encryption ({encryption}): on.")),
        "{out}"
    );
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
    // With the service stopped, admin reset names the recovery. Where the system names a
    // peer by its user alone (Linux), anyone but root is refused before the service is
    // asked anything, in the specification's words (SIGN-33).
    let (ok, reset_out, reset_err) = account.run_in_terminal(&["admin", "reset"]);
    assert!(!ok, "{reset_out}");
    assert!(
        reset_err.contains("agentic-org service start")
            || reset_err.contains(
                "Resetting the administrator's password on Linux needs root. Run it with \
                 sudo: sudo agentic-org admin reset"
            ),
        "{reset_err}"
    );
    assert!(link_token(&reset_out).is_none(), "{reset_out}");

    // While the service is stopped, another program takes the saved port: setup names the
    // port and the way out instead of starting a service that cannot serve it.
    let busy = listen(port.parse().unwrap()).unwrap();
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

/// A named instance without `--port` takes the first free port from the search base
/// (7421 in production, asserted by `setup_flow::tests`; this account's own base here, so
/// nothing else on the machine competes for it). The test holds the base, so setup must
/// skip it and take the next port. If the base cannot be held, another program has it and
/// only the lower bound is asserted.
#[test]
fn a_named_instance_takes_the_first_free_port_from_the_base() {
    let account = Account::new();
    let base = account.port_base;
    let held = listen(base).ok();
    let output = account.run(&["setup", "--name", "lab"]);
    let (out, err) = text(&output);
    assert!(output.status.success(), "stdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("Setting up instance \"lab\"."), "{out}");
    let port = Account::address(&out);
    if held.is_some() {
        assert_eq!(port, base + 1, "{base} is held by the test");
    } else {
        assert!(port > base, "{base} is held by another program: {port}");
    }
    assert_ne!(port, 7420);
    drop(held);
    // The unit serves the named instance, and a rerun keeps the frozen port even though
    // the base is free now.
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
    let busy = listen(0).unwrap();
    let port = busy.local_addr().unwrap().port().to_string();
    let output = account.run(&["setup", "--name", "lab", "--port", &port]);
    let (out, err) = text(&output);
    assert!(!output.status.success(), "{out}");
    assert!(err.contains("is in use") && err.contains("--port"), "{err}");
    assert!(!account.base.join("app/setup/lab/setup.v1.json").exists());
    assert!(account.service_pids().is_empty());
}

/// X5 review V-a and P3-1: a development build's service is given its key folder by its
/// absolute path; a relative one, which would name another folder where the service runs,
/// or none at all where its host keeps its keys there, is refused before anything is saved.
/// A missing folder is made, private, before the service is registered.
#[test]
fn the_development_key_folder_is_absolute_and_made_before_the_service_starts() {
    let account = Account::new();
    let port = free_port().to_string();
    let mut relative = account.command(&["setup", "--port", &port]);
    relative.env("ORG_OFFICE_DEV_KEY_DIR", "keys");
    let refused = output(&mut relative);
    let (out, err) = text(&refused);
    assert!(!refused.status.success(), "{out}");
    assert!(
        err.contains("ORG_OFFICE_DEV_KEY_DIR (keys) is not an absolute path")
            && err.contains("nothing was changed"),
        "{err}"
    );
    assert!(!account
        .base
        .join("app/setup/default/setup.v1.json")
        .exists());
    assert!(account.service_pids().is_empty());
    // A folder that does not exist yet is made before the service is registered, which
    // then starts with it.
    let fresh = account.base.join("fresh-keys");
    let mut absent = account.command(&["setup", "--port", &port]);
    absent.env("ORG_OFFICE_DEV_KEY_DIR", &fresh);
    let made = output(&mut absent);
    let (out, err) = text(&made);
    assert!(made.status.success(), "stdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("Service running."), "{out}");
    assert_eq!(
        std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o7777,
        0o700
    );
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
    let output = output(
        Command::new(BIN)
            .args(["setup"])
            .env_clear()
            .env("ORG_DEV_ACCOUNT_BASE", "relative/base")
            .env("ORG_DEV_SERVICE_DIR", account.base.join("svc")),
    );
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
    let mut child = spawn(
        Command::new(BIN)
            .args(["serve", "--name", "default"])
            .env_clear()
            .env("HOME", &account.base)
            .env("ORG_DEV_ACCOUNT_BASE", &app)
            .env("ORG_OFFICE_DEV_KEY_DIR", account.base.join("keys"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null()),
    );
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

mod configured;
mod systemd;
