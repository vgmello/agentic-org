use std::error::Error;
use std::fmt;

use org_proto::ErrorCode;

/// Domain failures. The server maps these to the identically named wire errors.
/// No variant contains message bodies, notes, credentials or database diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailboxError {
    InvalidRequest,
    TooLarge,
    UnknownRecipient,
    AlreadyExists,
    NotFound,
    NotBound,
    NotAuthorized,
    StorageError,
    Timeout,
    Unavailable,
}

impl fmt::Display for MailboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRequest => "invalid messaging request",
            Self::TooLarge => "messaging value exceeds its size limit",
            Self::UnknownRecipient => "unknown message recipient",
            Self::AlreadyExists => "message id already has different content",
            Self::NotFound => "message or member not found",
            Self::NotBound => "connection is no longer bound to this inbox",
            Self::NotAuthorized => "messaging authorization refused",
            Self::StorageError => "the office could not read or write message storage",
            Self::Timeout => "message request deadline passed",
            Self::Unavailable => "messaging is temporarily unavailable",
        })
    }
}

impl Error for MailboxError {}

/// The boundary mapping: each variant is the identically named wire code.
impl From<MailboxError> for ErrorCode {
    fn from(error: MailboxError) -> Self {
        match error {
            MailboxError::InvalidRequest => Self::InvalidRequest,
            MailboxError::TooLarge => Self::TooLarge,
            MailboxError::UnknownRecipient => Self::UnknownRecipient,
            MailboxError::AlreadyExists => Self::AlreadyExists,
            MailboxError::NotFound => Self::NotFound,
            MailboxError::NotBound => Self::NotBound,
            MailboxError::NotAuthorized => Self::NotAuthorized,
            MailboxError::StorageError => Self::StorageError,
            MailboxError::Timeout => Self::Timeout,
            MailboxError::Unavailable => Self::Unavailable,
        }
    }
}

/// Storage outcomes with domain meaning, plus an opaque infrastructure failure.
/// The mailbox does not automatically retry: a commit may already be durable.
pub enum StoreError {
    AlreadyExists,
    NotFound,
    InvalidTransition,
    Failure(Box<dyn Error + Send + Sync>),
}

impl StoreError {
    pub fn new(cause: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self::Failure(cause.into())
    }
}

impl fmt::Debug for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AlreadyExists => "StoreError::AlreadyExists",
            Self::NotFound => "StoreError::NotFound",
            Self::InvalidTransition => "StoreError::InvalidTransition",
            Self::Failure(_) => "StoreError::Failure(<opaque cause>)",
        })
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        MailboxError::from(self).fmt(f)
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Failure(cause) => Some(cause.as_ref()),
            _ => None,
        }
    }
}

impl From<&StoreError> for MailboxError {
    fn from(error: &StoreError) -> Self {
        match error {
            StoreError::AlreadyExists => Self::AlreadyExists,
            StoreError::NotFound => Self::NotFound,
            StoreError::InvalidTransition => Self::InvalidRequest,
            StoreError::Failure(_) => Self::StorageError,
        }
    }
}

impl From<StoreError> for MailboxError {
    fn from(error: StoreError) -> Self {
        Self::from(&error)
    }
}

/// Session authorization never exposes credentials through its error values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    Denied,
    Unavailable,
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("session authorization refused")
    }
}

impl Error for AuthError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_maps_to_its_wire_code() {
        for (error, code) in [
            (MailboxError::InvalidRequest, ErrorCode::InvalidRequest),
            (MailboxError::TooLarge, ErrorCode::TooLarge),
            (MailboxError::UnknownRecipient, ErrorCode::UnknownRecipient),
            (MailboxError::AlreadyExists, ErrorCode::AlreadyExists),
            (MailboxError::NotFound, ErrorCode::NotFound),
            (MailboxError::NotBound, ErrorCode::NotBound),
            (MailboxError::NotAuthorized, ErrorCode::NotAuthorized),
            (MailboxError::StorageError, ErrorCode::StorageError),
            (MailboxError::Timeout, ErrorCode::Timeout),
            (MailboxError::Unavailable, ErrorCode::Unavailable),
        ] {
            assert_eq!(ErrorCode::from(error), code);
            // The wire code's serialized name is the variant's snake_case name.
            let wire = serde_json::to_value(code).unwrap();
            assert_eq!(
                wire.as_str().unwrap().replace('_', ""),
                format!("{error:?}").to_ascii_lowercase()
            );
        }
        assert_eq!(
            MailboxError::from(StoreError::new("disk")),
            MailboxError::StorageError
        );
        assert_eq!(
            MailboxError::from(StoreError::AlreadyExists),
            MailboxError::AlreadyExists
        );
    }
}
