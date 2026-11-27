//! `office invitations`.

use clap::Args;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org office invitations --org <org-id>\n  agentic-org office invitations --json";

#[derive(Args, Debug)]
pub(crate) struct InvitationsArgs {
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const COLUMNS: [Column; 6] = [
    column("ID", "invitation_id", Show::Id),
    column("STATUS", "status", Show::Status),
    column("OFFICE", "office_id", Show::Dim),
    column("NAME", "office_name", Show::Name),
    column("EXPIRES (MS)", "expires_at_ms", Show::Dim),
    column("CERTIFICATE", "issued_cert_serial_hex", Show::Dim),
];

pub(super) fn run(args: InvitationsArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let listed = org_host_setup::office_invitations(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
    )?;
    let json = view::json(&listed)?;
    let rows = view::items(&json, "invitations");
    view::list(
        ctx,
        rows,
        &json,
        "invitation_id",
        &COLUMNS,
        "No invitations.",
    )
}
