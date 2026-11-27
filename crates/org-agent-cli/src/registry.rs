//! The one composition point of `agent-os`: every feature slice's rows, shorthands, guidance and
//! dispatch, assembled into the shared `org-command` table (B2.4 / B3, `doc-28` §3.2), its help,
//! and the [`Office`] the table runs against. No slice names another slice or this module.

use std::borrow::Cow;
use std::io::{BufRead, Write};
use std::sync::OnceLock;

use org_command::{
    Alias, Answer, Command, CommandTable, MutationPolicy, Office, Output, PolicyOverride, Refusal,
    Request,
};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::features::{
    auth, budget, files, git, hiring, messaging, office, organization, processes, projects,
    providers, session, tess, work,
};
use crate::runtime::domain::Domain;
use crate::runtime::help::{self, Guidance};
use crate::runtime::transport::{refuse_durable, OfficeTransport, TransportOffice};

/// Every slice of the table, in the order dispatch asks them.
const DOMAINS: &[Domain] = &[
    organization::DOMAIN,
    projects::DOMAIN,
    hiring::DOMAIN,
    session::DOMAIN,
    messaging::DOMAIN,
    work::DOMAIN,
    files::DOMAIN,
    git::DOMAIN,
    processes::DOMAIN,
    budget::DOMAIN,
    providers::DOMAIN,
    office::DOMAIN,
    tess::DOMAIN,
];

/// Declarative mutation-policy overrides on top of `Command::mutation_policy()` defaults
/// (`Read`/`List` -> `ReadOnly`, `Write` -> `ConfirmRequired`).
///
/// Enforced server-side by `org-office-server` when the Chief of Staff acts on behalf of a human
/// caller (`TASK-80.4`): `AutoSafe` actions run directly; `ConfirmRequired` mutations require a
/// preflighted `agent-os suggestion create` proposal; `ForbiddenForAgent` calls are always denied.
const POLICY_OVERRIDES: &[PolicyOverride] = &[
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
        resource: "task-note",
        verb: "add",
        policy: MutationPolicy::AutoSafe,
    },
    PolicyOverride {
        resource: "task-note",
        verb: "update",
        policy: MutationPolicy::AutoSafe,
    },
    PolicyOverride {
        resource: "task-note",
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

/// Every shorthand, in the order the catalogue lists them. The table rewrites them before it
/// reads any line, so a shorthand is the same command for a call, its `--help` and `--schema`,
/// every line of `session attach`, and the drift guard of the built-in text.
const ALIASES: &[Alias] = &[
    session::BOOT_ALIAS,
    session::INSTRUCTIONS_ALIAS,
    work::TASK_NOTE_ALIAS,
    organization::WHO_ALIAS,
];

/// What the help text adds to `org-command`'s: every slice's guidance, `session attach`, and the
/// sign-in verbs outside the table.
const GUIDANCE: Guidance = Guidance {
    domains: DOMAINS,
    operator_resources: session::OPERATOR_RESOURCES,
    agent_verbs: session::AGENT_VERBS,
    operator_verbs: hiring::OPERATOR_VERBS,
    agent_footer: auth::SIGN_IN_HELP,
};

/// What `agent-os login` answers from the rest of the crate.
const LOGIN_HOOKS: auth::LoginHooks = auth::LoginHooks {
    catalogue,
    briefing,
};

/// Every slice's rows: the one list both tables below are built from.
fn rows() -> Vec<&'static [Command]> {
    DOMAINS
        .iter()
        .flat_map(|domain| domain.commands.iter().copied())
        .collect()
}

/// Builds and validates the shared organization/project/HR/session/messaging command table:
/// every row's names, fields and example are checked. For the contract tests; a running
/// command uses [`runtime_table`].
pub fn table() -> CommandTable {
    CommandTable::new(&rows())
        .expect("agent-os command table is valid")
        .with_aliases(ALIASES)
        .with_policies(POLICY_OVERRIDES)
}

