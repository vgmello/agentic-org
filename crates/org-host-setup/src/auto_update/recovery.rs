//! Mid-update crash recovery reconciling any persisted `pending_health_check` checkpoint
//! before checking a new release pointer (ADR-0009, P0-1).

use std::fs;
use std::path::{Path, PathBuf};

use org_host::release::{PendingHealthCheck, ReleasePin, UpdateState, UpdateStateStore};

use super::health::{
    evaluate_post_restart_health, probe_instances, summarize_health_failures, HealthTarget,
    InstanceHealthReport,
};
use super::rollback::{execute_automatic_rollback, AutoRollbackContext};
use super::support::{
    inspect_services, read_store_head_readonly, release_program, AutoUpdateConfig, AutoUpdatePorts,
    AutoUpdateReceipt, AutoUpdateStatus, PolicyMode,
};
use crate::service_refresh::ManagedServiceInfo;

/// Result of reconciling any unresolved `pending_health_check` at the start of an auto-update.
pub(super) enum RecoveryOutcome {
    /// No pending health check was persisted.
    NoPending,
    /// Preview (`--dry-run`) detected an unresolved checkpoint; caller returns without mutating.
    DryRunNotice(AutoUpdateReceipt),
    /// Pending checkpoint was verified healthy and confirmed (or pre-commit state restored cleanly).
    RecoveredHealthy {
        health_checks: Vec<InstanceHealthReport>,
        note: String,
    },
    /// Pending target was unhealthy/stopped and was automatically rolled back & quarantined.
    RolledBack(AutoUpdateReceipt),
}

/// Reconciles any unresolved `pending_health_check` BEFORE evaluating a new signed pointer.
///
/// Never allows a subsequent pointer to overwrite `previous_pin` with an unverified `target_pin`,
/// and never returns `UpToDate` while `pending_health_check` remains unresolved.
pub(super) fn reconcile_pending_health_check(
    config: &AutoUpdateConfig,
    ports: &AutoUpdatePorts<'_>,
    mode: &PolicyMode<'_>,
    update_state: &UpdateState,
) -> Result<RecoveryOutcome, String> {
    let Some(pending) = update_state.pending_health_check.clone() else {
        return Ok(RecoveryOutcome::NoPending);
    };
    if config.dry_run {
        return Ok(RecoveryOutcome::DryRunNotice(dry_run_pending_receipt(
            config,
            update_state,
            &pending,
        )?));
    }

    let state_store = UpdateStateStore::open(&config.state_dir).map_err(|e| {
        format!(
            "opening update state store ({}) for crash recovery: {e}",
            config.state_dir.display()
        )
    })?;
    let store_pin = recover_store_pin(&config.store_dir, mode)?;
    let rollback_pin = pending
        .previous_pin
        .clone()
        .or_else(|| update_state.confirmed.clone())
        .filter(|pin| pin != &pending.target_pin);

    let target_quarantined = !config.allow_quarantined
        && update_state
            .quarantined_entry(&pending.target_pin.release_id)
            .is_some();

    if store_pin.as_ref() == Some(&pending.target_pin) {
        reconcile_committed_target(
            config,
            ports,
            mode,
            &state_store,
            &pending,
            rollback_pin,
            target_quarantined,
        )
    } else {
        reconcile_uncommitted_target(
            config,
            ports,
            &state_store,
            &pending,
            store_pin,
            target_quarantined,
        )
    }
}

fn dry_run_pending_receipt(
    config: &AutoUpdateConfig,
    update_state: &UpdateState,
    pending: &PendingHealthCheck,
) -> Result<AutoUpdateReceipt, String> {
    let (active_pin, _) = read_store_head_readonly(&config.store_dir)?;
    let previous_pin = pending
        .previous_pin
        .clone()
        .or_else(|| update_state.confirmed.clone());
    Ok(AutoUpdateReceipt {
        status: AutoUpdateStatus::DryRun,
        store: config.store_dir.display().to_string(),
        state_dir: config.state_dir.display().to_string(),
        channel: "recovery".into(),
        pointer_release_id: pending.target_pin.release_id.clone(),
        pointer_sequence: pending.target_pin.sequence,
        previous_pin,
        active_pin,
        drain: None,
        services: Vec::new(),
        health_checks: Vec::new(),
        rollback_services: Vec::new(),
        rollback_health_checks: Vec::new(),
        quarantined_release_id: None,
        note: format!(
            "unresolved pending health check for release `{}` (sequence {}); run without --dry-run to verify health or automatically roll back",
            pending.target_pin.release_id, pending.target_pin.sequence
        ),
    })
}

