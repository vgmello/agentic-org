//! `backup <org> --out <path> [--encrypt]`.

use org_backup::{backup, BackupRequest};
use org_cli_shell::context::ExecutionContext;
use serde_json::json;

use super::passphrase::{self, Use};
use super::{cli_error, office_home, show, BackupArgs};
use crate::error::CliError;

pub(crate) fn run(args: BackupArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    if !args.encrypt && args.passphrase.given() {
        return Err(CliError::usage(
            "--passphrase-file and --passphrase-fd need --encrypt",
        ));
    }
    let home = office_home(&args.instance)?;
    if ctx.modes().dry_run {
        let target =
            org_backup::locate(&home, &args.org, args.office.as_deref()).map_err(cli_error)?;
        let plan = json!({
            "action": "backup",
            "org_id": target.org_id,
            "office_id": target.office_id,
            "durable_root": target.durable.display().to_string(),
            "out": args.out.display().to_string(),
            "encrypt": args.encrypt,
        });
        let line = format!(
            "would back up office {} to {}",
            target.office_id,
            args.out.display()
        );
        return Ok(ctx.render_dry_run(&line, &plan)?);
    }
    let passphrase = if args.encrypt {
        Some(passphrase::read(&args.passphrase, ctx, Use::Seal)?)
    } else {
        None
    };
    let report = backup(&BackupRequest {
        office_home: &home,
        org: &args.org,
        office: args.office.as_deref(),
        out: &args.out,
        passphrase: passphrase.as_ref(),
    })
    .map_err(cli_error)?;
    let manifest = &report.manifest;
    let json = json!({
        "archive": report.archive.display().to_string(),
        "archive_sha256": report.archive_sha256,
        "archive_bytes": report.archive_bytes,
        "encrypted": report.encrypted,
        "org_id": manifest.org_id,
        "office_id": manifest.office_id,
        "created_at": manifest.created_at,
        "journal_incarnation": manifest.journal.incarnation,
        "head_ordinal": manifest.journal.head_ordinal,
        "files": manifest.files.len(),
        "directories": manifest.directories.len(),
        "content_bytes": manifest.content_bytes(),
    });
    show(ctx, &report.archive.display().to_string(), &json)?;
    ctx.success(&format!(
        "Backed up office {} to {}. Check it with `agentic-org restore --verify {}`.",
        manifest.office_id,
        report.archive.display(),
        report.archive.display()
    ));
    Ok(())
}
