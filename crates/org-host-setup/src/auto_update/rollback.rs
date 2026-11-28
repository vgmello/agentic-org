//! Automatic post-health-check rollback & operator-initiated rollback for pull-verified releases.

use std::path::{Path, PathBuf};
use std::time::Duration;

use org_host::release::{ReleasePin, UpdateStateStore};
use org_platform::service::ServiceManager;
#[cfg(any(test, feature = "test-support"))]
use org_release::{TrustedKeys, TrustedPolicy};
use serde::Serialize;

use super::drain::{wait_for_idle_drain, DrainConfig, DrainPhase, DrainProbe, DrainReceipt};
use super::health::{evaluate_post_restart_health, HealthProbe, InstanceHealthReport};
use super::support::{
    ensure_no_inspection_failures, inspect_services, open_store, read_store_head_readonly,
    release_program, AutoUpdateConfig, AutoUpdatePorts, AutoUpdateReceipt, AutoUpdateStatus,
    PolicyMode,
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
    pub drain_receipt: Option<DrainReceipt>,
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
    let Some(prev_pin) = ctx.previous_pin.clone() else {
        state_store
            .record_rollback_and_quarantine(&ctx.failed_pin, None, &ctx.failure_summary)
            .map_err(|e| {
                format!(
                    "post-restart health check failed ({}) and persisting quarantine for `{}` failed: {e}",
                    ctx.failure_summary, ctx.failed_pin.release_id
                )
            })?;
        let note = format!(
            "post-restart health check failed ({}); no previous pinned release exists to roll back to",
            ctx.failure_summary
        );
        return Ok(aborted_rollback_receipt(config, ctx, None, note));
    };

    let mut rb_services = inspect_services(
        ports.manager,
        config.setup_base.as_deref(),
        &config.store_dir,
    )?;
    let restored_pin = match rb_services.install(ports.manager, || {
        rollback_store_once(&config.store_dir, &prev_pin, mode)
    }) {
        Ok(pin) => pin,
        Err(rb_err) => {
            state_store
                .record_rollback_and_quarantine(&ctx.failed_pin, None, &ctx.failure_summary)
                .map_err(|e| {
                    format!(
                        "automatic rollback failed ({rb_err}) and persisting quarantine for `{}` failed: {e}",
                        ctx.failed_pin.release_id
                    )
                })?;
            let note = format!(
                "post-restart health check failed ({}); automatic rollback failed to restore pin: {rb_err}",
                ctx.failure_summary
            );
            return Ok(aborted_rollback_receipt(config, ctx, Some(prev_pin), note));
        }
    };

    let prev_program = release_program(&config.store_dir, &restored_pin.release_id)?;
    let should_restart = running_instance_names(&ctx.managed_before);
    let rollback_reports = rb_services.rollback_to_program(
        ports.manager,
        &prev_program,
        &should_restart,
        config.restart,
    );
    let (rb_healthy, rb_health_reports) = evaluate_post_restart_health(
        ports.health,
        &ctx.managed_before,
        &rollback_reports,
        config.restart,
        config.health_timeout,
    );

    state_store
        .record_rollback_and_quarantine(
            &ctx.failed_pin,
            rb_healthy.then_some(&restored_pin),
            &ctx.failure_summary,
        )
        .map_err(|e| {
            format!(
                "persisting rollback and quarantine state for `{}`: {e}",
                ctx.failed_pin.release_id
            )
        })?;
    Ok(completed_auto_rollback_receipt(
        config,
        ctx,
        restored_pin,
        rb_healthy,
        rollback_reports,
        rb_health_reports,
    ))
}

