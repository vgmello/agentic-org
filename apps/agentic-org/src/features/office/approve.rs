//! `office approve`.

use clap::Args;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str =
    "Examples:\n  agentic-org office approve <invitation-id> --code <check-code> --org <org-id>";

#[derive(Args, Debug)]
pub(crate) struct ApproveArgs {
    /// The invitation to approve
    #[arg(value_name = "INVITATION_ID")]
    pub(super) invitation_id: String,
    /// The check code the satellite printed when it joined
    #[arg(long, value_name = "CODE")]
    pub(super) code: String,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const FIELDS: [Column; 5] = [
    column("office_id", "office_id", Show::Id),
    column("status", "invitation.status", Show::Status),
    column("slug", "slug", Show::Plain),
    column("office_code", "office_code", Show::Plain),
    column("cert_serial_hex", "cert_serial_hex", Show::Dim),
];

pub(super) fn run(args: ApproveArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let approved = org_host::setup_flow::office_approve(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
        &args.invitation_id,
        &args.code,
    )?;
    let json = view::json(&approved)?;
    view::detail(ctx, &json, &json, "office_id", &FIELDS)?;
    ctx.success("Approved: the satellite completes its enrollment over mTLS.");
    Ok(())
}
