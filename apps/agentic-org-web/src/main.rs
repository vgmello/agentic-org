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
            println!("agentic-org-web: host-managed browser service\nRequires a verified host --serve handoff.\nGateway startup is currently available through debug/test instance composition; the agentic-org serve launcher is pending.");
            ExitCode::SUCCESS
        }
        [flag] if flag == "--serve" => {
            let result = std::env::current_exe().and_then(|exe| {
                let root = exe
                    .parent()
                    .ok_or_else(|| std::io::Error::other("missing release root"))?;
                agentic_org_web::process::serve(&root.join("ui"))
            });
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => {
                    eprintln!("agentic-org-web: authenticated startup unavailable");
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
