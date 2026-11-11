//! Typed diagnostic classification; never infer policy from an error's text.

use std::time::Instant;
use std::{fmt, io};

use super::ScanFailure;

/// Why recovery failed, independently of the underlying I/O error kind.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanFailureKind {
    /// An absolute recovery deadline elapsed.
    DeadlineElapsed,
    /// The cumulative saved inventory exceeds its identity bound.
    InventoryOverflow,
    /// Any other refusal, observation, signal or reap failure.
    Other,
}

impl ScanFailure {
    /// Read the source's typed classification without interpreting messages.
    pub fn kind(&self) -> ScanFailureKind {
        self.error
            .get_ref()
            .and_then(|error| error.downcast_ref::<FailureTag>())
            .map_or(ScanFailureKind::Other, |tag| tag.kind)
    }
}

#[derive(Debug)]
struct FailureTag {
    kind: ScanFailureKind,
    message: String,
}

impl fmt::Display for FailureTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FailureTag {}

pub(super) fn tagged(
    io_kind: io::ErrorKind,
    kind: ScanFailureKind,
    message: impl Into<String>,
) -> io::Error {
    io::Error::new(
        io_kind,
        FailureTag {
            kind,
            message: message.into(),
        },
    )
}

/// One deadline boundary shared by policy, optional-job and native adapters.
pub(super) fn check_deadline(
    now: Instant,
    deadline: Instant,
    message: impl fmt::Display,
) -> io::Result<()> {
    if now >= deadline {
        Err(tagged(
            io::ErrorKind::TimedOut,
            ScanFailureKind::DeadlineElapsed,
            message.to_string(),
        ))
    } else {
        Ok(())
    }
}
