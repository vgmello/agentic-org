//! End-to-end process-level tests for `agentic-org instance rebind` (`TASK-60` AC #2, FC-148).
//!
//! Runs the real `agentic-org` binary against a hermetic temporary sandbox with
//! `ORG_DEV_ACCOUNT_BASE` and `ORG_DEV_SERVICE_DIR` and proves fail-closed confirmation,
//! running-lock and running-service refusal, symlink/unreadable-directory refusal, atomic
//! inode rebinding after copying/restoring an instance root, and `RebindRecoveryRequired` on
//! tampered configuration.

#![cfg(debug_assertions)]

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{symlink, DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use org_host::instance::registry::Registry;
use org_host::instance::Error as InstanceError;
use org_host_setup::fixture::registered_account;
use org_platform::service::{ServiceManager, Unit};
use org_platform::testing::ChildServiceManager;
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_agentic-org");

struct RebindSandbox {
    _dir: tempfile::TempDir,
    root: PathBuf,
    base: PathBuf,
    temp: PathBuf,
    svc: PathBuf,
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
        let svc = root.join("svc");
        for sub in ["home", "cache", "config", "work"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(sub))
                .unwrap();
        }
        fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&temp).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&svc).unwrap();
        Self {
            _dir: dir,
            root,
            base,
            temp,
            svc,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("ORG_DEV_ACCOUNT_BASE", &self.base)
            .env("ORG_DEV_SERVICE_DIR", &self.svc)
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

fn displace_onto_new_inode(sb: &RebindSandbox, root: &Path, suffix: &str) {
    let copy_dir = sb.root.join(format!("copied-root-{suffix}"));
    copy_dir_owner_only(root, &copy_dir);
    let old_dir = sb.root.join(format!("displaced-old-root-{suffix}"));
    fs::rename(root, &old_dir).unwrap();
    fs::rename(&copy_dir, root).unwrap();
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
    displace_onto_new_inode(&sb, &acct.root, "main");

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

    // 4. Held `office.lock` inside `office-state/offices/<id>` fails closed with `instance::busy`.
    let office_dir = acct.office_home.join("offices").join("off-live");
    fs::create_dir_all(&office_dir).unwrap();
    let office_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(office_dir.join("office.lock"))
        .unwrap();
    office_lock.try_lock().unwrap();
    let busy_off = sb.run(&["--json", "instance", "rebind", "migrated", "--yes"]);
    assert_eq!(
        busy_off.status.code(),
        Some(1),
        "{}",
        text(&busy_off.stderr)
    );
    let busy_off_env: Value = serde_json::from_slice(&busy_off.stderr).unwrap();
    assert_eq!(busy_off_env["error"]["code"], "instance::busy");
    office_lock.unlock().unwrap();
    drop(office_lock);

    // 5. Running OS service fails closed with `instance::busy`.
    let svc_mgr = ChildServiceManager::new(&sb.svc);
    let svc_id = acct.instance_id.to_ascii_lowercase();
    let sleeper = sb.root.join("sleep-svc.sh");
    fs::write(&sleeper, b"#!/bin/sh\nsleep 30\n").unwrap();
    fs::set_permissions(&sleeper, fs::Permissions::from_mode(0o700)).unwrap();
    svc_mgr
        .register(&Unit {
            instance_id: svc_id.clone(),
            program: sleeper,
            args: Vec::<OsString>::new(),
            env: Vec::new(),
        })
        .unwrap();
    svc_mgr.start(&svc_id).unwrap();
    let running_svc = sb.run(&["--json", "instance", "rebind", "migrated", "--yes"]);
    let _ = svc_mgr.stop(&svc_id);
    let _ = svc_mgr.unregister(&svc_id);
    assert_eq!(
        running_svc.status.code(),
        Some(1),
        "{}",
        text(&running_svc.stderr)
    );
    let svc_env: Value = serde_json::from_slice(&running_svc.stderr).unwrap();
    assert_eq!(svc_env["error"]["code"], "instance::busy");

    // 6. Confirmed rebind succeeds and updates both instance.json and locator/instances.json.
    let ok = sb.run(&["--json", "instance", "rebind", "migrated", "--yes"]);
    assert_eq!(ok.status.code(), Some(0), "{}", text(&ok.stderr));
    let ok_json: Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(ok_json["rebound"], true);
    assert_eq!(ok_json["instance"], "migrated");

    // Reservation now succeeds cleanly!
    let reservation = registry.reserve(Some("migrated"), &acct.root).unwrap();
    drop(reservation);

    // 7. Tampered instance.json (identity matches neither recorded nor actual inode) fails closed
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

#[test]
fn instance_rebind_fails_closed_on_symlinked_or_unreadable_office_state_and_offices() {
    let sb = RebindSandbox::new();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let acct = registered_account(&sb.base, Some("symrb"), port).unwrap();
    displace_onto_new_inode(&sb, &acct.root, "sym");
    let registry = Registry::open(&sb.base.join("locator"), &sb.temp).unwrap();
    let decoy = sb.root.join("decoy");
    fs::DirBuilder::new().mode(0o700).create(&decoy).unwrap();

    // 1. Symlinked `office-state` must fail closed without rebinding.
    let backup_office_home = sb.root.join("saved-office-state");
    fs::rename(&acct.office_home, &backup_office_home).unwrap();
    symlink(&decoy, &acct.office_home).unwrap();
    let res1 = sb.run(&["--json", "instance", "rebind", "symrb", "--yes"]);
    assert_eq!(res1.status.code(), Some(1), "{}", text(&res1.stderr));
    let env1: Value = serde_json::from_slice(&res1.stderr).unwrap();
    assert_eq!(env1["error"]["code"], "instance::unsafe_lock");
    assert!(registry.reserve(Some("symrb"), &acct.root).is_err());
    fs::remove_file(&acct.office_home).unwrap();
    fs::rename(&backup_office_home, &acct.office_home).unwrap();

    // 2. Symlinked `office-state/offices` must fail closed without rebinding.
    let offices_dir = acct.office_home.join("offices");
    if offices_dir.exists() {
        fs::remove_dir_all(&offices_dir).unwrap();
    }
    symlink(&decoy, &offices_dir).unwrap();
    let res2 = sb.run(&["--json", "instance", "rebind", "symrb", "--yes"]);
    assert_eq!(res2.status.code(), Some(1), "{}", text(&res2.stderr));
    let env2: Value = serde_json::from_slice(&res2.stderr).unwrap();
    assert_eq!(env2["error"]["code"], "instance::unsafe_lock");
    assert!(registry.reserve(Some("symrb"), &acct.root).is_err());
    fs::remove_file(&offices_dir).unwrap();

    // 3. Unreadable `office-state/offices` (mode 0000) must fail closed without rebinding.
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&offices_dir)
        .unwrap();
    fs::set_permissions(&offices_dir, fs::Permissions::from_mode(0o000)).unwrap();
    let res3 = sb.run(&["--json", "instance", "rebind", "symrb", "--yes"]);
    fs::set_permissions(&offices_dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(res3.status.code(), Some(1), "{}", text(&res3.stderr));
    let env3: Value = serde_json::from_slice(&res3.stderr).unwrap();
    assert_eq!(env3["error"]["code"], "instance::unsafe_lock");
    assert!(registry.reserve(Some("symrb"), &acct.root).is_err());

    // 4. Symlinked office entry inside `offices` must fail closed without rebinding.
    symlink(&decoy, offices_dir.join("office-sym")).unwrap();
    let res4 = sb.run(&["--json", "instance", "rebind", "symrb", "--yes"]);
    assert_eq!(res4.status.code(), Some(1), "{}", text(&res4.stderr));
    let env4: Value = serde_json::from_slice(&res4.stderr).unwrap();
    assert_eq!(env4["error"]["code"], "instance::unsafe_lock");
    assert!(registry.reserve(Some("symrb"), &acct.root).is_err());
}
