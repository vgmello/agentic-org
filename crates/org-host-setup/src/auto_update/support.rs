//! Shared auto-update configuration, receipt contracts, read-only pin inspection, and policy
//! adapters used across forward updates, crash recovery, and rollback.

use std::fs;
use std::io::Read as _;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use org_host::release::{
    check_pointer, stage_verified_pointer, PointerCheck, PointerCheckState, PullConfig, PullError,
    ReleaseFetcher, ReleasePin, ReleaseStore, StagedRelease,
};
#[cfg(any(test, feature = "test-support"))]
use org_host::release::{check_pointer_with_policy, stage_verified_pointer_with_policy};
use org_platform::service::ServiceManager;
#[cfg(any(test, feature = "test-support"))]
use org_release::{TrustedKeys, TrustedPolicy};
use serde::Serialize;

use super::drain::{DrainConfig, DrainProbe, DrainReceipt};
use super::health::{HealthProbe, InstanceHealthReport};
use crate::service_refresh::{InstallAttempt, ServiceRefresh, ServiceRefreshReport};

/// Default post-restart health probe deadline.
pub const DEFAULT_HEALTH_TIMEOUT: Duration = Duration::from_secs(60);

const MAX_PIN_RECORD_BYTES: u64 = 16 * 1024;

/// Parameters governing one pull-verified auto-update run.
#[derive(Debug, Clone)]
pub struct AutoUpdateConfig {
    pub store_dir: PathBuf,
    pub state_dir: PathBuf,
    pub setup_base: Option<PathBuf>,
    pub pull: PullConfig,
    pub drain: DrainConfig,
    pub health_timeout: Duration,
    pub restart: bool,
    pub dry_run: bool,
    pub allow_quarantined: bool,
    pub create_default_store_parent: bool,
}

/// Injectable consumer-owned ports for auto-update execution.
pub struct AutoUpdatePorts<'a> {
    pub fetcher: &'a dyn ReleaseFetcher,
    pub manager: &'a dyn ServiceManager,
    pub drain: &'a dyn DrainProbe,
    pub health: &'a dyn HealthProbe,
}

/// Terminal outcome of an auto-update attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoUpdateStatus {
    /// Already running the release named by the signed pointer.
    UpToDate,
    /// Preview only (`--dry-run`); nothing was downloaded, stopped, or pinned.
    DryRun,
    /// New release staged, pinned, restarted, and confirmed healthy.
    Updated,
    /// Active agent turns did not drain within timeout; running release untouched.
    BusyTimeout,
    /// Pointer release is quarantined after a prior failed health check; skipped safely.
    Quarantined,
    /// Pointer sequence is behind or conflicts with active/highest sequence; refused.
    DowngradeRejected,
    /// Post-restart health check failed; automatically rolled back and quarantined broken release.
    RolledBack,
    /// Post-restart health check failed AND rollback could not fully restore service health.
    RollbackFailed,
}

/// Structured receipt returned by [`super::run_auto_update`].
#[derive(Debug, Clone, Serialize)]
pub struct AutoUpdateReceipt {
    pub status: AutoUpdateStatus,
    pub store: String,
    pub state_dir: String,
    pub channel: String,
    pub pointer_release_id: String,
    pub pointer_sequence: u64,
    pub previous_pin: Option<ReleasePin>,
    pub active_pin: Option<ReleasePin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drain: Option<DrainReceipt>,
    pub services: Vec<ServiceRefreshReport>,
    pub health_checks: Vec<InstanceHealthReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rollback_services: Vec<ServiceRefreshReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rollback_health_checks: Vec<InstanceHealthReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quarantined_release_id: Option<String>,
    pub note: String,
}

/// Reads the currently pinned release and highest sequence from `store_dir` without mutating it.
///
/// Works even when a running host holds `store.lock` by inspecting the latest committed pin record
/// read-only via `open_regular_beneath`, refusing symlinked `store_dir` or `pins` directories.
pub fn read_store_head_readonly(store_dir: &Path) -> Result<(Option<ReleasePin>, u64), String> {
    let norm_store: PathBuf = store_dir.components().collect();
    if !verify_real_dir_if_present(&norm_store, "release store directory")? {
        return Ok((None, 0));
    }
    let pins_dir = norm_store.join("pins");
    if !verify_real_dir_if_present(&pins_dir, "release store pins directory")? {
        return Ok((None, 0));
    }
    let Some(best_name) = find_latest_pin_filename(&pins_dir)? else {
        return Ok((None, 0));
    };
    let file = org_platform::fs::open_regular_beneath(&pins_dir, Path::new(&best_name))
        .map_err(|e| format!("opening latest pin record safely: {e}"))?;
    parse_pin_record(file)
}

