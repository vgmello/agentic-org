//! `restore --verify <archive> [--to <dir>]` and `restore <archive> --org <org>`.

use std::path::Path;

use org_backup::{
    archive_kind, restore_in_place, verify, ArchiveKind, Limits, LiveJournal, Passphrase, Rebuilt,
    RestoreRequest, VerifyRequest, SCOPE_NOTICE,
};
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::prompt::require_confirmation;
use serde_json::{json, Value};

use super::passphrase::{self, Use};
use super::{cli_error, office_home, show, RestoreArgs};
use crate::error::CliError;

pub(crate) fn run(args: RestoreArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match (&args.verify, &args.archive, &args.org) {
        (Some(archive), None, None) => run_verify(&args, archive, ctx),
        (None, Some(archive), Some(org)) => run_in_place(&args, archive, org, ctx),
        _ => Err(
            CliError::usage("give --verify <archive>, or an archive and --org <org-id>")
                .with_help("agentic-org restore --help"),
        ),
    }
}

/// The passphrase, only when the archive is encrypted.
fn passphrase_for(
    args: &RestoreArgs,
    archive: &Path,
    ctx: &ExecutionContext,
) -> Result<Option<Passphrase>, CliError> {
    match archive_kind(archive).map_err(super::cli_error)? {
        ArchiveKind::Encrypted => Ok(Some(passphrase::read(&args.passphrase, ctx, Use::Open)?)),
        ArchiveKind::Plain => Ok(None),
    }
}

fn rebuilt(rebuilt: &Rebuilt) -> Value {
    json!({
        "head_ordinal": rebuilt.head_ordinal,
        "stopped_cleanly": rebuilt.stopped_cleanly,
        "offices": rebuilt.offices,
        "employees": rebuilt.employees,
        "projects": rebuilt.projects,
    })
}

fn run_verify(
    args: &RestoreArgs,
    archive: &Path,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    if ctx.modes().dry_run {
        let plan = json!({
            "action": "restore_verify",
            "archive": archive.display().to_string(),
            "to": args.to.as_ref().map(|to| to.display().to_string()),
            "work_dir": args.work_dir.as_ref().map(|dir| dir.display().to_string()),
        });
        return Ok(ctx.render_dry_run(&format!("would verify {}", archive.display()), &plan)?);
    }
    let passphrase = passphrase_for(args, archive, ctx)?;
    let report = verify(&VerifyRequest {
        archive,
        passphrase: passphrase.as_ref(),
        to: args.to.as_deref(),
        work_dir: args.work_dir.as_deref(),
        limits: Limits::default(),
    })
    .map_err(cli_error)?;
    let manifest = &report.manifest;
    let json = json!({
        "archive": archive.display().to_string(),
        "verified": true,
        "encrypted": report.encrypted,
        "org_id": manifest.org_id,
        "org_name": manifest.org_name,
        "office_id": manifest.office_id,
        "created_at": manifest.created_at,
        "journal_incarnation": manifest.journal.incarnation,
        "head_ordinal": manifest.journal.head_ordinal,
        "files": manifest.files.len(),
        "content_bytes": manifest.content_bytes(),
        "private_notes_checked": report.private_notes_checked,
        "private_user_space_checked": report.private_user_space_checked,
        "rebuilt": rebuilt(&report.rebuilt),
        "extracted_to": report.extracted_to.as_ref().map(|to| to.display().to_string()),
    });
    show(ctx, &manifest.office_id, &json)?;
    ctx.success(&format!(
        "{} verified: every digest matches, the office's projections rebuilt from it at journal ordinal {}.",
        archive.display(),
        report.rebuilt.head_ordinal
    ));
    Ok(())
}

fn run_in_place(
    args: &RestoreArgs,
    archive: &Path,
    org: &str,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let home = office_home(&args.instance)?;
    let target = org_backup::locate(&home, org, args.office.as_deref()).map_err(cli_error)?;
    let action = format!(
        "Replace the durable data of office {} of organization {} with {} (the current data is kept in the restore-backups folder beside it)",
        target.office_id,
        target.org_id,
        archive.display()
    );
    if ctx.modes().dry_run {
        let plan = json!({
            "action": "restore",
            "archive": archive.display().to_string(),
            "org_id": target.org_id,
            "office_id": target.office_id,
            "durable_root": target.durable.display().to_string(),
        });
        return Ok(ctx.render_dry_run(&format!("would {}", action.to_lowercase()), &plan)?);
    }
    require_confirmation(ctx, &action)?;
    let passphrase = passphrase_for(args, archive, ctx)?;
    let report = restore_in_place(&RestoreRequest {
        archive,
        passphrase: passphrase.as_ref(),
        office_home: &home,
        org,
        office: args.office.as_deref(),
        limits: Limits::default(),
    })
    .map_err(cli_error)?;
    let live = match &report.live_journal {
        LiveJournal::Missing => json!({ "state": "missing" }),
        LiveJournal::Unreadable(why) => json!({ "state": "unreadable", "reason": why }),
        LiveJournal::Head { ordinal } => json!({ "state": "head", "ordinal": ordinal }),
    };
    let mut json = json!({
        "restored": true,
        "archive": archive.display().to_string(),
        "org_id": report.manifest.org_id,
        "office_id": report.manifest.office_id,
        "head_ordinal": report.manifest.journal.head_ordinal,
        "durable_root": report.durable.display().to_string(),
        "previous_durable_root": report.previous_durable.as_ref().map(|p| p.display().to_string()),
        "live_journal": live,
        "rebuilt": rebuilt(&report.rebuilt),
    });
    // A person reads the notice once, as the closing warning; a machine gets it as a field.
    let human = !ctx.modes().json;
    if !human {
        json["scope_notice"] = SCOPE_NOTICE.into();
    }
    show(ctx, &report.manifest.office_id, &json)?;
    // A machine reads `live_journal.state`; under --json standard error stays empty.
    if human && matches!(report.live_journal, LiveJournal::Unreadable(_)) {
        ctx.warn(
            "The office's previous journal did not verify; it was kept in the restore-backups \
             folder beside the restored one.",
        );
    }
    ctx.success(&format!(
        "Restored office {} from {}. Start it with `agentic-org service start`.",
        report.manifest.office_id,
        archive.display()
    ));
    if human {
        ctx.warn(SCOPE_NOTICE);
    }
    Ok(())
}
