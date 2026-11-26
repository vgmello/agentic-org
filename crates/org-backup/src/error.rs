//! The one error every backup, verification and restore returns. Its text names paths,
//! identifiers and fixed reasons only: never a passphrase, a key or file contents.

use std::path::PathBuf;

/// What went wrong, classified for the command line.
#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    /// A request that cannot be used (an argument, a path, an identifier).
    #[error("{0}")]
    Usage(String),
    /// The organization has no office here, or the office named is not one of it.
    #[error("{0}")]
    NotFound(String),
    /// A host or office holds its lock: it is running.
    #[error("{what} is running ({lock} is held); {help}")]
    Live {
        what: String,
        lock: PathBuf,
        help: &'static str,
    },
    /// The archive is not one this build wrote, or it was changed, cut or crafted.
    #[error("the archive is not valid: {0}")]
    Invalid(String),
    /// An encrypted archive's authentication failed.
    #[error("the passphrase is wrong or the encrypted archive was changed")]
    Authentication,
    /// An encrypted archive needs a passphrase and none was given.
    #[error("the archive is encrypted and needs its passphrase")]
    PassphraseRequired,
    /// Something the operation found makes it unsafe; nothing was changed.
    #[error("{0}")]
    Refused(String),
    /// A file system operation failed.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    /// The office's own libraries refused or failed.
    #[error("{context}: {message}")]
    Office {
        context: &'static str,
        message: String,
    },
}

/// A marker inside an [`std::io::Error`]: an encrypted chunk failed authentication.
#[derive(Debug)]
pub(crate) struct AuthFailed;

impl std::fmt::Display for AuthFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("authentication failed")
    }
}

impl std::error::Error for AuthFailed {}

/// A marker inside an [`std::io::Error`]: a stream ran past its byte budget.
#[derive(Debug)]
pub(crate) struct OverBudget;

impl std::fmt::Display for OverBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the archive expands past its bound")
    }
}

impl std::error::Error for OverBudget {}

impl BackupError {
    /// An I/O failure while doing `context`. A failed authentication or an exceeded
    /// budget carried inside it keeps its own meaning.
    pub(crate) fn io(context: impl Into<String>) -> impl FnOnce(std::io::Error) -> Self {
        let context = context.into();
        move |source| {
            let inner = source.get_ref();
            if inner.is_some_and(|e| e.is::<AuthFailed>()) {
                return Self::Authentication;
            }
            if inner.is_some_and(|e| e.is::<OverBudget>()) {
                return Self::Invalid(
                    "it expands past the size its manifest and the limits allow".into(),
                );
            }
            Self::Io { context, source }
        }
    }

    /// A failure of the office's libraries while doing `context`.
    pub(crate) fn office(context: &'static str) -> impl FnOnce(org_office::OfficeError) -> Self {
        move |error| Self::Office {
            context,
            message: error.to_string(),
        }
    }

    pub(crate) fn invalid(reason: impl Into<String>) -> Self {
        Self::Invalid(reason.into())
    }
}