/// The same rows for a running command, built once per process and without the checks:
/// `agent-os` does nothing before its call that the call does not need (`doc-28` §2.11,
/// `doc-7` #18), and [`table`]'s contract tests are what make skipping the checks safe.
pub fn runtime_table() -> &'static CommandTable {
    static TABLE: OnceLock<CommandTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        CommandTable::unchecked(&rows())
            .with_aliases(ALIASES)
            .with_policies(POLICY_OVERRIDES)
    })
}

/// The [`runtime_table`] by value, for a composition root that takes a table builder (the
/// office's standing instructions, `org_office_sessions::install_command_catalogue`): the rows
/// unchecked, built once per process, never re-validating every example per session.
pub fn catalogue_table() -> CommandTable {
    runtime_table().clone()
}

/// The live table's launch catalogue.
fn catalogue() -> String {
    runtime_table().catalogue()
}

/// The bound `agent-os boot` briefing, for `agent-os login`.
fn briefing(transport: &mut dyn OfficeTransport) -> Result<Value, Refusal> {
    session::bound_briefing(transport, &catalogue())
}

/// `args` with a leading shorthand replaced by the command it stands for; anything else as it is.
pub(crate) fn normalize_args(args: &[String]) -> Cow<'_, [String]> {
    runtime_table().expand(args)
}

/// `agent approve <code>` and the other operator verbs with their bare words read as named flags,
/// plus `file render --format pdf` mapped onto `--to pdf` without colliding with `--format json|yaml|compact`.
pub(crate) fn rewrite_positional(args: &[String]) -> Option<Vec<String>> {
    hiring::rewrite_positional(runtime_table(), args).or_else(|| files::rewrite_render_args(args))
}

/// `org-command`'s plain-text help of `args`, enriched with every slice's `Use when:` guidance.
pub(crate) fn augment_help(program: &str, args: &[String], output: Output) -> Output {
    help::augment_help(runtime_table(), &GUIDANCE, program, args, output)
}

/// Runs `agent-os login|whoami|logout|exec`; `None` for any other command line.
pub(crate) fn run_lifecycle(args: &[String]) -> Option<(String, u8)> {
    auth::run_lifecycle(args, &LOGIN_HOOKS)
}

/// Runs the `session attach` line loop over the live table.
pub(crate) fn run_attach_loop<R: BufRead, W: Write>(
    program: &str,
    reader: R,
    writer: W,
    office: &mut dyn Office,
) -> u8 {
    session::run_attach_loop(runtime_table(), program, reader, writer, office)
}

/// Dispatches one parsed `org-command` [`Request`] over `transport` to the slice that owns its
/// command. A `--durable` the command cannot honor is refused here, before any slice sends
/// anything ([`refuse_durable`]).
fn execute(transport: &mut dyn OfficeTransport, request: &Request) -> Result<Answer, Refusal> {
    refuse_durable(request)?;
    let table = runtime_table();
    for domain in DOMAINS {
        if let Some(outcome) = (domain.dispatch)(transport, request, table) {
            return outcome;
        }
    }
    let (resource, verb) = (request.command.resource, request.command.verb);
    Err(Refusal::new(
        ErrorCode::Unsupported,
        format!("`{resource} {verb}` is not wired to the office yet."),
        "Run `--help` to see available commands.",
    ))
}

/// The command table's [`Office`] over a transport: each request is dispatched by [`execute`],
/// and answered with the `waiting_messages` hint of its own calls only.
impl<T: OfficeTransport> Office for TransportOffice<T> {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        // A hint left by a call outside this command (a direct call, a bind) is never reported.
        let _ = self.0.take_waiting_messages();
        let mut answer = execute(&mut self.0, request)?;
        answer.waiting_messages = self.0.take_waiting_messages();
        Ok(answer)
    }
}

#[cfg(test)]
mod tests;
