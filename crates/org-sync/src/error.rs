use std::{error::Error, fmt};

/// The adapter retries an entire replayable attempt at most three times (four invocations).
pub const MAX_STORE_RETRIES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotAuthorized,
    UnsupportedVersion,
    InvalidInput,
    OperationReused,
    Expired,
    QuotaExceeded,
    RecoveryRequired,
    RestorePending,
    /// The declared immutable content digests do not match the actual content.
    IntegrityFailure,
    /// The same identity conflicts with previously retained history.
    Divergence,
    InvariantFailure,
}
impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::NotAuthorized => "not_authorized",
            Self::UnsupportedVersion => "unsupported_version",
            Self::InvalidInput => "invalid_request",
            Self::OperationReused => "operation_reused",
            Self::Expired => "expired",
            Self::QuotaExceeded => "quota_exceeded",
            Self::RecoveryRequired => "recovery_required",
            Self::RestorePending => "restore_pending",
            Self::IntegrityFailure => "integrity_failure",
            Self::Divergence => "divergence",
            Self::InvariantFailure => "invariant_failure",
        }
    }
}

/// Server-only cause retained for diagnosis; Display is safe public text.
#[derive(Debug)]
pub struct StoreError {
    cause: Box<dyn Error + Send + Sync>,
    retryable: bool,
}
impl StoreError {
    pub fn unavailable(cause: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self {
            cause: cause.into(),
            retryable: false,
        }
    }
    /// Only busy/locked is retryable; an adapter must never retry an arbitrary I/O failure.
    pub fn busy_or_locked(cause: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self {
            cause: cause.into(),
            retryable: true,
        }
    }
    pub fn is_retryable(&self) -> bool {
        self.retryable
    }
}
impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("storage unavailable")
    }
}
impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&*self.cause)
    }
}

#[derive(Debug)]
pub enum SyncError {
    Refused(Refusal),
    Storage(StoreError),
}
impl SyncError {
    /// The writer must abort the whole batch for these failures, retaining no
    /// partial application, cursor change or reply.
    pub fn aborts_batch(&self) -> bool {
        matches!(
            self,
            Self::Refused(
                Refusal::IntegrityFailure | Refusal::Divergence | Refusal::InvariantFailure
            )
        )
    }
}
impl From<StoreError> for SyncError {
    fn from(value: StoreError) -> Self {
        Self::Storage(value)
    }
}
impl From<Refusal> for SyncError {
    fn from(value: Refusal) -> Self {
        Self::Refused(value)
    }
}
impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(value) => f.write_str(value.code()),
            Self::Storage(value) => fmt::Display::fmt(value, f),
        }
    }
}
impl Error for SyncError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(value) => Some(value),
            _ => None,
        }
    }
}
