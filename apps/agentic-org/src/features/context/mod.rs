//! `agentic-org context show|use|clear` (A5 / SES-8): the saved default instance and office
//! that commands act on when no flag, variable or `agentic-org.toml` names them.

mod clear;
mod show;
mod use_cmd;

use clap::{Args, Subcommand};
use org_cli_shell::context::ExecutionContext;

use crate::error::CliError;

const EXAMPLES: &str = "Examples:
  agentic-org context show
  agentic-org context use --office <office-id>
  agentic-org context use --name lab --office <office-id> --json
  agentic-org context clear --yes";

#[derive(Args, Debug)]
#[command(after_help = EXAMPLES)]
pub(crate) struct ContextArgs {
    #[command(subcommand)]
    command: ContextCommand,
}

#[derive(Subcommand, Debug)]
enum ContextCommand {
    /// Show the saved instance and office
    /// Use when: checking which instance and office commands default to.
    #[command(verbatim_doc_comment, after_help = show::EXAMPLES)]
    Show(show::ShowArgs),
    /// Save an instance and office as the default target
    /// Use when: pinning the office your terminal commands act on.
    #[command(name = "use", verbatim_doc_comment, after_help = use_cmd::EXAMPLES)]
    Use(use_cmd::UseArgs),
    /// Forget the saved instance and office
    /// Use when: going back to explicit --name/--office or the built-in default.
    #[command(verbatim_doc_comment, after_help = clear::EXAMPLES)]
    Clear(clear::ClearArgs),
}

impl ContextArgs {
    pub(crate) fn yes(&self) -> bool {
        matches!(&self.command, ContextCommand::Clear(args) if args.yes.yes)
    }
}

pub(crate) fn run(args: ContextArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match args.command {
        ContextCommand::Show(args) => show::run(args, ctx),
        ContextCommand::Use(args) => use_cmd::run(args, ctx),
        ContextCommand::Clear(args) => clear::run(args, ctx),
    }
}
