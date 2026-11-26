//! Real-process pure-CLI E2E test (`TASK-113`):
//! proves that an operator can create an organization, invite a remote satellite office,
//! join the satellite, approve enrollment, verify live mTLS replication sync, cancel an
//! invitation, and remove a satellite using only `agentic-org` CLI subcommands (no browser,
//! no passwords in local authless HTTP mode).

#![cfg(debug_assertions)]

mod support;

use std::fs::{self, File};
use std::io::Write;
use std::net::TcpListener;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use org_host::setup_flow::fixture::{
    registered_account, registered_account_with_federation, RegisteredAccount,
};
use org_host::setup_flow::FederationConfig;
use serde_json::Value;
use support::{may_run_the_service, sibling_program};

const WAIT: Duration = Duration::from_secs(60);

struct HostProcess(Child);

impl Drop for HostProcess {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

struct Node {
    _dir: tempfile::TempDir,
    root: PathBuf,
    name: &'static str,
    account: RegisteredAccount,
    worker: PathBuf,
    _host: Option<HostProcess>,
}

impl Node {
    fn new(name: &'static str, federation: Option<FederationConfig>) -> Self {
        let dir = support::short_tempdir("cf");
        let root = dir.path().canonicalize().unwrap();
        for sub in ["a", "k", "u", "b"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(sub))
                .unwrap();
        }
        let web_port = free_port();
        let account = match federation {
            Some(fed) => {
                registered_account_with_federation(&root.join("a"), Some(name), web_port, Some(fed))
                    .unwrap()
            }
            None => registered_account(&root.join("a"), Some(name), web_port).unwrap(),
        };
        let bin = root.join("b");
        for program in ["agentic-org-office", "agentic-org-web"] {
            fs::copy(sibling_program(program), bin.join(program)).unwrap();
            fs::set_permissions(bin.join(program), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::DirBuilder::new()
            .mode(0o700)
            .create(bin.join("ui"))
            .unwrap();
        fs::write(
            bin.join("ui/index.html"),
            b"<html><body>CLI E2E<script>/* bootstrap */</script></body></html>",
        )
        .unwrap();
        Self {
            _dir: dir,
            root,
            name,
            account,
            worker: bin.join("agentic-org-office"),
            _host: None,
        }
    }

    fn start(&mut self) {
        let stderr = File::create(self.root.join("host.err")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
            .args(["serve", "--name", self.name, "--server-bin"])
            .arg(&self.worker)
            .env_clear()
            .env("HOME", self.root.join("u"))
            .env("ORG_DEV_ACCOUNT_BASE", self.root.join("a"))
            .env("ORG_OFFICE_DEV_KEY_DIR", self.root.join("k"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap();
        self._host = Some(HostProcess(child));
        let deadline = Instant::now() + WAIT;
        while !self.account.host_socket().exists() {
            assert!(
                Instant::now() < deadline,
                "host `{}` did not create host.sock:\n{}",
                self.name,
                self.host_log()
            );
            thread::sleep(Duration::from_millis(25));
        }
        // Wait until `org list` answers cleanly (host registered and admin installed).
        loop {
            if self.try_cli(&["org", "list", "--json"], None).is_ok() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "host `{}` did not become ready for CLI:\n{}",
                self.name,
                self.host_log()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn host_log(&self) -> String {
        let mut out = fs::read_to_string(self.root.join("host.err")).unwrap_or_default();
        let logs_dir = self.account.office_home.join("logs");
        if let Ok(entries) = fs::read_dir(&logs_dir) {
            for entry in entries.flatten() {
                if let Ok(text) = fs::read_to_string(entry.path()) {
                    out.push_str(&format!(
                        "\n--- worker log {} ---\n{text}",
                        entry.path().display()
                    ));
                }
            }
        }
        out
    }

    fn try_cli(&self, args: &[&str], stdin_data: Option<&str>) -> Result<Value, String> {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_agentic-org"));
        cmd.args(args)
            .args(["--name", self.name])
            .env_clear()
            .env("HOME", self.root.join("u"))
            .env("ORG_DEV_ACCOUNT_BASE", self.root.join("a"))
            .env("ORG_OFFICE_DEV_KEY_DIR", self.root.join("k"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if stdin_data.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        if let Some(data) = stdin_data {
            let mut pipe = child.stdin.take().unwrap();
            pipe.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
            drop(pipe);
        }
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "exit {:?}\nstdout: {}\nstderr: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(&stdout).map_err(|e| format!("invalid JSON ({e}): {stdout}"))
    }

    fn cli(&self, args: &[&str], stdin_data: Option<&str>) -> Value {
        self.try_cli(args, stdin_data).unwrap_or_else(|err| {
            panic!(
                "CLI `{args:?}` on `{}` failed: {err}\nhost log:\n{}",
                self.name,
                self.host_log()
            )
        })
    }
}

#[test]
fn pure_cli_org_create_and_remote_satellite_enrollment_end_to_end() {
    if !may_run_the_service("pure_cli_org_create_and_remote_satellite_enrollment_end_to_end") {
        return;
    }

    // 1. Start HQ with persisted FederationConfig and a clean Satellite host (both local authless).
    let fed_port = free_port();
    let fed_addr = format!("127.0.0.1:{fed_port}");
    let fed_cfg = FederationConfig::parse(&fed_addr, vec!["localhost".into()]).unwrap();
    let mut hq = Node::new("hq", Some(fed_cfg));
    let mut sat = Node::new("sat", None);
    hq.start();
    sat.start();

    // 2. Create organization purely from CLI (no browser, no password).
    let created = hq.cli(
        &[
            "org",
            "create",
            "Acme CLI",
            "--slug",
            "acme-cli",
            "--repo",
            "https://example.test/acme-cli.git",
            "--branch",
            "main",
            "--json",
        ],
        None,
    );
    let org_id = created["id"].as_str().expect("org id").to_owned();
    assert_eq!(created["slug"], "acme-cli");
    let hq_office_id = created["offices"][0]["id"]
        .as_str()
        .expect("hq office id")
        .to_owned();

    // 3. Verify `org list` and wait until HQ office is active via `office list`.
    let listed_orgs = hq.cli(&["org", "list", "--json"], None);
    assert_eq!(listed_orgs["organizations"].as_array().unwrap().len(), 1);
    assert_eq!(listed_orgs["organizations"][0]["id"], org_id);

    let deadline = Instant::now() + WAIT;
    loop {
        let offices = hq.cli(&["office", "list", "--org", &org_id, "--json"], None);
        if offices["offices"][0]["status"] == "active" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "HQ office never became active: {offices}\n{}",
            hq.host_log()
        );
        thread::sleep(Duration::from_millis(50));
    }

    // 4. Create a one-time satellite invitation on HQ (`office invite`).
    let invite = hq.cli(
        &[
            "office",
            "invite",
            "Berlin",
            "--address",
            &fed_addr,
            "--org",
            &org_id,
            "--json",
        ],
        None,
    );
    let invitation_id = invite["invitation"]["invitation_id"]
        .as_str()
        .expect("invitation_id")
        .to_owned();
    let sat_office_id = invite["invitation"]["office_id"]
        .as_str()
        .expect("satellite office_id")
        .to_owned();
    let token = invite["token"].as_str().expect("token").to_owned();
    assert!(token.starts_with("agorg-inv1."), "unexpected token shape");

    // 5. Join the satellite host purely from CLI (`office join --invitation-stdin --yes`).
    let joined = sat.cli(
        &["office", "join", "--invitation-stdin", "--yes", "--json"],
        Some(&token),
    );
    assert_eq!(joined["office_id"], sat_office_id);
    assert_eq!(joined["org_id"], org_id);
    assert_eq!(joined["state"], "join_pending");
    let check_code = joined["check_code"]
        .as_str()
        .expect("check_code")
        .to_owned();

    // 6. Inspect invitations on HQ (`office invitations`): status is pending_approval and HQ
    //    deliberately does NOT disclose the check_code (must come out-of-band from the satellite).
    let inv_list = hq.cli(&["office", "invitations", "--org", &org_id, "--json"], None);
    let listed_inv = inv_list["invitations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["invitation_id"] == invitation_id)
        .expect("invitation listed on HQ");
    assert_eq!(listed_inv["status"], "awaiting_approval");
    assert!(
        listed_inv.get("check_code").is_none() || listed_inv["check_code"].is_null(),
        "HQ must not leak out-of-band check_code in invitation list: {listed_inv}"
    );

    // 7. Approve enrollment on HQ (`office approve`).
    let approved = hq.cli(
        &[
            "office",
            "approve",
            &invitation_id,
            "--code",
            &check_code,
            "--org",
            &org_id,
            "--json",
        ],
        None,
    );
    assert_eq!(approved["office_id"], sat_office_id);
    assert!(!approved["cert_serial_hex"].as_str().unwrap().is_empty());

    // 8. Wait until the satellite becomes active AND completes initial snapshot sync over mTLS.
    let deadline = Instant::now() + WAIT;
    loop {
        let last_sat_err = match sat.try_cli(&["office", "sync-status", "--json"], None) {
            Ok(status) => {
                if status["local_office"] == sat_office_id
                    && status["hq_office"] == hq_office_id
                    && status["initial_snapshot_required"] == false
                {
                    break;
                }
                format!("unexpected sync-status: {status}")
            }
            Err(err) => err,
        };
        assert!(
            Instant::now() < deadline,
            "satellite never completed mTLS snapshot sync.\nLast sat CLI result: {last_sat_err}\nHQ log:\n{}\nSat log:\n{}",
            hq.host_log(),
            sat.host_log()
        );
        thread::sleep(Duration::from_millis(100));
    }

    let sat_offices = sat.cli(&["office", "list", "--json"], None);
    let sat_entry = sat_offices["offices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == sat_office_id)
        .expect("satellite host admin sees joined satellite office");
    assert_eq!(sat_entry["organization_id"], org_id);
    assert_eq!(sat_entry["role"], "satellite");
    assert_eq!(sat_entry["status"], "active");

    // 9. Verify HQ `office sync-status`, invitation `cancel`, and satellite `remove`.
    let hq_sync = hq.cli(&["office", "sync-status", "--org", &org_id, "--json"], None);
    assert_eq!(hq_sync["local_office"], hq_office_id);
    assert_eq!(hq_sync["hq_office"], hq_office_id);

    let extra_invite = hq.cli(
        &[
            "office",
            "invite",
            "Temp",
            "--address",
            &fed_addr,
            "--org",
            &org_id,
            "--json",
        ],
        None,
    );
    let extra_inv_id = extra_invite["invitation"]["invitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let cancelled = hq.cli(
        &[
            "office",
            "cancel",
            &extra_inv_id,
            "--org",
            &org_id,
            "--yes",
            "--json",
        ],
        None,
    );
    assert_eq!(cancelled["invitation"]["status"], "cancelled");

    let decline_invite = hq.cli(
        &[
            "office",
            "invite",
            "Declined-Site",
            "--address",
            &fed_addr,
            "--org",
            &org_id,
            "--json",
        ],
        None,
    );
    let decline_inv_id = decline_invite["invitation"]["invitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let declined = hq.cli(
        &[
            "office",
            "decline",
            &decline_inv_id,
            "--org",
            &org_id,
            "--yes",
            "--json",
        ],
        None,
    );
    assert_eq!(declined["invitation"]["status"], "declined");

    let removed = hq.cli(
        &[
            "office",
            "remove",
            &sat_office_id,
            "--org",
            &org_id,
            "--yes",
            "--json",
        ],
        None,
    );
    assert_eq!(removed["office_id"], sat_office_id);
    assert!(
        !removed["revoked_serials"].as_array().unwrap().is_empty(),
        "removing satellite must revoke its issued certificate"
    );
}

#[test]
fn setup_cli_persists_and_freezes_federation_config() {
    if !may_run_the_service("setup_cli_persists_and_freezes_federation_config") {
        return;
    }
    let dir = support::short_tempdir("cs");
    let root = dir.path().canonicalize().unwrap();
    for sub in ["a", "u", "d"] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join(sub))
            .unwrap();
    }
    let web_port = free_port();
    let fed_port = free_port();
    let cfg_path = root.join("setup.json");
    let cfg = serde_json::json!({
        "version": 1,
        "service": "foreground",
        "root": root.join("d").to_string_lossy(),
        "listen": {
            "address": "127.0.0.1",
            "port": web_port,
            "scheme": "http_loopback",
            "origin": format!("http://127.0.0.1:{web_port}"),
            "hsts": "off"
        },
        "federation": {
            "bind": format!("127.0.0.1:{fed_port}"),
            "advertise": ["localhost", "2001:db8::1"]
        }
    });
    fs::write(&cfg_path, serde_json::to_vec_pretty(&cfg).unwrap()).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
        .args(["setup", "--non-interactive", "--config"])
        .arg(&cfg_path)
        .args(["--name", "hq-cfg"])
        .env_clear()
        .env("HOME", root.join("u"))
        .env("ORG_DEV_ACCOUNT_BASE", root.join("a"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "setup failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(&format!(
            "Federation: 127.0.0.1:{fed_port} (advertise: localhost, 2001:db8::1)"
        )),
        "stdout missing federation summary:\n{stdout}"
    );

    // Re-running with a different federation bind must fail closed and preserve the frozen record.
    let changed_path = root.join("setup-changed.json");
    let changed = serde_json::json!({
        "version": 1,
        "service": "foreground",
        "root": root.join("d").to_string_lossy(),
        "listen": {
            "address": "127.0.0.1",
            "port": web_port,
            "scheme": "http_loopback",
            "origin": format!("http://127.0.0.1:{web_port}"),
            "hsts": "off"
        },
        "federation": {
            "bind": format!("127.0.0.1:{}", free_port()),
            "advertise": ["localhost"]
        }
    });
    fs::write(&changed_path, serde_json::to_vec_pretty(&changed).unwrap()).unwrap();
    let mismatch = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
        .args(["setup", "--non-interactive", "--config"])
        .arg(&changed_path)
        .args(["--name", "hq-cfg"])
        .env_clear()
        .env("HOME", root.join("u"))
        .env("ORG_DEV_ACCOUNT_BASE", root.join("a"))
        .output()
        .unwrap();
    assert!(!mismatch.status.success());
    let mismatch_err = String::from_utf8_lossy(&mismatch.stderr);
    assert!(
        mismatch_err.contains("federation"),
        "expected federation mismatch error, got:\n{mismatch_err}"
    );
}
