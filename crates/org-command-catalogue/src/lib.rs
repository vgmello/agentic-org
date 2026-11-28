//! Pure declarative `agent-os` command catalogue (`TASK-254`).
//!
//! Owns every declarative [`Command`] row, input/output field schema, shorthand [`Alias`], and
//! [`MutationPolicy`] override for the `agent-os` command surface without linking any transport,
//! filesystem, socket, or CLI execution code. Both server-side crates (`org-office-server`,
//! `org-office-sessions`, `org-tess`) and the CLI runtime (`org-agent-cli`) depend downward on
//! this Layer-0 foundation crate.

#![forbid(unsafe_code)]

pub mod budget;
pub mod docs;
pub mod files;
pub mod git;
pub mod hiring;
pub mod messaging;
pub mod office;
pub mod organization;
pub mod processes;
pub mod projects;
pub mod providers;
pub mod reviews;
pub mod search;
pub mod session;
pub mod session_resume;
pub mod shared;
pub mod tess;
pub mod work;

use std::sync::OnceLock;

pub use org_command::{
    validate_input, Ack, Alias, Command, CommandTable, Example, Field, IfMatch, Kind,
    MutationPolicy, PolicyOverride, Type,
};

/// All declarative command row groups in canonical table order.
const DOMAIN_ROW_GROUPS: &[&[&[Command]]] = &[
    organization::ALL,
    projects::ALL,
    docs::ALL,
    hiring::ALL,
    session::ALL,
    messaging::ALL,
    work::ALL,
    reviews::ALL,
    files::ALL,
    search::ALL,
    git::ALL,
    processes::ALL,
    budget::ALL,
    providers::ALL,
    office::ALL,
    tess::ALL,
];

/// Declarative mutation-policy overrides on top of `Command::mutation_policy()` defaults
/// (`Read`/`List` -> `ReadOnly`, `Write` -> `ConfirmRequired`).
///
/// Enforced server-side by `org-office-server` when the Chief of Staff acts on behalf of a human
/// caller (`TASK-80.4`): `AutoSafe` actions run directly; `ConfirmRequired` mutations require a
/// preflighted `agent-os suggestion create` proposal; `ForbiddenForAgent` calls are always denied.
pub const POLICY_OVERRIDES: &[PolicyOverride] = &[
    // AutoSafe: proposal & UI presentation helpers, caller 1:1 DM reply (enforced server-side),
    // status line, private task notes.
    PolicyOverride {
        resource: "suggestion",
        verb: "create",
        policy: MutationPolicy::AutoSafe,
    },
    PolicyOverride {
        resource: "suggestion",
        verb: "withdraw",
        policy: MutationPolicy::AutoSafe,
    },
    PolicyOverride {
        resource: "ui",
        verb: "present",
        policy: MutationPolicy::AutoSafe,
    },
    PolicyOverride {
        resource: "status",
        verb: "set",
        policy: MutationPolicy::AutoSafe,
    },
    PolicyOverride {
        resource: "status",
        verb: "clear",
        policy: MutationPolicy::AutoSafe,
    },
    PolicyOverride {
        resource: "note",
        verb: "add",
        policy: MutationPolicy::AutoSafe,
    },
    PolicyOverride {
        resource: "note",
        verb: "update",
        policy: MutationPolicy::AutoSafe,
    },
    PolicyOverride {
        resource: "note",
        verb: "remove",
        policy: MutationPolicy::AutoSafe,
    },
    PolicyOverride {
        resource: "msg",
        verb: "reply",
        policy: MutationPolicy::AutoSafe,
    },
    // ForbiddenForAgent: security/governance boundaries no agent may ever execute directly.
    PolicyOverride {
        resource: "repo",
        verb: "push",
        policy: MutationPolicy::ForbiddenForAgent,
    },
    PolicyOverride {
        resource: "agent",
        verb: "execution-set",
        policy: MutationPolicy::ForbiddenForAgent,
    },
    PolicyOverride {
        resource: "agent",
        verb: "approve",
        policy: MutationPolicy::ForbiddenForAgent,
    },
    PolicyOverride {
        resource: "agent",
        verb: "revoke",
        policy: MutationPolicy::ForbiddenForAgent,
    },
    PolicyOverride {
        resource: "agent",
        verb: "sessions",
        policy: MutationPolicy::ForbiddenForAgent,
    },
    PolicyOverride {
        resource: "member",
        verb: "set-hiring",
        policy: MutationPolicy::ForbiddenForAgent,
    },
];

/// Every shorthand alias, in the order the catalogue lists them.
pub const ALIASES: &[Alias] = &[
    session::CONTEXT_ALIAS,
    session_resume::RESUME_ALIAS,
    session_resume::ME_ALIAS,
    work::TASK_NOTE_LEGACY_ALIAS,
    work::TASK_NOTE_ALIAS,
    organization::WHO_ALIAS,
];

/// Every domain's command row slices in canonical order.
pub fn rows() -> Vec<&'static [Command]> {
    DOMAIN_ROW_GROUPS
        .iter()
        .flat_map(|group| group.iter().copied())
        .collect()
}

/// Builds and validates the shared `agent-os` [`CommandTable`]: every row's names, fields, and
/// example are checked. Intended for contract tests; runtime paths use [`runtime_table`].
pub fn table() -> CommandTable {
    CommandTable::new(&rows())
        .expect("agent-os command table is valid")
        .with_aliases(ALIASES)
        .with_policies(POLICY_OVERRIDES)
}

/// The live `agent-os` [`CommandTable`], built once per process without re-validating every
/// example on startup (`doc-28` §2.11, `doc-7` #18). Contract tests on [`table`] verify every row.
pub fn runtime_table() -> &'static CommandTable {
    static TABLE: OnceLock<CommandTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        CommandTable::unchecked(&rows())
            .with_aliases(ALIASES)
            .with_policies(POLICY_OVERRIDES)
    })
}

/// Returns [`runtime_table`] by value for callers that take a table builder (`CommandCatalogue`).
pub fn catalogue_table() -> CommandTable {
    runtime_table().clone()
}

#[cfg(test)]
mod tests;
