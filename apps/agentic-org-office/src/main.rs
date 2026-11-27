//! # agentic-org-office: the office daemon
//!
//! A thin binary: it moves its control pipe out of the way, installs a panic hook, wires
//! concrete runtime adapters into the session manager, and runs
//! `org_office_server::run_with_runtimes`. All logic lives in library crates.

#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::io::{self, Write};
use std::sync::Arc;

use org_agent_runtime::AgentRuntime;

/// What `--help` prints. The office is only ever started by its host.
const HELP: &str = "agentic-org-office: the host-managed office worker\n\
Started only by its host, which hands it a verified control pipe; run by hand it opens no \
socket and touches no office.\n";

/// Concrete runtime adapters injected from the office composition root (`TASK-216`).
fn production_runtimes() -> org_office_sessions::RuntimeRegistry {
    org_office_sessions::RuntimeRegistry::with_adapters([
        Arc::new(org_agent_runtime_claude::ClaudeRuntime::new()) as Arc<dyn AgentRuntime>,
        Arc::new(org_agent_runtime_codex::CodexRuntime::new()) as Arc<dyn AgentRuntime>,
        Arc::new(org_agent_runtime_agy::AgyRuntime::new()) as Arc<dyn AgentRuntime>,
    ])
}

/// The answer to `--help`/`-h` or `--version`/`-V` given alone, if that is the command line.
fn standalone_answer(args: &[&OsStr]) -> Option<String> {
    match args {
        [flag] if *flag == "--help" || *flag == "-h" => {
            Some(format!("{HELP}{}\n", org_office_server::USAGE))
        }
        [flag] if *flag == "--version" || *flag == "-V" => Some(format!(
            "agentic-org-office {}\n",
            env!("CARGO_PKG_VERSION")
        )),
        _ => None,
    }
}

/// Writes `text` to standard output: 0 once written or when the reader is gone (a closed
/// pipe), 1 when the write fails otherwise.
fn print(text: &str) -> i32 {
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => 0,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(_) => 1,
    }
}

fn main() {
    // Before startup concurrency or key material: inherited by all threads and children.
    if let Err(error) = org_platform::process::disable_core_dumps() {
        let _ = writeln!(io::stderr(), "cannot disable core dumps: {error}");
        std::process::exit(1);
    }

    // `--help` and `--version` answer before the descriptors are touched: nothing is opened,
    // moved or created.
    let raw: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let raw: Vec<&OsStr> = raw.iter().map(|arg| arg.as_os_str()).collect();
    if let Some(text) = standalone_answer(&raw) {
        std::process::exit(print(&text));
    }

    // First thing, before any thread or child exists (see `org_office_server::fdswap`).
    let control = match org_office_server::fdswap::take_control_pipe() {
        Ok(control) => control,
        Err(error) => {
            let _ = writeln!(
                io::stderr(),
                "agentic-org-office: cannot set up its descriptors: {error}"
            );
            std::process::exit(1);
        }
    };
    org_office_server::install_panic_hook();
    // Composition root: inject concrete storage backend and runtime adapters.
    org_office_sqlite::install();
    org_office_sessions::install_default_runtimes(production_runtimes);
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(org_office_server::run_with_runtimes(
        &args,
        control,
        production_runtimes(),
    ));
}
