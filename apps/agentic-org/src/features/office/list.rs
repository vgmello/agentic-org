//! `office list`.

use clap::Args;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::error::CliError;
use crate::view::{self, column, Column, Show};
use crate::{completers, selection};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org office list\n  agentic-org office ls --org <org-id> -q\n  agentic-org office list --json";

#[derive(Args, Debug)]
pub(crate) struct ListArgs {
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const COLUMNS: [Column; 7] = [
    column("ID", "id", Show::Id),
    column("ROLE", "role", Show::Status),
    column("STATUS", "status", Show::Status),
    column("NAME", "name", Show::Name),
    column("ORGANIZATION", "organization_id", Show::Dim),
    column("HOST", "host", Show::Plain),
    column("REASON", "status_reason", Show::Dim),
];

pub(super) fn run(args: ListArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let listed = org_host_setup::office_list(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
    )?;
    let json = view::json(&listed)?;
    completers::remember(
        target.instance(),
        completers::OFFICES,
        view::candidates(&json, "offices", "id", "name"),
    );
    let rows = view::items(&json, "offices");
    view::list(ctx, rows, &json, "id", &COLUMNS, "No offices.")
}
