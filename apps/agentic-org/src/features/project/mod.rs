//! `agentic-org project list|inspect|create`: an organization's projects and their owning
//! teams. Signs in as the other host commands do: the local session, a browser approval when
//! the host requires authentication, or `--login` with a password.

mod create;
mod inspect;
mod list;

use clap::{Args, Subcommand};
use org_cli_shell::context::ExecutionContext;

use crate::error::CliError;
use crate::view::{column, Column, Show};

/// The columns of one project, as `project inspect` and `project create` show it.
const RECORD: [Column; 9] = [
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

const EXAMPLES: &str = "Examples:
  agentic-org project list
  agentic-org project inspect billing --json
  agentic-org project create Billing --slug billing --owner-team <team-id>";

#[derive(Args, Debug)]
#[command(after_help = EXAMPLES)]
pub(crate) struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommand,
}

#[derive(Subcommand, Debug)]
enum ProjectCommand {
    /// List the organization's projects
    /// Use when: discovering projects and their owning teams.
    #[command(visible_alias = "ls", verbatim_doc_comment, after_help = list::EXAMPLES)]
    List(list::ListArgs),
    /// Show one project
    /// Use when: reading a project's slug, owning team and state.
    #[command(visible_alias = "view", verbatim_doc_comment, after_help = inspect::EXAMPLES)]
    Inspect(inspect::InspectArgs),
    /// Create a project and its owning team
    /// Use when: starting a new project for the organization.
    #[command(visible_alias = "new", verbatim_doc_comment, after_help = create::EXAMPLES)]
    Create(create::CreateArgs),
}

impl ProjectArgs {
    pub(crate) fn reads_a_secret(&self) -> bool {
        match &self.command {
            ProjectCommand::List(args) => args.login.reads_a_secret(),
            ProjectCommand::Inspect(args) => args.login.reads_a_secret(),
            ProjectCommand::Create(args) => args.login.reads_a_secret(),
        }
    }
}

pub(crate) fn run(args: ProjectArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match args.command {
        ProjectCommand::List(args) => list::run(args, ctx),
        ProjectCommand::Inspect(args) => inspect::run(args, ctx),
        ProjectCommand::Create(args) => create::run(args, ctx),
    }
}
