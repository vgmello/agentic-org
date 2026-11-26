//! Who a `msg.bind` names: the organization's members, from one consistent read of the
//! office's projection. The office's domain owns the addressing rule
//! (`org_office::directory::resolve_member`); this is the mailbox's port over it.

use std::sync::Arc;

use org_mailbox::{MemberDirectory, StoreError};
use org_office::Office;
use org_proto::{Member, OrgSnapshot, Position};

/// Where the directory reads its snapshot: the office in production, a fixed one in tests.
pub(crate) type SnapshotFn =
    Box<dyn Fn() -> Result<OrgSnapshot, org_office::OfficeError> + Send + Sync>;

pub(crate) struct OfficeDirectory {
    snapshot: SnapshotFn,
}

impl OfficeDirectory {
    pub(crate) fn of(office: Arc<Office>) -> Self {
        Self::new(Box::new(move || office.org_snapshot()))
    }

    pub(crate) fn new(snapshot: SnapshotFn) -> Self {
        Self { snapshot }
    }
}

impl MemberDirectory for OfficeDirectory {
    fn resolve_member(&self, id_or_handle: &str) -> Result<Option<(Member, Position)>, StoreError> {
        let snapshot = (self.snapshot)().map_err(StoreError::new)?;
        // Two members with one handle is a storage error: the mailbox then refuses, closed.
        match org_office::directory::resolve_member(&snapshot, id_or_handle) {
            Ok(found) => Ok(found.map(|member| (member.clone(), snapshot.position))),
            Err(_) => Err(StoreError::new("a handle names two members")),
        }
    }
}

#[cfg(test)]
mod tests;
