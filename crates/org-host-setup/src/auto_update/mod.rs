//! Pull-verified, idle-drained auto-update & automatic rollback coordinator (ADR-0009).
//!
//! Coordinates:
//! 1. Signed pointer verification (`org-release` / `org-host-release::pull`) + quarantine check;
//! 2. Bounded streaming download & ADR-0009 archive verification BEFORE touching services;
//! 3. Pre-swap idle drain of active agent turns ([`DrainProbe`]);
//! 4. Durable atomic `pending_health_check` checkpoint ([`UpdateStateStore`]);
//! 5. Service stop -> `ReleaseStore` atomic pin -> `ServiceManager::redefine` + restart;
//! 6. Post-restart health check ([`HealthProbe`]);
//! 7. Automatic rollback to the previous known-good pin + quarantine tombstone on failure.

pub mod drain;
pub mod health;
pub mod rollback;

use std::fs;
use std::io::Read as _;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use org_host::release::{
    check_pointer, stage_verified_pointer, PointerCheck, PointerDecision, PullConfig, PullError,
    ReleaseFetcher, ReleasePin, ReleaseStore, StagedRelease, UpdateState, UpdateStateStore,
};
#[cfg(any(test, feature = "test-support"))]
use org_host::release::{check_pointer_with_policy, stage_verified_pointer_with_policy};
use org_platform::service::ServiceManager;
#[cfg(any(test, feature = "test-support"))]
use org_release::{TrustedKeys, TrustedPolicy};
use serde::Serialize;

use crate::service_refresh::{
    InstallAttempt, ManagedServiceInfo, ServiceRefresh, ServiceRefreshReport,
};
use drain::{wait_for_idle_drain, DrainConfig, DrainPhase, DrainProbe, DrainReceipt};
use health::{probe_instances, HealthProbe, HealthTarget, InstanceHealthReport};
use rollback::{execute_automatic_rollback, AutoRollbackContext};

pub use drain::{ActiveTurnSnapshot, DiskActiveTurnDrainProbe};
pub use health::HostSocketHealthProbe;
#[cfg(any(test, feature = "test-support"))]
pub use rollback::run_manual_rollback_with_policy;
pub use rollback::{
    run_manual_rollback, RollbackConfig, RollbackPorts, RollbackReceipt, RollbackStatus,
};

/// Default post-restart health probe deadline.
pub const DEFAULT_HEALTH_TIMEOUT: Duration = Duration::from_secs(60);

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

