//! The crate's one error type. Its texts name paths and operations, never a variable's value
//! (the session token can't end up in a log through an error).

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Why a spawn, a write, a stop or a cleanup failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The request can't be carried out as given (a relative program path, a value with a
    /// NUL byte, a plan asking for something its targets don't allow, ...).
    Invalid {
        /// What was wrong.
        what: String,
    },
    /// A planned file or directory path met a symlink, or something that isn't a directory,
    /// on its way: the launch is refused rather than write through it.
    Refused {
        /// The path being written.
        path: PathBuf,
        /// Why.
        why: &'static str,
    },
    /// A system call failed.
    Io {
        /// What was being done.
        op: &'static str,
        /// The path involved, if any.
        path: Option<PathBuf>,
        /// The error.
        source: io::Error,
    },
}

impl Error {
    pub(crate) fn invalid(what: impl Into<String>) -> Self {
        Error::Invalid { what: what.into() }
    }

    pub(crate) fn io(op: &'static str, source: io::Error) -> Self {
        Error::Io {
            op,
            path: None,
            source,
        }
    }

    pub(crate) fn io_at(op: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Error::Io {
            op,
            path: Some(path.into()),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Invalid { what } => write!(f, "invalid request: {what}"),
            Error::Refused { path, why } => write!(f, "refused {}: {why}", path.display()),
            Error::Io {
                op,
                path: Some(path),
                source,
            } => write!(f, "{op} {}: {source}", path.display()),
            Error::Io {
                op,
                path: None,
                source,
            } => write!(f, "{op}: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            Error::Invalid { .. } | Error::Refused { .. } => None,
        }
    }
}

/// The crate's result type.
pub type Result<T, E = Error> = std::result::Result<T, E>;
