//! `service stop`: destructive (the instance's offices stop serving), so it asks on a
//! terminal (no by default) and needs `--yes` where no one can be asked, like `office
//! restart` and `context clear`. `--dry-run` previews it without asking.

use clap::Args;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::prompt::require_confirmation;
use org_host::setup_flow::ServiceAction;

use crate::args::{InstanceArgs, YesArgs};
use crate::error::CliError;

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org service stop\n  agentic-org service stop --yes\n  agentic-org service stop --dry-run";

#[derive(Args, Debug)]
pub(crate) struct StopArgs {
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) yes: YesArgs,
}

pub(super) fn run(args: StopArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    // Proceeds on `--yes` or `--dry-run` (which `act` previews); refuses (exit 2) without a
    // terminal to ask; asks otherwise, before the platform's service adapter is reached.
    require_confirmation(ctx, "Stop the instance's registered service")?;
    super::act(args.instance, ServiceAction::Stop, ctx)
}
