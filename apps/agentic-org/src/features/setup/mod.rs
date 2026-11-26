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
    match run::run(arguments, may_ask(ctx, arguments), &mut out)? {
        run::SetUp::Help => Ok(ctx.out(&format!("{HELP}\n"))?),
        run::SetUp::Done if out.closed() => Err(ShellError::OutputClosed.into()),
        run::SetUp::Done => Ok(()),
    }
}

/// Whether setup may ask for its port: someone can be asked
/// ([`ExecutionContext::can_prompt`]: standard input and standard error are terminals, and
/// neither `--no-input` nor `AGENTIC_ORG_NO_INPUT=1` is on), and `--no-input` is not written
/// among setup's own words either.
fn may_ask(ctx: &ExecutionContext, arguments: &[String]) -> bool {
    ctx.can_prompt() && !arguments.iter().any(|arg| arg == run::NO_INPUT)
}

/// A command line setup cannot use is exit 2 with setup's help named; a refusal or a
/// failure is the host's (exit 1).
impl From<run::SetupError> for CliError {
    fn from(error: run::SetupError) -> Self {
        match error {
            run::SetupError::Usage(message) => {
                CliError::usage(message).with_help("agentic-org setup --help")
            }
            run::SetupError::Failed(message) => CliError::from(message),
        }
    }
}
