//! `agentic-org provider ...`: organization-wide AI provider connections (on HQ) and their
//! runtime bindings and model reports on each office.

mod bind;
mod bindings;
mod create;
mod list;
mod models;
mod refresh;

use clap::{Args, Subcommand};
use org_cli_shell::context::ExecutionContext;

use crate::error::CliError;

const EXAMPLES: &str = "Examples:
  agentic-org provider create \"Claude team\" --key claude-team --provider anthropic --runtime claude
  agentic-org provider list
  agentic-org provider bind <connection-id> --auth-ref keychain:claude
  agentic-org provider models --json";

#[derive(Args, Debug)]
#[command(after_help = EXAMPLES)]
pub(crate) struct ProviderArgs {
    #[command(subcommand)]
    command: ProviderCommand,
}

#[derive(Subcommand, Debug)]
enum ProviderCommand {
    /// Create an organization-wide AI provider connection (on HQ)
    /// Use when: configuring a Claude, Codex or Agy connection for the organization.
    #[command(visible_alias = "new", verbatim_doc_comment, after_help = create::EXAMPLES)]
    Create(create::CreateArgs),
    /// List the organization's provider connections
    /// Use when: finding a connection ID before binding or refreshing it.
    #[command(visible_alias = "ls", verbatim_doc_comment, after_help = list::EXAMPLES)]
    List(list::ListArgs),
    /// Bind a provider connection to a runtime on this office
    /// Use when: making a connection usable by this office's agents.
    #[command(verbatim_doc_comment, after_help = bind::EXAMPLES)]
    Bind(bind::BindArgs),
    /// Report the models a bound runtime offers (driver version and model list)
    /// Use when: updating which models agents on this office may use.
    #[command(verbatim_doc_comment, after_help = refresh::EXAMPLES)]
    Refresh(refresh::RefreshArgs),
    /// List the runtime bindings of provider connections
    /// Use when: checking which offices can use a connection.
    #[command(verbatim_doc_comment, after_help = bindings::EXAMPLES)]
    Bindings(bindings::BindingsArgs),
    /// List the model profiles agents may use
    /// Use when: choosing a model for an agent.
    #[command(verbatim_doc_comment, after_help = models::EXAMPLES)]
    Models(models::ModelsArgs),
}

impl ProviderArgs {
    pub(crate) fn reads_a_secret(&self) -> bool {
        let login = match &self.command {
            ProviderCommand::Create(args) => &args.login,
            ProviderCommand::List(args) => &args.login,
            ProviderCommand::Bind(args) => &args.login,
            ProviderCommand::Refresh(args) => &args.login,
            ProviderCommand::Bindings(args) => &args.login,
            ProviderCommand::Models(args) => &args.login,
        };
        login.reads_a_secret()
    }
}

pub(crate) fn run(args: ProviderArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match args.command {
        ProviderCommand::Create(args) => create::run(args, ctx),
        ProviderCommand::List(args) => list::run(args, ctx),
        ProviderCommand::Bind(args) => bind::run(args, ctx),
        ProviderCommand::Refresh(args) => refresh::run(args, ctx),
        ProviderCommand::Bindings(args) => bindings::run(args, ctx),
        ProviderCommand::Models(args) => models::run(args, ctx),
    }
}
