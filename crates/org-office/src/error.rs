use std::error::Error;
use std::fmt;

pub use org_proto::NameError;
use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Refusal};

/// Encodes an [`ApiError`] as a recorded store [`Refusal::Rejected`].
pub(crate) fn api_refusal(code: ErrorCode, message: impl Into<String>) -> Refusal {
    let payload = ApiError::new(code, message);
    Refusal::rejected(serde_json::to_vec(&payload).expect("ApiError serializes"))
}

/// Decodes a store [`Refusal`] carrying a serialized [`ApiError`], falling back to `fallback`.
pub(crate) fn decode_api_refusal(refusal: &Refusal, fallback: &str) -> ApiError {
    match refusal {
        Refusal::Rejected(bytes) => serde_json::from_slice::<ApiError>(bytes)
            .unwrap_or_else(|_| ApiError::new(ErrorCode::InvalidRequest, fallback)),
        Refusal::Invalid(err) => ApiError::new(ErrorCode::InvalidRequest, err.to_string()),
    }
}

/// Encodes a storage-layer error as a recorded [`ErrorCode::StorageError`] refusal.
pub(crate) fn storage_api_refusal(context: &str, err: impl fmt::Display) -> Refusal {
    api_refusal(ErrorCode::StorageError, format!("{context}: {err}"))
}

/// Validates that `raw` is a canonical lowercase UUIDv7 and parses it into an [`Id`].
pub(crate) fn parse_uuid_v7_refusal(raw: &str, field: &str) -> Result<Id, Refusal> {
    if !org_proto::is_uuid_v7(raw) {
        return Err(api_refusal(
            ErrorCode::InvalidRequest,
            format!("{field} must be a canonical lowercase UUIDv7"),
        ));
    }
    Id::parse(raw).map_err(|_| api_refusal(ErrorCode::InvalidRequest, format!("invalid {field}")))
}

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
    /// Enrollment has not installed the organization's first materialized snapshot yet.
    SnapshotPending,
    /// Creation found an office already in the folder (an `office.json` or a creation intent for
    /// another request), or `Office::discard` found a creation intent, an `office.json`, the host's
    /// custody marker or a file it stages one in.
    OfficeExists,
    /// The host's writer admission owner/identity did not match the opened office's canonical
    /// owner or office identity, so startup was refused before mutating companion stores.
    CustodyMismatch,
    /// Disk, database or OS failure. The cause is kept for the server's log only; `Display`
    /// never includes paths or SQL, so it is safe to show to a client.
    Storage(Box<dyn Error + Send + Sync>),
}

impl OfficeError {
    pub fn storage(cause: impl Error + Send + Sync + 'static) -> Self {
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
            Self::SnapshotPending => f.write_str("organization snapshot is not available yet"),
            Self::OfficeExists => f.write_str("the office already exists"),
            Self::CustodyMismatch => f.write_str("protected writer admission requires recovery"),
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
