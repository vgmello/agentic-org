//! `provider create`.

use clap::Args;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::error::CliError;
use org_host::setup_flow::ProviderCreateInput;
use serde_json::json;

use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org provider create \"Claude team\" --key claude-team --provider anthropic --runtime claude\n  agentic-org provider create Codex --key codex --provider openai --runtime codex --auth-mode subscription --dry-run";

#[derive(Args, Debug)]
pub(crate) struct CreateArgs {
    /// The connection's display name
    #[arg(id = "display_name", value_name = "NAME")]
    pub(super) name: String,
    /// Its unique key
    #[arg(long)]
    pub(super) key: String,
    /// The provider type (for example anthropic, openai, google)
    #[arg(long = "provider", value_name = "PROVIDER")]
    pub(super) provider_type: String,
    /// The runtime driver (for example claude, codex, agy)
    #[arg(long = "runtime", value_name = "RUNTIME")]
    pub(super) runtime_driver: String,
    /// How it authenticates
    #[arg(long, value_name = "MODE")]
    pub(super) auth_mode: Option<String>,
    /// The subscription identity, for subscription authentication
    #[arg(long = "subscription-id", value_name = "ID")]
    pub(super) subscription_identity: Option<String>,
    /// The most turns a run may take
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    pub(super) max_turns: Option<u64>,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const FIELDS: [Column; 4] = [
    column("connection_id", "connection_id", Show::Id),
    column("default_profile_id", "default_profile_id", Show::Plain),
    column("generation", "generation", Show::Dim),
    column("revision", "revision", Show::Dim),
];

pub(super) fn run(args: CreateArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let input = ProviderCreateInput {
        org_id: args.org.org.as_deref(),
        name: &args.name,
        key: &args.key,
        provider_type: &args.provider_type,
        runtime_driver: &args.runtime_driver,
        auth_mode: args.auth_mode.as_deref().unwrap_or("subscription_cli"),
        subscription_identity: args.subscription_identity.as_deref(),
        max_concurrent_turns: args.max_turns.unwrap_or(2),
    };
    if ctx.modes().dry_run {
        let plan = json!({
            "action": "provider_create",
            "name": input.name,
            "key": input.key,
            "provider_type": input.provider_type,
            "runtime_driver": input.runtime_driver,
            "auth_mode": input.auth_mode,
            "max_concurrent_turns": input.max_concurrent_turns,
        });
        let summary = format!(
            "would create provider connection \"{}\" ({})",
            input.name, input.key
        );
        return Ok(ctx.render_dry_run(&summary, &plan)?);
    }
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let created = org_host::setup_flow::provider_create(
        target.instance(),
        args.login.login.as_deref(),
        &input,
    )?;
    let json = view::json(&created)?;
    view::detail(ctx, &json, &json, "connection_id", &FIELDS)?;
    ctx.success(&format!(
        "Created provider connection \"{}\" ({}).",
        created.name, created.key
    ));
    Ok(())
}
