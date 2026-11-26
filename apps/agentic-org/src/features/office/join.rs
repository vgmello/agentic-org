//! `office join`.

use clap::Args;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, YesArgs};
use crate::error::CliError;
use serde_json::json;

use super::guard;
use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org office join --invitation-file ./invite.txt\n  agentic-org office join --invitation-stdin --yes < invite.txt";

#[derive(Args, Debug)]
pub(crate) struct JoinArgs {
    #[command(flatten)]
    pub(super) invitation: super::InvitationArgs,
    #[command(flatten)]
    pub(super) yes: YesArgs,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

pub(super) const JOINED: [Column; 7] = [
    column("office_id", "office_id", Show::Id),
    column("state", "state", Show::Status),
    column("check_code", "check_code", Show::Name),
    column("org_id", "org_id", Show::Plain),
    column("org_name", "org_name", Show::Name),
    column("office_name", "office_name", Show::Plain),
    column("hq_office_id", "hq_office_id", Show::Dim),
];

pub(super) fn run(args: JoinArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let plan = json!({"action": "office_join"});
    if !guard(
        ctx,
        "join this host to the organization as a satellite office",
        &plan,
    )? {
        return Ok(());
    }
    let invitation = args.invitation.read()?;
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let joined = org_host::setup_flow::office_join(
        target.instance(),
        args.login.login.as_deref(),
        invitation,
        true,
    )?;
    let json = view::json(&joined)?;
    view::detail(ctx, &json, &json, "office_id", &JOINED)?;
    if let Some(code) = json["check_code"].as_str() {
        ctx.note(&format!("Read the check code {code} to HQ's administrator, who approves it with `agentic-org office approve`."));
    }
    Ok(())
}
