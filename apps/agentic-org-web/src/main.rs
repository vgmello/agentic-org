//! Thin single-thread process entry; secrets arrive only after confinement.
#![forbid(unsafe_code)]
use std::io::{self, Write};
use std::process::ExitCode;

/// The exit code of a command line this executable does not take.
const EXIT_USAGE: u8 = 2;

/// Writes `text` to standard output: success once written or when the reader is gone (a
/// closed pipe), failure when the write fails otherwise.
fn print(text: &str) -> ExitCode {
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let arguments: Vec<&str> = arguments
        .iter()
        .map(|argument| argument.to_str().unwrap_or("\u{fffd}"))
        .collect();
    match arguments.as_slice() {
        ["--version" | "-V"] => print(&format!("agentic-org-web {}\n", env!("CARGO_PKG_VERSION"))),
        ["--help" | "-h"] => print("agentic-org-web: the host-managed web process\nUsage: agentic-org-web --serve | --version | --help\n--serve: loads the verified release's ui/ build, confines itself, then accepts its listener and control connection only through the verified handoff of the host that spawned it. Without that handoff it opens no listener.\n"),
        ["--serve"] => {
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
            // A command line it does not take: a usage error, best effort on a closed stderr.
            let _ = writeln!(
                io::stderr(),
                "agentic-org-web: a verified host --serve handoff is required; no listener was opened"
            );
            ExitCode::from(EXIT_USAGE)
        }
    }
}
