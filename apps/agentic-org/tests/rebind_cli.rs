//! End-to-end process-level tests for `agentic-org instance rebind` (`TASK-60` AC #2, FC-148).
//!
//! Runs the real `agentic-org` binary against a hermetic temporary sandbox with
//! `ORG_DEV_ACCOUNT_BASE` and proves fail-closed confirmation, running-lock refusal, atomic
//! inode rebinding after copying/restoring an instance root, and `RebindRecoveryRequired` on
//! tampered configuration.

#![cfg(debug_assertions)]

use std::fs::{self, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use org_host::instance::registry::Registry;
use org_host::instance::Error as InstanceError;
use org_host_setup::fixture::registered_account;
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_agentic-org");

struct RebindSandbox {
    _dir: tempfile::TempDir,
    root: PathBuf,
    base: PathBuf,
    temp: PathBuf,
}

impl RebindSandbox {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("rb")
            .tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap())
            .unwrap();
        let root = dir.path().canonicalize().unwrap();
        let base = root.join("base");
        let temp = base.join("temp");
        for sub in ["home", "cache", "config", "work"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(sub))
                .unwrap();
        }
        fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&temp).unwrap();
        Self {
            _dir: dir,
            root,
            base,
            temp,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("ORG_DEV_ACCOUNT_BASE", &self.base)
            .env("AGENTIC_ORG_CACHE_DIR", self.root.join("cache"))
            .env("AGENTIC_ORG_CONFIG_DIR", self.root.join("config"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .current_dir(self.root.join("work"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

fn copy_dir_owner_only(src: &Path, dst: &Path) {
    fs::DirBuilder::new().mode(0o700).create(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let meta = fs::symlink_metadata(&from).unwrap();
        if meta.is_dir() {
            copy_dir_owner_only(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            fs::set_permissions(&to, fs::Permissions::from_mode(mode)).unwrap();
        }
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn instance_rebind_repairs_restored_root_inode_and_fails_closed_when_unsafe_or_locked() {
    let sb = RebindSandbox::new();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let acct = registered_account(&sb.base, Some("migrated"), port).unwrap();

    // Simulate restoring/copying the instance root onto a brand-new directory inode.
    let copy_dir = sb.root.join("copied-root");
    copy_dir_owner_only(&acct.root, &copy_dir);
    let old_dir = sb.root.join("displaced-old-root");
    fs::rename(&acct.root, &old_dir).unwrap();
    fs::rename(&copy_dir, &acct.root).unwrap();

    // Before rebind, Registry::reserve MUST fail closed with RootUnavailable mentioning rebind.
    let registry = Registry::open(&sb.base.join("locator"), &sb.temp).unwrap();
    let err = match registry.reserve(Some("migrated"), &acct.root) {
        Ok(_) => panic!("changed inode must be refused before rebind"),
        Err(e) => e,
    };
    assert!(
        matches!(err, InstanceError::RootUnavailable { .. }),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("agentic-org instance rebind"),
        "{err}"
    );

    // 1. `--dry-run` previews without mutating anything.
    let dry = sb.run(&["--dry-run", "--json", "instance", "rebind", "migrated"]);
    assert_eq!(dry.status.code(), Some(0), "{}", text(&dry.stderr));
    let dry_json: Value = serde_json::from_slice(&dry.stdout).unwrap();
    assert_eq!(dry_json["action"], "instance_rebind");
    assert_eq!(dry_json["instance"], "migrated");
    assert!(registry.reserve(Some("migrated"), &acct.root).is_err());

    // 2. Missing `--yes` in non-interactive mode fails closed with exit 2 (confirmation_required).
    let no_yes = sb.run(&["--json", "instance", "rebind", "migrated"]);
    assert_eq!(no_yes.status.code(), Some(2), "{}", text(&no_yes.stderr));
    let env: Value = serde_json::from_slice(&no_yes.stderr).unwrap();
    assert_eq!(env["error"]["code"], "cli::confirmation_required");
    assert!(registry.reserve(Some("migrated"), &acct.root).is_err());

    // 3. Held `host.lock` fails closed with `instance::busy`.
    let lock_path = acct.office_home.join("host.lock");
    let host_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .unwrap();
    host_lock.try_lock().unwrap();
    let busy = sb.run(&["--json", "instance", "rebind", "migrated", "--yes"]);
    assert_eq!(busy.status.code(), Some(1), "{}", text(&busy.stderr));
    let busy_env: Value = serde_json::from_slice(&busy.stderr).unwrap();
    assert_eq!(busy_env["error"]["code"], "instance::busy");
    host_lock.unlock().unwrap();
    drop(host_lock);

    // 4. Confirmed rebind succeeds and updates both instance.json and locator/instances.json.
    let ok = sb.run(&["--json", "instance", "rebind", "migrated", "--yes"]);
    assert_eq!(ok.status.code(), Some(0), "{}", text(&ok.stderr));
    let ok_json: Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(ok_json["rebound"], true);
    assert_eq!(ok_json["instance"], "migrated");

    // Reservation now succeeds cleanly!
    let reservation = registry.reserve(Some("migrated"), &acct.root).unwrap();
    drop(reservation);

    // 5. Tampered instance.json (identity matches neither recorded nor actual inode) fails closed
    // with `instance::rebind_recovery_required`.
    let cfg_path = acct.root.join("instance.json");
    let mut cfg: Value = serde_json::from_slice(&fs::read(&cfg_path).unwrap()).unwrap();
    cfg["binding"]["identity"]["inode"] = Value::from(9_999_999_999_u64);
    fs::write(&cfg_path, serde_json::to_vec(&cfg).unwrap()).unwrap();
    let tampered = sb.run(&["--json", "instance", "rebind", "migrated", "--yes"]);
    assert_eq!(
        tampered.status.code(),
        Some(1),
        "{}",
        text(&tampered.stderr)
    );
    let tamp_env: Value = serde_json::from_slice(&tampered.stderr).unwrap();
    assert_eq!(
        tamp_env["error"]["code"],
        "instance::rebind_recovery_required"
    );
}
