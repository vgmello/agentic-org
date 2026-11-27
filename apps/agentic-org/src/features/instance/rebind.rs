//! `agentic-org instance rebind [<name>]`: explicit confirmation-bound rebind of a stopped
//! instance root whose filesystem identity (device/inode) changed after a copy or restore
//! (`FC-148`).

use std::fs::{self, TryLockError};
use std::io;
use std::path::Path;

use clap::Args;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::error::ShellError;
use org_cli_shell::output::Cell;
use org_cli_shell::prompt::require_confirmation;
use org_host::instance::registry::Registry;
use org_host::instance::{Error as InstanceError, RebindPreview};
use serde::Serialize;

use crate::args::{InstanceArgs, YesArgs};
use crate::error::CliError;
use crate::selection;

pub(super) const EXAMPLES: &str = "Examples:
  agentic-org instance rebind --dry-run
  agentic-org instance rebind lab --yes
  agentic-org instance rebind --name lab --yes --json";

#[derive(Args, Debug)]
pub(super) struct RebindArgs {
    /// Instance name to rebind (optional positional alternative to `--name` / `--instance`)
    #[arg(value_name = "NAME", conflicts_with = "name")]
    positional_name: Option<String>,
    #[command(flatten)]
    instance: InstanceArgs,
    #[command(flatten)]
    pub(super) yes: YesArgs,
}

#[derive(Debug, Serialize)]
struct RebindPlan {
    action: &'static str,
    instance: String,
    instance_id: String,
    root: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct RebindReceipt {
    rebound: bool,
    instance: String,
    instance_id: String,
    root: String,
    description: String,
}

pub(super) fn run(args: RebindArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let explicit_name = args
        .positional_name
        .as_deref()
        .or(args.instance.name.as_deref());
    let target = selection::select(explicit_name, None)?;
    let paths = org_host::human::paths(target.instance())
        .map_err(|e| CliError::failed("instance::paths", e))?;
    let registry = Registry::open(&paths.locator, &paths.user_temp).map_err(map_instance_error)?;

    let mut observed: Option<RebindPlan> = None;
    let mut confirm_error: Option<CliError> = None;
    let dry_run = ctx.modes().dry_run;
    let _ = args.yes;

    let result = registry.rebind(target.instance(), |preview: &RebindPreview| {
        let plan = build_plan(preview);
        if dry_run {
            observed = Some(plan);
            return Err(InstanceError::RebindUnconfirmed);
        }
        if let Err(err) = ensure_instance_stopped(preview.root()) {
            confirm_error = Some(err);
            return Err(InstanceError::RebindUnconfirmed);
        }
        if let Err(err) = require_confirmation(ctx, &plan.description) {
            confirm_error = Some(CliError::from(err));
            return Err(InstanceError::RebindUnconfirmed);
        }
        observed = Some(plan);
        Ok(())
    });

    if dry_run {
        if let Some(plan) = observed {
            let summary = format!("would {}", plan.description);
            return Ok(ctx.render_dry_run(&summary, &plan)?);
        }
    }
    if let Some(err) = confirm_error {
        return Err(err);
    }
    result.map_err(map_instance_error)?;
    let plan = observed.ok_or_else(|| {
        CliError::failed(
            "instance::rebind",
            "rebind completed without a confirmation preview",
        )
    })?;
    let receipt = RebindReceipt {
        rebound: true,
        instance: plan.instance,
        instance_id: plan.instance_id,
        root: plan.root,
        description: plan.description,
    };
    let fields = [
        ("instance", Cell::id(&receipt.instance)),
        ("instance_id", Cell::id(&receipt.instance_id)),
        ("root", Cell::plain(&receipt.root)),
        ("rebound", Cell::plain("true")),
    ];
    let json = serde_json::to_value(&receipt)
        .map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    ctx.render_detail(&receipt.instance, &fields, &json)?;
    ctx.success(&format!(
        "Rebound instance `{}` ({}) at {}.",
        receipt.instance, receipt.instance_id, receipt.root
    ));
    Ok(())
}

fn build_plan(preview: &RebindPreview) -> RebindPlan {
    RebindPlan {
        action: "instance_rebind",
        instance: preview.name().to_owned(),
        instance_id: preview.instance_id().to_owned(),
        root: preview.root().display().to_string(),
        description: preview.description(),
    }
}

/// Refuses to rebind an instance while its `host.lock` or any `office.lock` is held.
fn ensure_instance_stopped(instance_root: &Path) -> Result<(), CliError> {
    let office_home = instance_root.join("office-state");
    check_lock_free(&office_home.join("host.lock"), "host")?;
    let offices_dir = office_home.join("offices");
    let Ok(entries) = fs::read_dir(&offices_dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
            check_lock_free(&path.join("office.lock"), "office")?;
        }
    }
    Ok(())
}

fn check_lock_free(lock_path: &Path, label: &str) -> Result<(), CliError> {
    let file = match org_platform::fs::open_nofollow_ro(lock_path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(CliError::failed(
                "instance::unsafe_lock",
                format!(
                    "cannot inspect {label} lock `{}`: {e}; nothing was changed",
                    lock_path.display()
                ),
            ))
        }
    };
    let meta = file.metadata().map_err(|e| {
        CliError::failed(
            "instance::unsafe_lock",
            format!(
                "cannot read {label} lock metadata `{}`: {e}; nothing was changed",
                lock_path.display()
            ),
        )
    })?;
    if !meta.is_file() {
        return Err(CliError::failed(
            "instance::unsafe_lock",
            format!(
                "{label} lock `{}` is not a regular file; nothing was changed",
                lock_path.display()
            ),
        ));
    }
    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            Ok(())
        }
        Err(TryLockError::WouldBlock) => Err(CliError::failed(
            "instance::busy",
            format!(
                "instance {label} is currently running (`{}` is locked); nothing was changed",
                lock_path.display()
            ),
        )
        .with_help(
            "Stop the running host and offices first (`agentic-org service stop --yes`), then re-run `agentic-org instance rebind --yes`.",
        )),
        Err(TryLockError::Error(e)) => Err(CliError::failed(
            "instance::unsafe_lock",
            format!(
                "cannot probe {label} lock `{}`: {e}; nothing was changed",
                lock_path.display()
            ),
        )),
    }
}

fn map_instance_error(error: InstanceError) -> CliError {
    let code = match &error {
        InstanceError::Invalid(_) => return CliError::usage(error.to_string()),
        InstanceError::RebindUnconfirmed => return CliError::from(ShellError::Cancelled),
        InstanceError::RebindRecoveryRequired { .. } => "instance::rebind_recovery_required",
        InstanceError::RebindChanged { .. } => "instance::rebind_changed",
        InstanceError::IdentityClash { .. } => "instance::identity_clash",
        InstanceError::RootUnavailable { .. } => "instance::root_unavailable",
        InstanceError::ReservedRoot { .. } => "instance::reserved_root",
        InstanceError::Busy(_) => "instance::busy",
        InstanceError::Unsafe(_) => "instance::unsafe",
        InstanceError::Conflict(_) => "instance::conflict",
        InstanceError::CaseInsensitive(_) | InstanceError::UnsupportedFilesystem { .. } => {
            "instance::unsupported_filesystem"
        }
        InstanceError::FilesystemProbe { .. } | InstanceError::Io(_) => "instance::io",
    };
    CliError::failed(code, error.to_string())
}