fn verify_real_dir_if_present(path: &Path, label: &str) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        _ => Err(format!("{label} is unreadable or unsafe")),
    }
}

fn find_latest_pin_filename(pins_dir: &Path) -> Result<Option<String>, String> {
    let mut best: Option<(u64, String)> = None;
    let entries =
        fs::read_dir(pins_dir).map_err(|e| format!("reading release pins directory: {e}"))?;
    for entry_res in entries {
        let entry = entry_res.map_err(|e| format!("iterating release pins directory: {e}"))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(ord) = parse_pin_ordinal(&name) else {
            continue;
        };
        if best.as_ref().is_none_or(|(cur, _)| ord > *cur) {
            best = Some((ord, name));
        }
    }
    Ok(best.map(|(_, name)| name))
}

fn parse_pin_ordinal(name: &str) -> Option<u64> {
    if name.starts_with(".pending-") {
        return None;
    }
    name.strip_suffix(".json")
        .filter(|s| s.len() == 20 && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0)
}

fn parse_pin_record(file: fs::File) -> Result<(Option<ReleasePin>, u64), String> {
    let mut buf = Vec::new();
    std::io::Read::take(file, MAX_PIN_RECORD_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("reading latest pin record: {e}"))?;
    if buf.len() as u64 > MAX_PIN_RECORD_BYTES {
        return Err("latest pin record exceeds size limit".into());
    }
    #[derive(serde::Deserialize)]
    struct CommitMini {
        version: u32,
        highest_sequence: u64,
        pin: ReleasePin,
    }
    let commit: CommitMini =
        serde_json::from_slice(&buf).map_err(|_| "latest pin record is malformed".to_owned())?;
    if commit.version != 1 {
        return Err("unsupported pin record version".into());
    }
    commit
        .pin
        .validate()
        .map_err(|e| format!("invalid pin record: {e}"))?;
    Ok((Some(commit.pin), commit.highest_sequence))
}

#[cfg(any(test, feature = "test-support"))]
struct ExplicitPolicy<'a> {
    policy: &'a TrustedPolicy,
    keys: &'a TrustedKeys,
    skip_platform_check: bool,
}

pub(super) struct PolicyMode<'a> {
    #[cfg(any(test, feature = "test-support"))]
    explicit: Option<ExplicitPolicy<'a>>,
    _marker: PhantomData<&'a ()>,
}

impl<'a> PolicyMode<'a> {
    pub(super) const fn embedded() -> Self {
        Self {
            #[cfg(any(test, feature = "test-support"))]
            explicit: None,
            _marker: PhantomData,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) const fn explicit(
        policy: &'a TrustedPolicy,
        keys: &'a TrustedKeys,
        skip_platform_check: bool,
    ) -> Self {
        Self {
            explicit: Some(ExplicitPolicy {
                policy,
                keys,
                skip_platform_check,
            }),
            _marker: PhantomData,
        }
    }

    pub(super) fn check_pointer(
        &self,
        fetcher: &dyn ReleaseFetcher,
        pull: &PullConfig,
        state: &PointerCheckState<'_>,
    ) -> Result<PointerCheck, PullError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(exp) = &self.explicit {
            return check_pointer_with_policy(fetcher, pull, exp.policy, exp.keys, state);
        }
        check_pointer(fetcher, pull, state)
    }

