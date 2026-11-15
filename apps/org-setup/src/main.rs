//! Human CLI and service-host composition. Agent tools use the separate agent-os binary.

use std::process::ExitCode;

fn main() -> ExitCode {
    // Before concurrency or key material; retain the host's existing crash-dump policy.
    let limits = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: RLIMIT_CORE accepts this valid limits structure.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limits) } != 0 {
        eprintln!(
            "cannot disable core dumps: {}",
            std::io::Error::last_os_error()
        );
        return ExitCode::FAILURE;
    }
    match org_setup::run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("agentic-org: {message}");
            ExitCode::FAILURE
        }
    }
}