/// Recovers any incomplete store journal transaction without disrupting a healthy running host
/// that already holds `store.lock`.
fn recover_store_pin(
    store_dir: &Path,
    mode: &PolicyMode<'_>,
) -> Result<Option<ReleasePin>, String> {
    match mode.open_release_store(store_dir) {
        Ok(mut store) => mode
            .recover_store(&mut store)
            .map_err(|e| format!("recovering release store during pending health check: {e}")),
        Err(org_host::release::Error::Busy) => Ok(read_store_head_readonly(store_dir)?.0),
        Err(e) => Err(format!(
            "opening release store ({}) during pending health check: {e}",
            store_dir.display()
        )),
    }
}

fn reconcile_committed_target(
    config: &AutoUpdateConfig,
    ports: &AutoUpdatePorts<'_>,
    mode: &PolicyMode<'_>,
    state_store: &UpdateStateStore,
    pending: &PendingHealthCheck,
    rollback_pin: Option<ReleasePin>,
    target_quarantined: bool,
) -> Result<RecoveryOutcome, String> {
    let services = inspect_services(
        ports.manager,
        config.setup_base.as_deref(),
        &config.store_dir,
    )?;
    let managed_now = services.managed_targets();
    let inspect_failures = services.inspection_failures();
    let (health_reports, summary) = if target_quarantined {
        let msg = format!(
            "retrying failed automatic rollback for quarantined release `{}`",
            pending.target_pin.release_id
        );
        (Vec::new(), msg)
    } else {
        let (healthy, reports) = evaluate_target_on_disk(
            config,
            ports,
            &pending.target_pin.release_id,
            pending.expected_running_instances.as_deref(),
            &managed_now,
            &inspect_failures,
        );
        if healthy {
            state_store
                .confirm_healthy(&pending.target_pin)
                .map_err(|e| format!("confirming recovered healthy release state: {e}"))?;
            let note = format!(
                "reconciled interrupted update to `{}` (sequence {}) and confirmed healthy",
                pending.target_pin.release_id, pending.target_pin.sequence
            );
            return Ok(RecoveryOutcome::RecoveredHealthy {
                health_checks: reports,
                note,
            });
        }
        let summary = format!(
            "interrupted update recovery failed: {}",
            summarize_health_failures(&reports)
        );
        (reports, summary)
    };

    let managed_for_rollback = build_rollback_expectations(
        &managed_now,
        pending.expected_running_instances.as_deref(),
        config.restart,
    );
    let rb_receipt = execute_automatic_rollback(
        config,
        ports,
        mode,
        state_store,
        AutoRollbackContext {
            previous_pin: rollback_pin,
            failed_pin: pending.target_pin.clone(),
            channel: "recovery".into(),
            drain_receipt: None,
            managed_before: managed_for_rollback,
            forward_services: Vec::new(),
            forward_health: health_reports,
            failure_summary: summary,
        },
    )?;
    Ok(RecoveryOutcome::RolledBack(rb_receipt))
}

