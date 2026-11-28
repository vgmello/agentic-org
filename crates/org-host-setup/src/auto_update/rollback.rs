//! Automatic post-health-check rollback & operator-initiated rollback for pull-verified releases.

use std::path::PathBuf;
use std::time::Duration;

use org_host::release::{ReleasePin, UpdateStateStore};
use org_platform::service::ServiceManager;
#[cfg(any(test, feature = "test-support"))]
use org_release::{TrustedKeys, TrustedPolicy};
use serde::Serialize;

use super::drain::{wait_for_idle_drain, DrainConfig, DrainPhase, DrainProbe, DrainReceipt};
use super::health::{probe_instances, HealthProbe, InstanceHealthReport};
use super::{
    health_targets_for, inspect_services, open_store, read_store_head_readonly, release_program,
    AutoUpdateConfig, AutoUpdatePorts, AutoUpdateReceipt, AutoUpdateStatus, PolicyMode,
};
use crate::service_refresh::{InstallAttempt, ManagedServiceInfo, ServiceRefreshReport};

/// Parameters for an explicit operator rollback (`agentic-org release rollback`).
#[derive(Debug, Clone)]
pub struct RollbackConfig {
    pub store_dir: PathBuf,
    pub state_dir: PathBuf,
    pub setup_base: Option<PathBuf>,
    pub target_release_id: Option<String>,
    pub drain: DrainConfig,
    pub health_timeout: Duration,
    pub restart: bool,
    pub dry_run: bool,
    pub quarantine_current: bool,
}

/// Injectable ports for manual rollback.
pub struct RollbackPorts<'a> {
    pub manager: &'a dyn ServiceManager,
    pub drain: &'a dyn DrainProbe,
    pub health: &'a dyn HealthProbe,
}

/// Terminal status of a manual rollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackStatus {
    RolledBack,
    AlreadyActive,
    DryRun,
    BusyTimeout,
    UnhealthyAfterRollback,
}

/// Structured receipt returned by [`run_manual_rollback`].
#[derive(Debug, Clone, Serialize)]
pub struct RollbackReceipt {
    pub status: RollbackStatus,
    pub store: String,
    pub state_dir: String,
    pub previous_pin: Option<ReleasePin>,
    pub active_pin: Option<ReleasePin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drain: Option<DrainReceipt>,
    pub services: Vec<ServiceRefreshReport>,
    pub health_checks: Vec<InstanceHealthReport>,
    pub note: String,
}

pub(super) struct AutoRollbackContext {
    pub previous_pin: Option<ReleasePin>,
    pub failed_pin: ReleasePin,
    pub channel: String,
    pub drain_receipt: DrainReceipt,
    pub managed_before: Vec<ManagedServiceInfo>,
    pub forward_services: Vec<ServiceRefreshReport>,
    pub forward_health: Vec<InstanceHealthReport>,
    pub failure_summary: String,
}

