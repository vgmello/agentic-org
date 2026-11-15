//! Process entry point reserved for the authenticated host handoff.
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [flag] if matches!(flag.as_str(), "--version" | "-V") => {
            println!("agentic-org-web {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [flag] if matches!(flag.as_str(), "--help" | "-h") => {
            println!("agentic-org-web: host-managed browser service\nStarted by agentic-org serve using an authenticated listener handoff.\nProduction handoff/authentication composition is not wired in v1 yet.");
            ExitCode::SUCCESS
        }
        _ => {
            // Do not promote the test-only CSP listener or fabricate authenticated backends.
            eprintln!("agentic-org-web: unavailable: authenticated host handoff and browser backend are not wired in v1 yet; no listener was opened");
            ExitCode::FAILURE
        }
    }
}