/// Handles a crash that occurred AFTER persisting `pending_health_check` but BEFORE committing
/// `pending.target_pin` into `ReleaseStore` (or after a rollback restored `previous_pin` in the
/// store but post-rollback service refresh/health check failed): verifies/restores the existing
/// active pin and clears the checkpoint once healthy.
fn reconcile_uncommitted_target(
    config: &AutoUpdateConfig,
    ports: &AutoUpdatePorts<'_>,
    state_store: &UpdateStateStore,
    pending: &PendingHealthCheck,
    store_pin: Option<ReleasePin>,
    target_quarantined: bool,
) -> Result<RecoveryOutcome, String> {
    let Some(active_pin) = store_pin else {
        state_store
            .clear_pending()
            .map_err(|e| format!("clearing pre-commit pending checkpoint: {e}"))?;
        return Ok(RecoveryOutcome::NoPending);
    };

    let services = inspect_services(
        ports.manager,
        config.setup_base.as_deref(),
        &config.store_dir,
    )?;
    let managed_now = services.managed_targets();
    let inspect_failures = services.inspection_failures();
    let (already_healthy, reports) = evaluate_target_on_disk(
        config,
        ports,
        &active_pin.release_id,
        pending.expected_running_instances.as_deref(),
        &managed_now,
        &inspect_failures,
    );
    if already_healthy {
        state_store
            .confirm_healthy(&active_pin)
            .map_err(|e| format!("confirming active pin during recovery: {e}"))?;
        if target_quarantined {
            return Ok(RecoveryOutcome::RolledBack(recovered_rollback_receipt(
                config,
                pending,
                active_pin,
                Vec::new(),
                reports,
            )));
        }
        return Ok(RecoveryOutcome::RecoveredHealthy {
            health_checks: reports,
            note: format!(
                "cleared interrupted pre-commit update for `{}` and confirmed `{}` healthy",
                pending.target_pin.release_id, active_pin.release_id
            ),
        });
    }

    let managed_expected = build_rollback_expectations(
        &managed_now,
        pending.expected_running_instances.as_deref(),
        config.restart,
    );
    let active_program = release_program(&config.store_dir, &active_pin.release_id)?;
    let should_restart: Vec<String> = managed_expected
        .iter()
        .filter(|m| m.was_running)
        .map(|m| m.instance.clone())
        .collect();
    let rb_reports = services.rollback_to_program(
        ports.manager,
        &active_program,
        &should_restart,
        config.restart,
    );
    let (restored_ok, restored_health) = evaluate_post_restart_health(
        ports.health,
        &managed_expected,
        &rb_reports,
        config.restart,
        config.health_timeout,
    );
    if restored_ok {
        state_store.confirm_healthy(&active_pin).map_err(|e| {
            format!("confirming restored active pin after interrupted install: {e}")
        })?;
        if target_quarantined {
            return Ok(RecoveryOutcome::RolledBack(recovered_rollback_receipt(
                config,
                pending,
                active_pin,
                rb_reports,
                restored_health,
            )));
        }
        return Ok(RecoveryOutcome::RecoveredHealthy {
            health_checks: restored_health,
            note: format!(
                "restored interrupted services on `{}` (sequence {}) after pre-commit crash",
                active_pin.release_id, active_pin.sequence
            ),
        });
    }
    Ok(RecoveryOutcome::RolledBack(AutoUpdateReceipt {
        status: AutoUpdateStatus::RollbackFailed,
        store: config.store_dir.display().to_string(),
        state_dir: config.state_dir.display().to_string(),
        channel: "recovery".into(),
        pointer_release_id: pending.target_pin.release_id.clone(),
        pointer_sequence: pending.target_pin.sequence,
        previous_pin: pending.previous_pin.clone(),
        active_pin: Some(active_pin.clone()),
        drain: None,
        services: Vec::new(),
        health_checks: reports,
        rollback_services: rb_reports,
        rollback_health_checks: restored_health,
        quarantined_release_id: target_quarantined.then(|| pending.target_pin.release_id.clone()),
        note: format!(
            "interrupted update recovery for `{}` left `{}` services unhealthy after recovery attempt",
            pending.target_pin.release_id, active_pin.release_id
        ),
    }))
}

fn recovered_rollback_receipt(
    config: &AutoUpdateConfig,
    pending: &PendingHealthCheck,
    active_pin: ReleasePin,
    rollback_services: Vec<crate::service_refresh::ServiceRefreshReport>,
    rollback_health_checks: Vec<InstanceHealthReport>,
) -> AutoUpdateReceipt {
    let note = format!(
        "completed pending rollback recovery from quarantined `{}` to `{}` (sequence {}) and verified health",
        pending.target_pin.release_id, active_pin.release_id, active_pin.sequence
    );
    AutoUpdateReceipt {
        status: AutoUpdateStatus::RolledBack,
        store: config.store_dir.display().to_string(),
        state_dir: config.state_dir.display().to_string(),
        channel: "recovery".into(),
        pointer_release_id: pending.target_pin.release_id.clone(),
        pointer_sequence: pending.target_pin.sequence,
        previous_pin: Some(active_pin.clone()),
        active_pin: Some(active_pin),
        drain: None,
        services: Vec::new(),
        health_checks: Vec::new(),
        rollback_services,
        rollback_health_checks,
        quarantined_release_id: Some(pending.target_pin.release_id.clone()),
        note,
    }
}

