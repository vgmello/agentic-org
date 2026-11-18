//! Thin single-thread process entry; secrets arrive only after confinement.
#![deny(unsafe_code)]
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [flag] if matches!(flag.as_str(), "--version" | "-V") => {
            println!("agentic-org-web {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [flag] if matches!(flag.as_str(), "--help" | "-h") => {
            println!("agentic-org-web: the host-managed web process\nUsage: agentic-org-web --serve | --version | --help\n--serve: loads the verified release's ui/ build, confines itself, then accepts its listener and control connection only through the verified handoff of the host that spawned it. Without that handoff it opens no listener.");
            ExitCode::SUCCESS
        }
        [flag] if flag == "--serve" => {
            // The release is where this image's file is, or, for an image the host
            // started from a sealed copy, where its pinned path (the first argument
            // the host gives it) is.
            let result = agentic_org_web::process::release_root(
                std::env::current_exe(),
                std::env::args_os().next(),
            )
            .and_then(|root| agentic_org_web::process::serve(&root.join("ui")));
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    // One line: why it could not confine itself, or a fixed text.
                    eprintln!("{}", agentic_org_web::process::startup_failure_line(&error));
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("agentic-org-web: a verified host --serve handoff is required; no listener was opened");
            ExitCode::FAILURE
        }
    }
}
