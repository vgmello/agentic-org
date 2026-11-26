//! `project list`.

use clap::Args;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::error::CliError;
use crate::view::{self, column, Column, Show};
use crate::{completers, selection};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org project list\n  agentic-org project ls --include-archived -q\n  agentic-org project list --org <org-id> --json";

#[derive(Args, Debug)]
pub(crate) struct ListArgs {
    /// Include archived projects
    #[arg(long)]
    pub(super) include_archived: bool,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const COLUMNS: [Column; 6] = [
    column("ID", "id", Show::Id),
    column("KEY", "key", Show::Plain),
    column("SLUG", "slug", Show::Plain),
    column("NAME", "name", Show::Name),
    column("OWNER TEAM", "owner_team_id", Show::Dim),
    column("ARCHIVED", "archived", Show::Plain),
];

pub(super) fn run(args: ListArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let listed = org_host::setup_flow::project_list(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
        args.include_archived,
    )?;
    let json = view::json(&listed)?;
    completers::remember(
        target.instance(),
        completers::PROJECTS,
        view::candidates(&json, "projects", "slug", "name"),
    );
    let rows = view::items(&json, "projects");
    view::list(ctx, rows, &json, "id", &COLUMNS, "No projects.")
}
