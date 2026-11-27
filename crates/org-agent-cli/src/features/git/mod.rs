//! The Git slice: `repo` and `worktree` (C7, `TASK-51`, `TASK-173`), with handlers and guidance
//! in each resource's file and declarative rows in `org_command_catalogue::git`.

mod repo;
mod worktree;

use org_command::{Answer, CommandTable, Request};

use crate::runtime::domain::Domain;
use crate::runtime::input::command_id;
use crate::runtime::transport::{OfficeTransport, Refusal};

/// The Git slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::git::ALL,
    resources: &[
        ("repo", "registering or listing Git repositories, attaching them to projects, checking cache readiness, fetching new origin branches, or pushing task branches."),
        ("worktree", "listing or preparing isolated per-task Git worktrees before making code changes."),
    ],
    cues: &[repo::CUES, worktree::CUES],
    dispatch,
};

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
