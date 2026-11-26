//! `office cancel`.

use clap::Args;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg, YesArgs};
use crate::error::CliError;
use serde_json::json;

use super::guard;
use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str =
    "Examples:\n  agentic-org office cancel <invitation-id> --org <org-id> --yes";

#[derive(Args, Debug)]
pub(crate) struct CancelArgs {
    /// The invitation to cancel
    #[arg(value_name = "INVITATION_ID")]
    pub(super) invitation_id: String,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) yes: YesArgs,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const FIELDS: [Column; 4] = [
    column("invitation_id", "invitation.invitation_id", Show::Id),
    column("status", "invitation.status", Show::Status),
    column("office_id", "invitation.office_id", Show::Plain),
    column("office_name", "invitation.office_name", Show::Name),
];

pub(super) fn run(args: CancelArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let plan = json!({"action": "office_cancel", "invitation_id": args.invitation_id, "org_id": args.org.org});
    if !guard(
        ctx,
        &format!("cancel invitation {}", args.invitation_id),
        &plan,
    )? {
        return Ok(());
    }
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let done = org_host::setup_flow::office_cancel(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
        &args.invitation_id,
    )?;
    let json = view::json(&done)?;
    view::detail(ctx, &json, &json, "invitation.invitation_id", &FIELDS)?;
    ctx.success(&format!("Cancelled invitation {}.", args.invitation_id));
    Ok(())
}
