//! Errors of the store. Every one is plain data, so one failure can answer many waiters.

use std::fmt;

use crate::{change::DurablePath, files::FilesError, journal::JournalError};

/// A failure of the projection's database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineError {
    pub message: String,
    /// The engine ended the writer's transaction (disk full, an interrupt). Everything the
    /// batch applied is gone, so its undecided operations decide again in a new one.
    pub transaction_ended: bool,
}

impl EngineError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            transaction_ended: false,
        }
    }

    pub fn ended(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            transaction_ended: true,
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "projection engine: {}", self.message)
    }
}

impl std::error::Error for EngineError {}

/// A projector could not turn a file into rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionError {
    pub path: Option<DurablePath>,
    pub reason: String,
    /// In a rebuild: `true` refuses the whole rebuild, `false` leaves this record out and
    /// lists it in the report. On the write path either value fails the operation.
    pub refuses_rebuild: bool,
    /// Set when the cause was an engine failure that ended the transaction.
    pub transaction_ended: bool,
}

impl ProjectionError {
    pub fn new(path: &DurablePath, reason: impl Into<String>) -> Self {
        Self {
            path: Some(path.clone()),
            reason: reason.into(),
            refuses_rebuild: true,
            transaction_ended: false,
        }
    }

    /// A record whose failure a rebuild reports and continues past.
    pub fn skippable(path: &DurablePath, reason: impl Into<String>) -> Self {
        Self {
            refuses_rebuild: false,
            ..Self::new(path, reason)
        }
    }
}

impl From<EngineError> for ProjectionError {
    fn from(error: EngineError) -> Self {
        Self {
            path: None,
            reason: error.message,
            refuses_rebuild: true,
            transaction_ended: error.transaction_ended,
        }
    }
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(path) => write!(f, "projection of {path}: {}", self.reason),
            None => write!(f, "projection: {}", self.reason),
        }
    }
}

impl std::error::Error for ProjectionError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// Refused before admission. Nothing was recorded.
    Invalid(String),
    /// The operation ID was used before with another actor, kind or fingerprint. Discloses
    /// nothing about the earlier use.
    Conflict,
    Engine(EngineError),
    Projection(ProjectionError),
    /// The decision panicked. Its operation failed alone.
    Panicked,
    /// An ordinal, origin position or feed position has no successor.
    Exhausted(&'static str),
    /// The journal append failed. The whole batch was rolled back; nothing was accepted.
    Append(String),
    /// The store is not accepting. When this answers a request whose entry was already
    /// appended, the outcome is unknown, not failed: retry by operation ID after reopening.
    Stopped {
        path: Option<String>,
        reason: String,
    },
    /// The journal refused: at opening, or a failure that stops it.
    Journal(JournalError),
    /// File publication failed.
    Files(FilesError),
    /// The host's writer custody refused or could not be reached.
    Custody(String),
    /// A broken internal rule. Never a refusal of the caller's input.
    Internal(String),
}

impl StoreError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "invalid: {message}"),
            Self::Conflict => f.write_str("operation ID conflict"),
            Self::Engine(error) => error.fmt(f),
            Self::Projection(error) => error.fmt(f),
            Self::Panicked => f.write_str("the decision panicked"),
            Self::Exhausted(what) => write!(f, "{what} exhausted"),
            Self::Append(message) => write!(f, "journal append failed: {message}"),
            Self::Stopped {
                path: Some(path),
                reason,
            } => write!(f, "store stopped at {path}: {reason}"),
            Self::Stopped { path: None, reason } => write!(f, "store stopped: {reason}"),
            Self::Journal(error) => error.fmt(f),
            Self::Files(error) => error.fmt(f),
            Self::Custody(message) => write!(f, "writer custody: {message}"),
            Self::Internal(message) => write!(f, "internal store error: {message}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<EngineError> for StoreError {
    fn from(error: EngineError) -> Self {
        Self::Engine(error)
    }
}

impl From<JournalError> for StoreError {
    fn from(error: JournalError) -> Self {
        Self::Journal(error)
    }
}

impl From<FilesError> for StoreError {
    fn from(error: FilesError) -> Self {
        Self::Files(error)
    }
}

impl StoreError {
    pub(crate) fn stopped(reason: impl Into<String>) -> Self {
        Self::Stopped {
            path: None,
            reason: reason.into(),
        }
    }

    /// The engine ended the writer's transaction while this happened.
    pub(crate) fn transaction_ended(&self) -> bool {
        match self {
            Self::Engine(error) => error.transaction_ended,
            Self::Projection(error) => error.transaction_ended,
            _ => false,
        }
    }
}

impl From<ProjectionError> for StoreError {
    fn from(error: ProjectionError) -> Self {
        Self::Projection(error)
    }
}