fn aborted_rollback_receipt(
    config: &AutoUpdateConfig,
    ctx: AutoRollbackContext,
    previous_pin: Option<ReleasePin>,
    note: String,
) -> AutoUpdateReceipt {
    AutoUpdateReceipt {
        status: AutoUpdateStatus::RollbackFailed,
        store: config.store_dir.display().to_string(),
        state_dir: config.state_dir.display().to_string(),
        channel: ctx.channel,
        pointer_release_id: ctx.failed_pin.release_id.clone(),
        pointer_sequence: ctx.failed_pin.sequence,
        previous_pin,
        active_pin: Some(ctx.failed_pin.clone()),
        drain: ctx.drain_receipt,
        services: ctx.forward_services,
        health_checks: ctx.forward_health,
        rollback_services: Vec::new(),
        rollback_health_checks: Vec::new(),
        quarantined_release_id: Some(ctx.failed_pin.release_id),
        note,
    }
}

fn completed_auto_rollback_receipt(
    config: &AutoUpdateConfig,
    ctx: AutoRollbackContext,
    restored_pin: ReleasePin,
    rb_healthy: bool,
    rollback_reports: Vec<ServiceRefreshReport>,
    rb_health_reports: Vec<InstanceHealthReport>,
) -> AutoUpdateReceipt {
    let status = if rb_healthy {
        AutoUpdateStatus::RolledBack
    } else {
        AutoUpdateStatus::RollbackFailed
    };
    let note = if rb_healthy {
        format!(
            "post-restart health check failed for `{}` ({}); automatically rolled back to `{}` (sequence {}), verified rollback health, and quarantined `{}`",
            ctx.failed_pin.release_id,
            ctx.failure_summary,
            restored_pin.release_id,
            restored_pin.sequence,
            ctx.failed_pin.release_id
        )
    } else {
        format!(
            "post-restart health check failed for `{}` ({}); rolled back pin to `{}`, but rollback service refresh or health check failed; quarantined `{}`",
            ctx.failed_pin.release_id,
            ctx.failure_summary,
            restored_pin.release_id,
            ctx.failed_pin.release_id
        )
    };
    AutoUpdateReceipt {
        status,
        store: config.store_dir.display().to_string(),
        state_dir: config.state_dir.display().to_string(),
        channel: ctx.channel,
        pointer_release_id: ctx.failed_pin.release_id.clone(),
        pointer_sequence: ctx.failed_pin.sequence,
        previous_pin: Some(restored_pin.clone()),
        active_pin: Some(restored_pin),
        drain: ctx.drain_receipt,
        services: ctx.forward_services,
        health_checks: ctx.forward_health,
        rollback_services: rollback_reports,
        rollback_health_checks: rb_health_reports,
        quarantined_release_id: Some(ctx.failed_pin.release_id),
        note,
    }
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
    if let Some(early) = manual_rollback_preflight(config, &current) {
        return Ok(early);
    }

    let mut services = inspect_services(
        ports.manager,
        config.setup_base.as_deref(),
        &config.store_dir,
    )?;
    ensure_no_inspection_failures(&services)?;
    let managed_before = services.managed_targets();

    let drain_receipt = wait_for_idle_drain(ports.drain, config.drain)?;
    if drain_receipt.phase == DrainPhase::BusyTimeout {
        let mut receipt = base_manual_rollback_receipt(
            config,
            RollbackStatus::BusyTimeout,
            current.clone(),
            current,
            format!(
                "rollback deferred: active agent turns did not finish within {}s",
                config.drain.timeout.as_secs()
            ),
        );
        receipt.drain = Some(drain_receipt);
        return Ok(receipt);
    }

    let state_store = UpdateStateStore::open(&config.state_dir).map_err(|e| {
        format!(
            "opening update state store ({}): {e}",
            config.state_dir.display()
        )
    })?;
    let target_pin = services.install(ports.manager, || {
        manual_rollback_store_once(
            &config.store_dir,
            config.target_release_id.as_deref(),
            &mode,
        )
    })?;
    let target_program = release_program(&config.store_dir, &target_pin.release_id)?;
    let should_restart = running_instance_names(&managed_before);
    let refresh_reports = services.rollback_to_program(
        ports.manager,
        &target_program,
        &should_restart,
        config.restart,
    );
    let (healthy, health_reports) = evaluate_post_restart_health(
        ports.health,
        &managed_before,
        &refresh_reports,
        config.restart,
        config.health_timeout,
    );
    persist_manual_rollback_state(
        &state_store,
        &current,
        &target_pin,
        healthy,
        config.quarantine_current,
    )?;

    let (status, note) = manual_rollback_outcome(&current, &target_pin, healthy);
    let mut receipt = base_manual_rollback_receipt(config, status, current, target_pin, note);
    receipt.drain = Some(drain_receipt);
    receipt.services = refresh_reports;
    receipt.health_checks = health_reports;
    Ok(receipt)
}

