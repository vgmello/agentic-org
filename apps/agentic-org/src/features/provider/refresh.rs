//! `provider refresh`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::completers;
use crate::error::CliError;
use org_host::setup_flow::ProviderRefreshInput;
use serde_json::json;

use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org provider refresh <connection-id> --driver-version 1.4.0 --model sonnet:claude-sonnet-5-5,haiku:claude-haiku-5-5";

#[derive(Args, Debug)]
pub(crate) struct RefreshArgs {
    /// The connection
    #[arg(value_name = "CONNECTION_ID")]
    pub(super) connection_id: String,
    /// The runtime driver's version
    #[arg(long, value_name = "VERSION")]
    pub(super) driver_version: String,
    /// The models: comma-separated `model_id` or `model_id:driver_model_id`
    #[arg(long = "model", value_name = "MODELS")]
    pub(super) models: String,
    /// The account identity reference
    #[arg(long = "account-ref", value_name = "REF")]
    pub(super) account_identity_ref: Option<String>,
    /// How long the report stays valid, in seconds
    #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
    pub(super) ttl: Option<u64>,
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
    column("MODEL", "model_id", Show::Id),
    column("DRIVER MODEL", "driver_model_id", Show::Plain),
    column("ENTITLEMENT", "entitlement", Show::Status),
    column("VALID UNTIL", "valid_until", Show::Dim),
    column("STALE", "stale", Show::Plain),
];

/// `model_id` or `model_id:driver_model_id`, comma-separated.
fn model_specs(raw: &str) -> Result<Vec<(String, String)>, CliError> {
    let mut out = Vec::new();
    for item in raw
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let (model_id, driver_id) = match item.split_once(':') {
            Some((id, driver)) => (id.trim(), driver.trim()),
            None => (item, item),
        };
        if model_id.is_empty() || driver_id.is_empty() {
            return Err(CliError::usage(format!("invalid --model entry {item:?}")));
        }
        out.push((model_id.to_owned(), driver_id.to_owned()));
    }
    if out.is_empty() {
        return Err(CliError::usage("--model requires at least one model id"));
    }
    Ok(out)
}

pub(super) fn run(args: RefreshArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let input = ProviderRefreshInput {
        org_id: args.org.org.as_deref(),
        office_id: args.office.as_deref(),
        connection_id: &args.connection_id,
        driver_version: &args.driver_version,
        models: model_specs(&args.models)?,
        account_identity_ref: args.account_identity_ref.as_deref(),
        ttl_seconds: args.ttl,
    };
    if ctx.modes().dry_run {
        let plan = json!({
            "action": "provider_refresh",
            "connection_id": input.connection_id,
            "driver_version": input.driver_version,
            "models": input.models,
        });
        let summary = format!(
            "would report {} model(s) for connection {}",
            input.models.len(),
            input.connection_id
        );
        return Ok(ctx.render_dry_run(&summary, &plan)?);
    }
    let target = selection::select(args.instance.name.as_deref(), args.office.as_deref())?;
    let refreshed = org_host::setup_flow::provider_refresh(
        target.instance(),
        args.login.login.as_deref(),
        &input,
    )?;
    let json = view::json(&refreshed)?;
    let rows = view::items(&json, "models");
    view::list(
        ctx,
        rows,
        &json,
        "model_id",
        &COLUMNS,
        "No models reported.",
    )
}
