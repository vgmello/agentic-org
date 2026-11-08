use std::error::Error;
use std::fmt;

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
