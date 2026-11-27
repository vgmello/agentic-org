//! The organization slice: `org`, `team`, `person`, `directory` (shorthand `who`) and `member`
//! (B2.4, B3, `CLI-14`), each command's row, guidance and handler in its resource's file.

mod directory;
mod member;
mod org;
mod person;
mod schema;
mod team;

use org_command::{Answer, CommandTable, Request};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use crate::runtime::domain::Domain;
use crate::runtime::input::send_command;
use crate::runtime::transport::{OfficeTransport, Refusal};

/// The organization slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::organization::ALL,
    resources: &[
        ("directory", "listing active people and agents with handles, roles, and live presence (shorthand: `who`)."),
        ("member", "granting or revoking an employee's `may_hire` permission."),
        ("org", "inspecting the full organization snapshot of teams, members, and log position."),
        ("person", "inspecting, adding, updating, moving, or deactivating human members in the organization."),
        ("team", "listing, inspecting, creating, renaming, moving, capping, or archiving teams."),
    ],
    cues: &[org::CUES, team::CUES, person::CUES, directory::CUES, member::CUES],
    dispatch,
};

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match request.command.resource {
        "org" => org::dispatch(transport, request),
        "team" => team::dispatch(transport, request),
        "person" => person::dispatch(transport, request),
        "directory" => directory::dispatch(transport, request),
        "member" => member::dispatch(transport, request),
        _ => None,
    }
}

/// The record a revisioned write changes, read again when the office refuses it as a conflict.
enum ConflictTarget<'a> {
    Team(&'a str),
    Member(&'a str),
}

/// Sends an organization command ([`send_command`]); a `conflict` refusal carries the record's
/// current revision in `retry_with` (`if-match`) when it can still be read.
fn send_write<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    method: &str,
    params: Map<String, Value>,
    target_id: &str,
    conflict_target: Option<ConflictTarget<'_>>,
) -> Result<Answer, Refusal> {
    send_command(transport, request, method, params, target_id).map_err(|mut refusal| {
        if refusal.code == ErrorCode::Conflict {
            if let Some(target) = conflict_target {
                attach_conflict_retry_with(transport, target, &mut refusal);
            }
        }
        refusal
    })
}

fn attach_conflict_retry_with<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    target: ConflictTarget<'_>,
    refusal: &mut Refusal,
) {
    let Ok(snapshot) = schema::fetch_snapshot(transport) else {
        return;
    };
    let current = match target {
        ConflictTarget::Team(id) => snapshot
            .teams
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.version),
        ConflictTarget::Member(id) => snapshot
            .members
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.version),
    };
    if let Some(version) = current {
        let mut map = Map::new();
        map.insert("if-match".into(), json!(version));
        refusal.retry_with = Some(map);
    }
}

#[cfg(test)]
mod tests;
