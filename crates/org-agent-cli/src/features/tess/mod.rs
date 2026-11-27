//! The Chief of Staff (Tess) slice: `suggestion create|list|inspect|withdraw` and `ui present`
//! (`TASK-80.4`).

mod suggestion;
mod ui;

use org_command::{Answer, CommandTable, Refusal, Request};
use org_proto::ErrorCode;

use crate::runtime::domain::Domain;
use crate::runtime::transport::OfficeTransport;

/// The Chief of Staff slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: &[suggestion::COMMANDS, ui::COMMANDS],
    resources: &[
        (
            "suggestion",
            "preflighting, listing, inspecting, and withdrawing caller-scoped mutation proposals.",
        ),
        (
            "ui",
            "emitting server-verified entity links, metrics, navigation links, and follow-up prompts.",
        ),
    ],
    cues: &[suggestion::CUES, ui::CUES],
    dispatch,
};

pub(super) const TESS_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

pub(super) const TESS_WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Conflict,
    ErrorCode::PolicyViolation,
    ErrorCode::Unavailable,
];

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match (request.command.resource, request.command.verb) {
        ("suggestion", "create") => Some(suggestion::create_suggestion(transport, request)),
        ("suggestion", "list") => Some(suggestion::list_suggestions(transport, request)),
        ("suggestion", "inspect") => Some(suggestion::inspect_suggestion(transport, request)),
        ("suggestion", "withdraw") => Some(suggestion::withdraw_suggestion(transport, request)),
        ("ui", "present") => Some(ui::present_ui(transport, request)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
