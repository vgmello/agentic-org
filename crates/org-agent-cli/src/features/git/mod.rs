//! The Git slice: `repo` and `worktree` (C7, `TASK-51`, `TASK-173`), each command's row,
//! guidance and handler in its resource's file.

mod repo;
mod worktree;

use org_command::{Answer, CommandTable, Request, Type};
use org_proto::ErrorCode;

use crate::runtime::domain::Domain;
use crate::runtime::input::command_id;
use crate::runtime::transport::{OfficeTransport, Refusal};

/// The Git slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: &[repo::COMMANDS, worktree::COMMANDS],
    resources: &[
        ("repo", "registering or listing Git repositories, attaching them to projects, checking cache readiness, fetching new origin branches, or pushing task branches."),
        ("worktree", "listing or preparing isolated per-task Git worktrees before making code changes."),
    ],
    cues: &[repo::CUES, worktree::CUES],
    dispatch,
};

const STRING_TYPE: Type = Type::String;

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::Unavailable,
];

const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::InvalidName,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::AlreadyExists,
    ErrorCode::Conflict,
    ErrorCode::OperationReused,
    ErrorCode::Unavailable,
];

const FETCH_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::RateLimited,
    ErrorCode::StorageError,
    ErrorCode::Unavailable,
    ErrorCode::Unsupported,
];

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match request.command.resource {
        "repo" => repo::dispatch(transport, request),
        "worktree" => worktree::dispatch(transport, request),
        _ => None,
    }
}

/// The command ID of a repository or worktree write ([`command_id`] in the `git-command`
/// domain): no dry run, and the same key always gives the same ID.
fn cmd_id(request: &Request) -> Result<String, Refusal> {
    command_id(request, "git-command")
}

#[cfg(test)]
mod tests;
