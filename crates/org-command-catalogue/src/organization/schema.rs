//! Shared schema constants for the organization domain.

use org_command::{Field, Type};
use org_proto::ErrorCode;

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
