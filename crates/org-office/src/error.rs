use std::error::Error;
use std::fmt;

pub use crate::name::NameError;

#[derive(Debug)]
pub enum OfficeError {
    InvalidName(NameError),
    AlreadyExists,
    /// A value the caller gave is not acceptable (an id, a folder that does not match the
    /// office id, an `office.json` that names another office). The text is fixed, never data.
    Invalid(&'static str),
    /// Another process already runs this office from the same home directory.
    InUse,
    /// `Office::open` found no office in the folder: no `office.json`. Opening never creates one.
    NotFound,
    /// `Office::create` found an office (an `office.json`, databases or a workspace) already in
    /// the folder, or `Office::discard` found one that is finished or holds an organization.
    OfficeExists,
    /// `Office::create` found a key already saved for the office id it was about to use.
    /// It never reuses or replaces one.
    KeyExists,
    /// The office's databases exist but their key is gone (`key_missing`, fatal). Only
    /// `Office::create` makes keys, so opening never replaces a lost one.
    KeyMissing,
    /// Disk, database or OS failure. The cause is kept for the server's log only; `Display`
    /// never includes paths or SQL, so it is safe to show to a client.
    Storage(Box<dyn Error + Send + Sync>),
}

impl OfficeError {
    pub(crate) fn storage(cause: impl Error + Send + Sync + 'static) -> Self {
        Self::Storage(Box::new(cause))
    }
}

impl fmt::Display for OfficeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(reason) => write!(f, "invalid organization name: {reason}"),
            Self::AlreadyExists => f.write_str("an organization with this name already exists"),
            Self::Invalid(why) => write!(f, "invalid: {why}"),
            Self::InUse => f.write_str("the office is already running"),
            Self::NotFound => f.write_str("the office does not exist"),
            Self::OfficeExists => f.write_str("the office already exists"),
            Self::KeyExists => f.write_str("a key for this office already exists"),
            Self::KeyMissing => f.write_str("the office's database key is missing"),
            Self::Storage(_) => f.write_str("the office could not read or write its storage"),
        }
    }
}

impl Error for OfficeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(cause) => Some(cause.as_ref()),
            _ => None,
        }
    }
}
