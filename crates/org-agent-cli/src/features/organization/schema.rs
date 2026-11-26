//! The organization's shared field schema and views: log positions, teams, the snapshot read,
//! and the acknowledgement of an organization command.

use org_command::{Field, Type};
use org_proto::org::{OrgSnapshot, Team};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use crate::runtime::transport::{OfficeTransport, Refusal};

pub(super) const TEAM_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Team ID"),
    Field::required("name", Type::String, "Team name"),
    Field::optional("owner_team_id", Type::String, "Parent team ID"),
    Field::optional("lead_member_id", Type::String, "Lead employee ID"),
    Field::required("version", Type::Integer, "Record revision"),
];

pub(super) const TEAM_TYPE: Type = Type::Object(TEAM_FIELDS);

pub(super) const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

pub(super) const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::InvalidName,
    ErrorCode::AlreadyExists,
    ErrorCode::NotFound,
    ErrorCode::NotEmpty,
    ErrorCode::Conflict,
    ErrorCode::OperationReused,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

/// The organization snapshot (`org.snapshot`).
pub(super) fn fetch_snapshot<T: OfficeTransport + ?Sized>(
    transport: &mut T,
) -> Result<OrgSnapshot, Refusal> {
    let raw = transport.call_method("org.snapshot", json!({}))?;
    serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "The office returned an unreadable organization snapshot.",
            "Retry the command or check the office version.",
        )
    })
}

/// A team as `team list` and `org inspect` print it.
pub(super) fn team_value(team: &Team) -> Value {
    Value::Object(team_map(team))
}

/// The fields of [`team_value`], for a view that adds its own (`team inspect`).
pub(super) fn team_map(team: &Team) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("id".into(), json!(team.id));
    map.insert("name".into(), json!(team.name));
    if let Some(parent) = &team.owner_team_id {
        map.insert("owner_team_id".into(), json!(parent));
    }
    if let Some(lead) = &team.lead_member_id {
        map.insert("lead_member_id".into(), json!(lead));
    }
    map.insert("version".into(), json!(team.version));
    map
}
