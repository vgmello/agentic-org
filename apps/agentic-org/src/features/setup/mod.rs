//! `agentic-org setup`: set up this computer's instance and register its service.

mod run;
#[cfg(test)]
mod tests;

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::error::ShellError;

pub(crate) use run::HELP;

use crate::error::CliError;
use crate::host::ReportOut;

/// `agentic-org setup ...`: its words reach its own parser as written (the order of its
/// refusals is a tested contract). Its report is the command's output, on standard output.
pub(crate) fn run(arguments: &[String], ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let mut out = ReportOut::new(std::io::stdout());
    match run::run(arguments, &mut out)? {
        run::SetUp::Help => Ok(ctx.out(&format!("{HELP}\n"))?),
        run::SetUp::Done if out.closed() => Err(ShellError::OutputClosed.into()),
        run::SetUp::Done => Ok(()),
    }
}
