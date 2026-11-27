//! `provider list`.

use clap::Args;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org provider list\n  agentic-org provider ls --include-archived -q\n  agentic-org provider list --json";

#[derive(Args, Debug)]
pub(crate) struct ListArgs {
    /// Include archived connections
    #[arg(long)]
    pub(super) include_archived: bool,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const COLUMNS: [Column; 7] = [
    column("ID", "id", Show::Id),
    column("KEY", "key", Show::Plain),
    column("NAME", "name", Show::Name),
    column("PROVIDER", "provider_type", Show::Plain),
    column("RUNTIME", "runtime_driver", Show::Plain),
    column("AUTH", "auth_mode", Show::Dim),
    column("ENABLED", "enabled", Show::Plain),
];

pub(super) fn run(args: ListArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let listed = org_host_setup::provider_list(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
        args.include_archived,
    )?;
    let json = view::json(&listed)?;
    let rows = view::items(&json, "connections");
    view::list(ctx, rows, &json, "id", &COLUMNS, "No provider connections.")
}
