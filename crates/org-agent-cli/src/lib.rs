//! Library entry point for `agent-os` on top of `org-command` and `org-client`.
//!
//! Keeps `apps/agent-os/src/main.rs` a thin composition root (`AGENTS.md` Rule 1,
//! Non-Negotiable #15). Legacy `org-spool` and file-queue fallbacks are removed.

#![deny(unsafe_code)]

pub mod keys;
pub mod office;
pub mod sources;
pub mod table;

use std::io::{self, Write};

use keys::UuidV7KeySource;
use office::{Binding, BoundOffice};
use org_command::Output;
use sources::StdSources;

/// Runs `agent-os` with the supplied command-line arguments and binding, returning the
/// formatted stdout text and process exit code.
pub fn execute(args: &[String], binding: Binding) -> Output {
    if matches!(args, [flag] if flag == "--version" || flag == "-V") {
        return Output {
            stdout: format!("agent-os {}\n", env!("CARGO_PKG_VERSION")),
            exit_code: 0,
        };
    }
    let table = table::table();
    let mut sources = StdSources;
    let mut keys = UuidV7KeySource;
    let mut office = BoundOffice::new(binding);
    org_command::run(
        &table,
        "agent-os",
        args,
        &mut sources,
        &mut keys,
        &mut office,
    )
}

/// Runs `agent-os` using `AGENT_OS_*` environment variables, writing the single envelope or
/// help output to stdout and returning the exit code.
pub fn run(args: &[String]) -> u8 {
    let output = execute(args, Binding::from_env());
    let _ = io::stdout().write_all(output.stdout.as_bytes());
    output.exit_code
}
