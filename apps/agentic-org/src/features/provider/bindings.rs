//! `provider bindings`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::completers;
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org provider bindings\n  agentic-org provider bindings --connection <connection-id> --json";

#[derive(Args, Debug)]
pub(crate) struct BindingsArgs {
    /// Only this connection's bindings
    #[arg(long = "connection", value_name = "CONNECTION_ID")]
    pub(super) connection_id: Option<String>,
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

const COLUMNS: [Column; 6] = [
    column("CONNECTION", "binding.connection_id", Show::Id),
    column("OFFICE", "binding.office_id", Show::Dim),
    column("STATUS", "state.status", Show::Status),
    column("AUTH REF", "binding.auth_context_ref", Show::Plain),
    column("DRIVER", "state.driver_version", Show::Plain),
    column("MODELS", "models", Show::Count),
];

pub(super) fn run(args: BindingsArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), args.office.as_deref())?;
    let listed = org_host_setup::provider_bindings(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
        target.office(),
        args.connection_id.as_deref(),
    )?;
    let json = view::json(&listed)?;
    let rows = view::items(&json, "bindings");
    view::list(
        ctx,
        rows,
        &json,
        "binding.connection_id",
        &COLUMNS,
        "No provider bindings.",
    )
}
