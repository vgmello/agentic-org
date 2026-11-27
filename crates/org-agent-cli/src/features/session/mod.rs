//! The session slice: `session inspect|boot` and `grant list` (C3, `CLI-9`, `CLI-10`), the
//! `boot` and `instructions overview` shorthands, and `session attach`, the batch mode
//! `agent-os` runs outside the table ([`attach`]).

mod attach;
mod boot;
mod inspect;
mod skills;

use org_command::{Alias, Answer, Command, CommandTable, Request, Type};
use org_proto::ErrorCode;

pub use attach::SESSION_ATTACH_HELP;
pub(crate) use attach::{run_attach_loop, usage_refusal, wants_help};
pub(crate) use boot::bound_briefing;
pub use boot::CORE_RULES;
pub use skills::SKILL_INDEX;

use crate::runtime::domain::{CommandCue, Domain, ResourceCue};
use crate::runtime::help::ExtraVerb;
use crate::runtime::transport::{OfficeTransport, Refusal};

/// The session slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: &[BOOT_COMMANDS, inspect::COMMANDS],
    resources: &[
        ("grant", "inspecting effective permission presets, tool rules, and sandbox boundaries for an agent."),
        ("session", "loading your rules and catalogue (`agent-os boot`), inspecting agent session state/usage, or batching commands over a persistent socket (`session attach`)."),
    ],
    cues: &[BOOT_CUES, inspect::CUES, inspect::GRANT_CUES],
    dispatch,
};

/// `boot` is `session boot`.
pub(crate) const BOOT_ALIAS: Alias = Alias {
    words: &["boot"],
    command: &["session", "boot"],
};

/// `instructions overview` is `session boot`.
pub(crate) const INSTRUCTIONS_ALIAS: Alias = Alias {
    words: &["instructions", "overview"],
    command: &["session", "boot"],
};

/// `session attach`, which `agent-os` runs outside the table, in `agent-os session --help`.
pub(crate) const AGENT_VERBS: &[ExtraVerb] = &[ExtraVerb {
    resource: "session",
    verb: "attach",
    summary: "Run one JSON command per stdin line over a persistent office socket",
    use_when: "batching multiple agent-os calls in one shell turn without rebinding.",
}];

/// What `session` is for in `agentic-org`, which has no `session attach` or `boot`.
pub(crate) const OPERATOR_RESOURCES: &[ResourceCue] = &[(
    "session",
    "inspecting agent session state, workspace paths, stall status, and usage.",
)];

pub(super) const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::Unavailable,
];

pub(super) const STRING_TYPE: Type = Type::String;

const BOOT_COMMANDS: &[Command] = &[boot::SESSION_BOOT];

/// `Use when:` guidance of each `session` command here.
const BOOT_CUES: &[CommandCue] = &[
    ("session", "boot", "loading the standing rules, command catalogue, guide index and your must-follow processes at once (`agent-os boot`; offline with `--offline`); a managed session already has them from turn 1."),
];

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match (request.command.resource, request.command.verb) {
        ("session", "inspect") => Some(inspect::read_session_inspect(transport, request)),
        ("session", "boot") => Some(boot::read_session_boot(
            transport,
            request,
            &table.catalogue(),
        )),
        ("grant", "list") => Some(inspect::read_grant_list(transport, request)),
        _ => None,
    }
}
