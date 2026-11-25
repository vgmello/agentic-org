//! Human CLI and service-host composition. Agent tools use the separate agent-os binary.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    // The first action, before an argument is read, a thread exists or key material is held:
    // no core dump, and where the system has the attribute this process is no longer open to
    // the other processes of its user (the platform's one call; the core-dump limit is what
    // every system does). Every command gets it: the service, and the commands that take a
    // password or a recovery code.
    if let Err(error) = org_host::harden_process() {
        eprintln!("agentic-org: cannot close this process to other processes: {error}");
        return ExitCode::FAILURE;
    }
    match agentic_org::run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("agentic-org: {message}");
            ExitCode::FAILURE
        }
    }
}
