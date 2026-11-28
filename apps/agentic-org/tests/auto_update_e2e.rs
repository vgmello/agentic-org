//! End-to-end deterministic integration tests for `TASK-134` (ADR-0009):
//! pull-verified, idle-drained remote satellite & macOS HQ auto-update, post-restart health
//! probing, automatic rollback + quarantine tombstone, manual rollback, security negative cases,
//! CLI fail-closed validation, and cross-language verification of `scripts/release_tool.py pointer`.

#![cfg(debug_assertions)]

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{symlink, DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use org_host::release::{
    LocalDirectoryFetcher, PullConfig, ReleasePin, ReleaseStore, UpdateStateStore,
};
use org_host_setup::auto_update::{
    drain::{ActiveTurnSnapshot, DrainConfig, DrainPhase, DrainProbe},
    health::{HealthProbe, HealthTarget},
    run_auto_update_with_policy, run_manual_rollback_with_policy, AutoUpdateConfig,
    AutoUpdatePorts, AutoUpdateStatus, RollbackConfig, RollbackPorts, RollbackStatus,
};
use org_host_setup::fixture::{registered_account, seed_administrator};
use org_platform::service::{Registration, ServiceError, ServiceManager, ServiceStatus, Unit};
use org_release::{
    verify_pointer, Architecture, Platform, TrustedKeys, TrustedPolicy, UrlPolicy, POINTER_FORMAT,
};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::Value;
use sha2::{Digest, Sha256};

const BIN: &str = env!("CARGO_BIN_EXE_agentic-org");

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Minimal 64-byte valid ELF64 header matching Linux x86_64 or aarch64 (passes real `platform::verify`).
fn synthetic_elf_binary(arch: Architecture, tag: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 96];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN (PIE)
    let machine: u16 = match arch {
        Architecture::X86_64 => 62,
        Architecture::Aarch64 => 183,
    };
    bytes[18..20].copy_from_slice(&machine.to_le_bytes());
    let copy_len = tag.len().min(32);
    bytes[64..64 + copy_len].copy_from_slice(&tag[..copy_len]);
    bytes
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManagerFlavor {
    /// Linux systemd: `redefine` reloads and leaves unit `Stopped` until `start`.
    LinuxSystemd,
    /// macOS launchd: `redefine` unloads the agent (`Unloaded`) until `register` + `start`.
    MacosLaunchd,
}

struct FakePlatformServiceManager {
    flavor: ManagerFlavor,
    state: Mutex<FakeServiceState>,
}

struct FakeServiceState {
    unit: Unit,
    status: ServiceStatus,
    events: Vec<String>,
    /// When set, `start` leaves the process crashed (`Stopped`) if the program path contains this substring.
    crash_if_program_contains: Option<String>,
}

impl FakePlatformServiceManager {
    fn new(
        flavor: ManagerFlavor,
        id: &str,
        instance_name: &str,
        program: PathBuf,
        running: bool,
    ) -> Self {
        Self {
            flavor,
            state: Mutex::new(FakeServiceState {
                unit: Unit {
                    instance_id: id.to_owned(),
                    program,
                    args: vec![
                        OsString::from("serve"),
                        OsString::from("--name"),
                        OsString::from(instance_name),
                    ],
                    env: vec![(OsString::from("HOME"), OsString::from("/home/agents"))],
                },
                status: if running {
                    ServiceStatus::Running { pid: 4242 }
                } else {
                    ServiceStatus::Stopped
                },
                events: Vec::new(),
                crash_if_program_contains: None,
            }),
        }
    }

    fn set_crash_on(&self, release_substr: Option<&str>) {
        self.state.lock().unwrap().crash_if_program_contains = release_substr.map(str::to_owned);
    }

    fn program(&self) -> PathBuf {
        self.state.lock().unwrap().unit.program.clone()
    }

    fn is_running(&self) -> bool {
        matches!(
            self.state.lock().unwrap().status,
            ServiceStatus::Running { .. }
        )
    }

    fn events(&self) -> Vec<String> {
        self.state.lock().unwrap().events.clone()
    }
}

impl ServiceManager for FakePlatformServiceManager {
    fn register(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        let mut st = self.state.lock().unwrap();
        st.unit = unit.clone();
        if st.status == ServiceStatus::Unloaded {
            st.status = ServiceStatus::Stopped;
        }
        st.events
            .push(format!("register:{}", unit.program.display()));
        Ok(Registration::Loaded)
    }

    fn start(&self, id: &str) -> Result<(), ServiceError> {
        let mut st = self.state.lock().unwrap();
        assert_eq!(st.unit.instance_id, id);
        if st.status == ServiceStatus::Unloaded {
            return Err(ServiceError::System(
                "launchd agent must be registered before start".into(),
            ));
        }
        let crashed = st
            .crash_if_program_contains
            .as_deref()
            .is_some_and(|needle| st.unit.program.to_string_lossy().contains(needle));
        if crashed {
            st.status = ServiceStatus::Stopped;
            st.events.push("start:crashed".into());
        } else {
            st.status = ServiceStatus::Running { pid: 5050 };
            st.events.push("start:ok".into());
        }
        Ok(())
    }

    fn stop(&self, id: &str) -> Result<(), ServiceError> {
        let mut st = self.state.lock().unwrap();
        assert_eq!(st.unit.instance_id, id);
        st.status = ServiceStatus::Stopped;
        st.events.push("stop".into());
        Ok(())
    }

    fn status(&self, id: &str) -> Result<ServiceStatus, ServiceError> {
        let st = self.state.lock().unwrap();
        if st.unit.instance_id != id {
            return Ok(ServiceStatus::NotRegistered);
        }
        Ok(st.status)
    }

    fn unregister(&self, id: &str) -> Result<(), ServiceError> {
        let mut st = self.state.lock().unwrap();
        assert_eq!(st.unit.instance_id, id);
        st.status = ServiceStatus::NotRegistered;
        st.events.push("unregister".into());
        Ok(())
    }

    fn definition(&self, id: &str) -> Result<Option<Unit>, ServiceError> {
        let st = self.state.lock().unwrap();
        Ok((st.unit.instance_id == id).then(|| st.unit.clone()))
    }

    fn redefine(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        let mut st = self.state.lock().unwrap();
        if matches!(st.status, ServiceStatus::Running { .. }) {
            return Err(ServiceError::Conflict("service is still running".into()));
        }
        st.unit = unit.clone();
        st.status = match self.flavor {
            ManagerFlavor::LinuxSystemd => ServiceStatus::Stopped,
            ManagerFlavor::MacosLaunchd => ServiceStatus::Unloaded,
        };
        st.events
            .push(format!("redefine:{}", unit.program.display()));
        Ok(Registration::Loaded)
    }
}

struct SequenceDrainProbe {
    busy_polls_left: AtomicUsize,
}

impl SequenceDrainProbe {
    fn new(busy_polls: usize) -> Self {
        Self {
            busy_polls_left: AtomicUsize::new(busy_polls),
        }
    }
}

impl DrainProbe for SequenceDrainProbe {
    fn active_turns(&self) -> Result<Vec<ActiveTurnSnapshot>, String> {
        let cur = self.busy_polls_left.load(Ordering::Acquire);
        if cur == 0 {
            return Ok(Vec::new());
        }
        self.busy_polls_left.fetch_sub(1, Ordering::AcqRel);
        Ok(vec![ActiveTurnSnapshot {
            instance: "prod".into(),
            office_id: "off-sat-1".into(),
            employee_id: "emp-worker".into(),
            run_id: "run-active-42".into(),
        }])
    }
}

struct ScriptedHealthProbe {
    healthy_for_v2: AtomicBool,
    calls: Mutex<Vec<String>>,
    service_manager_program_check: Option<std::sync::Arc<FakePlatformServiceManager>>,
}

impl ScriptedHealthProbe {
    fn always_ok() -> Self {
        Self {
            healthy_for_v2: AtomicBool::new(true),
            calls: Mutex::new(Vec::new()),
            service_manager_program_check: None,
        }
    }

    fn fail_on_v2(mgr: std::sync::Arc<FakePlatformServiceManager>) -> Self {
        Self {
            healthy_for_v2: AtomicBool::new(false),
            calls: Mutex::new(Vec::new()),
            service_manager_program_check: Some(mgr),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl HealthProbe for ScriptedHealthProbe {
    fn check_healthy(&self, target: &HealthTarget, _timeout: Duration) -> Result<String, String> {
        let program = self
            .service_manager_program_check
            .as_ref()
            .map(|m| m.program().display().to_string())
            .unwrap_or_default();
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}:{program}", target.instance));
        if !self.healthy_for_v2.load(Ordering::Acquire) && program.contains("v0.2.0") {
            return Err("hello + instance.status failed: new v0.2.0 host crashed on boot".into());
        }
        Ok("hello + instance.status healthy".into())
    }
}

struct E2eHarness {
    _dir: tempfile::TempDir,
    root: PathBuf,
    base: PathBuf,
    keys_dir: PathBuf,
    svc_dir: PathBuf,
    store: PathBuf,
    state_dir: PathBuf,
    mirror: PathBuf,
    service_id: String,
    key: Ed25519KeyPair,
    key_id: String,
    keys: TrustedKeys,
    policy: TrustedPolicy,
    platform: Platform,
    arch: Architecture,
}

impl E2eHarness {
    fn new(platform: Platform, arch: Architecture) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("au134")
            .tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap())
            .unwrap();
        let root = dir.path().canonicalize().unwrap();
        let base = root.join("base");
        let keys_dir = root.join("keys");
        let svc_dir = root.join("svc");
        let store = root.join("release-store");
        let state_dir = root.join("release-updater");
        let mirror = root.join("mirror");
        for sub in ["home", "cache", "config", "work"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(sub))
                .unwrap();
        }
        for d in [&base, &keys_dir, &svc_dir, &store, &mirror] {
            fs::DirBuilder::new().mode(0o700).create(d).unwrap();
        }
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let _acct = registered_account(&base, Some("prod"), port).unwrap();
        seed_administrator(&base, Some("prod"), &keys_dir).unwrap();

        let setup_record = base.join("setup/prod/setup.v1.json");
        let mut rec: Value = serde_json::from_slice(&fs::read(&setup_record).unwrap()).unwrap();
        let instance_id = rec["instance_id"].as_str().unwrap().to_owned();
        let service_id = instance_id.to_ascii_lowercase();
        rec["input"]["service"] = Value::String("registered".to_owned());
        rec["service_registered"] = Value::Bool(true);
        fs::write(&setup_record, serde_json::to_vec(&rec).unwrap()).unwrap();

        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let pub_bytes = key.public_key().as_ref();
        let key_id = hex(Sha256::digest(pub_bytes).as_slice());
        let reg = serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "keys": [{
                "key_id": key_id,
                "public_key_hex": hex(pub_bytes),
                "min_sequence": 1,
                "max_sequence": 100
            }]
        }))
        .unwrap();
        let keys = TrustedKeys::from_registry_bytes(&reg).unwrap();
        let policy = TrustedPolicy::new(platform, arch, 1, 1).unwrap();

        Self {
            _dir: dir,
            root,
            base,
            keys_dir,
            svc_dir,
            store,
            state_dir,
            mirror,
            service_id,
            key,
            key_id,
            keys,
            policy,
            platform,
            arch,
        }
    }

    fn platform_str(&self) -> &'static str {
        match self.platform {
            Platform::Macos => "macos",
            Platform::Linux => "linux",
        }
    }

    fn arch_str(&self) -> &'static str {
        match self.arch {
            Architecture::Aarch64 => "aarch64",
            Architecture::X86_64 => "x86_64",
        }
    }

    fn write_signed_bundle(&self, release_id: &str, sequence: u64) -> (PathBuf, String, u64) {
        let exe_bytes = match self.platform {
            Platform::Linux => synthetic_elf_binary(self.arch, release_id.as_bytes()),
            Platform::Macos => {
                let mut b = vec![0u8; 64];
                b[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
                let cpu: u32 = match self.arch {
                    Architecture::Aarch64 => 0x0100_000c,
                    Architecture::X86_64 => 0x0100_0007,
                };
                b[4..8].copy_from_slice(&cpu.to_le_bytes());
                b[12..16].copy_from_slice(&2u32.to_le_bytes());
                b
            }
        };
        let ui_bytes = format!("<html>{release_id}</html>").into_bytes();
        let cdhash = (self.platform == Platform::Macos).then(|| "a".repeat(40));

        let mut files = Vec::new();
        for exe in org_release::EXECUTABLE_PATHS {
            files.push(serde_json::json!({
                "path": exe,
                "size": exe_bytes.len(),
                "sha256": hex(Sha256::digest(&exe_bytes).as_slice()),
                "role": "executable",
                "macos_cdhash": cdhash
            }));
        }
        files.push(serde_json::json!({
            "path": "ui/index.html",
            "size": ui_bytes.len(),
            "sha256": hex(Sha256::digest(&ui_bytes).as_slice()),
            "role": "data",
            "macos_cdhash": null
        }));

        let manifest = serde_json::to_vec_pretty(&serde_json::json!({
            "format": org_release::FORMAT,
            "release_id": release_id,
            "sequence": sequence,
            "platform": self.platform_str(),
            "architecture": self.arch_str(),
            "installer_contract": 1,
            "key_id": self.key_id,
            "files": files
        }))
        .unwrap();
        let sig = self.key.sign(&manifest).as_ref().to_vec();

        let mut tar = tar::Builder::new(Vec::new());
        for exe in org_release::EXECUTABLE_PATHS {
            let mut h = tar::Header::new_ustar();
            h.set_size(exe_bytes.len() as u64);
            h.set_mode(0o500);
            h.set_cksum();
            tar.append_data(&mut h, exe, exe_bytes.as_slice()).unwrap();
        }
        for (name, bytes) in [
            ("ui/index.html", ui_bytes.as_slice()),
            ("release.v1.json", manifest.as_slice()),
            ("release.v1.sig", sig.as_slice()),
        ] {
            let mut h = tar::Header::new_ustar();
            h.set_size(bytes.len() as u64);
            h.set_mode(0o400);
            h.set_cksum();
            tar.append_data(&mut h, name, bytes).unwrap();
        }
        let tar_bytes = tar.into_inner().unwrap();
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&tar_bytes).unwrap();
        let gz = enc.finish().unwrap();

        let filename = format!(
            "agentic-org-{release_id}-{}-{}.tar.gz",
            self.platform_str(),
            self.arch_str()
        );
        let dest = self.mirror.join(filename);
        fs::write(&dest, &gz).unwrap();
        let digest = hex(Sha256::digest(&gz).as_slice());
        (dest, digest, gz.len() as u64)
    }

    fn write_signed_pointer(
        &self,
        release_id: &str,
        sequence: u64,
        bundle_url: &str,
        bundle_sha: &str,
        bundle_size: u64,
    ) -> PullConfig {
        let ptr_bytes = serde_json::to_vec_pretty(&serde_json::json!({
            "format": POINTER_FORMAT,
            "channel": "stable",
            "release_id": release_id,
            "sequence": sequence,
            "installer_contract": 1,
            "key_id": self.key_id,
            "published_at": "2026-11-27T23:00:00Z",
            "targets": [{
                "platform": self.platform_str(),
                "architecture": self.arch_str(),
                "url": bundle_url,
                "sha256": bundle_sha,
                "size": bundle_size
            }]
        }))
        .unwrap();
        let sig = self.key.sign(&ptr_bytes).as_ref().to_vec();
        let ptr_path = self.mirror.join("release-pointer.v1.json");
        let sig_path = self.mirror.join("release-pointer.v1.sig");
        fs::write(&ptr_path, ptr_bytes).unwrap();
        fs::write(&sig_path, sig).unwrap();
        PullConfig::with_url_policy(
            format!("file://{}", ptr_path.display()),
            None,
            UrlPolicy::AllowLocalForTesting,
        )
    }

    fn seed_initial_v1(&self, skip_platform_check: bool) -> (ReleasePin, PathBuf) {
        let (bundle_path, sha, size) = self.write_signed_bundle("v0.1.0", 1);
        let pull = self.write_signed_pointer(
            "v0.1.0",
            1,
            &format!("file://{}", bundle_path.display()),
            &sha,
            size,
        );
        let fetcher = LocalDirectoryFetcher::new(&self.mirror).unwrap();
        let mgr = FakePlatformServiceManager::new(
            ManagerFlavor::LinuxSystemd,
            "unused-seed",
            "unused",
            PathBuf::from("/unused"),
            false,
        );
        let drain = SequenceDrainProbe::new(0);
        let health = ScriptedHealthProbe::always_ok();
        let cfg = self.update_config(pull, Duration::from_secs(2), false);
        let receipt = run_auto_update_with_policy(
            &cfg,
            &AutoUpdatePorts {
                fetcher: &fetcher,
                manager: &mgr,
                drain: &drain,
                health: &health,
            },
            &self.policy,
            &self.keys,
            skip_platform_check,
        )
        .unwrap();
        assert_eq!(receipt.status, AutoUpdateStatus::Updated, "{receipt:?}");
        let pin = receipt.active_pin.unwrap();
        let bin = self
            .store
            .join("releases/v0.1.0/agentic-org")
            .canonicalize()
            .unwrap();
        (pin, bin)
    }

    fn update_config(
        &self,
        pull: PullConfig,
        drain_timeout: Duration,
        force: bool,
    ) -> AutoUpdateConfig {
        AutoUpdateConfig {
            store_dir: self.store.clone(),
            state_dir: self.state_dir.clone(),
            setup_base: Some(self.base.clone()),
            pull,
            drain: DrainConfig {
                timeout: drain_timeout,
                poll_interval: Duration::from_millis(10),
                force,
            },
            health_timeout: Duration::from_secs(2),
            restart: true,
            dry_run: false,
            allow_quarantined: false,
            create_default_store_parent: false,
        }
    }

    fn run_cli(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("ORG_DEV_ACCOUNT_BASE", &self.base)
            .env("ORG_OFFICE_DEV_KEY_DIR", &self.keys_dir)
            .env("ORG_DEV_SERVICE_DIR", &self.svc_dir)
            .env("AGENTIC_ORG_CACHE_DIR", self.root.join("cache"))
            .env("AGENTIC_ORG_CONFIG_DIR", self.root.join("config"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .current_dir(self.root.join("work"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

impl Drop for E2eHarness {
    fn drop(&mut self) {
        fn make_writable(path: &Path) {
            if fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
                if let Ok(entries) = fs::read_dir(path) {
                    for entry in entries.flatten() {
                        make_writable(&entry.path());
                    }
                }
            }
        }
        make_writable(self.root.as_path());
    }
}

#[test]
fn linux_satellite_happy_path_drains_swaps_redefines_systemd_and_confirms_health() {
    let fx = E2eHarness::new(Platform::Linux, Architecture::X86_64);
    let (v1_pin, v1_bin) = fx.seed_initial_v1(false);

    let (v2_bundle, v2_sha, v2_size) = fx.write_signed_bundle("v0.2.0", 2);
    let pull = fx.write_signed_pointer(
        "v0.2.0",
        2,
        &format!("file://{}", v2_bundle.display()),
        &v2_sha,
        v2_size,
    );
    let fetcher = LocalDirectoryFetcher::new(&fx.mirror).unwrap();
    let mgr = FakePlatformServiceManager::new(
        ManagerFlavor::LinuxSystemd,
        &fx.service_id,
        "prod",
        v1_bin,
        true,
    );
    let drain = SequenceDrainProbe::new(2);
    let health = ScriptedHealthProbe::always_ok();

    let receipt = run_auto_update_with_policy(
        &fx.update_config(pull, Duration::from_secs(2), false),
        &AutoUpdatePorts {
            fetcher: &fetcher,
            manager: &mgr,
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        false, // Exercises REAL Linux ELF `platform::verify`!
    )
    .unwrap();

    assert_eq!(receipt.status, AutoUpdateStatus::Updated, "{receipt:?}");
    assert_eq!(receipt.previous_pin, Some(v1_pin));
    let active = receipt.active_pin.as_ref().unwrap();
    assert_eq!(active.release_id, "v0.2.0");
    assert_eq!(active.sequence, 2);
    assert_eq!(receipt.drain.as_ref().unwrap().phase, DrainPhase::Ready);
    assert!(mgr.program().to_string_lossy().contains("v0.2.0"));
    assert!(mgr.is_running());
    assert_eq!(health.calls().len(), 1);

    let state = UpdateStateStore::read_readonly(&fx.state_dir).unwrap();
    assert!(state.pending_health_check.is_none());
    assert_eq!(state.confirmed.as_ref(), Some(active));
}

#[test]
fn macos_hq_happy_path_redefines_launchd_unloaded_then_registers_and_starts() {
    let fx = E2eHarness::new(Platform::Macos, Architecture::Aarch64);
    let (_v1_pin, v1_bin) = fx.seed_initial_v1(true);

    let (v2_bundle, v2_sha, v2_size) = fx.write_signed_bundle("v0.2.0", 2);
    let pull = fx.write_signed_pointer(
        "v0.2.0",
        2,
        &format!("file://{}", v2_bundle.display()),
        &v2_sha,
        v2_size,
    );
    let fetcher = LocalDirectoryFetcher::new(&fx.mirror).unwrap();
    let mgr = FakePlatformServiceManager::new(
        ManagerFlavor::MacosLaunchd,
        &fx.service_id,
        "prod",
        v1_bin,
        true,
    );
    let drain = SequenceDrainProbe::new(1);
    let health = ScriptedHealthProbe::always_ok();

    let receipt = run_auto_update_with_policy(
        &fx.update_config(pull, Duration::from_secs(2), false),
        &AutoUpdatePorts {
            fetcher: &fetcher,
            manager: &mgr,
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        true,
    )
    .unwrap();

    assert_eq!(receipt.status, AutoUpdateStatus::Updated, "{receipt:?}");
    assert!(mgr.program().to_string_lossy().contains("v0.2.0"));
    assert!(mgr.is_running());
    let events = mgr.events();
    assert!(
        events.iter().any(|e| e.starts_with("redefine:"))
            && events.iter().any(|e| e.starts_with("register:"))
            && events.iter().any(|e| e == "start:ok"),
        "launchd lifecycle must redefine -> register -> start: {events:?}"
    );
}

#[test]
fn busy_drain_timeout_refuses_swap_with_zero_disruption_to_running_release() {
    let fx = E2eHarness::new(Platform::Linux, Architecture::X86_64);
    let (v1_pin, v1_bin) = fx.seed_initial_v1(false);

    let (v2_bundle, v2_sha, v2_size) = fx.write_signed_bundle("v0.2.0", 2);
    let pull = fx.write_signed_pointer(
        "v0.2.0",
        2,
        &format!("file://{}", v2_bundle.display()),
        &v2_sha,
        v2_size,
    );
    let fetcher = LocalDirectoryFetcher::new(&fx.mirror).unwrap();
    let mgr = FakePlatformServiceManager::new(
        ManagerFlavor::LinuxSystemd,
        &fx.service_id,
        "prod",
        v1_bin.clone(),
        true,
    );
    let drain = SequenceDrainProbe::new(999);
    let health = ScriptedHealthProbe::always_ok();

    let receipt = run_auto_update_with_policy(
        &fx.update_config(pull, Duration::from_millis(50), false),
        &AutoUpdatePorts {
            fetcher: &fetcher,
            manager: &mgr,
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        false,
    )
    .unwrap();

    assert_eq!(receipt.status, AutoUpdateStatus::BusyTimeout, "{receipt:?}");
    assert_eq!(receipt.active_pin, Some(v1_pin.clone()));
    assert_eq!(
        receipt.drain.as_ref().unwrap().phase,
        DrainPhase::BusyTimeout
    );
    assert_eq!(mgr.program(), v1_bin);
    assert!(mgr.is_running());
    assert!(
        mgr.events().is_empty(),
        "running service must NEVER be stopped on drain timeout: {:?}",
        mgr.events()
    );
    let mut store =
        ReleaseStore::open_with_policy(&fx.store, fx.policy.clone(), fx.keys.clone()).unwrap();
    assert_eq!(store.recover().unwrap(), Some(v1_pin));
}

#[test]
fn failed_post_restart_health_triggers_automatic_rollback_quarantine_and_later_v3_recovery() {
    let fx = E2eHarness::new(Platform::Linux, Architecture::X86_64);
    let (v1_pin, v1_bin) = fx.seed_initial_v1(false);

    // 1. Deploy v0.2.0 which passes Ed25519 + SHA-256 verification but fails health check (and even crashes).
    let (v2_bundle, v2_sha, v2_size) = fx.write_signed_bundle("v0.2.0", 2);
    let pull_v2 = fx.write_signed_pointer(
        "v0.2.0",
        2,
        &format!("file://{}", v2_bundle.display()),
        &v2_sha,
        v2_size,
    );
    let fetcher = LocalDirectoryFetcher::new(&fx.mirror).unwrap();
    let mgr = std::sync::Arc::new(FakePlatformServiceManager::new(
        ManagerFlavor::LinuxSystemd,
        &fx.service_id,
        "prod",
        v1_bin.clone(),
        true,
    ));
    mgr.set_crash_on(Some("v0.2.0"));
    let drain = SequenceDrainProbe::new(0);
    let health = ScriptedHealthProbe::fail_on_v2(std::sync::Arc::clone(&mgr));

    let rb_receipt = run_auto_update_with_policy(
        &fx.update_config(pull_v2.clone(), Duration::from_secs(2), false),
        &AutoUpdatePorts {
            fetcher: &fetcher,
            manager: mgr.as_ref(),
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        false,
    )
    .unwrap();

    assert_eq!(
        rb_receipt.status,
        AutoUpdateStatus::RolledBack,
        "{rb_receipt:?}"
    );
    assert_eq!(rb_receipt.active_pin, Some(v1_pin.clone()));
    assert_eq!(rb_receipt.quarantined_release_id.as_deref(), Some("v0.2.0"));
    assert_eq!(
        mgr.program(),
        v1_bin,
        "service unit must point back to v0.1.0 after automatic rollback"
    );
    assert!(
        mgr.is_running(),
        "v0.1.0 service must be running again even though v0.2.0 crashed"
    );
    assert_eq!(rb_receipt.rollback_health_checks.len(), 1);
    assert!(rb_receipt.rollback_health_checks[0].healthy);

    let mut store =
        ReleaseStore::open_with_policy(&fx.store, fx.policy.clone(), fx.keys.clone()).unwrap();
    assert_eq!(store.recover().unwrap(), Some(v1_pin.clone()));
    assert_eq!(store.highest_sequence(), 2);
    drop(store);

    // 2. Subsequent update poll seeing the same broken v0.2.0 pointer skips it (quarantined; no crash loop!).
    let events_before_skip = mgr.events().len();
    let skipped = run_auto_update_with_policy(
        &fx.update_config(pull_v2, Duration::from_secs(2), false),
        &AutoUpdatePorts {
            fetcher: &fetcher,
            manager: mgr.as_ref(),
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        false,
    )
    .unwrap();
    assert_eq!(skipped.status, AutoUpdateStatus::Quarantined, "{skipped:?}");
    assert_eq!(
        mgr.events().len(),
        events_before_skip,
        "quarantined release must not restart services"
    );

    // 3. A fixed v0.3.0 (sequence 3) installs cleanly on top of the rollback commit!
    mgr.set_crash_on(None);
    let (v3_bundle, v3_sha, v3_size) = fx.write_signed_bundle("v0.3.0", 3);
    let pull_v3 = fx.write_signed_pointer(
        "v0.3.0",
        3,
        &format!("file://{}", v3_bundle.display()),
        &v3_sha,
        v3_size,
    );
    let v3_receipt = run_auto_update_with_policy(
        &fx.update_config(pull_v3, Duration::from_secs(2), false),
        &AutoUpdatePorts {
            fetcher: &fetcher,
            manager: mgr.as_ref(),
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        false,
    )
    .unwrap();
    assert_eq!(
        v3_receipt.status,
        AutoUpdateStatus::Updated,
        "{v3_receipt:?}"
    );
    assert_eq!(v3_receipt.active_pin.as_ref().unwrap().release_id, "v0.3.0");
    assert!(mgr.program().to_string_lossy().contains("v0.3.0"));
    assert!(mgr.is_running());

    // 4. Operator manual rollback reverts v0.3.0 back to v0.1.0 and optionally quarantines v0.3.0.
    let manual_rb = run_manual_rollback_with_policy(
        &RollbackConfig {
            store_dir: fx.store.clone(),
            state_dir: fx.state_dir.clone(),
            setup_base: Some(fx.base.clone()),
            target_release_id: Some("v0.1.0".into()),
            drain: DrainConfig {
                timeout: Duration::from_secs(2),
                poll_interval: Duration::from_millis(10),
                force: false,
            },
            health_timeout: Duration::from_secs(2),
            restart: true,
            dry_run: false,
            quarantine_current: true,
        },
        &RollbackPorts {
            manager: mgr.as_ref(),
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        false,
    )
    .unwrap();
    assert_eq!(
        manual_rb.status,
        RollbackStatus::RolledBack,
        "{manual_rb:?}"
    );
    assert_eq!(manual_rb.active_pin, Some(v1_pin));
    assert_eq!(mgr.program(), v1_bin);
    let state = UpdateStateStore::read_readonly(&fx.state_dir).unwrap();
    assert!(state.quarantined_entry("v0.3.0").is_some());
}

#[test]
fn security_negative_cases_fail_closed_without_mutating_store_or_services() {
    let fx = E2eHarness::new(Platform::Linux, Architecture::X86_64);
    let (v1_pin, v1_bin) = fx.seed_initial_v1(false);
    let (v2_bundle, v2_sha, v2_size) = fx.write_signed_bundle("v0.2.0", 2);
    let fetcher = LocalDirectoryFetcher::new(&fx.mirror).unwrap();
    let mgr = FakePlatformServiceManager::new(
        ManagerFlavor::LinuxSystemd,
        &fx.service_id,
        "prod",
        v1_bin.clone(),
        true,
    );
    let drain = SequenceDrainProbe::new(0);
    let health = ScriptedHealthProbe::always_ok();

    // 1. Forged pointer signature fails closed.
    let pull = fx.write_signed_pointer(
        "v0.2.0",
        2,
        &format!("file://{}", v2_bundle.display()),
        &v2_sha,
        v2_size,
    );
    let sig_path = fx.mirror.join("release-pointer.v1.sig");
    let mut bad_sig = fs::read(&sig_path).unwrap();
    bad_sig[0] ^= 0xff;
    fs::write(&sig_path, bad_sig).unwrap();
    assert!(run_auto_update_with_policy(
        &fx.update_config(pull, Duration::from_secs(1), false),
        &AutoUpdatePorts {
            fetcher: &fetcher,
            manager: &mgr,
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        false,
    )
    .is_err());

    // 2. Sequence downgrade / conflict replay (same seq 1 with different release_id) returns DowngradeRejected.
    let pull_replay = fx.write_signed_pointer(
        "v0.1.0-evil",
        1,
        &format!("file://{}", v2_bundle.display()),
        &v2_sha,
        v2_size,
    );
    let replay = run_auto_update_with_policy(
        &fx.update_config(pull_replay, Duration::from_secs(1), false),
        &AutoUpdatePorts {
            fetcher: &fetcher,
            manager: &mgr,
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        false,
    )
    .unwrap();
    assert_eq!(replay.status, AutoUpdateStatus::DowngradeRejected);

    // 3. Tampered bundle SHA-256 fails closed before stopping services.
    let pull_bad_hash = fx.write_signed_pointer(
        "v0.2.0",
        2,
        &format!("file://{}", v2_bundle.display()),
        &"f".repeat(64),
        v2_size,
    );
    assert!(run_auto_update_with_policy(
        &fx.update_config(pull_bad_hash, Duration::from_secs(1), false),
        &AutoUpdatePorts {
            fetcher: &fetcher,
            manager: &mgr,
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        false,
    )
    .is_err());

    // 4. Symlink trap inside mirror directory fails closed.
    let link_bundle = fx.mirror.join("symlink-bundle.tar.gz");
    symlink(&v2_bundle, &link_bundle).unwrap();
    let pull_symlink = fx.write_signed_pointer(
        "v0.2.0",
        2,
        &format!("file://{}", link_bundle.display()),
        &v2_sha,
        v2_size,
    );
    assert!(run_auto_update_with_policy(
        &fx.update_config(pull_symlink, Duration::from_secs(1), false),
        &AutoUpdatePorts {
            fetcher: &fetcher,
            manager: &mgr,
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        false,
    )
    .is_err());

    // 5. Duplicate JSON keys in signed pointer fail closed.
    let dup_json = format!(
        r#"{{"format":"{POINTER_FORMAT}","channel":"stable","release_id":"v0.2.0","sequence":2,"\u0073equence":2,"installer_contract":1,"key_id":"{}","published_at":"2026-11-27T23:00:00Z","targets":[{{"platform":"linux","architecture":"x86_64","url":"file://{}","sha256":"{}","size":{}}}]}}"#,
        fx.key_id,
        v2_bundle.display(),
        v2_sha,
        v2_size
    );
    let dup_ptr = fx.mirror.join("dup-pointer.v1.json");
    let dup_sig = fx.mirror.join("dup-pointer.v1.sig");
    fs::write(&dup_ptr, dup_json.as_bytes()).unwrap();
    fs::write(&dup_sig, fx.key.sign(dup_json.as_bytes()).as_ref()).unwrap();
    let pull_dup = PullConfig::with_url_policy(
        format!("file://{}", dup_ptr.display()),
        None,
        UrlPolicy::AllowLocalForTesting,
    );
    assert!(run_auto_update_with_policy(
        &fx.update_config(pull_dup, Duration::from_secs(1), false),
        &AutoUpdatePorts {
            fetcher: &fetcher,
            manager: &mgr,
            drain: &drain,
            health: &health,
        },
        &fx.policy,
        &fx.keys,
        false,
    )
    .is_err());

    // 6. HTTP (non-HTTPS) production URL rejected by compiled CLI (`release check` & `release update`).
    let http_check = fx.run_cli(&[
        "release",
        "check",
        "--pointer-url",
        "http://releases.example.com/release-pointer.v1.json",
        "--store",
        fx.store.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !http_check.status.success(),
        "production CLI must reject non-HTTPS pointer URL"
    );
    let file_update = fx.run_cli(&[
        "release",
        "update",
        "--pointer-url",
        "file:///tmp/release-pointer.v1.json",
        "--store",
        fx.store.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !file_update.status.success(),
        "production CLI must reject file:// pointer URL"
    );

    // 7. Dry-run rollback works cleanly through the real CLI binary without mutating the store.
    let dry_rb = fx.run_cli(&[
        "--dry-run",
        "--json",
        "release",
        "rollback",
        "--store",
        fx.store.to_str().unwrap(),
        "--state-dir",
        fx.state_dir.to_str().unwrap(),
    ]);
    assert_eq!(
        dry_rb.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&dry_rb.stdout),
        String::from_utf8_lossy(&dry_rb.stderr)
    );

    assert!(
        mgr.events().is_empty(),
        "no negative test may disrupt the running service: {:?}",
        mgr.events()
    );
    assert_eq!(mgr.program(), v1_bin);
    let mut store =
        ReleaseStore::open_with_policy(&fx.store, fx.policy.clone(), fx.keys.clone()).unwrap();
    assert_eq!(store.recover().unwrap(), Some(v1_pin));
}

#[test]
fn python_release_tool_pointer_output_verifies_strictly_with_rust_org_release() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let keys_dir = root.join("keys");
    let bin_dir = root.join("bins");
    let ui_dir = root.join("ui");
    let out_dir = root.join("artifacts");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&ui_dir).unwrap();
    fs::create_dir_all(&out_dir).unwrap();

    for exe in org_release::EXECUTABLE_PATHS {
        fs::write(
            bin_dir.join(exe),
            synthetic_elf_binary(Architecture::X86_64, exe.as_bytes()),
        )
        .unwrap();
    }
    fs::write(ui_dir.join("index.html"), b"<html>ok</html>").unwrap();

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let tool = repo_root.join("scripts/release_tool.py");
    let pointer_sh = repo_root.join("scripts/generate-release-pointer.sh");

    let keygen = Command::new("python3")
        .arg(&tool)
        .args(["keygen", "--out-dir", keys_dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        keygen.status.success(),
        "{}",
        String::from_utf8_lossy(&keygen.stderr)
    );

    for arch in ["x86_64", "aarch64"] {
        let pkg = Command::new("python3")
            .arg(&tool)
            .args([
                "package",
                "--bin-dir",
                bin_dir.to_str().unwrap(),
                "--ui-dir",
                ui_dir.to_str().unwrap(),
                "--release-id",
                "v0.9.0",
                "--sequence",
                "7",
                "--platform",
                "linux",
                "--arch",
                arch,
                "--out-dir",
                out_dir.to_str().unwrap(),
                "--signing-key",
                keys_dir.join("publisher.key.pem").to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            pkg.status.success(),
            "{}",
            String::from_utf8_lossy(&pkg.stderr)
        );
    }

    let gen_ptr = Command::new("bash")
        .arg(&pointer_sh)
        .args([
            "--artifacts-dir",
            out_dir.to_str().unwrap(),
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--release-id",
            "v0.9.0",
            "--sequence",
            "7",
            "--channel",
            "stable",
            "--base-url",
            "https://github.com/vgmello/agentic-org/releases/download/v0.9.0",
            "--signing-key",
            keys_dir.join("publisher.key.pem").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        gen_ptr.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&gen_ptr.stdout),
        String::from_utf8_lossy(&gen_ptr.stderr)
    );

    // Verify CI-generated pointer bytes & 64-byte signature directly with Rust `org-release`!
    let reg_bytes = fs::read(keys_dir.join("release-trust-registry.json")).unwrap();
    let ptr_bytes = fs::read(out_dir.join("release-pointer.v1.json")).unwrap();
    let sig_bytes = fs::read(out_dir.join("release-pointer.v1.sig")).unwrap();
    let trusted = TrustedKeys::from_registry_bytes(&reg_bytes).unwrap();

    for arch in [Architecture::X86_64, Architecture::Aarch64] {
        let policy = TrustedPolicy::new(Platform::Linux, arch, 1, 1).unwrap();
        let verified = verify_pointer(
            &ptr_bytes,
            &sig_bytes,
            &policy,
            &trusted,
            UrlPolicy::HttpsOnly,
        )
        .unwrap();
        assert_eq!(verified.pointer().release_id(), "v0.9.0");
        assert_eq!(verified.pointer().sequence(), 7);
        assert_eq!(verified.pointer().channel(), "stable");
        assert_eq!(verified.pointer().target().architecture(), arch);
        assert!(verified
            .pointer()
            .target()
            .url()
            .starts_with("https://github.com/vgmello/agentic-org/releases/download/v0.9.0/"));
    }
}