pub(super) fn execute_automatic_rollback(
    config: &AutoUpdateConfig,
    ports: &AutoUpdatePorts<'_>,
    mode: &PolicyMode<'_>,
    state_store: &UpdateStateStore,
    ctx: AutoRollbackContext,
) -> Result<AutoUpdateReceipt, String> {
    let AutoRollbackContext {
        previous_pin,
        failed_pin,
        channel,
        drain_receipt,
        managed_before,
        forward_services,
        forward_health,
        failure_summary,
    } = ctx;

    let Some(prev_pin) = previous_pin else {
        let _ = state_store.record_rollback_and_quarantine(&failed_pin, None, &failure_summary);
        return Ok(AutoUpdateReceipt {
            status: AutoUpdateStatus::RollbackFailed,
            store: config.store_dir.display().to_string(),
            state_dir: config.state_dir.display().to_string(),
            channel,
            pointer_release_id: failed_pin.release_id.clone(),
            pointer_sequence: failed_pin.sequence,
            previous_pin: None,
            active_pin: Some(failed_pin.clone()),
            drain: Some(drain_receipt),
            services: forward_services,
            health_checks: forward_health,
            rollback_services: Vec::new(),
            rollback_health_checks: Vec::new(),
            quarantined_release_id: Some(failed_pin.release_id),
            note: format!(
                "post-restart health check failed ({failure_summary}); no previous pinned release exists to roll back to"
            ),
        });
    };

    let should_restart: Vec<String> = managed_before
        .iter()
        .filter(|m| m.was_running)
        .map(|m| m.instance.clone())
        .collect();

    let mut rb_services = inspect_services(
        ports.manager,
        config.setup_base.as_deref(),
        &config.store_dir,
    )?;
    let rollback_pin_res = rb_services.install(ports.manager, || {
        rollback_store_once(&config.store_dir, &prev_pin, mode)
    });
    let restored_pin = match rollback_pin_res {
        Ok(pin) => pin,
        Err(rb_err) => {
            let _ = state_store.record_rollback_and_quarantine(&failed_pin, None, &failure_summary);
            return Ok(AutoUpdateReceipt {
                status: AutoUpdateStatus::RollbackFailed,
                store: config.store_dir.display().to_string(),
                state_dir: config.state_dir.display().to_string(),
                channel,
                pointer_release_id: failed_pin.release_id.clone(),
                pointer_sequence: failed_pin.sequence,
                previous_pin: Some(prev_pin),
                active_pin: Some(failed_pin.clone()),
                drain: Some(drain_receipt),
                services: forward_services,
                health_checks: forward_health,
                rollback_services: Vec::new(),
                rollback_health_checks: Vec::new(),
                quarantined_release_id: Some(failed_pin.release_id),
                note: format!(
                    "post-restart health check failed ({failure_summary}); automatic rollback failed to restore pin: {rb_err}"
                ),
            });
        }
    };

    let prev_program = release_program(&config.store_dir, &restored_pin.release_id)?;
    let rollback_reports = rb_services.rollback_to_program(
        ports.manager,
        &prev_program,
        &should_restart,
        config.restart,
    );
    let rollback_targets = health_targets_for(&managed_before, &rollback_reports, config.restart);
    let (rb_healthy, rb_health_reports) = if rollback_targets.is_empty() {
        (true, Vec::new())
    } else {
        probe_instances(ports.health, &rollback_targets, config.health_timeout)
    };

    let _ = state_store.record_rollback_and_quarantine(
        &failed_pin,
        rb_healthy.then_some(&restored_pin),
        &failure_summary,
    );
    let status = if rb_healthy {
        AutoUpdateStatus::RolledBack
    } else {
        AutoUpdateStatus::RollbackFailed
    };
    let note = if rb_healthy {
        format!(
            "post-restart health check failed for `{}` ({failure_summary}); automatically rolled back to `{}` (sequence {}), verified rollback health, and quarantined `{}`",
            failed_pin.release_id,
            restored_pin.release_id,
            restored_pin.sequence,
            failed_pin.release_id
        )
    } else {
        format!(
            "post-restart health check failed for `{}` ({failure_summary}); rolled back pin to `{}`, but rollback health check failed; quarantined `{}`",
            failed_pin.release_id, restored_pin.release_id, failed_pin.release_id
        )
    };
    Ok(AutoUpdateReceipt {
        status,
        store: config.store_dir.display().to_string(),
        state_dir: config.state_dir.display().to_string(),
        channel,
        pointer_release_id: failed_pin.release_id.clone(),
        pointer_sequence: failed_pin.sequence,
        previous_pin: Some(restored_pin.clone()),
        active_pin: Some(restored_pin),
        drain: Some(drain_receipt),
        services: forward_services,
        health_checks: forward_health,
        rollback_services: rollback_reports,
        rollback_health_checks: rb_health_reports,
        quarantined_release_id: Some(failed_pin.release_id),
        note,
    })
}

/// Manually roll back to the previous committed pin (or an explicit previously-pinned `release_id`).
pub fn run_manual_rollback(
    config: &RollbackConfig,
    ports: &RollbackPorts<'_>,
) -> Result<RollbackReceipt, String> {
    run_manual_rollback_inner(config, ports, PolicyMode::embedded())
}

/// Test-harness entry point for manual rollback with an explicit policy and trust registry.
#[cfg(any(test, feature = "test-support"))]
pub fn run_manual_rollback_with_policy(
    config: &RollbackConfig,
    ports: &RollbackPorts<'_>,
    policy: &TrustedPolicy,
    keys: &TrustedKeys,
    skip_platform_check: bool,
) -> Result<RollbackReceipt, String> {
    run_manual_rollback_inner(
        config,
        ports,
        PolicyMode::explicit(policy, keys, skip_platform_check),
    )
}

