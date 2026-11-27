//! The processes slice: `process` and `skill` (C5, `TASK-44`), with handlers and guidance in
//! each resource's file and declarative rows in `org_command_catalogue::processes`.

mod process;
mod process_inspect;
mod skill;

use org_command::{Answer, CommandTable, Request};
use org_proto::ErrorCode;

use crate::runtime::domain::Domain;
use crate::runtime::transport::{OfficeTransport, Refusal};

pub(crate) use skill::{OFFLINE_SKILL_CAVEAT, OFFLINE_SKILL_MODE};

/// The processes slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::processes::ALL,
    resources: &[
        ("process", "listing, inspecting (including `--why` inheritance), creating, or updating org/team/project processes and standards."),
        ("skill", "discovering and reading built-in `agent-os` product skills on demand."),
    ],
    cues: &[process::CUES, process_inspect::CUES, skill::CUES],
    dispatch,
};

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match request.command.resource {
        "process" => process::dispatch(transport, request),
        "skill" => skill::dispatch(transport, request),
        _ => None,
    }
}

/// The refusal of an office answer this build cannot read: the same read once more, then the
/// likely cause.
fn unreadable(method: &str, command: &str) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        format!("unreadable {method} response"),
        format!(
            "Retry `agent-os {command}` once; if it fails again, the office and agent-os versions differ."
        ),
    )
}

/// The refusal of a write whose answer this build cannot read: the office may have applied it.
fn unreadable_write(method: &str) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        format!("unreadable {method} response"),
        "The write may have been applied: retry with the same --idempotency-key to read its result without writing twice.",
    )
}

#[cfg(test)]
mod tests;
