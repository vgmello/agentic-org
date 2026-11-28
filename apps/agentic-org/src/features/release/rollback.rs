//! `release rollback`: idle-drained rollback to the previous known-good pinned release
//! (or a named previously-committed release), redefining registered services and verifying health.

use std::time::Duration;

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, Style};
use org_host_setup::auto_update::{
    account_setup_base, drain::DrainConfig, run_manual_rollback, DiskActiveTurnDrainProbe,
    HostSocketHealthProbe, RollbackConfig, RollbackPorts, RollbackReceipt, RollbackStatus,
};
use org_host_setup::ServiceRefresh;
use org_platform::service::{ServiceManager, ServiceMode};

use super::{state_dir_for, store_dir};
use crate::error::CliError;
use crate::host;

pub(super) struct RollbackArgs<'a> {
    pub(super) to: Option<&'a str>,
    pub(super) store: Option<&'a str>,
    pub(super) state_dir: Option<&'a str>,
    pub(super) drain_timeout_secs: u64,
    pub(super) health_timeout_secs: u64,
    pub(super) force: bool,
    pub(super) restart: bool,
    pub(super) quarantine_current: bool,
}

pub(super) fn run(args: RollbackArgs<'_>, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let store = store_dir(args.store)?;
    let state_dir = state_dir_for(&store, args.store.is_some(), args.state_dir)?;
    let setup_base = account_setup_base()?;
    let manager = resolve_service_manager()?;
    let drain_targets: Vec<(String, std::path::PathBuf)> =
        ServiceRefresh::inspect(&*manager, &store)
            .map(|sr| {
                sr.managed_targets()
                    .into_iter()
                    .map(|t| (t.instance, t.root))
                    .collect()
            })
            .unwrap_or_default();

    let drain_probe = DiskActiveTurnDrainProbe::new(drain_targets);
    let health_probe = HostSocketHealthProbe;
    let config = RollbackConfig {
        store_dir: store,
        state_dir,
        setup_base,
        target_release_id: args.to.map(str::to_owned),
        drain: DrainConfig {
            timeout: Duration::from_secs(args.drain_timeout_secs),
            poll_interval: Duration::from_millis(250),
            force: args.force,
        },
        health_timeout: Duration::from_secs(args.health_timeout_secs.max(1)),
        restart: args.restart,
        dry_run: ctx.modes().dry_run,
        quarantine_current: args.quarantine_current,
    };
    let ports = RollbackPorts {
        manager: &*manager,
        drain: &drain_probe,
        health: &health_probe,
    };

    let receipt = run_manual_rollback(&config, &ports)?;
    render_receipt(&receipt, ctx)?;

    if matches!(
        receipt.status,
        RollbackStatus::BusyTimeout | RollbackStatus::UnhealthyAfterRollback
    ) {
        return Err(CliError::Exited(1));
    }
    Ok(())
}

fn resolve_service_manager() -> Result<Box<dyn ServiceManager>, CliError> {
    match host::service_manager() {
        Ok(mgr) => Ok(mgr),
        Err(_) => {
            org_platform::service::service(ServiceMode::Foreground, std::path::Path::new("/"))
                .map_err(|e| CliError::from(e.to_string()))
        }
    }
}

fn render_receipt(receipt: &RollbackReceipt, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let json = serde_json::to_value(receipt)
        .map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    if receipt.status == RollbackStatus::DryRun {
        return Ok(ctx.render_dry_run(&receipt.note, &receipt)?);
    }
    if ctx.modes().json {
        return Ok(ctx.write_json(&json)?);
    }
    let (label, style) = match receipt.status {
        RollbackStatus::RolledBack => ("rolled_back", Style::Ok),
        RollbackStatus::AlreadyActive => ("already_active", Style::Active),
        RollbackStatus::DryRun => ("dry_run", Style::Dim),
        RollbackStatus::BusyTimeout => ("busy_timeout", Style::Warn),
        RollbackStatus::UnhealthyAfterRollback => ("unhealthy_after_rollback", Style::Error),
    };
    let active_id = receipt
        .active_pin
        .as_ref()
        .map(|p| p.release_id.as_str())
        .unwrap_or("none");
    let prev_id = receipt
        .previous_pin
        .as_ref()
        .map(|p| p.release_id.as_str())
        .unwrap_or("none");
    let fields = [
        ("status", Cell::status(label, style)),
        ("previous_release", Cell::plain(prev_id)),
        ("active_release", Cell::id(active_id)),
        ("store", Cell::plain(&receipt.store)),
        ("note", Cell::plain(&receipt.note)),
    ];
    ctx.render_detail(active_id, &fields, &json)?;
    Ok(())
}
