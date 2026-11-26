//! `provider bind`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::completers;
use crate::error::CliError;
use org_host::setup_flow::ProviderBindInput;
use serde_json::json;

use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org provider bind <connection-id> --auth-ref keychain:claude\n  agentic-org provider bind <connection-id> --auth-ref env:ANTHROPIC_API_KEY --binary /usr/local/bin/claude --dry-run";

#[derive(Args, Debug)]
pub(crate) struct BindArgs {
    /// The connection to bind
    #[arg(value_name = "CONNECTION_ID")]
    pub(super) connection_id: String,
    /// Where this office finds the credential (a reference, never the secret itself)
    #[arg(long = "auth-ref", value_name = "REF")]
    pub(super) auth_context_ref: String,
    /// The runtime executable on this office
    #[arg(long = "binary", value_name = "PATH")]
    pub(super) runtime_binary_path: Option<String>,
    /// The account identity reference
    #[arg(long = "account-ref", value_name = "REF")]
    pub(super) account_identity_ref: Option<String>,
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

const FIELDS: [Column; 6] = [
    column("connection_id", "binding.connection_id", Show::Id),
    column("office_id", "binding.office_id", Show::Plain),
    column("auth_context_ref", "binding.auth_context_ref", Show::Plain),
    column(
        "runtime_binary_path",
        "binding.runtime_binary_path",
        Show::Plain,
    ),
    column("revision", "binding.revision", Show::Dim),
    column("replayed", "replayed", Show::Dim),
];

pub(super) fn run(args: BindArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), args.office.as_deref())?;
    let input = ProviderBindInput {
        org_id: args.org.org.as_deref(),
        office_id: target.office(),
        connection_id: &args.connection_id,
        auth_context_ref: &args.auth_context_ref,
        runtime_binary_path: args.runtime_binary_path.as_deref(),
        account_identity_ref: args.account_identity_ref.as_deref(),
    };
    if ctx.modes().dry_run {
        let plan = json!({
            "action": "provider_bind",
            "connection_id": input.connection_id,
            "office_id": input.office_id,
            "auth_context_ref": input.auth_context_ref,
            "runtime_binary_path": input.runtime_binary_path,
        });
        let summary = format!(
            "would bind provider connection {} on this office",
            input.connection_id
        );
        return Ok(ctx.render_dry_run(&summary, &plan)?);
    }
    let bound = org_host::setup_flow::provider_bind(
        target.instance(),
        args.login.login.as_deref(),
        &input,
    )?;
    let json = view::json(&bound)?;
    view::detail(ctx, &json, &json, "binding.connection_id", &FIELDS)?;
    ctx.success(&format!(
        "Bound provider connection {}.",
        args.connection_id
    ));
    Ok(())
}