    pub(super) fn stage_bundle(
        &self,
        fetcher: &dyn ReleaseFetcher,
        pull: &PullConfig,
        check: PointerCheck,
        staging_parent: &Path,
    ) -> Result<StagedRelease, PullError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(exp) = &self.explicit {
            return stage_verified_pointer_with_policy(
                fetcher,
                pull,
                check.verified,
                staging_parent,
                exp.policy,
                exp.keys,
            );
        }
        stage_verified_pointer(fetcher, pull, check.verified, staging_parent)
    }

    pub(super) fn open_release_store(
        &self,
        store_dir: &Path,
    ) -> Result<ReleaseStore, org_host::release::Error> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(exp) = &self.explicit {
            return ReleaseStore::open_with_policy(store_dir, exp.policy.clone(), exp.keys.clone());
        }
        ReleaseStore::open(store_dir)
    }

    pub(super) fn install_archive(
        &self,
        store: &mut ReleaseStore,
        staged: &mut StagedRelease,
    ) -> Result<ReleasePin, org_host::release::Error> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(exp) = &self.explicit {
            if exp.skip_platform_check {
                return store.install_archive_without_platform_check(staged.bundle_mut());
            }
        }
        store.install_archive(staged.bundle_mut())
    }

    pub(super) fn recover_store(
        &self,
        store: &mut ReleaseStore,
    ) -> Result<Option<ReleasePin>, org_host::release::Error> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(exp) = &self.explicit {
            if exp.skip_platform_check {
                return store.recover_without_platform_check();
            }
        }
        store.recover()
    }

    pub(super) fn rollback_store(
        &self,
        store: &mut ReleaseStore,
        target: &ReleasePin,
    ) -> Result<ReleasePin, org_host::release::Error> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(exp) = &self.explicit {
            if exp.skip_platform_check {
                return store.rollback_to_without_platform_check(target);
            }
        }
        store.rollback_to(target)
    }
}

pub(super) fn open_store(
    store_dir: &Path,
    mode: &PolicyMode<'_>,
) -> Result<ReleaseStore, InstallAttempt> {
    mode.open_release_store(store_dir).map_err(|e| {
        let why = format!("opening release store ({}): {e}", store_dir.display());
        if matches!(e, org_host::release::Error::Busy) {
            InstallAttempt::Busy(why)
        } else {
            InstallAttempt::Failed(why)
        }
    })
}

pub(super) fn inspect_services(
    manager: &dyn ServiceManager,
    explicit_base: Option<&Path>,
    store_dir: &Path,
) -> Result<ServiceRefresh, String> {
    match explicit_base {
        Some(base) => Ok(ServiceRefresh::inspect_at(manager, base, store_dir)),
        None => ServiceRefresh::inspect(manager, store_dir),
    }
}

pub(super) fn ensure_no_inspection_failures(services: &ServiceRefresh) -> Result<(), String> {
    let failures = services.inspection_failures();
    if failures.is_empty() {
        return Ok(());
    }
    let detail = failures
        .into_iter()
        .map(|(instance, why)| format!("{instance}: {why}"))
        .collect::<Vec<_>>()
        .join("; ");
    Err(format!(
        "refusing release swap because registered service inspection failed ({detail})"
    ))
}

pub(super) fn prepare_store_parent_if_default(
    store_dir: &Path,
    is_default: bool,
) -> Result<(), String> {
    if !is_default {
        return Ok(());
    }
    if let Some(parent) = store_dir.parent() {
        org_platform::fs::create_owner_only_dir_all(parent)
            .and_then(|()| org_platform::fs::set_owner_only_dir(parent))
            .map_err(|e| {
                format!(
                    "creating release store parent owner-only ({}): {e}",
                    parent.display()
                )
            })?;
    }
    Ok(())
}

pub(super) fn release_program(store_dir: &Path, release_id: &str) -> Result<PathBuf, String> {
    let program = store_dir
        .join("releases")
        .join(release_id)
        .join("agentic-org");
    fs::canonicalize(&program)
        .map_err(|e| format!("locating installed `{}`: {e}", program.display()))
}

pub(super) fn pin_from_staged(staged: &StagedRelease) -> ReleasePin {
    let verified = staged.bundle().manifest();
    ReleasePin {
        release_id: verified.manifest().release_id().to_owned(),
        manifest_sha256: verified
            .manifest_sha256()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        sequence: verified.manifest().sequence(),
    }
}

/// Resolves this account's setup base directory for constructing [`super::DiskActiveTurnDrainProbe`].
pub fn account_setup_base() -> Result<Option<PathBuf>, String> {
    ServiceRefresh::account_setup_base()
}
