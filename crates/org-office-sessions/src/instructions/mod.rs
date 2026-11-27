//! Standing agent instructions in static-to-volatile prefix order, and per-turn objectives with
//! untrusted content fenced (`doc-27` §1, `FC-171`, `INSTR-4, 5, 7, 8, 12, 15, 21`).
//!
//! - [`assemble_standing_instructions`] builds the standing prompt. Byte 0 is release-static
//!   for every agent and turn: the built-in text, then the live table's own grouped catalogue
//!   (`CommandTable::catalogue`, the one source of it); the
//!   organization's processes, the role, the agent's identity and its own prompt follow, most
//!   shared first; the project repository's instructions come back apart, to go last.
//! - [`compose_turn_objective`] builds a turn's input; every body that comes from someone else
//!   or from a tool sits in an untrusted-data fence with its sender.
//! - [`verify_command_references`] dry-parses every `agent-os` command line cited in text
//!   against the live command table: the drift guard of all built-in text.

use org_command::CommandTable;

mod assemble;
mod identity;
mod objective;
mod verify;

pub use assemble::{assemble_standing_instructions, InstructionInput, StandingInstructions};
pub use identity::{resolve_placement, LeadLink, Placement};
pub use objective::{compose_turn_objective, TurnSender, TurnTrigger, MAX_TURN_BODY_BYTES};
pub use org_process::{
    BUILTIN_INSTRUCTIONS_MAX_BYTES, CHIEF_OF_STAFF_GUIDANCE, CORE_RULES, HR_LEAD, MANAGED_SESSION,
};
pub use verify::{cited_command_lines, verify_command_references};

/// Produces the live `agent-os` command table rendered into standing instructions (`INSTR-5`).
pub type CommandCatalogue = fn() -> CommandTable;

/// Returns the static `agent-os` command catalogue (`TASK-254`).
pub fn installed_command_catalogue() -> Option<CommandCatalogue> {
    Some(org_command_catalogue::catalogue_table)
}

/// The live command table from `catalogue`, or an empty one. A caller that explicitly passes
/// `None` is reported once on stderr, so agents' missing command list is visible.
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

/// The built-in standing text every managed agent starts with, release-static: the core rules,
/// then managed-session behavior (`INSTR-4`, at most [`BUILTIN_INSTRUCTIONS_MAX_BYTES`]).
pub fn builtin_instructions() -> String {
    format!(
        "{}\n\n{}",
        CORE_RULES.trim_end(),
        MANAGED_SESSION.trim_end()
    )
}
