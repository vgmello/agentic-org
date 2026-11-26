//! Standing agent instructions in static-to-volatile prefix order, and per-turn objectives with
//! untrusted content fenced (`doc-27` §1, `FC-171`, `INSTR-4, 5, 7, 8, 12, 15, 21`).
//!
//! - [`assemble_standing_instructions`] builds the standing prompt. Byte 0 is release-static
//!   for every agent and turn: the built-in text, then the generated command catalogue; the
//!   organization's processes, the role, the agent's identity and its own prompt follow, most
//!   shared first; the project repository's instructions come back apart, to go last.
//! - [`compose_turn_objective`] builds a turn's input; every body that comes from someone else
//!   or from a tool sits in an untrusted-data fence with its sender.
//! - [`verify_command_references`] dry-parses every `agent-os` command line cited in text
//!   against the live command table: the drift guard of all built-in text.

use std::sync::OnceLock;

use org_command::CommandTable;

mod assemble;
mod identity;
mod objective;
mod verify;

pub use assemble::{assemble_standing_instructions, InstructionInput, StandingInstructions};
pub use identity::{resolve_placement, LeadLink, Placement};
pub use objective::{compose_turn_objective, TurnSender, TurnTrigger, MAX_TURN_BODY_BYTES};
pub use org_process::{BUILTIN_INSTRUCTIONS_MAX_BYTES, CORE_RULES, HR_LEAD, MANAGED_SESSION};
pub use verify::{cited_command_lines, verify_command_references};

/// Produces the live `agent-os` command table rendered into standing instructions (`INSTR-5`).
pub type CommandCatalogue = fn() -> CommandTable;

static INSTALLED_COMMAND_CATALOGUE: OnceLock<CommandCatalogue> = OnceLock::new();

/// Installs the process-wide `agent-os` command catalogue. The office binary's composition root
/// calls this once, before the server starts, with the CLI's own table builder, so this crate
/// never links the CLI implementation (`TASK-171`). Returns `false` when one was already set.
pub fn install_command_catalogue(catalogue: CommandCatalogue) -> bool {
    INSTALLED_COMMAND_CATALOGUE.set(catalogue).is_ok()
}

/// The catalogue installed by [`install_command_catalogue`], if any.
pub fn installed_command_catalogue() -> Option<CommandCatalogue> {
    INSTALLED_COMMAND_CATALOGUE.get().copied()
}

/// The live command table from `catalogue`, or an empty one. A composition root that never
/// installed a catalogue is reported once on stderr, so agents' missing command list is visible.
pub(crate) fn command_table_or_empty(catalogue: Option<CommandCatalogue>) -> CommandTable {
    static REPORTED_MISSING: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    if let Some(catalogue) = catalogue {
        return catalogue();
    }
    if !REPORTED_MISSING.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!(
            "org-office-sessions: warning: no agent-os command catalogue installed; standing instructions list no commands"
        );
    }
    CommandTable::unchecked(&[])
}

/// Notice surfaced to both the agent and the UI when running under the same OS user (`FC-102`).
pub const SAME_USER_ISOLATION_NOTICE: &str = "This agent runs in a private 0700 home directory under the host's OS user account; OS-level cross-user isolation is not active on this host. Do not read or modify files outside your workspace and private home.";

/// The heading and first line of the generated command catalogue.
const CATALOGUE_HEADING: &str =
    "## agent-os commands\nRun each as `agent-os <resource> <verb>`; add `--help` for its flags.";

/// The built-in standing text every managed agent starts with, release-static: the core rules,
/// then managed-session behavior (`INSTR-4`, at most [`BUILTIN_INSTRUCTIONS_MAX_BYTES`]).
pub fn builtin_instructions() -> String {
    format!(
        "{}\n\n{}",
        CORE_RULES.trim_end(),
        MANAGED_SESSION.trim_end()
    )
}

/// The launch catalogue generated from the live `org-command` table: one line per command, at
/// most [`org_command::CATALOGUE_LIMIT`] bytes with its heading (`INSTR-5`, `INSTR-7`).
pub fn render_command_catalogue(table: &CommandTable) -> String {
    let lines = table.catalogue();
    if lines.is_empty() {
        return CATALOGUE_HEADING.to_owned();
    }
    format!("{CATALOGUE_HEADING}\n{}", lines.trim_end())
}
