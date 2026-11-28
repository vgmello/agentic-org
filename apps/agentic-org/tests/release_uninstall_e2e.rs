//! End-to-end integration test for `TASK-60` AC #1 (FC-022, FC-154):
//! signed release staging/verification/install/update activation (including rollback safety via
//! `ServiceRefresh`) and clean uninstall explicitly clearing `host-registry.created` so a fresh
//! setup succeeds cleanly without dangling markers or clobbering non-installed files.

#![cfg(debug_assertions)]

use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use org_host_setup::fixture::{registered_account, seed_administrator};
use org_host_setup::{InstallAttempt, RefreshOutcome, ServiceRefresh};
use org_platform::service::{Registration, ServiceError, ServiceManager, ServiceStatus, Unit};
use serde_json::Value;
use std::ffi::OsString;
use std::sync::Mutex;

const BIN: &str = env!("CARGO_BIN_EXE_agentic-org");

struct ReleaseSandbox {
    _dir: tempfile::TempDir,
    root: PathBuf,
    base: PathBuf,
    keys: PathBuf,
    svc: PathBuf,
    store: PathBuf,
}

impl ReleaseSandbox {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("rel")
            .tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap())
            .unwrap();
        let root = dir.path().canonicalize().unwrap();
        let base = root.join("base");
        let keys = root.join("keys");
        let svc = root.join("svc");
        let store = root.join("release-store");
        for sub in ["home", "cache", "config", "work"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(sub))
                .unwrap();
        }
        for dir in [&base, &keys, &svc, &store] {
            fs::DirBuilder::new().mode(0o700).create(dir).unwrap();
        }
        Self {
            _dir: dir,
            root,
            base,
            keys,
            svc,
            store,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("ORG_DEV_ACCOUNT_BASE", &self.base)
            .env("ORG_OFFICE_DEV_KEY_DIR", &self.keys)
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

/// In-memory service manager verifying update activation (`v1 -> v2`) and rollback safety when a
/// busy/failed install stops services and then restores them cleanly.
struct MockServiceManager {
    state: Mutex<MockServiceState>,
}

struct MockServiceState {
    unit: Unit,
    running: bool,
    events: Vec<String>,
}

impl MockServiceManager {
    fn new(id: &str, instance_name: &str, program: PathBuf, running: bool) -> Self {
        Self {
            state: Mutex::new(MockServiceState {
                unit: Unit {
                    instance_id: id.to_owned(),
                    program,
                    args: vec![
                        OsString::from("serve"),
                        OsString::from("--name"),
                        OsString::from(instance_name),
                    ],
                    env: Vec::new(),
                },
                running,
                events: Vec::new(),
            }),
        }
    }

    fn program(&self) -> PathBuf {
        self.state.lock().unwrap().unit.program.clone()
    }

    fn running(&self) -> bool {
        self.state.lock().unwrap().running
    }

    fn events(&self) -> Vec<String> {
        self.state.lock().unwrap().events.clone()
    }
}

impl ServiceManager for MockServiceManager {
    fn register(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        let mut st = self.state.lock().unwrap();
        st.unit = unit.clone();
        st.events
            .push(format!("register:{}", unit.program.display()));
        Ok(Registration::Loaded)
    }

    fn start(&self, id: &str) -> Result<(), ServiceError> {
        let mut st = self.state.lock().unwrap();
        assert_eq!(st.unit.instance_id, id);
        st.running = true;
        st.events.push("start".to_owned());
        Ok(())
    }

    fn stop(&self, id: &str) -> Result<(), ServiceError> {
        let mut st = self.state.lock().unwrap();
        assert_eq!(st.unit.instance_id, id);
        st.running = false;
        st.events.push("stop".to_owned());
        Ok(())
    }

    fn status(&self, id: &str) -> Result<ServiceStatus, ServiceError> {
        let st = self.state.lock().unwrap();
        if st.unit.instance_id != id {
            return Ok(ServiceStatus::NotRegistered);
        }
        Ok(if st.running {
            ServiceStatus::Running { pid: 4242 }
        } else {
            ServiceStatus::Stopped
        })
    }

    fn unregister(&self, id: &str) -> Result<(), ServiceError> {
        let mut st = self.state.lock().unwrap();
        assert_eq!(st.unit.instance_id, id);
        st.running = false;
        st.events.push("unregister".to_owned());
        Ok(())
    }

    fn definition(&self, id: &str) -> Result<Option<Unit>, ServiceError> {
        let st = self.state.lock().unwrap();
        Ok((st.unit.instance_id == id).then(|| st.unit.clone()))
    }

    fn redefine(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        let mut st = self.state.lock().unwrap();
        if st.running {
            return Err(ServiceError::Conflict("service is running".into()));
        }
        st.unit = unit.clone();
        st.events
            .push(format!("redefine:{}", unit.program.display()));
        Ok(Registration::Loaded)
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn create_fake_release_bin(store: &Path, release_id: &str) -> PathBuf {
    let dir = store.join("releases").join(release_id);
    fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("agentic-org");
    fs::write(&bin, b"#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
    bin.canonicalize().unwrap()
}

#[test]
fn release_verify_archive_never_creates_or_touches_store() {
    let sb = ReleaseSandbox::new();
    let forged = sb.root.join("forged-verify.tar.gz");
    fs::write(&forged, b"not-a-valid-signed-archive").unwrap();

    // 1. Nonexistent `--store` must remain completely uncreated after verifying a `.tar.gz`.
    let nonexistent_store = sb.root.join("uncreated-verify-store");
    let verify_fail = sb.run(&[
        "release",
        "verify",
        forged.to_str().unwrap(),
        "--store",
        nonexistent_store.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !verify_fail.status.success(),
        "forged/untrusted archive must fail closed"
    );
    assert!(
        !nonexistent_store.exists(),
        "release verify on .tar.gz must NEVER create a nonexistent --store"
    );

    // 2. Read-only (`0500`) `--store` must never be written to or used for staging.
    let ro_store = sb.root.join("readonly-verify-store");
    fs::DirBuilder::new().mode(0o700).create(&ro_store).unwrap();
    fs::set_permissions(&ro_store, fs::Permissions::from_mode(0o500)).unwrap();
    let verify_ro = sb.run(&[
        "release",
        "verify",
        forged.to_str().unwrap(),
        "--store",
        ro_store.to_str().unwrap(),
        "--json",
    ]);
    fs::set_permissions(&ro_store, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        !verify_ro.status.success(),
        "forged/untrusted archive must fail closed"
    );
    assert!(
        !text(&verify_ro.stderr).contains("staging folder"),
        "release verify must not stage inside read-only --store: {}",
        text(&verify_ro.stderr)
    );
    assert_eq!(fs::read_dir(&ro_store).unwrap().count(), 0);
}

#[test]
fn release_verify_install_update_activation_rollback_and_clean_uninstall_lifecycle() {
    let sb = ReleaseSandbox::new();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let acct = registered_account(&sb.base, Some("prod"), port).unwrap();
    seed_administrator(&sb.base, Some("prod"), &sb.keys).unwrap();
    let marker = acct.root.join("host-registry.created");
    assert!(
        marker.is_file(),
        "seeded instance must have host-registry.created"
    );

    // Create a user-owned unrelated file outside the instance that uninstall must preserve.
    let unrelated = sb.root.join("keep-user-file.txt");
    fs::write(&unrelated, "MUST_SURVIVE_UNINSTALL").unwrap();

    // 1. CLI `release verify` & `release install` fail closed on unsigned/forged archive and
    //    `release install --dry-run` plans without mutating store or service state.
    let forged = sb.root.join("forged.tar.gz");
    fs::write(&forged, b"not-a-valid-signed-archive").unwrap();
    let verify_fail = sb.run(&[
        "release",
        "verify",
        forged.to_str().unwrap(),
        "--store",
        sb.store.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !verify_fail.status.success(),
        "forged/untrusted archive must fail closed"
    );

    let dry_install = sb.run(&[
        "--dry-run",
        "--json",
        "release",
        "install",
        forged.to_str().unwrap(),
        "--store",
        sb.store.to_str().unwrap(),
    ]);
    assert_eq!(
        dry_install.status.code(),
        Some(0),
        "{}",
        text(&dry_install.stderr)
    );
    let plan: Value = serde_json::from_slice(&dry_install.stdout).unwrap();
    assert_eq!(plan["action"], "release_install");
    assert_eq!(plan["kind"], "archive");

    // 2. Service activation & rollback safety across release v1 -> failed v2 -> activated v2.
    let v1_bin = create_fake_release_bin(&sb.store, "rel-1.0.0");
    let v2_bin = create_fake_release_bin(&sb.store, "rel-2.0.0");

    // Point the setup record for `prod` at a registered service so ServiceRefresh inspects it.
    let setup_record = sb.base.join("setup/prod/setup.v1.json");
    let mut rec: Value = serde_json::from_slice(&fs::read(&setup_record).unwrap()).unwrap();
    let instance_id = rec["instance_id"].as_str().unwrap().to_owned();
    let service_id = instance_id.to_ascii_lowercase();
    rec["input"]["service"] = Value::String("registered".to_owned());
    rec["service_registered"] = Value::Bool(true);
    fs::write(&setup_record, serde_json::to_vec(&rec).unwrap()).unwrap();

    // Rollback safety: if the store is busy (first attempt) and the retry fails, ServiceRefresh
    // stops the v1 service to release the store lock and restarts it cleanly on v1!
    let mgr = MockServiceManager::new(&service_id, "prod", v1_bin.clone(), true);
    let mut refresh_fail = ServiceRefresh::inspect_at(&mgr, &sb.base, &sb.store);
    let mut attempts = 0usize;
    let err = refresh_fail
        .install::<()>(&mgr, || {
            attempts += 1;
            if attempts == 1 {
                Err(InstallAttempt::Busy("store locked by running v1".into()))
            } else {
                Err(InstallAttempt::Failed("tampered v2 refused".into()))
            }
        })
        .unwrap_err();
    assert!(err.contains("tampered v2 refused"), "{err}");
    assert_eq!(mgr.program(), v1_bin, "failed update must remain on v1");
    assert!(
        mgr.running(),
        "failed update must restart stopped v1 service"
    );
    assert_eq!(mgr.events(), vec!["stop", "start"]);

    // Successful update activation: stops running v1 on busy lock, installs v2, re-registers v2,
    // and starts the service on v2!
    let mgr_ok = MockServiceManager::new(&service_id, "prod", v1_bin.clone(), true);
    let mut refresh_ok = ServiceRefresh::inspect_at(&mgr_ok, &sb.base, &sb.store);
    let mut tries = 0usize;
    let activated: &str = refresh_ok
        .install(&mgr_ok, || {
            tries += 1;
            if tries == 1 {
                Err(InstallAttempt::Busy("store locked".into()))
            } else {
                Ok("rel-2.0.0")
            }
        })
        .unwrap();
    assert_eq!(activated, "rel-2.0.0");
    let reports = refresh_ok.refresh(&mgr_ok, &v2_bin, true);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, RefreshOutcome::Refreshed);
    assert_eq!(mgr_ok.program(), v2_bin);
    assert!(mgr_ok.running());

    // Restore setup record to foreground mode so real `uninstall` runs cleanly without systemd.
    rec["input"]["service"] = Value::String("foreground".to_owned());
    fs::write(&setup_record, serde_json::to_vec(&rec).unwrap()).unwrap();

    // 3. Clean uninstall removes host artifacts and EXPLICITLY clears `host-registry.created`.
    let uninstall = sb.run(&["--json", "uninstall", "--instance", "prod", "--yes"]);
    assert_eq!(
        uninstall.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        text(&uninstall.stdout),
        text(&uninstall.stderr)
    );
    let ujson: Value = serde_json::from_slice(&uninstall.stdout).unwrap();
    assert_eq!(ujson["registry_created_cleared"], true, "{ujson}");
    assert_eq!(ujson["locator_entry_removed"], true, "{ujson}");
    assert!(!marker.exists(), "host-registry.created must be cleared");
    assert!(
        !acct.office_home.join("host-registry.db").exists(),
        "host-registry.db must be removed"
    );
    assert_eq!(
        fs::read_to_string(&unrelated).unwrap(),
        "MUST_SURVIVE_UNINSTALL",
        "non-installed user files must remain untouched"
    );

    // 4. Fresh setup + administrator seeding succeeds cleanly on the same instance name!
    let fresh = registered_account(&sb.base, Some("prod"), port).unwrap();
    seed_administrator(&sb.base, Some("prod"), &sb.keys).unwrap();
    assert!(fresh.root.join("host-registry.created").is_file());
    assert!(fresh.office_home.join("host-registry.db").is_file());
}
