//! `service start`.

use org_cli_shell::context::ExecutionContext;
use org_host::setup_flow::ServiceAction;

use crate::args::InstanceArgs;
use crate::error::CliError;

pub(super) fn run(args: InstanceArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    super::act(args, ServiceAction::Start, ctx)
}
