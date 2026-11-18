//! Real-process tests of `agentic-org setup`, `admin` and `service` (A2 B4). The binary runs
//! with a development application base in a temporary folder and the test adapters
//! (`ORG_DEV_SERVICE_DIR`: the service is a detached child, never launchd; disk encryption is a
//! fixed answer, never `fdesetup`). Development key files replace the Keychain. No test opens a
//! real organization root.
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

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

    fn run(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .env_clear()
            .env("HOME", &self.base)
            .env("PATH", "/usr/bin:/bin")
            .env("ORG_DEV_ACCOUNT_BASE", self.base.join("app"))
            .env("ORG_OFFICE_DEV_KEY_DIR", self.base.join("keys"))
            .env("ORG_DEV_SERVICE_DIR", self.base.join("svc"))
            .env("ORG_DEV_DISK_ENCRYPTION", "on")
            .output()
            .unwrap()
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
    let first = account.run(&["setup", "--port", &port]);
    let (out, err) = text(&first);
    assert!(first.status.success(), "stdout:\n{out}\nstderr:\n{err}");
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
    let token = link_token(&out).expect("the first run prints the link");
    assert!(token.len() >= 40, "{out}");

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
    let reset = account.run(&["admin", "reset"]);
    assert!(!reset.status.success());
    assert!(
        text(&reset).1.contains("agentic-org service start"),
        "{:?}",
        text(&reset)
    );
}

/// A named instance without `--port` takes the first free port from 7421 (7420 is
/// `default`'s). The test holds 7421 when it can, so the named instance must skip it.
#[test]
fn a_named_instance_takes_the_first_free_port_from_7421() {
    let account = Account::new();
    let held = std::net::TcpListener::bind(("127.0.0.1", 7421)).ok();
    let output = account.run(&["setup", "--name", "lab"]);
    let (out, err) = text(&output);
    assert!(output.status.success(), "stdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("Setting up instance \"lab\"."), "{out}");
    let port: u16 = out
        .lines()
        .find_map(|l| l.trim().strip_prefix("Address: http://127.0.0.1:"))
        .and_then(|p| p.trim_end_matches('/').parse().ok())
        .unwrap_or_else(|| panic!("no address in:\n{out}"));
    assert!(
        port >= 7422,
        "7421 is held by the test or another program: {port}"
    );
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
