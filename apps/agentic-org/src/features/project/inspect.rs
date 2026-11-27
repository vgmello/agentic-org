//! `project inspect`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use super::RECORD;
use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::completers;
use crate::error::CliError;
use crate::selection;
use crate::view;

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org project inspect billing\n  agentic-org project view <project-id> --json";

#[derive(Args, Debug)]
pub(crate) struct InspectArgs {
    /// The project's ID or slug
    #[arg(value_name = "PROJECT", add = ArgValueCompleter::new(completers::projects))]
    pub(super) project: String,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

pub(super) fn run(args: InspectArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let project = org_host_setup::project_inspect(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
        &args.project,
    )?;
    let json = view::json(&project)?;
    view::detail(ctx, &json, &json, "id", &RECORD)
}
