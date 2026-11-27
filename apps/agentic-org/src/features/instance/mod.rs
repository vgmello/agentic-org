//! `agentic-org instance rebind` (`TASK-60`, FC-148): explicit, fail-closed repair of an
//! instance's recorded filesystem identity after its root directory was copied or restored onto
//! a new inode/device.

mod rebind;

use clap::{Args, Subcommand};
use org_cli_shell::context::ExecutionContext;

use crate::error::CliError;

const EXAMPLES: &str = "Examples:
  agentic-org instance rebind --dry-run
  agentic-org instance rebind lab --yes
  agentic-org instance rebind --name lab --yes --json";

#[derive(Args, Debug)]
#[command(after_help = EXAMPLES)]
pub(crate) struct InstanceArgs {
    #[command(subcommand)]
    command: InstanceCommand,
}

#[derive(Subcommand, Debug)]
enum InstanceCommand {
    /// Rebind an instance's recorded directory identity after copying or restoring its root
    /// Use when: host startup reports `RootUnavailable` after restoring or moving an instance root onto a new inode.
    #[command(verbatim_doc_comment, after_help = rebind::EXAMPLES)]
    Rebind(rebind::RebindArgs),
}

impl InstanceArgs {
    pub(crate) fn yes(&self) -> bool {
        match &self.command {
            InstanceCommand::Rebind(args) => args.yes.yes,
        }
    }
}

pub(crate) fn run(args: InstanceArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match args.command {
        InstanceCommand::Rebind(args) => rebind::run(args, ctx),
    }
}
