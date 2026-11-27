//! The session slice: `session inspect|context` and `grant list` (C3, `CLI-9`, `CLI-10`), the
//! `context` shorthand, and `session attach`, the batch mode `agent-os` runs outside the table
//! ([`attach`]).

mod attach;
mod context;
mod inspect;
mod skills;

use org_command::{Answer, CommandTable, Request};

pub use attach::SESSION_ATTACH_HELP;
pub(crate) use attach::{run_attach_loop, usage_refusal, wants_help};
pub(crate) use context::bound_briefing;
pub use context::{CORE_RULES, EXTERNAL_RULES};
pub use skills::SKILL_INDEX;

use crate::runtime::domain::{CommandCue, Domain, ResourceCue};
use crate::runtime::help::ExtraVerb;
use crate::runtime::transport::{OfficeTransport, Refusal};

/// The session slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::session::ALL,
    resources: &[
        ("grant", "inspecting effective permission presets, tool rules, and sandbox boundaries for an agent."),
        ("session", "loading your rules and catalogue (`agent-os context`), inspecting agent session state/usage, or batching commands over a persistent socket (`session attach`)."),
    ],
    cues: &[CONTEXT_CUES, inspect::CUES, inspect::GRANT_CUES],
    dispatch,
};

/// `session attach`, which `agent-os` runs outside the table, in `agent-os session --help`.
pub(crate) const AGENT_VERBS: &[ExtraVerb] = &[ExtraVerb {
    resource: "session",
    verb: "attach",
    summary: "Run one JSON command per stdin line over a persistent office socket",
    use_when: "batching multiple agent-os calls in one shell turn without rebinding.",
}];

/// What `session` is for in `agentic-org`, which has no `session attach` or `context`.
pub(crate) const OPERATOR_RESOURCES: &[ResourceCue] = &[(
    "session",
    "inspecting agent session state, workspace paths, stall status, and usage.",
)];

/// `Use when:` guidance of each `session` command here.
const CONTEXT_CUES: &[CommandCue] = &[
    ("session", "context", "loading the standing rules, command catalogue, guide index and your must-follow processes at once (`agent-os context`; offline with `--offline`); a managed session already has them from turn 1 and `agent-os login` returns them automatically."),
];

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match (request.command.resource, request.command.verb) {
        ("session", "inspect") => Some(inspect::read_session_inspect(transport, request)),
        ("session", "list") => Some(inspect::read_session_list(transport, request)),
        ("session", "context") => Some(context::read_session_context(
            transport,
            request,
            &table.catalogue(),
        )),
        ("grant", "list") => Some(inspect::read_grant_list(transport, request)),
        _ => None,
    }
}
