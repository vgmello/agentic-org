//! The host's writer custody: who may write this office's journal, and how far the host
//! has seen it flushed. Called once at opening and once per flush, never on a read.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    change::{Id, Sha256},
    sequences::HighWaters,
};

/// One writer of one journal incarnation of one office.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeIdentity {
    pub instance_id: Id,
    pub organization_id: Id,
    pub office_id: Id,
    pub journal_incarnation: Id,
    pub writer_epoch: u64,
    pub machine_id: Id,
}

/// A flushed journal position. Ordinal zero is the anchor.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JournalHead {
    pub ordinal: u64,
    pub sha256: Sha256,
    pub high_waters: HighWaters,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CustodyError(pub String);

impl fmt::Display for CustodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "writer custody: {}", self.0)
    }
}

impl std::error::Error for CustodyError {}

/// Implemented by the authenticated host channel. A confirmation persists outside the
/// organization root before it returns; a closed channel fails closed.
///
/// The rule a custodian applies to `confirm`: the same identity as the stored head;
/// `before` equal to the stored head; the ordinal strictly greater, or equal with the
/// same hash; and no stream's high-water lower than the stored one. One confirmation may
/// advance many ordinals: it is sent once per group flush.
pub trait WriterAuthority: Send + Sync {
    fn checkpoint(&self, identity: &OfficeIdentity) -> Result<Option<JournalHead>, CustodyError>;
    fn confirm(
        &self,
        identity: &OfficeIdentity,
        before: Option<&JournalHead>,
        after: &JournalHead,
    ) -> Result<(), CustodyError>;
}
