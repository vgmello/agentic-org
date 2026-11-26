//! Library entry point for `agent-os` and shared `org-command` execution for `agentic-org`.
//!
//! Keeps `apps/agent-os/src/main.rs` a thin composition root (`AGENTS.md` Rule 1,
//! Non-Negotiable #15). Legacy `org-spool` and file-queue fallbacks are removed.

#![forbid(unsafe_code)]

pub mod aliases;
pub mod attach;
pub mod boot;
pub mod budget_dispatch;
pub mod budget_table;
pub mod dispatch;
pub mod external_auth;
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
pub mod transport;

use std::borrow::Cow;
use std::io::{self, Write};

use aliases::normalize_args;
use keys::UuidV7KeySource;
use office::{Binding, BoundOffice};
use org_command::{Office, Output};
use sources::StdSources;
use transport::OfficeTransport;

/// Help text for `agent-os session attach`.
pub const SESSION_ATTACH_HELP: &str = "Usage: agent-os session attach\n\
Use when: batching multiple agent-os commands in one shell turn over a persistent office socket.\n\
Binds once to the office socket and executes one JSON command per line of stdin:\n  \
{\"command\":\"msg inbox\",\"input\":{},\"options\":{\"limit\":5}}\n\
A line has only `command`, `input` and `options`; every answer is one line of JSON, so the \
`format` option is json or absent, and `stdin` and `help` are refused.\n";

/// Runs the shared `org-command` table against any supplied [`Office`] implementation
/// (either a bound agent socket or a grant-bound person host channel). A shorthand (`who`,
/// `task note`, `boot`, `instructions overview`) is read as its command ([`normalize_args`]).
pub fn execute_with_office(program: &str, args: &[String], office: &mut dyn Office) -> Output {
    if matches!(args, [flag] if flag == "--version" || flag == "-V") {
        return Output {
            stdout: format!("{program} {}\n", env!("CARGO_PKG_VERSION")),
            exit_code: 0,
        };
    }
    let expanded = normalize_args(args);
    // `agent approve <code>` and the other operator verbs read their bare words as named flags.
    let effective_args = match external_auth::operator::rewrite_positional(&expanded) {
        Some(rewritten) => Cow::Owned(rewritten),
        None => expanded,
    };
    let table = table::runtime_table();
    let mut sources = StdSources;
    let mut keys = UuidV7KeySource;
    let output = org_command::run(
        table,
        program,
        &effective_args,
        &mut sources,
        &mut keys,
        office,
    );
    help_guidance::augment_help(table, program, &effective_args, output)
}

/// Runs `agent-os` with the supplied command-line arguments and binding, returning the
/// formatted stdout text and process exit code.
pub fn execute(args: &[String], binding: Binding) -> Output {
    execute_bound(args, &mut BoundOffice::new(binding))
}

/// Runs `agent-os` over `office`; `agent-os --help` opens with the quickstart of the shell's
/// binding state ([`help_guidance::quickstart`]).
fn execute_bound(args: &[String], office: &mut BoundOffice) -> Output {
    let mut output = execute_with_office("agent-os", args, office);
    if output.exit_code == 0 && matches!(args, [flag] if is_help_flag(flag)) {
        output.stdout = format!(
            "{}\n{}",
            help_guidance::quickstart(office.is_bound()),
            output.stdout
        );
    }
    output
}

fn is_help_flag(arg: &str) -> bool {
    arg == "--help" || arg == "-h"
}

/// The office of this process ([`external_auth::resolve_binding`]): its `AGENT_OS_*` binding,
/// bound when the environment names a session token or a binding file
/// ([`office::env_requests_binding`]), else the signed-in workspace of the working folder, bound
/// too. Either is bound even when its token cannot be read yet, so `boot` never drops to offline.
fn env_office() -> BoundOffice {
    BoundOffice::new(external_auth::resolve_binding())
        .with_binding_requested(office::env_requests_binding())
}

/// Runs `agent-os`, writing the single envelope or help output to stdout and returning the exit
/// code. `login|whoami|logout|exec` are the external lifecycle ([`external_auth`]); every other
/// command binds through `AGENT_OS_*` environment variables, else the signed-in workspace of the
/// working folder ([`external_auth::resolve_binding`]).
pub fn run(args: &[String]) -> u8 {
    if let Some((stdout, exit_code)) = external_auth::run_lifecycle(args) {
        return emit(&mut io::stdout().lock(), &stdout, exit_code);
    }
    if matches!(args, [a, b, ..] if a == "session" && b == "attach") {
        return run_session_attach(args, env_office());
    }
    let output = execute_bound(args, &mut env_office());
    emit(&mut io::stdout().lock(), &output.stdout, output.exit_code)
}

/// Writes `text` and answers the exit code. A reader that closed the pipe
/// (`agent-os ... | head -1`) is not a failure of its own: the command's `exit_code` stands
/// (0 for a success, 1 or 2 for a refusal, so `set -o pipefail` still sees it). Standard
/// output that fails otherwise (a full disk, an I/O error) is 1, which the caller must see.
pub fn emit(out: &mut dyn Write, text: &str, exit_code: u8) -> u8 {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => exit_code,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => exit_code,
        Err(_) => 1,
    }
}

fn run_session_attach(args: &[String], mut office: BoundOffice) -> u8 {
    if attach::wants_help(args) {
        return emit(&mut io::stdout().lock(), SESSION_ATTACH_HELP, 0);
    }
    // A flag is refused before anything binds: exit 2, as every command line that cannot be
    // read is.
    if let Some(refusal) = attach::usage_refusal(args) {
        let line = org_command::failure_envelope_json(&refusal, false);
        return emit(
            &mut io::stdout().lock(),
            &format!("{line}\n"),
            org_command::EXIT_USAGE,
        );
    }
    if let Err(refusal) = office.ensure_connected() {
        // The refusal's own hint says how to get a binding; exit 1 as a one-shot command's
        // refusal does (`doc-96` AX-15). Exit 2 is only for a command line that cannot be read.
        let line = org_command::failure_envelope_json(&refusal, false);
        return emit(&mut io::stdout().lock(), &format!("{line}\n"), 1);
    }
    attach::run_attach_loop(
        "agent-os",
        io::stdin().lock(),
        io::stdout().lock(),
        &mut office,
    )
}

#[cfg(test)]
mod ax_tests;

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::emit;

    struct Failing(io::ErrorKind);

    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_closed_pipe_keeps_the_exit_code_and_any_other_write_failure_exits_1() {
        let mut written = Vec::new();
        assert_eq!(emit(&mut written, "{}\n", 2), 2);
        assert_eq!(written, b"{}\n");
        assert_eq!(emit(&mut Failing(io::ErrorKind::BrokenPipe), "x", 0), 0);
        assert_eq!(emit(&mut Failing(io::ErrorKind::BrokenPipe), "x", 1), 1);
        assert_eq!(emit(&mut Failing(io::ErrorKind::StorageFull), "x", 0), 1);
        assert_eq!(emit(&mut Failing(io::ErrorKind::Other), "x", 0), 1);
    }
}
