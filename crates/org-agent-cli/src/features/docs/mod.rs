//! Project documentation tree VSA slice (`TASK-55`, `FC-180`):
//! `doc index|ls|show|new|edit|move|delete|put`.

mod handlers;
#[cfg(test)]
mod tests;

use org_command::{Answer, CommandTable, Refusal, Request};

use crate::runtime::domain::Domain;
use crate::runtime::transport::OfficeTransport;

/// The `doc` vertical slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::docs::ALL,
    resources: &[(
        "doc",
        "reading, indexing, creating, editing, moving, deleting, and uploading media assets in a project's docs/ tree.",
    )],
    cues: &[handlers::CUES],
    dispatch,
};

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    if request.command.resource != "doc" {
        return None;
    }
    Some(match request.command.verb {
        "index" => handlers::read_index(transport, request),
        "ls" => handlers::read_ls(transport, request),
        "show" => handlers::read_show(transport, request),
        "new" => handlers::write_new(transport, request),
        "edit" => handlers::write_edit(transport, request),
        "move" => handlers::write_move(transport, request),
        "delete" => handlers::write_delete(transport, request),
        "put" => handlers::write_put(transport, request),
        _ => return None,
    })
}
