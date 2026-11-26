//! `office restart`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, YesArgs};
use crate::completers;
use crate::error::CliError;
use serde_json::json;

use super::{guard, DONE};
use crate::selection;
use crate::view;

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org office restart <office-id>\n  agentic-org office restart <office-id> --yes";

#[derive(Args, Debug)]
pub(crate) struct RestartArgs {
    /// The office to restart
    #[arg(value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(super) office_id: String,
    #[command(flatten)]
    pub(super) yes: YesArgs,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

pub(super) fn run(args: RestartArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let plan = json!({"action": "office_restart", "office_id": args.office_id});
    if !guard(ctx, &format!("restart office {}", args.office_id), &plan)? {
        return Ok(());
    }
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let done = org_host::setup_flow::office_restart(
        target.instance(),
        args.login.login.as_deref(),
        &args.office_id,
    )?;
    let json = view::json(&done)?;
    view::detail(ctx, &json, &json, "office_id", &DONE)?;
    ctx.success(&format!("Restarted office {}.", args.office_id));
    Ok(())
}
