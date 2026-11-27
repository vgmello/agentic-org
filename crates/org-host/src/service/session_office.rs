//! Helpers the session office operations share: authorizing the signed-in session before any
//! work and answering a journal failure.

use org_proto::{ApiError, ErrorCode, Request};

use crate::hostdb::{accounts::Account, Error as DbError};

use super::Authentication;
use crate::control_listener::Connection;

pub(super) fn journal_error(error: DbError) -> ApiError {
    match error {
        DbError::OperationReused => ApiError::new(
            ErrorCode::OperationReused,
            "this operation ID was already used for another request",
        ),
        DbError::Expired => ApiError::new(ErrorCode::Expired, "this operation ID has expired"),
        DbError::ClockSkew => ApiError::new(
            ErrorCode::ClockSkew,
            "operation ID timestamp is too far ahead of host clock",
        ),
        DbError::InvalidId => ApiError::new(ErrorCode::InvalidRequest, "invalid operation_id"),
        _ => ApiError::new(ErrorCode::Unavailable, "host registry unavailable"),
    }
}

/// Authenticate a native `host.sock` session before full parameter decoding so retired
/// session-less `office.list` / `office.restart` calls always fail closed with `NotAuthorized`.
pub(super) fn authorize_session_first(
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) -> Result<Account, ApiError> {
    let raw = request
        .params
        .get("session")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ApiError::new(ErrorCode::NotAuthorized, "session required"))?;
    let handle = org_host_control::SessionHandle::new(raw.to_owned())
        .map_err(|_| ApiError::new(ErrorCode::NotAuthorized, "not authorized"))?;
    authentication.authorize(connection, &handle)
}
