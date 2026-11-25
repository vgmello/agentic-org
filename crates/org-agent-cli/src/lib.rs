//! Library entry point for `agent-os` and shared `org-command` execution for `agentic-org`.
//!
//! Keeps `apps/agent-os/src/main.rs` a thin composition root (`AGENTS.md` Rule 1,
//! Non-Negotiable #15). Legacy `org-spool` and file-queue fallbacks are removed.

#![forbid(unsafe_code)]

pub mod attach;
pub mod budget_dispatch;
pub mod budget_table;
pub mod dispatch;
pub mod git_dispatch;
pub mod git_table;
pub mod help_guidance;
pub mod hiring_dispatch;
pub mod hiring_table;
pub mod keys;
pub mod msg_dispatch;
pub mod msg_table;
pub mod office;
pub mod process_dispatch;
pub mod process_table;
pub mod session_dispatch;
pub mod session_table;
pub mod sources;
pub mod table;
pub mod task_dispatch;
pub mod task_table;

use std::io::{self, Write};

use keys::UuidV7KeySource;
use office::{Binding, BoundOffice};
use org_command::{Office, Output};
use sources::StdSources;

/// Help text for `agent-os session attach`.
pub const SESSION_ATTACH_HELP: &str = "Usage: agent-os session attach\n\
Use when: batching multiple agent-os commands in one shell turn over a persistent office socket.\n\
Binds once to the office socket and executes one JSON command per line of stdin:\n  \
{\"command\":\"msg inbox\",\"input\":{},\"options\":{\"limit\":5}}\n";

/// Runs the shared `org-command` table against any supplied [`Office`] implementation
/// (either a bound agent socket or a grant-bound person host channel).
pub fn execute_with_office(program: &str, args: &[String], office: &mut dyn Office) -> Output {
    if matches!(args, [flag] if flag == "--version" || flag == "-V") {
        return Output {
            stdout: format!("{program} {}\n", env!("CARGO_PKG_VERSION")),
            exit_code: 0,
        };
    }
    let normalized: Vec<String>;
    let effective_args: &[String] = if args.first().map(String::as_str) == Some("who") {
        let mut rewritten = Vec::with_capacity(args.len() + 1);
        rewritten.push("directory".to_owned());
        rewritten.push("list".to_owned());
        rewritten.extend_from_slice(&args[1..]);
        normalized = rewritten;
        &normalized
    } else if matches!(args, [a, b, ..] if a == "task" && b == "note") {
        let mut rewritten = Vec::with_capacity(args.len().saturating_sub(1));
        rewritten.push("task-note".to_owned());
        rewritten.extend_from_slice(&args[2..]);
        normalized = rewritten;
        &normalized
    } else {
        args
    };
    let table = table::table();
    let mut sources = StdSources;
    let mut keys = UuidV7KeySource;
    let output = org_command::run(
        &table,
        program,
        effective_args,
        &mut sources,
        &mut keys,
        office,
    );
    help_guidance::augment_help(&table, program, effective_args, output)
}

/// Runs `agent-os` with the supplied command-line arguments and binding, returning the
/// formatted stdout text and process exit code.
pub fn execute(args: &[String], binding: Binding) -> Output {
    let mut office = BoundOffice::new(binding);
    execute_with_office("agent-os", args, &mut office)
}

/// Runs `agent-os` using `AGENT_OS_*` environment variables, writing the single envelope or
/// help output to stdout and returning the exit code.
pub fn run(args: &[String]) -> u8 {
    if matches!(args, [a, b, ..] if a == "session" && b == "attach") {
        return run_session_attach(args, Binding::from_env());
    }
    let output = execute(args, Binding::from_env());
    let _ = io::stdout().write_all(output.stdout.as_bytes());
    output.exit_code
}

fn run_session_attach(args: &[String], binding: Binding) -> u8 {
    if matches!(args, [_, _, flag] if flag == "--help" || flag == "-h") {
        let _ = io::stdout().write_all(SESSION_ATTACH_HELP.as_bytes());
        return 0;
    }
    let mut office = BoundOffice::new(binding);
    if let Err(refusal) = office.ensure_connected() {
        let line = attach::error_envelope(refusal.code, &refusal.message);
        let _ = writeln!(io::stdout(), "{line}");
        return 2;
    }
    attach::run_attach_loop(
        "agent-os",
        io::stdin().lock(),
        io::stdout().lock(),
        &mut office,
    )
}
