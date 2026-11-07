//! The error every API method can fail with.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::hire::FieldError;
use crate::model::ClaudeCodeState;

/// Methods reject with an `ApiError`. Over JSON-RPC it travels as the `data` of the error
/// object, whose `code` is set per kind and whose `message` is the same `message`.
// Rust: `ApiError::rpc_code` gives the code, and `rpc::ErrorObject` converts both ways.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApiError {
    NotFound {
        message: String,
    },
    Conflict {
        message: String,
    },
    Invalid {
        message: String,
        errors: Vec<FieldError>,
    },
    ClaudeCodeUnavailable {
        message: String,
        state: ClaudeCodeState,
    },
    /// The machine's agents can't be reached right now (lost connection), or this orgd doesn't
    /// offer the method yet (M1). Screens show the area as unavailable instead of failing.
    Unavailable {
        message: String,
    },
    Internal {
        message: String,
        details: String,
    },
}

/// JSON-RPC error codes. The standard ones (-32700..-32600) are for protocol errors; the API's
/// own errors use the server range (-32099..-32000), one code per [`ApiError`] kind.
pub mod codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    pub const NOT_FOUND: i32 = -32001;
    pub const CONFLICT: i32 = -32002;
    pub const INVALID: i32 = -32003;
    pub const CLAUDE_CODE_UNAVAILABLE: i32 = -32004;
    pub const UNAVAILABLE: i32 = -32005;
    pub const INTERNAL: i32 = -32006;
}

impl ApiError {
    pub fn not_found(message: impl Into<String>) -> Self {
        ApiError::NotFound {
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        ApiError::Conflict {
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>, errors: Vec<FieldError>) -> Self {
        ApiError::Invalid {
            message: message.into(),
            errors,
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        ApiError::Unavailable {
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>, details: impl Into<String>) -> Self {
        ApiError::Internal {
            message: message.into(),
            details: details.into(),
        }
    }

    /// The plain-words message every kind carries.
    pub fn message(&self) -> &str {
        match self {
            ApiError::NotFound { message }
            | ApiError::Conflict { message }
            | ApiError::Invalid { message, .. }
            | ApiError::ClaudeCodeUnavailable { message, .. }
            | ApiError::Unavailable { message }
            | ApiError::Internal { message, .. } => message,
        }
    }

    /// The JSON-RPC error code for this kind (see [`codes`]).
    pub fn rpc_code(&self) -> i32 {
        match self {
            ApiError::NotFound { .. } => codes::NOT_FOUND,
            ApiError::Conflict { .. } => codes::CONFLICT,
            ApiError::Invalid { .. } => codes::INVALID,
            ApiError::ClaudeCodeUnavailable { .. } => codes::CLAUDE_CODE_UNAVAILABLE,
            ApiError::Unavailable { .. } => codes::UNAVAILABLE,
            ApiError::Internal { .. } => codes::INTERNAL,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ApiError {}
