//! The hiring slice: `role`, `hr` and `agent` (`TASK-41`, `ADR-0029`, `CLI-14`, `TASK-137`),
//! each command's row, guidance and handler in its resource's file. `agent` is whole here: its
//! directory card, lifecycle and execution settings ([`agent`]) and the operator's external
//! sessions ([`agent_sessions`]).

mod agent;
mod agent_sessions;
mod hr;
mod role;
mod schema;

use org_command::{Answer, CommandTable, Request};

pub(crate) use agent_sessions::rewrite_positional;

use crate::runtime::domain::Domain;
use crate::runtime::transport::{OfficeTransport, Refusal};

/// The hiring slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: &[
        role::COMMANDS,
        hr::COMMANDS,
        agent::COMMANDS,
        agent_sessions::COMMANDS,
    ],
    resources: &[
        ("agent", "inspecting or managing agent profiles, team placement, pause/resume, offboarding, or external (bring-your-own) agent sessions."),
        ("hr", "listing, inspecting, drafting, confirming, or withdrawing agent hire requests."),
        ("role", "listing, inspecting, creating, or updating reusable role definitions in the role catalog."),
    ],
    cues: &[role::CUES, hr::CUES, agent::CUES, agent_sessions::CUES],
    dispatch,
};

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match request.command.resource {
        "role" => role::dispatch(transport, request),
        "hr" => hr::dispatch(transport, request),
        "agent" => agent::dispatch(transport, request)
            .or_else(|| agent_sessions::dispatch(transport, request)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