fn evaluate_target_on_disk(
    config: &AutoUpdateConfig,
    ports: &AutoUpdatePorts<'_>,
    expected_release_id: &str,
    expected_running: Option<&[String]>,
    managed_now: &[ManagedServiceInfo],
    inspect_failures: &[(String, String)],
) -> (bool, Vec<InstanceHealthReport>) {
    let target_program = match release_program(&config.store_dir, expected_release_id) {
        Ok(path) => path,
        Err(err) => {
            return (
                false,
                vec![InstanceHealthReport {
                    instance: "release-store".into(),
                    healthy: false,
                    detail: err,
                }],
            );
        }
    };

    let failures = collect_pending_service_failures(
        &target_program,
        expected_running,
        managed_now,
        inspect_failures,
        config.restart,
    );
    if !failures.is_empty() {
        return (false, failures);
    }

    let targets: Vec<HealthTarget> = managed_now
        .iter()
        .filter(|m| m.was_running)
        .map(|m| HealthTarget {
            instance: m.instance.clone(),
            instance_id: m.instance_id.clone(),
            root: m.root.clone(),
        })
        .collect();
    if targets.is_empty() {
        return (true, Vec::new());
    }
    probe_instances(ports.health, &targets, config.health_timeout)
}

fn collect_pending_service_failures(
    target_program: &Path,
    expected_running: Option<&[String]>,
    managed_now: &[ManagedServiceInfo],
    inspect_failures: &[(String, String)],
    restart: bool,
) -> Vec<InstanceHealthReport> {
    let mut failures: Vec<InstanceHealthReport> = inspect_failures
        .iter()
        .map(|(instance, why)| InstanceHealthReport {
            instance: instance.clone(),
            healthy: false,
            detail: format!("service inspection failed during crash recovery: {why}"),
        })
        .collect();
    append_missing_expected_failures(expected_running, managed_now, &mut failures);
    for managed in managed_now {
        if let Some(failure) =
            check_managed_service_state(managed, target_program, expected_running, restart)
        {
            failures.push(failure);
        }
    }
    failures
}

fn append_missing_expected_failures(
    expected_running: Option<&[String]>,
    managed_now: &[ManagedServiceInfo],
    failures: &mut Vec<InstanceHealthReport>,
) {
    let Some(expected_names) = expected_running else {
        return;
    };
    for name in expected_names {
        if !managed_now.iter().any(|m| &m.instance == name) {
            failures.push(InstanceHealthReport {
                instance: name.clone(),
                healthy: false,
                detail: "expected running service is missing during crash recovery".into(),
            });
        }
    }
}

fn check_managed_service_state(
    managed: &ManagedServiceInfo,
    target_program: &Path,
    expected_running: Option<&[String]>,
    restart: bool,
) -> Option<InstanceHealthReport> {
    if !same_program(&managed.previous_program, target_program) {
        return Some(InstanceHealthReport {
            instance: managed.instance.clone(),
            healthy: false,
            detail: format!(
                "service points to `{}` instead of expected `{}`",
                managed.previous_program.display(),
                target_program.display()
            ),
        });
    }
    let must_run = restart
        && expected_running
            .map(|names| names.iter().any(|n| n == &managed.instance))
            .unwrap_or(true);
    if must_run && !managed.was_running {
        return Some(InstanceHealthReport {
            instance: managed.instance.clone(),
            healthy: false,
            detail: "service is stopped or crash-looping after interrupted update".into(),
        });
    }
    None
}

fn same_program(actual: &Path, expected: &Path) -> bool {
    actual == expected || fs::canonicalize(actual).is_ok_and(|canon| canon == expected)
}

fn build_rollback_expectations(
    managed_now: &[ManagedServiceInfo],
    expected_running: Option<&[String]>,
    restart: bool,
) -> Vec<ManagedServiceInfo> {
    let mut list: Vec<ManagedServiceInfo> = managed_now
        .iter()
        .map(|m| {
            let should_run = match expected_running {
                Some(names) => names.iter().any(|n| n == &m.instance),
                None => restart || m.was_running,
            };
            ManagedServiceInfo {
                was_running: should_run,
                ..m.clone()
            }
        })
        .collect();
    if let Some(names) = expected_running {
        for name in names {
            if !list.iter().any(|m| &m.instance == name) {
                list.push(ManagedServiceInfo {
                    instance: name.clone(),
                    instance_id: String::new(),
                    service_id: String::new(),
                    root: PathBuf::new(),
                    previous_program: PathBuf::new(),
                    was_running: true,
                });
            }
        }
    }
    list
}
