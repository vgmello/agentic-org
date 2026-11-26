//! `project inspect`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::completers;
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

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

pub(super) const RECORD: [Column; 9] = [
    column("id", "id", Show::Id),
    column("key", "key", Show::Plain),
    column("slug", "slug", Show::Plain),
    column("name", "name", Show::Name),
    column("owner_team_id", "owner_team_id", Show::Plain),
    column(
        "contributing_team_ids",
        "contributing_team_ids",
        Show::Plain,
    ),
    column("archived", "archived", Show::Plain),
    column("revision", "revision", Show::Dim),
    column("created_at", "created_at", Show::Dim),
];

pub(super) fn run(args: InspectArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let project = org_host::setup_flow::project_inspect(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
        &args.project,
    )?;
    let json = view::json(&project)?;
    view::detail(ctx, &json, &json, "id", &RECORD)
}
