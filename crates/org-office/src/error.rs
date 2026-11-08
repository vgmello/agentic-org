use std::error::Error;
use std::fmt;

pub use org_proto::NameError;

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
    /// The office's databases exist but the key given does not open them (`key_missing`, fatal;
    /// never retried). The server never makes a key: the host holds it. SQLCipher cannot tell a
    /// wrong key from a damaged or truncated file ("not a database"), so a corrupt database is
    /// reported the same way.
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
            Self::KeyMissing => f.write_str("the key given does not open the office's databases"),
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
