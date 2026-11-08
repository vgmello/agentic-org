//! How a decision fails: a refusal the caller is answered with, or a storage failure that rolls
//! the whole request back.

use org_proto::{ApiError, ErrorCode};

use crate::ports::StoreError;

/// Why a decision stopped.
#[derive(Debug)]
pub(crate) enum Fail {
    /// The call is refused with this answer (stored for retries, §2.3).
    Refuse(ApiError),
    /// Storage failed: nothing is stored and the request rolls back.
    Store(StoreError),
}

impl From<StoreError> for Fail {
    fn from(e: StoreError) -> Self {
        Fail::Store(e)
    }
}

impl From<ApiError> for Fail {
    fn from(e: ApiError) -> Self {
        Fail::Refuse(e)
    }
}

pub(crate) fn refuse(code: ErrorCode, message: impl Into<String>) -> Fail {
    Fail::Refuse(ApiError::new(code, message))
}

pub(crate) fn invalid(message: impl Into<String>) -> Fail {
    refuse(ErrorCode::InvalidRequest, message)
}

pub(crate) fn not_found(message: impl Into<String>) -> Fail {
    refuse(ErrorCode::NotFound, message)
}

pub(crate) fn conflict(message: impl Into<String>) -> Fail {
    refuse(ErrorCode::Conflict, message)
}

pub(crate) fn already_exists(message: impl Into<String>) -> Fail {
    refuse(ErrorCode::AlreadyExists, message)
}

pub(crate) fn unknown_recipient(message: impl Into<String>) -> Fail {
    refuse(ErrorCode::UnknownRecipient, message)
}
