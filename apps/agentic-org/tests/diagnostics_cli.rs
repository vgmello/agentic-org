//! End-to-end process-level tests for `TASK-60` (`logs`, `doctor`, `support`, `uninstall`).
//!
//! Every test runs the real `agentic-org` binary inside a hermetic temporary sandbox with
//! `ORG_DEV_ACCOUNT_BASE` and never touches a real user's home or workspace.

#![cfg(debug_assertions)]

use std::fs::{self, OpenOptions};
use std::os::unix::fs::{symlink, DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use org_host::setup_flow::fixture::{registered_account, seed_administrator, RegisteredAccount};
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_agentic-org");

struct DiagSandbox {
    _dir: tempfile::TempDir,
    root: PathBuf,
    base: PathBuf,
    keys: PathBuf,
    svc: PathBuf,
}

impl DiagSandbox {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("dg")
            .tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap())
            .unwrap();
        let root = dir.path().canonicalize().unwrap();
        let base = root.join("base");
        let keys = root.join("keys");
        let svc = root.join("svc");
        for sub in ["home", "cache", "config", "work"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(sub))
                .unwrap();
        }
        fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&keys).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&svc).unwrap();
        Self {
            _dir: dir,
            root,
            base,
            keys,
            svc,
        }
    }

    fn setup_instance(&self, name: Option<&str>) -> RegisteredAccount {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        registered_account(&self.base, name, port).unwrap()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(BIN);
        cmd.args(args)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("ORG_DEV_ACCOUNT_BASE", &self.base)
            .env("ORG_OFFICE_DEV_KEY_DIR", &self.keys)
            .env("ORG_DEV_SERVICE_DIR", &self.svc)
            .env("AGENTIC_ORG_CACHE_DIR", self.root.join("cache"))
            .env("AGENTIC_ORG_CONFIG_DIR", self.root.join("config"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .current_dir(self.root.join("work"))
            .stdin(Stdio::null());
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn logs_redacts_secrets_respects_bounds_and_refuses_symlinks() {
    let sb = DiagSandbox::new();
    let acct = sb.setup_instance(Some("lab"));
    let logs_dir = acct.office_home.join("logs");

    let sess_secret = ["sess_secret", "_xyz123456"].join("");
    let ant_secret = ["sk-ant-", "api03-livekey1234567890"].join("");
    let ghp_secret = ["ghp_", "0123456789abcdefABCDEF012345678901"].join("");
    let pem_begin = format!("{}BEGIN PRIVATE KEY{}", "-----", "-----");
    let pem_end = format!("{}END PRIVATE KEY{}", "-----", "-----");
    let log_body = [
        "2026-11-27 info starting host".to_owned(),
        "Authorization: Bearer tok_bearer_secret_987654".to_owned(),
        format!("failed with {ant_secret}"),
        format!("token {ghp_secret}"),
        format!("AGENT_OS_SESSION_TOKEN={sess_secret}"),
        "invite https://example.test/register#RegTokSecret1234567890".to_owned(),
        "recovery code: QWER-ASDF-ZXCV-TYUI".to_owned(),
        pem_begin,
        "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC7".to_owned(),
        pem_end,
    ]
    .join("\n");
    write_file(&logs_dir.join("host.log"), &log_body);

    // Plant a symlink log pointing to a secret file outside logs/ — MUST be refused/skipped.
    let secret_outside = sb.root.join("outside.secret");
    fs::write(&secret_outside, "OUTSIDE_SYMLINK_SECRET_MARKER").unwrap();
    symlink(&secret_outside, logs_dir.join("evil.log")).unwrap();

    // JSON mode checks redaction, counts, and sources.
    let out = sb.run(&["logs", "--instance", "lab", "--lines", "50", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let serialized = serde_json::to_string(&json).unwrap();
    for secret in [
        "tok_bearer_secret_987654",
        ant_secret.as_str(),
        ghp_secret.as_str(),
        sess_secret.as_str(),
        "RegTokSecret1234567890",
        "QWER-ASDF-ZXCV-TYUI",
        "MIIEvgIBADANBgkqhkiG9w0",
        "OUTSIDE_SYMLINK_SECRET_MARKER",
    ] {
        assert!(
            !serialized.contains(secret),
            "leaked `{secret}` in {serialized}"
        );
    }
    assert!(json["redacted_count"].as_u64().unwrap() >= 7, "{json}");
    assert_eq!(json["sources"], serde_json::json!(["host.log"]));
    assert_eq!(json["truncated"], false);

    // Bounded --lines truncates cleanly.
    let bounded = sb.run(&["logs", "--instance", "lab", "--lines", "2", "--json"]);
    assert_eq!(bounded.status.code(), Some(0));
    let bjson: Value = serde_json::from_slice(&bounded.stdout).unwrap();
    assert_eq!(bjson["lines"].as_array().unwrap().len(), 2);
    assert_eq!(bjson["truncated"], true);

    // Oversized log (> 512 KiB) reads bounded tail safely and redacts secrets near the end.
    let mut big = "2026-11-27 info filler line padding for tail bound check\n".repeat(12_000);
    big.push_str("Authorization: Bearer tail_secret_999888777\n");
    write_file(&logs_dir.join("host.log"), &big);
    let huge = sb.run(&["logs", "--instance", "lab", "--lines", "5", "--json"]);
    assert_eq!(huge.status.code(), Some(0), "{}", text(&huge.stderr));
    let hjson: Value = serde_json::from_slice(&huge.stdout).unwrap();
    assert_eq!(hjson["truncated"], true);
    let hstr = serde_json::to_string(&hjson).unwrap();
    assert!(!hstr.contains("tail_secret_999888777"), "{hstr}");
    assert!(hjson["redacted_count"].as_u64().unwrap() >= 1);

    // Invalid --lines > 2000 is rejected with exit 2.
    let bad_lines = sb.run(&["logs", "--instance", "lab", "--lines", "9999"]);
    assert_eq!(bad_lines.status.code(), Some(2));

    // Quiet mode emits lines only without stderr notes.
    let quiet = sb.run(&["logs", "--instance", "lab", "--lines", "2", "-q"]);
    assert_eq!(quiet.status.code(), Some(0));
    assert!(quiet.stderr.is_empty());
    assert_eq!(text(&quiet.stdout).lines().count(), 2);

    // P0 #3: Multibyte --office filter never panics (rejected cleanly with exit 2).
    let mb = sb.run(&["logs", "--instance", "lab", "--office", "éaaaaaaaaaaa"]);
    assert_eq!(mb.status.code(), Some(2), "{}", text(&mb.stderr));
}

#[test]
fn doctor_checks_registry_marker_permissions_and_org_state_integrity() {
    let sb = DiagSandbox::new();
    let acct = sb.setup_instance(Some("doclab"));
    seed_administrator(&sb.base, Some("doclab"), &sb.keys).unwrap();

    // Healthy seeded instance passes doctor with exit 0.
    let healthy = sb.run(&["doctor", "--instance", "doclab", "--json"]);
    assert_eq!(
        healthy.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        text(&healthy.stdout),
        text(&healthy.stderr)
    );
    let hjson: Value = serde_json::from_slice(&healthy.stdout).unwrap();
    assert_eq!(hjson["ok"], true, "{hjson}");

    // Delete host-registry.db while leaving host-registry.created -> MUST FAIL with exit 1!
    fs::remove_file(acct.office_home.join("host-registry.db")).unwrap();
    let missing_db = sb.run(&["doctor", "--instance", "doclab", "--json"]);
    assert_eq!(
        missing_db.status.code(),
        Some(1),
        "{}",
        text(&missing_db.stdout)
    );
    let mjson: Value = serde_json::from_slice(&missing_db.stdout).unwrap();
    assert_eq!(mjson["ok"], false);
    assert!(mjson["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == "instance.registry_consistency" && c["status"] == "fail"));

    // Loose directory permission (0755 instead of 0700) fails instance.permissions check.
    fs::set_permissions(&acct.office_home, fs::Permissions::from_mode(0o755)).unwrap();
    let loose = sb.run(&["doctor", "--instance", "doclab", "--json"]);
    assert_eq!(loose.status.code(), Some(1));
    let ljson: Value = serde_json::from_slice(&loose.stdout).unwrap();
    assert!(ljson["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == "instance.permissions" && c["status"] == "fail"));

    // Standalone --org-dir check: valid passes (exit 0), corrupted fails (exit 1).
    let org_dir = sb.root.join("offline-org/durable/state");
    write_file(
        &org_dir.join("projects/p1/project.yaml"),
        "id: p1\nnumber: 1\n",
    );
    write_file(
        &org_dir.join("tasks/t1/task.md"),
        "---\nid: t1\nkey: T-A1\nproject_id: p1\n---\n# T1\n",
    );
    write_file(
        &org_dir.join("tasks/t2/task.md"),
        "---\nid: t2\nkey: T-A2\nproject_id: p1\ndepends_on: [T-A1]\n---\n# T2\n",
    );
    let ok_org = sb.run(&["doctor", "--org-dir", org_dir.to_str().unwrap(), "--json"]);
    assert_eq!(ok_org.status.code(), Some(0), "{}", text(&ok_org.stdout));

    // Corrupt with duplicate T-A1, dangling depends_on, and unclosed front matter.
    write_file(
        &org_dir.join("tasks/t3/task.md"),
        "---\nid: t3\nkey: T-A1\ndepends_on: [T-GHOST]\n---\n",
    );
    write_file(&org_dir.join("docs/broken.md"), "---\nid: d1\n");
    let bad_org = sb.run(&["doctor", "--org-dir", org_dir.to_str().unwrap(), "--json"]);
    assert_eq!(bad_org.status.code(), Some(1), "{}", text(&bad_org.stdout));
    let bad_json: Value = serde_json::from_slice(&bad_org.stdout).unwrap();
    let checks = bad_json["checks"].as_array().unwrap();
    assert!(checks
        .iter()
        .any(|c| c["id"] == "org.integrity.syntax" && c["status"] == "fail"));
    assert!(checks
        .iter()
        .any(|c| c["id"] == "org.integrity.display_keys" && c["status"] == "fail"));
    assert!(checks
        .iter()
        .any(|c| c["id"] == "org.integrity.cross_links" && c["status"] == "fail"));
}

#[test]
fn support_bundle_writes_0600_atomic_redacted_deterministic_json_and_refuses_symlinks() {
    let sb = DiagSandbox::new();
    let acct = sb.setup_instance(Some("sup"));
    seed_administrator(&sb.base, Some("sup"), &sb.keys).unwrap();
    write_file(
        &acct.office_home.join("logs/host.log"),
        "Authorization: Bearer support_secret_token_999\nAGENT_OS_SESSION_TOKEN=sup_sess_888\n",
    );

    let out1 = sb.root.join("bundle1.json");
    let out2 = sb.root.join("bundle2.json");
    for path in [&out1, &out2] {
        let res = sb.run(&[
            "support",
            "--instance",
            "sup",
            "--output",
            path.to_str().unwrap(),
            "--json",
        ]);
        assert_eq!(res.status.code(), Some(0), "{}", text(&res.stderr));
        let mode = fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "support bundle must be mode 0600");
    }

    let b1 = fs::read_to_string(&out1).unwrap();
    let b2 = fs::read_to_string(&out2).unwrap();
    assert_eq!(b1, b2, "support bundle must be deterministic");
    assert!(!b1.contains("support_secret_token_999"), "{b1}");
    assert!(!b1.contains("sup_sess_888"), "{b1}");
    assert!(
        !b1.contains("SQLite format 3"),
        "must never include host-registry.db"
    );

    let parsed: Value = serde_json::from_str(&b1).unwrap();
    assert_eq!(parsed["format"], "agentic-org-support-v1");
    assert!(parsed["redacted_count"].as_u64().unwrap() >= 2);
    assert!(parsed["doctor"]["checks"].is_array());
    assert!(parsed["logs"]["lines"].is_array());

    // Symlink output destination MUST be refused without clobbering target.
    let target = sb.root.join("keep.txt");
    fs::write(&target, "DO_NOT_OVERWRITE").unwrap();
    let link = sb.root.join("bundle-link.json");
    symlink(&target, &link).unwrap();
    let refused = sb.run(&[
        "support",
        "--instance",
        "sup",
        "--output",
        link.to_str().unwrap(),
    ]);
    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&target).unwrap(), "DO_NOT_OVERWRITE");
}

#[test]
fn uninstall_requires_yes_refuses_active_lock_and_clears_registry_created_for_fresh_setup() {
    let sb = DiagSandbox::new();
    let acct = sb.setup_instance(Some("fresh"));
    seed_administrator(&sb.base, Some("fresh"), &sb.keys).unwrap();
    let marker = acct.root.join("host-registry.created");
    assert!(
        marker.is_file(),
        "seeded instance must have host-registry.created"
    );

    // 1. Fails closed without --yes (exit 2, cli::confirmation_required).
    let no_yes = sb.run(&["--json", "uninstall", "--instance", "fresh"]);
    assert_eq!(no_yes.status.code(), Some(2), "{}", text(&no_yes.stderr));
    let env: Value = serde_json::from_slice(&no_yes.stderr).unwrap();
    assert_eq!(env["error"]["code"], "cli::confirmation_required");
    assert!(marker.is_file(), "refused uninstall changed nothing");

    // 2. --dry-run succeeds and changes nothing.
    let dry = sb.run(&["--dry-run", "--json", "uninstall", "--instance", "fresh"]);
    assert_eq!(dry.status.code(), Some(0), "{}", text(&dry.stderr));
    assert!(marker.is_file(), "dry-run changed nothing");

    // 3. Refuses while host.lock is held (simulating active service host).
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(acct.office_home.join("host.lock"))
        .unwrap();
    lock_file.try_lock().unwrap();
    let locked = sb.run(&["--json", "uninstall", "--instance", "fresh", "--yes"]);
    assert_eq!(locked.status.code(), Some(1), "{}", text(&locked.stderr));
    let lock_env: Value = serde_json::from_slice(&locked.stderr).unwrap();
    assert_eq!(lock_env["error"]["code"], "uninstall::host_running");
    assert!(
        marker.is_file(),
        "refused uninstall while locked changed nothing"
    );
    lock_file.unlock().unwrap();
    drop(lock_file);

    // 4. Clean uninstall with --yes clears host-registry.created and instance state.
    let done = sb.run(&["uninstall", "--instance", "fresh", "--yes", "--json"]);
    assert_eq!(done.status.code(), Some(0), "{}", text(&done.stderr));
    let rjson: Value = serde_json::from_slice(&done.stdout).unwrap();
    assert_eq!(rjson["registry_created_cleared"], true, "{rjson}");
    assert_eq!(rjson["locator_entry_removed"], true, "{rjson}");
    assert!(!marker.exists(), "host-registry.created must be cleared");

    // 5. Subsequent fresh setup + host registry creation succeeds cleanly without stale marker block!
    let fresh2 = sb.setup_instance(Some("fresh"));
    seed_administrator(&sb.base, Some("fresh"), &sb.keys).unwrap();
    assert!(fresh2.root.join("host-registry.created").is_file());
    assert!(fresh2.office_home.join("host-registry.db").is_file());

    // 6. Stale-marker-only recovery: if host-registry.db is gone while host-registry.created
    //    lingers, uninstall --yes still clears the marker cleanly.
    fs::remove_file(fresh2.office_home.join("host-registry.db")).unwrap();
    let recover = sb.run(&["uninstall", "--instance", "fresh", "--yes", "--json"]);
    assert_eq!(recover.status.code(), Some(0), "{}", text(&recover.stderr));
    let rec_json: Value = serde_json::from_slice(&recover.stdout).unwrap();
    assert_eq!(rec_json["registry_created_cleared"], true, "{rec_json}");
    assert!(!fresh2.root.join("host-registry.created").exists());
}

#[test]
fn uninstall_refuses_symlinks_traversal_busy_registry_lock_and_service_errors() {
    // 1. Symlinked office-state -> victim dir: zero deletion outside OR inside instance.
    let sb = DiagSandbox::new();
    let acct = sb.setup_instance(Some("symoff"));
    seed_administrator(&sb.base, Some("symoff"), &sb.keys).unwrap();
    let victim = sb.root.join("victim-office-state");
    write_file(&victim.join("host-registry.db"), "DO_NOT_DELETE");
    write_file(&victim.join("logs/keep.log"), "KEEP_LOG");
    fs::remove_dir_all(&acct.office_home).unwrap();
    symlink(&victim, &acct.office_home).unwrap();
    let res = sb.run(&["--json", "uninstall", "--instance", "symoff", "--yes"]);
    assert_eq!(res.status.code(), Some(1), "{}", text(&res.stderr));
    assert!(victim.join("host-registry.db").is_file());
    assert!(victim.join("logs/keep.log").is_file());
    assert!(acct.root.join("host-registry.created").is_file());

    // 2. Symlinked setup_dir -> victim dir: zero deletion.
    let sb2 = DiagSandbox::new();
    let acct2 = sb2.setup_instance(Some("symsetup"));
    seed_administrator(&sb2.base, Some("symsetup"), &sb2.keys).unwrap();
    let victim_setup = sb2.root.join("victim-setup");
    write_file(&victim_setup.join("setup.v1.json"), "KEEP_SETUP");
    write_file(&victim_setup.join("setup.lock"), "KEEP_LOCK");
    let setup_dir = sb2.base.join("setup/symsetup");
    fs::remove_dir_all(&setup_dir).unwrap();
    symlink(&victim_setup, &setup_dir).unwrap();
    let res2 = sb2.run(&["--json", "uninstall", "--instance", "symsetup", "--yes"]);
    assert_eq!(res2.status.code(), Some(1), "{}", text(&res2.stderr));
    assert!(victim_setup.join("setup.v1.json").is_file());
    assert!(victim_setup.join("setup.lock").is_file());
    assert!(acct2.root.join("host-registry.created").is_file());

    // 3. Symlinked `<user_temp>/ao` -> victim parent: zero deletion.
    let sb3 = DiagSandbox::new();
    let acct3 = sb3.setup_instance(Some("symao"));
    seed_administrator(&sb3.base, Some("symao"), &sb3.keys).unwrap();
    let handle = acct3.runtime.file_name().unwrap().to_owned();
    let victim_ao = sb3.root.join("victim-ao");
    write_file(&victim_ao.join(&handle).join("keep.txt"), "KEEP_AO");
    let ao_dir = sb3.base.join("temp/ao");
    fs::remove_dir_all(&ao_dir).unwrap();
    symlink(&victim_ao, &ao_dir).unwrap();
    let res3 = sb3.run(&["--json", "uninstall", "--instance", "symao", "--yes"]);
    assert_eq!(res3.status.code(), Some(1), "{}", text(&res3.stderr));
    assert!(victim_ao.join(&handle).join("keep.txt").is_file());
    assert!(acct3.root.join("host-registry.created").is_file());

    // 4. `..` traversal in instance.json (`temp_folder = <tmp>/ao/../../victim`): zero deletion.
    let sb4 = DiagSandbox::new();
    let acct4 = sb4.setup_instance(Some("trav"));
    seed_administrator(&sb4.base, Some("trav"), &sb4.keys).unwrap();
    let victim_trav = sb4.base.join("victim");
    write_file(&victim_trav.join("keep.txt"), "KEEP_TRAV");
    let cfg_path = acct4.root.join("instance.json");
    let mut cfg: Value = serde_json::from_slice(&fs::read(&cfg_path).unwrap()).unwrap();
    cfg["binding"]["temp_folder"] =
        Value::String(sb4.base.join("temp/ao/../../victim").display().to_string());
    fs::write(&cfg_path, serde_json::to_vec(&cfg).unwrap()).unwrap();
    let res4 = sb4.run(&["--json", "uninstall", "--instance", "trav", "--yes"]);
    assert!(
        !res4.status.success(),
        "traversal temp_folder must fail closed"
    );
    assert!(victim_trav.join("keep.txt").is_file());
    assert!(acct4.root.join("host-registry.created").is_file());

    // 5. Held `locator/registry.lock` fails closed BEFORE any instance/setup file is deleted.
    let sb5 = DiagSandbox::new();
    let acct5 = sb5.setup_instance(Some("reglock"));
    seed_administrator(&sb5.base, Some("reglock"), &sb5.keys).unwrap();
    let reg_lock_path = sb5.base.join("locator/registry.lock");
    let reg_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&reg_lock_path)
        .unwrap();
    reg_lock.try_lock().unwrap();
    let res5 = sb5.run(&["--json", "uninstall", "--instance", "reglock", "--yes"]);
    assert_eq!(res5.status.code(), Some(1), "{}", text(&res5.stderr));
    let env5: Value = serde_json::from_slice(&res5.stderr).unwrap();
    assert_eq!(env5["error"]["code"], "uninstall::locator_busy");
    assert!(acct5.root.join("host-registry.created").is_file());
    assert!(acct5.office_home.join("host-registry.db").is_file());
    assert!(sb5.base.join("setup/reglock/setup.v1.json").is_file());
    reg_lock.unlock().unwrap();
    drop(reg_lock);

    // 6. Symlinked `locator/registry.lock` fails closed BEFORE deleting anything.
    let lock_victim = sb5.root.join("lock-victim.txt");
    fs::write(&lock_victim, "LOCK_VICTIM").unwrap();
    fs::remove_file(&reg_lock_path).unwrap();
    symlink(&lock_victim, &reg_lock_path).unwrap();
    let res6 = sb5.run(&["--json", "uninstall", "--instance", "reglock", "--yes"]);
    assert_eq!(res6.status.code(), Some(1), "{}", text(&res6.stderr));
    let env6: Value = serde_json::from_slice(&res6.stderr).unwrap();
    assert_eq!(env6["error"]["code"], "uninstall::unsafe_lock");
    assert!(acct5.root.join("host-registry.created").is_file());
    assert_eq!(fs::read_to_string(&lock_victim).unwrap(), "LOCK_VICTIM");

    // 7. Failing service status (invalid service ID -> ServiceError::Invalid) fails closed with zero deletion.
    let sb7 = DiagSandbox::new();
    let acct7 = sb7.setup_instance(Some("svcerr"));
    seed_administrator(&sb7.base, Some("svcerr"), &sb7.keys).unwrap();
    let setup_rec = sb7.base.join("setup/svcerr/setup.v1.json");
    let mut sdoc: Value = serde_json::from_slice(&fs::read(&setup_rec).unwrap()).unwrap();
    sdoc["instance_id"] = Value::String("bad/service/id".to_owned());
    fs::write(&setup_rec, serde_json::to_vec(&sdoc).unwrap()).unwrap();
    let res7 = sb7.run(&["--json", "uninstall", "--instance", "svcerr", "--yes"]);
    assert_eq!(res7.status.code(), Some(1), "{}", text(&res7.stderr));
    let env7: Value = serde_json::from_slice(&res7.stderr).unwrap();
    assert_eq!(env7["error"]["code"], "uninstall::service");
    assert!(acct7.root.join("host-registry.created").is_file());
}
