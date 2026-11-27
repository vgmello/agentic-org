//! Errors of the store, of its journal and of its file publication. Every one is plain data, so
//! one failure can answer many waiters.

use std::{fmt, io, path::Path};

use crate::path::{DurablePath, InvalidPath};

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
    /// The store is not accepting. A request that was waiting for an answer when the
    /// store stopped may have been appended: retry by operation ID after reopening.
    Stopped {
        path: Option<String>,
        reason: String,
    },
    /// The operation's entry is in the journal, so it is accepted, but the projection
    /// could not take it and the store stopped. Not a failure of the operation: submit it
    /// again with the same operation ID after reopening, and it answers.
    OutcomeUnknown {
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
            Self::OutcomeUnknown { reason } => write!(
                f,
                "appended, outcome unknown ({reason}); retry with the same operation ID"
            ),
            Self::Journal(error) => error.fmt(f),
            Self::Files(error) => error.fmt(f),
            Self::Custody(message) => write!(f, "writer custody: {message}"),
            Self::Internal(message) => write!(f, "internal store error: {message}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<InvalidPath> for StoreError {
    fn from(error: InvalidPath) -> Self {
        Self::invalid(error.reason())
    }
}

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

/// A failure of the acceptance journal.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum JournalError {
    Io {
        path: String,
        action: &'static str,
        message: String,
    },
    /// A full flush failed. Never retried in this process.
    Flush {
        path: String,
        message: String,
    },
    /// A bad entry at or below the head the host confirmed, or an entry that is whole but
    /// does not continue the chain. Never truncated.
    Corrupt {
        ordinal: u64,
        reason: String,
    },
    /// The journal ends before the head the host confirmed.
    Behind {
        journal: u64,
        custody: u64,
    },
    /// The entry at the confirmed ordinal is not the one the host confirmed.
    Fork {
        ordinal: u64,
    },
    /// The entries before this ordinal are neither present nor covered by the checkpoint,
    /// or the entry at this ordinal, which the host confirmed, is no longer held.
    MissingPredecessor {
        ordinal: u64,
    },
    UnknownFormat(String),
    /// The journal belongs to another office, incarnation or writer.
    IdentityMismatch,
    /// `create` found a journal already there.
    Exists,
    Custody(String),
    Exhausted(&'static str),
    TooLarge {
        bytes: usize,
    },
    /// The append failed or was short. The segment is back at its last good entry.
    Append(String),
    /// A failed append could not be truncated away. The journal accepts nothing more.
    Broken {
        path: String,
        message: String,
    },
    Internal(String),
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                path,
                action,
                message,
            } => write!(f, "journal: {action} {path}: {message}"),
            Self::Flush { path, message } => write!(f, "journal: flush {path}: {message}"),
            Self::Corrupt { ordinal, reason } => {
                write!(f, "journal entry {ordinal} is corrupt: {reason}")
            }
            Self::Behind { journal, custody } => write!(
                f,
                "the journal ends at {journal}, behind the confirmed head {custody}"
            ),
            Self::Fork { ordinal } => {
                write!(
                    f,
                    "journal entry {ordinal} is not the one the host confirmed"
                )
            }
            Self::MissingPredecessor { ordinal } => {
                write!(f, "the journal entries before {ordinal} are missing")
            }
            Self::UnknownFormat(format) => write!(f, "unknown journal format {format:?}"),
            Self::IdentityMismatch => f.write_str("the journal belongs to another writer"),
            Self::Exists => f.write_str("a journal already exists here"),
            Self::Custody(message) => write!(f, "writer custody: {message}"),
            Self::Exhausted(what) => write!(f, "{what} exhausted"),
            Self::TooLarge { bytes } => write!(f, "a journal entry of {bytes} bytes is too large"),
            Self::Append(message) => write!(f, "journal append failed: {message}"),
            Self::Broken { path, message } => {
                write!(f, "journal segment {path} could not be repaired: {message}")
            }
            Self::Internal(message) => write!(f, "journal: {message}"),
        }
    }
}

impl std::error::Error for JournalError {}

/// A failure to publish or read a canonical file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FilesError {
    /// The file holds bytes this office did not write. Nothing was replaced.
    Foreign { path: DurablePath },
    Io {
        path: String,
        action: &'static str,
        message: String,
        /// A flush failed. It must not be retried in this process.
        flush: bool,
    },
}

impl FilesError {
    pub(crate) fn io(path: &Path, action: &'static str, error: &io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            action,
            message: error.to_string(),
            flush: false,
        }
    }

    pub(crate) fn flush(path: &Path, action: &'static str, error: &io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            action,
            message: error.to_string(),
            flush: true,
        }
    }

    /// The path the failure names.
    pub fn path(&self) -> String {
        match self {
            Self::Foreign { path } => path.to_string(),
            Self::Io { path, .. } => path.clone(),
        }
    }
}

impl fmt::Display for FilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Foreign { path } => {
                write!(f, "{path} holds bytes this office did not write")
            }
            Self::Io {
                path,
                action,
                message,
                ..
            } => write!(f, "{action} {path}: {message}"),
        }
    }
}

impl std::error::Error for FilesError {}
