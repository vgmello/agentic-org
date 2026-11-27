//! `office sync-status`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::completers;
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org office sync-status\n  agentic-org office sync-status --org <org-id> --json";

#[derive(Args, Debug)]
pub(crate) struct SyncStatusArgs {
    /// The office to report on [default: this host's office]
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(super) office: Option<String>,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const FIELDS: [Column; 11] = [
    column("local_office", "local_office", Show::Id),
    column("hq_office", "hq_office", Show::Plain),
    column("org_id", "org_id", Show::Dim),
    column("epoch", "epoch", Show::Plain),
    column("seq", "seq", Show::Plain),
    column("highest_epoch", "highest_epoch", Show::Plain),
    column("protocol_level", "protocol_level", Show::Dim),
    column(
        "initial_snapshot_required",
        "initial_snapshot_required",
        Show::Plain,
    ),
    column("restore_pending", "restore_pending", Show::Plain),
    column("pending_work_count", "pending_work_count", Show::Plain),
    column(
        "pending_message_count",
        "pending_message_count",
        Show::Plain,
    ),
];

pub(super) fn run(args: SyncStatusArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), args.office.as_deref())?;
    let status = org_host_setup::office_sync_status(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
        target.office(),
    )?;
    let json = view::json(&status)?;
    view::detail(ctx, &json, &json, "local_office", &FIELDS)
}
