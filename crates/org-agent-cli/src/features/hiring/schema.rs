//! The hiring slice's shared schema: its error lists and value types.

use org_command::Type;
use org_proto::ErrorCode;

pub(super) const STRING_TYPE: Type = Type::String;

/// `low|medium|high|xhigh|max`, the office's reasoning-effort names (`TASK-181`).
pub(super) const EFFORT_TYPE: Type = Type::Enum(&org_proto::hiring::AGENT_EFFORT_LEVELS);

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
