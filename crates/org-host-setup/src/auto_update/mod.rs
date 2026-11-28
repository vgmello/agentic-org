//! Pull-verified, idle-drained auto-update & automatic rollback coordinator (ADR-0009).
//!
//! Coordinates:
//! 1. Crash recovery of any unresolved `pending_health_check` checkpoint ([`recovery`]);
//! 2. Signed pointer verification (`org-release` / `org-host-release::pull`) + quarantine check;
//! 3. Bounded streaming download & ADR-0009 archive verification BEFORE touching services;
//! 4. Pre-swap idle drain of active agent turns ([`drain::DrainProbe`]);
//! 5. Durable atomic `pending_health_check` checkpoint ([`UpdateStateStore`]);
//! 6. Service stop -> `ReleaseStore` atomic pin -> `ServiceManager::redefine` + restart;
//! 7. Fail-closed post-restart health check ([`health::HealthProbe`]);
//! 8. Automatic rollback to the previous known-good pin + quarantine tombstone on failure.

pub mod drain;
pub mod health;
mod recovery;
pub mod rollback;
mod support;

use std::path::Path;

use org_host::release::{
    PointerCheck, PointerCheckState, PointerDecision, ReleasePin, StagedRelease, UpdateStateStore,
};
#[cfg(any(test, feature = "test-support"))]
use org_release::{TrustedKeys, TrustedPolicy};

use crate::service_refresh::{InstallAttempt, ServiceRefresh, ServiceRefreshReport};
use drain::{wait_for_idle_drain, DrainPhase, DrainReceipt};
use health::{evaluate_post_restart_health, summarize_health_failures, InstanceHealthReport};
use recovery::{reconcile_pending_health_check, RecoveryOutcome};
use rollback::{execute_automatic_rollback, AutoRollbackContext};
use support::{
    ensure_no_inspection_failures, inspect_services, open_store, pin_from_staged,
    prepare_store_parent_if_default, release_program, PolicyMode,
};

pub use drain::{ActiveTurnSnapshot, DiskActiveTurnDrainProbe};
pub use health::HostSocketHealthProbe;
#[cfg(any(test, feature = "test-support"))]
pub use rollback::run_manual_rollback_with_policy;
pub use rollback::{
    run_manual_rollback, RollbackConfig, RollbackPorts, RollbackReceipt, RollbackStatus,
};
pub use support::{
    account_setup_base, read_store_head_readonly, AutoUpdateConfig, AutoUpdatePorts,
    AutoUpdateReceipt, AutoUpdateStatus, DEFAULT_HEALTH_TIMEOUT,
};

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
    // 0. Reconcile any interrupted update checkpoint BEFORE consulting a new signed pointer.
    let initial_state = UpdateStateStore::read_readonly(&config.state_dir)
        .map_err(|e| format!("reading update state ({}): {e}", config.state_dir.display()))?;
    let recovered = match reconcile_pending_health_check(config, ports, &mode, &initial_state)? {
        RecoveryOutcome::NoPending => None,
        RecoveryOutcome::DryRunNotice(receipt) | RecoveryOutcome::RolledBack(receipt) => {
            return Ok(receipt);
        }
        RecoveryOutcome::RecoveredHealthy {
            health_checks,
            note,
        } => Some((health_checks, note)),
    };

    let (current_pin, highest_sequence) = read_store_head_readonly(&config.store_dir)?;
    let update_state = UpdateStateStore::read_readonly(&config.state_dir)
        .map_err(|e| format!("reading update state ({}): {e}", config.state_dir.display()))?;
    let check_state = PointerCheckState {
        current_pin: current_pin.as_ref(),
        highest_sequence,
        update_state: &update_state,
        allow_quarantined: config.allow_quarantined,
    };
    let check = mode
        .check_pointer(ports.fetcher, &config.pull, &check_state)
        .map_err(|e| format!("verifying release pointer: {e}"))?;
    let summary = PointerSummary::from_check(&check);
    if let Some(mut early) =
        early_decision_receipt(config, &summary, &check.decision, current_pin.as_ref())
    {
        if let Some((checks, rec_note)) = recovered {
            early.health_checks = checks;
            early.note = format!("{rec_note}; {}", early.note);
        }
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
    ensure_no_inspection_failures(&services)?;
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
    let expected_running: Vec<String> = managed_before
        .iter()
        .filter(|m| m.was_running)
        .map(|m| m.instance.clone())
        .collect();
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
        .record_pending_with_services(
            swap.current_pin.clone(),
            target_pin,
            Some(config.pull.pointer_url.clone()),
            expected_running,
        )
        .map_err(|e| format!("persisting pre-swap rollback checkpoint: {e}"))?;

    // 4. Stop running services (only if store is busy), commit new pin, and move services to it.
    let committed_pin = match swap.services.install(ports.manager, || {
        install_staged_once(&config.store_dir, &mut swap.staged, mode)
    }) {
        Ok(pin) => pin,
        Err(err) => {
            if let Err(clear_err) = state_store.clear_pending() {
                return Err(format!(
                    "{err} (additionally failed to clear pending checkpoint: {clear_err})"
                ));
            }
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
            drain_receipt: Some(swap.drain_receipt),
            managed_before,
            forward_services: refresh_reports,
            forward_health: health_reports,
            failure_summary,
        },
    )
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

fn install_staged_once(
    store_dir: &Path,
    staged: &mut StagedRelease,
    mode: &PolicyMode<'_>,
) -> Result<ReleasePin, InstallAttempt> {
    let mut store = open_store(store_dir, mode)?;
    mode.install_archive(&mut store, staged)
        .map_err(|e| InstallAttempt::Failed(format!("committing verified release archive: {e}")))
}

#[cfg(test)]
mod tests;
