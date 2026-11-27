//! Storage and validation errors for the `L3` budgets & holds port.

use std::error::Error;
use std::fmt;

/// Error returned by a [`crate::BudgetStore`] implementation.
///
/// The cause of [`Self::Storage`] is kept for server logs only; [`Display`] never includes
/// paths or SQL so it is safe to show to a client.
#[derive(Debug)]
pub enum BudgetError {
    /// Fixed static validation/state refusal (never carries runtime data).
    Invalid(&'static str),
    /// Disk, database, or OS failure with an internal cause kept out of `Display`.
    Storage(Box<dyn Error + Send + Sync>),
}

impl BudgetError {
    pub fn storage(cause: impl Error + Send + Sync + 'static) -> Self {
        Self::Storage(Box::new(cause))
    }
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(why) => write!(f, "invalid: {why}"),
            Self::Storage(_) => f.write_str("the budget store could not read or write storage"),
        }
    }
}

impl Error for BudgetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(cause) => Some(cause.as_ref()),
            Self::Invalid(_) => None,
        }
    }
}
