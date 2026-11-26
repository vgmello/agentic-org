//! `office remove-pending`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, YesArgs};
use crate::completers;
use crate::error::CliError;
use serde_json::json;

use super::guard;
use super::restart::DONE;
use crate::selection;
use crate::view;

pub(super) const EXAMPLES: &str =
    "Examples:\n  agentic-org office remove-pending <office-id> --yes";

#[derive(Args, Debug)]
pub(crate) struct RemovePendingArgs {
    /// The pending office
    #[arg(value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(super) office_id: String,
    #[command(flatten)]
    pub(super) yes: YesArgs,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

pub(super) fn run(args: RemovePendingArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let plan = json!({"action": "office_remove_pending", "office_id": args.office_id});
    let action = format!("remove the pending enrollment of office {}", args.office_id);
    if !guard(ctx, &action, &plan)? {
        return Ok(());
    }
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let done = org_host::setup_flow::office_remove_pending(
        target.instance(),
        args.login.login.as_deref(),
        &args.office_id,
    )?;
    let json = view::json(&done)?;
    view::detail(ctx, &json, &json, "office_id", &DONE)?;
    ctx.success(&format!(
        "Removed the pending enrollment of office {}.",
        args.office_id
    ));
    Ok(())
}
