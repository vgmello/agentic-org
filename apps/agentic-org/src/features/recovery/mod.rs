//! `agentic-org recovery status|release`: the protected organization recovery client. Its
//! arguments reach the host's own parser as written, and recovery codes are read from the
//! controlling terminal only (`/dev/tty`), never from a pipe or the command line.

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::error::ShellError;

use crate::error::CliError;
use crate::host::ReportOut;

pub(crate) fn run(arguments: Vec<String>, _ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let mut out = ReportOut::new(std::io::stdout());
    org_host::human::recovery(&arguments, &mut out)?;
    if out.closed() {
        return Err(ShellError::OutputClosed.into());
    }
    Ok(())
}
