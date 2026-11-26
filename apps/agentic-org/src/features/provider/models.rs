//! `provider models`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::completers;
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org provider models\n  agentic-org provider models --office <office-id> -q";

#[derive(Args, Debug)]
pub(crate) struct ModelsArgs {
    /// The office [default: this host's office]
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(super) office: Option<String>,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const COLUMNS: [Column; 5] = [
    column("ID", "id", Show::Id),
    column("KEY", "key", Show::Plain),
    column("NAME", "name", Show::Name),
    column("CONNECTIONS", "connection_ids", Show::Count),
    column("ARCHIVED", "archived", Show::Plain),
];

pub(super) fn run(args: ModelsArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), args.office.as_deref())?;
    let listed = org_host::setup_flow::provider_models(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
        args.office.as_deref(),
    )?;
    let json = view::json(&listed)?;
    let rows = view::items(&json, "profiles");
    view::list(ctx, rows, &json, "id", &COLUMNS, "No model profiles.")
}
