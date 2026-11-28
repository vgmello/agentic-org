//! `release update`: pull-verified, idle-drained auto-update with post-restart health check
//! and automatic rollback + quarantine (ADR-0009).

use std::time::Duration;

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, Style};
use org_host::release::{HttpsReleaseFetcher, PullConfig};
use org_host_setup::auto_update::{
    account_setup_base, drain::DrainConfig, run_auto_update, AutoUpdateConfig, AutoUpdatePorts,
    AutoUpdateReceipt, AutoUpdateStatus, DiskActiveTurnDrainProbe, HostSocketHealthProbe,
};

use super::{resolve_release_services, state_dir_for, store_dir};
use crate::error::CliError;

pub(super) struct UpdateArgs<'a> {
    pub(super) pointer_url: &'a str,
    pub(super) signature_url: Option<&'a str>,
    pub(super) store: Option<&'a str>,
    pub(super) state_dir: Option<&'a str>,
    pub(super) drain_timeout_secs: u64,
    pub(super) health_timeout_secs: u64,
    pub(super) fetch_timeout_secs: u64,
    pub(super) force: bool,
    pub(super) restart: bool,
    pub(super) allow_quarantined: bool,
    pub(super) foreground: bool,
}

pub(super) fn run(args: UpdateArgs<'_>, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let store = store_dir(args.store)?;
    let state_dir = state_dir_for(&store, args.store.is_some(), args.state_dir)?;
    let setup_base = account_setup_base()?;

    let mut pull = PullConfig::https(args.pointer_url, args.signature_url.map(str::to_owned));
    pull.bundle_timeout = Duration::from_secs(args.fetch_timeout_secs.max(1));

    let resolved = resolve_release_services(&store, setup_base.as_deref(), args.foreground)?;

    let fetcher = HttpsReleaseFetcher::new();
    let drain_probe = DiskActiveTurnDrainProbe::new(resolved.drain_targets);
    let health_probe = HostSocketHealthProbe;

    let config = AutoUpdateConfig {
        store_dir: store,
        state_dir,
        setup_base,
        pull,
        drain: DrainConfig {
            timeout: Duration::from_secs(args.drain_timeout_secs),
            poll_interval: Duration::from_millis(250),
            force: args.force,
        },
        health_timeout: Duration::from_secs(args.health_timeout_secs.max(1)),
        restart: args.restart,
        dry_run: ctx.modes().dry_run,
        allow_quarantined: args.allow_quarantined,
        create_default_store_parent: args.store.is_none(),
    };
    let ports = AutoUpdatePorts {
        fetcher: &fetcher,
        manager: &*resolved.manager,
        drain: &drain_probe,
        health: &health_probe,
    };

    let receipt = run_auto_update(&config, &ports)?;
    render_receipt(&receipt, ctx)?;

    if matches!(
        receipt.status,
        AutoUpdateStatus::BusyTimeout
            | AutoUpdateStatus::DowngradeRejected
            | AutoUpdateStatus::RolledBack
            | AutoUpdateStatus::RollbackFailed
    ) {
        return Err(CliError::Exited(1));
    }
    Ok(())
}

fn render_receipt(receipt: &AutoUpdateReceipt, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let json = serde_json::to_value(receipt)
        .map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    if receipt.status == AutoUpdateStatus::DryRun {
        return Ok(ctx.render_dry_run(&receipt.note, &receipt)?);
    }
    if ctx.modes().json {
        return Ok(ctx.write_json(&json)?);
    }
    let (status_label, style) = match receipt.status {
        AutoUpdateStatus::UpToDate => ("up_to_date", Style::Active),
        AutoUpdateStatus::DryRun => ("dry_run", Style::Dim),
        AutoUpdateStatus::Updated => ("updated", Style::Ok),
        AutoUpdateStatus::BusyTimeout => ("busy_timeout", Style::Warn),
        AutoUpdateStatus::Quarantined => ("quarantined", Style::Warn),
        AutoUpdateStatus::DowngradeRejected => ("downgrade_rejected", Style::Error),
        AutoUpdateStatus::RolledBack => ("rolled_back", Style::Warn),
        AutoUpdateStatus::RollbackFailed => ("rollback_failed", Style::Error),
    };
    let active_label = receipt
        .active_pin
        .as_ref()
        .map(|p| format!("{} (seq {})", p.release_id, p.sequence))
        .unwrap_or_else(|| "none".into());
    let fields = [
        ("status", Cell::status(status_label, style)),
        ("channel", Cell::plain(&receipt.channel)),
        ("pointer_release", Cell::id(&receipt.pointer_release_id)),
        ("active_release", Cell::plain(&active_label)),
        ("store", Cell::plain(&receipt.store)),
        ("note", Cell::plain(&receipt.note)),
    ];
    ctx.render_detail(&receipt.pointer_release_id, &fields, &json)?;
    Ok(())
}