fn manual_rollback_preflight(
    config: &RollbackConfig,
    current: &ReleasePin,
) -> Option<RollbackReceipt> {
    if config.target_release_id.as_deref() == Some(current.release_id.as_str()) {
        return Some(base_manual_rollback_receipt(
            config,
            RollbackStatus::AlreadyActive,
            current.clone(),
            current.clone(),
            format!("release `{}` is already active", current.release_id),
        ));
    }
    if config.dry_run {
        let target_label = config
            .target_release_id
            .as_deref()
            .unwrap_or("previous committed release");
        return Some(base_manual_rollback_receipt(
            config,
            RollbackStatus::DryRun,
            current.clone(),
            current.clone(),
            format!("would idle-drain and roll back to {target_label}"),
        ));
    }
    None
}

fn persist_manual_rollback_state(
    state_store: &UpdateStateStore,
    current: &ReleasePin,
    target_pin: &ReleasePin,
    healthy: bool,
    quarantine_current: bool,
) -> Result<(), String> {
    if quarantine_current {
        state_store
            .record_rollback_and_quarantine(
                current,
                healthy.then_some(target_pin),
                "operator manual rollback",
            )
            .map_err(|e| {
                format!(
                    "persisting manual rollback quarantine state for `{}`: {e}",
                    current.release_id
                )
            })?;
    } else if healthy {
        state_store
            .confirm_healthy(target_pin)
            .map_err(|e| format!("persisting confirmed healthy rollback state: {e}"))?;
    }
    Ok(())
}

fn manual_rollback_outcome(
    current: &ReleasePin,
    target_pin: &ReleasePin,
    healthy: bool,
) -> (RollbackStatus, String) {
    if healthy {
        (
            RollbackStatus::RolledBack,
            format!(
                "rolled back from `{}` to `{}` (sequence {}) and verified health",
                current.release_id, target_pin.release_id, target_pin.sequence
            ),
        )
    } else {
        (
            RollbackStatus::UnhealthyAfterRollback,
            format!(
                "rolled back pin from `{}` to `{}` (sequence {}), but service refresh or health check failed",
                current.release_id, target_pin.release_id, target_pin.sequence
            ),
        )
    }
}

fn base_manual_rollback_receipt(
    config: &RollbackConfig,
    status: RollbackStatus,
    previous_pin: ReleasePin,
    active_pin: ReleasePin,
    note: String,
) -> RollbackReceipt {
    RollbackReceipt {
        status,
        store: config.store_dir.display().to_string(),
        state_dir: config.state_dir.display().to_string(),
        previous_pin: Some(previous_pin),
        active_pin: Some(active_pin),
        drain: None,
        services: Vec::new(),
        health_checks: Vec::new(),
        note,
    }
}

fn running_instance_names(managed: &[ManagedServiceInfo]) -> Vec<String> {
    managed
        .iter()
        .filter(|m| m.was_running)
        .map(|m| m.instance.clone())
        .collect()
}

fn rollback_store_once(
    store_dir: &Path,
    target: &ReleasePin,
    mode: &PolicyMode<'_>,
) -> Result<ReleasePin, InstallAttempt> {
    let mut store = open_store(store_dir, mode)?;
    mode.rollback_store(&mut store, target)
        .map_err(|e| InstallAttempt::Failed(format!("rolling back release store pin: {e}")))
}

fn manual_rollback_store_once(
    store_dir: &Path,
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