/// Structured receipt returned by [`run_auto_update`].
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
/// read-only via `open_nofollow_ro`.
pub fn read_store_head_readonly(store_dir: &Path) -> Result<(Option<ReleasePin>, u64), String> {
    if !store_dir.exists() {
        return Ok((None, 0));
    }
    let pins_dir = store_dir.join("pins");
    let meta = match fs::symlink_metadata(&pins_dir) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((None, 0)),
        _ => return Err("release store pins directory is unreadable or unsafe".into()),
    };
    let _ = meta;
    let mut best: Option<(u64, PathBuf)> = None;
    let entries =
        fs::read_dir(&pins_dir).map_err(|e| format!("reading release pins directory: {e}"))?;
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.starts_with(".pending-") {
            continue;
        }
        let Some(ord) = name
            .strip_suffix(".json")
            .filter(|s| s.len() == 20 && s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&n| n > 0)
        else {
            continue;
        };
        if best.as_ref().is_none_or(|(cur, _)| ord > *cur) {
            best = Some((ord, entry.path()));
        }
    }
    let Some((_, pin_path)) = best else {
        return Ok((None, 0));
    };
    let file = org_platform::fs::open_nofollow_ro(&pin_path)
        .map_err(|e| format!("opening latest pin record safely: {e}"))?;
    let mut buf = Vec::new();
    std::io::Read::take(file, 16 * 1024 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("reading latest pin record: {e}"))?;
    if buf.len() > 16 * 1024 {
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

    fn check_pointer(
        &self,
        fetcher: &dyn ReleaseFetcher,
        pull: &PullConfig,
        current_pin: Option<&ReleasePin>,
        highest_sequence: u64,
        update_state: &UpdateState,
        allow_quarantined: bool,
    ) -> Result<PointerCheck, PullError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(exp) = &self.explicit {
            return check_pointer_with_policy(
                fetcher,
                pull,
                exp.policy,
                exp.keys,
                current_pin,
                highest_sequence,
                update_state,
                allow_quarantined,
            );
        }
        check_pointer(
            fetcher,
            pull,
            current_pin,
            highest_sequence,
            update_state,
            allow_quarantined,
        )
    }

    fn stage_bundle(
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

    fn install_archive(
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

/// Execute the full pull-verified, idle-drained auto-update & automatic rollback workflow using
/// the build-embedded trust registry and freshness floor.
pub fn run_auto_update(
    config: &AutoUpdateConfig,
    ports: &AutoUpdatePorts<'_>,
) -> Result<AutoUpdateReceipt, String> {
    run_auto_update_inner(config, ports, PolicyMode::embedded())
}

/// Test-harness entry point supplying an explicit Ed25519 trust registry and policy.
#[cfg(any(test, feature = "test-support"))]
pub fn run_auto_update_with_policy(
    config: &AutoUpdateConfig,
    ports: &AutoUpdatePorts<'_>,
    policy: &TrustedPolicy,
    keys: &TrustedKeys,
    skip_platform_check: bool,
) -> Result<AutoUpdateReceipt, String> {
    run_auto_update_inner(
        config,
        ports,
        PolicyMode::explicit(policy, keys, skip_platform_check),
    )
}

struct PointerSummary {
    channel: String,
    release_id: String,
    sequence: u64,
}

impl PointerSummary {
    fn from_check(check: &PointerCheck) -> Self {
        let ptr = check.verified.pointer();
        Self {
            channel: ptr.channel().to_owned(),
            release_id: ptr.release_id().to_owned(),
            sequence: ptr.sequence(),
        }
    }

    fn base_receipt(
        &self,
        config: &AutoUpdateConfig,
        current_pin: Option<ReleasePin>,
        status: AutoUpdateStatus,
        note: String,
    ) -> AutoUpdateReceipt {
        AutoUpdateReceipt {
            status,
            store: config.store_dir.display().to_string(),
            state_dir: config.state_dir.display().to_string(),
            channel: self.channel.clone(),
            pointer_release_id: self.release_id.clone(),
            pointer_sequence: self.sequence,
            previous_pin: current_pin.clone(),
            active_pin: current_pin,
            drain: None,
            services: Vec::new(),
            health_checks: Vec::new(),
            rollback_services: Vec::new(),
            rollback_health_checks: Vec::new(),
            quarantined_release_id: None,
            note,
        }
    }
}

fn early_decision_receipt(
    config: &AutoUpdateConfig,
    summary: &PointerSummary,
    decision: &PointerDecision,
    current_pin: Option<&ReleasePin>,
) -> Option<AutoUpdateReceipt> {
    let pin = current_pin.cloned();
    match decision {
        PointerDecision::UpToDate => Some(summary.base_receipt(
            config,
            pin,
            AutoUpdateStatus::UpToDate,
            format!(
                "already running release `{}` (sequence {})",
                summary.release_id, summary.sequence
            ),
        )),
        PointerDecision::Quarantined { reason } => {
            let mut receipt = summary.base_receipt(
                config,
                pin,
                AutoUpdateStatus::Quarantined,
                format!(
                    "skipping quarantined release `{}` (sequence {}): {reason}",
                    summary.release_id, summary.sequence
                ),
            );
            receipt.quarantined_release_id = Some(summary.release_id.clone());
            Some(receipt)
        }
        PointerDecision::DowngradeRejected {
            current_sequence,
            pointer_sequence,
        } => Some(summary.base_receipt(
            config,
            pin,
            AutoUpdateStatus::DowngradeRejected,
            format!(
                "refusing downgrade/replay: pointer sequence {pointer_sequence} <= current/highest sequence {current_sequence}"
            ),
        )),
        PointerDecision::UpdateAvailable if config.dry_run => Some(summary.base_receipt(
            config,
            pin,
            AutoUpdateStatus::DryRun,
            format!(
                "would pull, verify, idle-drain, and activate release `{}` (sequence {})",
                summary.release_id, summary.sequence
            ),
        )),
        PointerDecision::UpdateAvailable => None,
    }
}

struct SwapInputs {
    channel: String,
    current_pin: Option<ReleasePin>,
    staged: StagedRelease,
    services: ServiceRefresh,
    drain_receipt: DrainReceipt,
}

fn run_auto_update_inner(
    config: &AutoUpdateConfig,
    ports: &AutoUpdatePorts<'_>,
    mode: PolicyMode<'_>,
) -> Result<AutoUpdateReceipt, String> {
    let (current_pin, highest_sequence) = read_store_head_readonly(&config.store_dir)?;
    let update_state = UpdateStateStore::read_readonly(&config.state_dir)
        .map_err(|e| format!("reading update state ({}): {e}", config.state_dir.display()))?;
    let check = mode
        .check_pointer(
            ports.fetcher,
            &config.pull,
            current_pin.as_ref(),
            highest_sequence,
            &update_state,
            config.allow_quarantined,
        )
        .map_err(|e| format!("verifying release pointer: {e}"))?;
    let summary = PointerSummary::from_check(&check);
    if let Some(early) =
        early_decision_receipt(config, &summary, &check.decision, current_pin.as_ref())
    {
        return Ok(early);
    }

    // 1. Pull and fully verify the signed bundle in an isolated 0700 temp folder BEFORE touching
    //    any running service or the active release pin.
    let staging_parent = std::env::temp_dir()
        .canonicalize()
        .map_err(|e| format!("resolving staging temp directory: {e}"))?;
    let staged = mode
        .stage_bundle(ports.fetcher, &config.pull, check, &staging_parent)
        .map_err(|e| format!("staging verified release bundle: {e}"))?;

    // 2. Inspect registered services and drain active agent turns before stopping anything.
    let services = inspect_services(
        ports.manager,
        config.setup_base.as_deref(),
        &config.store_dir,
    )?;
    let drain_receipt = wait_for_idle_drain(ports.drain, config.drain)?;
    if drain_receipt.phase == DrainPhase::BusyTimeout {
        let mut receipt = summary.base_receipt(
            config,
            current_pin,
            AutoUpdateStatus::BusyTimeout,
            format!(
                "update deferred: {} active agent turn(s) did not finish within {}s; running release untouched",
                drain_receipt.remaining_turns.len(),
                config.drain.timeout.as_secs()
            ),
        );
        receipt.drain = Some(drain_receipt);
        return Ok(receipt);
    }

    apply_and_verify_update(
        config,
        ports,
        &mode,
        SwapInputs {
            channel: summary.channel,
            current_pin,
            staged,
            services,
            drain_receipt,
        },
    )
}

fn apply_and_verify_update(
    config: &AutoUpdateConfig,
    ports: &AutoUpdatePorts<'_>,
    mode: &PolicyMode<'_>,
    mut swap: SwapInputs,
) -> Result<AutoUpdateReceipt, String> {
    let managed_before = swap.services.managed_targets();
    let target_pin = pin_from_staged(&swap.staged);

    // 3. Prepare store parent (only for default account store) and persist durable atomic
    //    `pending_health_check` checkpoint BEFORE stopping services or moving the active pin.
    prepare_store_parent_if_default(&config.store_dir, config.create_default_store_parent)?;
    let state_store = UpdateStateStore::open(&config.state_dir).map_err(|e| {
        format!(
            "opening update state store ({}): {e}",
            config.state_dir.display()
        )
    })?;
    state_store
        .record_pending(
            swap.current_pin.clone(),
            target_pin,
            Some(config.pull.pointer_url.clone()),
        )
        .map_err(|e| format!("persisting pre-swap rollback checkpoint: {e}"))?;

    // 4. Stop running services (only if store is busy), commit new pin, and move services to it.
    let committed_pin = match swap.services.install(ports.manager, || {
        install_staged_once(&config.store_dir, &mut swap.staged, mode)
    }) {
        Ok(pin) => pin,
        Err(err) => {
            let _ = state_store.clear_pending();
            return Err(err);
        }
    };
    let target_program = release_program(&config.store_dir, &committed_pin.release_id)?;
    let refresh_reports = swap
        .services
        .refresh(ports.manager, &target_program, config.restart);

    // 5. Post-restart health check across every restarted managed instance.
    let (health_ok, health_reports) = evaluate_post_restart_health(
        ports.health,
        &managed_before,
        &refresh_reports,
        config.restart,
        config.health_timeout,
    );
    if health_ok {
        state_store
            .confirm_healthy(&committed_pin)
            .map_err(|e| format!("committing confirmed healthy update state: {e}"))?;
        return Ok(confirmed_receipt(
            config,
            swap.channel,
            swap.current_pin,
            committed_pin,
            swap.drain_receipt,
            refresh_reports,
            health_reports,
        ));
    }

    // 6. Post-restart health check FAILED -> AUTOMATIC ROLLBACK + QUARANTINE!
    let failure_summary = summarize_health_failures(&health_reports);
    execute_automatic_rollback(
        config,
        ports,
        mode,
        &state_store,
        AutoRollbackContext {
            previous_pin: swap.current_pin,
            failed_pin: committed_pin,
            channel: swap.channel,
            drain_receipt: swap.drain_receipt,
            managed_before,
            forward_services: refresh_reports,
            forward_health: health_reports,
            failure_summary,
        },
    )
}

fn evaluate_post_restart_health(
    health: &dyn HealthProbe,
    managed_before: &[ManagedServiceInfo],
    refresh_reports: &[ServiceRefreshReport],
    restart: bool,
    health_timeout: Duration,
) -> (bool, Vec<InstanceHealthReport>) {
    let restart_failed = restart
        && refresh_reports.iter().any(|r| {
            managed_before
                .iter()
                .any(|m| m.instance == r.instance && m.was_running)
                && !r.running
        });
    if restart_failed {
        return (
            false,
            vec![InstanceHealthReport {
                instance: "service-restart".into(),
                healthy: false,
                detail: "one or more running services failed to restart on the new release".into(),
            }],
        );
    }
    let targets = health_targets_for(managed_before, refresh_reports, restart);
    if targets.is_empty() {
        (true, Vec::new())
    } else {
        probe_instances(health, &targets, health_timeout)
    }
}

fn confirmed_receipt(
    config: &AutoUpdateConfig,
    channel: String,
    previous_pin: Option<ReleasePin>,
    committed_pin: ReleasePin,
    drain_receipt: DrainReceipt,
    services: Vec<ServiceRefreshReport>,
    health_checks: Vec<InstanceHealthReport>,
) -> AutoUpdateReceipt {
    let note = format!(
        "updated to release `{}` (sequence {}) and verified post-restart health",
        committed_pin.release_id, committed_pin.sequence
    );
    AutoUpdateReceipt {
        status: AutoUpdateStatus::Updated,
        store: config.store_dir.display().to_string(),
        state_dir: config.state_dir.display().to_string(),
        channel,
        pointer_release_id: committed_pin.release_id.clone(),
        pointer_sequence: committed_pin.sequence,
        previous_pin,
        active_pin: Some(committed_pin),
        drain: Some(drain_receipt),
        services,
        health_checks,
        rollback_services: Vec::new(),
        rollback_health_checks: Vec::new(),
        quarantined_release_id: None,
        note,
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

fn install_staged_once(
    store_dir: &Path,
    staged: &mut StagedRelease,
    mode: &PolicyMode<'_>,
) -> Result<ReleasePin, InstallAttempt> {
    let mut store = open_store(store_dir, mode)?;
    mode.install_archive(&mut store, staged)
        .map_err(|e| InstallAttempt::Failed(format!("committing verified release archive: {e}")))
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

fn prepare_store_parent_if_default(store_dir: &Path, is_default: bool) -> Result<(), String> {
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

fn pin_from_staged(staged: &StagedRelease) -> ReleasePin {
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

pub(super) fn health_targets_for(
    managed_before: &[ManagedServiceInfo],
    reports: &[ServiceRefreshReport],
    restart: bool,
) -> Vec<HealthTarget> {
    if !restart {
        return Vec::new();
    }
    managed_before
        .iter()
        .filter(|m| {
            m.was_running
                && reports
                    .iter()
                    .any(|r| r.instance == m.instance && r.running)
        })
        .map(|m| HealthTarget {
            instance: m.instance.clone(),
            instance_id: m.instance_id.clone(),
            root: m.root.clone(),
        })
        .collect()
}

fn summarize_health_failures(reports: &[InstanceHealthReport]) -> String {
    let failed: Vec<String> = reports
        .iter()
        .filter(|r| !r.healthy)
        .map(|r| format!("{}: {}", r.instance, r.detail))
        .collect();
    if failed.is_empty() {
        "post-restart health check failed".into()
    } else {
        failed.join("; ")
    }
}

/// Resolves this account's setup base directory for constructing [`DiskActiveTurnDrainProbe`].
pub fn account_setup_base() -> Result<Option<PathBuf>, String> {
    ServiceRefresh::account_setup_base()
}

#[cfg(test)]
mod tests;
