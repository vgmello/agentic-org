//! Storage and validation errors for the `L2` usage & cost accounting port.

use std::error::Error;
use std::fmt;

/// Error returned by an [`crate::AccountingStore`] implementation.
///
/// The cause of [`Self::Storage`] is kept for server logs only; [`Display`] never includes
/// paths or SQL so it is safe to show to a client.
#[derive(Debug)]
pub enum AccountingError {
    /// Fixed static validation/state refusal (never carries runtime data).
    Invalid(&'static str),
    /// Disk, database, or OS failure with an internal cause kept out of `Display`.
    Storage(Box<dyn Error + Send + Sync>),
}

impl AccountingError {
    pub fn storage(cause: impl Error + Send + Sync + 'static) -> Self {
        Self::Storage(Box::new(cause))
    }
}

impl fmt::Display for AccountingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(why) => write!(f, "invalid: {why}"),
            Self::Storage(_) => f.write_str("the accounting store could not read or write storage"),
        }
    }
}

impl Error for AccountingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(cause) => Some(cause.as_ref()),
            Self::Invalid(_) => None,
        }
    }
}
