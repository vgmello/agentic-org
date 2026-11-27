//! `office re-enroll`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, YesArgs};
use crate::completers;
use crate::error::CliError;
use serde_json::json;

use super::guard;
use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str =
    "Examples:\n  agentic-org office re-enroll <office-id> --invitation-file ./invite.txt --yes";

#[derive(Args, Debug)]
pub(crate) struct ReEnrollArgs {
    /// The satellite office
    #[arg(value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(super) office_id: String,
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

pub(super) fn run(args: ReEnrollArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let plan = json!({"action": "office_re_enroll", "office_id": args.office_id});
    let action = format!("re-enroll satellite office {}", args.office_id);
    if !guard(ctx, &action, &plan)? {
        return Ok(());
    }
    let invitation = args.invitation.read()?;
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let joined = org_host_setup::office_re_enroll(
        target.instance(),
        args.login.login.as_deref(),
        &args.office_id,
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
