//! Library entry point for `agent-os` and shared `org-command` execution for `agentic-org`.
//!
//! Keeps `apps/agent-os/src/main.rs` a thin composition root (`AGENTS.md` Rule 1,
//! Non-Negotiable #15). The crate is a two-tier vertical-slice layout
//! (`.agents/rules/architecture-and-layout.md`): `runtime`, the shared headless kernel;
//! `features/<domain>`, one bounded-context slice each; and `registry`, the one composition
//! point. This root composes them and states the narrow public API; none of the three tiers is
//! public (`crates/org-platform/tests/vsa_structure.rs`).

#![forbid(unsafe_code)]

mod features;
mod registry;
mod runtime;

use std::borrow::Cow;
use std::io::{self, BufRead, Write};

use org_command::{Office, Output};

pub use features::session::{CORE_RULES, SESSION_ATTACH_HELP, SKILL_INDEX};
pub use registry::{catalogue_table, runtime_table, table};
pub use runtime::binding::{api_refusal, Binding};
pub use runtime::office::{BoundOffice, WorkspaceConnector};
pub use runtime::transport::{hint_for, OfficeTransport, Refusal, TransportOffice};

/// External ("bring your own agent") workspace sign-in (`TASK-137`): the office-socket calls
/// and key of a workspace ([`external_auth::client`]), its saved state
/// ([`external_auth::store`]), and the `agentic-org` words `agent-os exec` approves with.
pub mod external_auth {
    pub use crate::features::auth::{client, operator_approve_args, store};
}

use features::auth;
use runtime::help::quickstart;

/// Runs the shared `org-command` table against any supplied [`Office`] implementation
/// (either a bound agent socket or a grant-bound person host channel). A shorthand (`who`,
/// `task note`, `boot`, `instructions overview`) is read as its command.
pub fn execute_with_office(program: &str, args: &[String], office: &mut dyn Office) -> Output {
    if matches!(args, [flag] if flag == "--version" || flag == "-V") {
        return Output {
            stdout: format!("{program} {}\n", env!("CARGO_PKG_VERSION")),
            exit_code: 0,
        };
    }
    let expanded = registry::normalize_args(args);
    // `agent approve <code>` and the other operator verbs read their bare words as named flags.
    let effective_args = match registry::rewrite_positional(&expanded) {
        Some(rewritten) => Cow::Owned(rewritten),
        None => expanded,
    };
    let table = registry::runtime_table();
    let mut sources = runtime::sources::StdSources;
    let mut keys = runtime::keys::UuidV7KeySource;
    let output = org_command::run(
        table,
        program,
        &effective_args,
        &mut sources,
        &mut keys,
        office,
    );
    registry::augment_help(program, &effective_args, output)
}

/// Runs `agent-os` with the supplied command-line arguments and binding, returning the
/// formatted stdout text and process exit code.
pub fn execute(args: &[String], binding: Binding) -> Output {
    execute_bound(args, &mut auth::office_for(binding))
}

/// Runs `agent-os` over `office`; `agent-os --help` opens with the quickstart of the shell's
/// binding state.
fn execute_bound(args: &[String], office: &mut BoundOffice) -> Output {
    let mut output = execute_with_office("agent-os", args, &mut TransportOffice(&mut *office));
    if output.exit_code == 0 && matches!(args, [flag] if is_help_flag(flag)) {
        output.stdout = format!("{}\n{}", quickstart(office.is_bound()), output.stdout);
    }
    output
}

fn is_help_flag(arg: &str) -> bool {
    arg == "--help" || arg == "-h"
}

/// Runs `agent-os`, writing the single envelope or help output to stdout and returning the exit
/// code. `login|whoami|logout|exec` are the external lifecycle; every other command binds
/// through `AGENT_OS_*` environment variables, else the signed-in workspace of the working
/// folder.
pub fn run(args: &[String]) -> u8 {
    if let Some((stdout, exit_code)) = registry::run_lifecycle(args) {
        return emit(&mut io::stdout().lock(), &stdout, exit_code);
    }
    if matches!(args, [a, b, ..] if a == "session" && b == "attach") {
        return run_session_attach(args, auth::env_office());
    }
    let output = execute_bound(args, &mut auth::env_office());
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

/// Runs the `session attach` line loop (`doc-28` §2.9) over arbitrary `reader`, `writer`, and
/// `office`: one JSON command per line, one single-line JSON envelope per line. Answers 0 when
/// the input ends or the reader of `writer` goes away (a closed pipe), and 1 when `writer` fails
/// otherwise.
pub fn run_attach_loop<R: BufRead, W: Write>(
    program: &str,
    reader: R,
    writer: W,
    office: &mut dyn Office,
) -> u8 {
    registry::run_attach_loop(program, reader, writer, office)
}

fn run_session_attach(args: &[String], mut office: BoundOffice) -> u8 {
    if features::session::wants_help(args) {
        return emit(&mut io::stdout().lock(), SESSION_ATTACH_HELP, 0);
    }
    // A flag is refused before anything binds: exit 2, as every command line that cannot be
    // read is.
    if let Some(refusal) = features::session::usage_refusal(args) {
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
    run_attach_loop(
        "agent-os",
        io::stdin().lock(),
        io::stdout().lock(),
        &mut TransportOffice(office),
    )
}

#[cfg(test)]
mod ax_tests;

#[cfg(test)]
mod tests;
