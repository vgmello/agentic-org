//! Unified `agent-os file` VSA slice (`TASK-98.4`, ADR-0037): `file upload|list|inspect|fetch|update`.
//!
//! Zero positional arguments, zero duplicate `attachment` resource, keeping the launch catalogue
//! within `CATALOGUE_LIMIT = 4096` and `boot --offline <= 6 KiB`.

mod handlers;
mod schema;
#[cfg(test)]
mod tests;

use org_command::{Answer, CommandTable, Refusal, Request};

use crate::runtime::domain::Domain;
use crate::runtime::transport::OfficeTransport;

/// The `file` vertical slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: &[handlers::COMMANDS],
    resources: &[(
        "file",
        "uploading, listing, inspecting, fetching, and updating org, project, task, or private files.",
    )],
    cues: &[handlers::CUES],
    dispatch,
};

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    if request.command.resource != "file" {
        return None;
    }
    Some(match request.command.verb {
        "upload" => handlers::write_file_upload(transport, request),
        "list" => handlers::read_file_list(transport, request),
        "inspect" => handlers::read_file_inspect(transport, request),
        "fetch" => handlers::read_file_fetch(transport, request),
        "update" => handlers::write_file_update(transport, request),
        _ => return None,
    })
}