fn run_manual_rollback_inner(
    config: &RollbackConfig,
    ports: &RollbackPorts<'_>,
    mode: PolicyMode<'_>,
) -> Result<RollbackReceipt, String> {
    let (current_pin, _) = read_store_head_readonly(&config.store_dir)?;
    let Some(current) = current_pin else {
        return Err("no active pinned release found in release store".into());
    };
    if let Some(target_id) = &config.target_release_id {
        if target_id == &current.release_id {
            return Ok(RollbackReceipt {
                status: RollbackStatus::AlreadyActive,
                store: config.store_dir.display().to_string(),
                state_dir: config.state_dir.display().to_string(),
                previous_pin: Some(current.clone()),
                active_pin: Some(current.clone()),
                drain: None,
                services: Vec::new(),
                health_checks: Vec::new(),
                note: format!("release `{}` is already active", current.release_id),
            });
        }
    }
    if config.dry_run {
        let target_label = config
            .target_release_id
            .as_deref()
            .unwrap_or("previous committed release");
        return Ok(RollbackReceipt {
            status: RollbackStatus::DryRun,
            store: config.store_dir.display().to_string(),
            state_dir: config.state_dir.display().to_string(),
            previous_pin: Some(current.clone()),
            active_pin: Some(current),
            drain: None,
            services: Vec::new(),
            health_checks: Vec::new(),
            note: format!("would idle-drain and roll back to {target_label}"),
        });
    }

    let mut services = inspect_services(
        ports.manager,
        config.setup_base.as_deref(),
        &config.store_dir,
    )?;
    let managed_before = services.managed_targets();
    let drain_receipt = wait_for_idle_drain(ports.drain, config.drain)?;
    if drain_receipt.phase == DrainPhase::BusyTimeout {
        return Ok(RollbackReceipt {
            status: RollbackStatus::BusyTimeout,
            store: config.store_dir.display().to_string(),
            state_dir: config.state_dir.display().to_string(),
            previous_pin: Some(current.clone()),
            active_pin: Some(current),
            drain: Some(drain_receipt),
            services: Vec::new(),
            health_checks: Vec::new(),
            note: format!(
                "rollback deferred: active agent turns did not finish within {}s",
                config.drain.timeout.as_secs()
            ),
        });
    }

    let target_pin = services.install(ports.manager, || {
        manual_rollback_store_once(
            &config.store_dir,
            config.target_release_id.as_deref(),
            &mode,
        )
    })?;
    let target_program = release_program(&config.store_dir, &target_pin.release_id)?;
    let should_restart: Vec<String> = managed_before
        .iter()
        .filter(|m| m.was_running)
        .map(|m| m.instance.clone())
        .collect();
    let refresh_reports = services.rollback_to_program(
        ports.manager,
        &target_program,
        &should_restart,
        config.restart,
    );
    let targets = health_targets_for(&managed_before, &refresh_reports, config.restart);
    let (healthy, health_reports) = if targets.is_empty() {
        (true, Vec::new())
    } else {
        probe_instances(ports.health, &targets, config.health_timeout)
    };

    if let Ok(state_store) = UpdateStateStore::open(&config.state_dir) {
        if config.quarantine_current {
            let _ = state_store.record_rollback_and_quarantine(
                &current,
                healthy.then_some(&target_pin),
                "operator manual rollback",
            );
        } else if healthy {
            let _ = state_store.confirm_healthy(&target_pin);
        }
    }

    let status = if healthy {
        RollbackStatus::RolledBack
    } else {
        RollbackStatus::UnhealthyAfterRollback
    };
    let note = if healthy {
        format!(
            "rolled back from `{}` to `{}` (sequence {}) and verified health",
            current.release_id, target_pin.release_id, target_pin.sequence
        )
    } else {
        format!(
            "rolled back pin from `{}` to `{}` (sequence {}), but health check failed",
            current.release_id, target_pin.release_id, target_pin.sequence
        )
    };
    Ok(RollbackReceipt {
        status,
        store: config.store_dir.display().to_string(),
        state_dir: config.state_dir.display().to_string(),
        previous_pin: Some(current),
        active_pin: Some(target_pin),
        drain: Some(drain_receipt),
        services: refresh_reports,
        health_checks: health_reports,
        note,
    })
}

fn rollback_store_once(
    store_dir: &std::path::Path,
    target: &ReleasePin,
    mode: &PolicyMode<'_>,
) -> Result<ReleasePin, InstallAttempt> {
    let mut store = open_store(store_dir, mode)?;
    let res = mode.rollback_store(&mut store, target);
    res.map_err(|e| InstallAttempt::Failed(format!("rolling back release store pin: {e}")))
}

fn manual_rollback_store_once(
    store_dir: &std::path::Path,
    target_release_id: Option<&str>,
    mode: &PolicyMode<'_>,
) -> Result<ReleasePin, InstallAttempt> {
    let mut store = open_store(store_dir, mode)?;
    mode.recover_store(&mut store)
        .map_err(|e| InstallAttempt::Failed(format!("recovering release store: {e}")))?;
    let target = match target_release_id {
        Some(id) => store
            .committed_pins()
            .into_iter()
            .find(|p| p.release_id == id)
            .ok_or_else(|| {
                InstallAttempt::Failed(format!(
                    "release `{id}` was never committed to this release store"
                ))
            })?,
        None => store.previous_pin().cloned().ok_or_else(|| {
            InstallAttempt::Failed(
                "no previous committed release exists in this store to roll back to".into(),
            )
        })?,
    };
    mode.rollback_store(&mut store, &target)
        .map_err(|e| InstallAttempt::Failed(format!("rolling back release pin: {e}")))
}
