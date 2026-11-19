//! Who a `msg.bind` names: the organization's members, from one consistent read of the
//! office's projection. The office's domain owns the addressing rule
//! (`org_office::directory::resolve_member`); this is the mailbox's port over it.

use std::sync::Arc;

use org_mailbox::{MemberDirectory, StoreError};
use org_office::journal_office::Office;
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
mod tests {
    use super::*;
    use org_office::journal_office::TestOffice;

    #[test]
    fn members_resolve_by_id_handle_and_owner_from_the_office_projection() {
        let office = Arc::new(Office::for_test(TestOffice::default()));
        let snapshot = office.org_snapshot().unwrap();
        let directory = OfficeDirectory::of(Arc::clone(&office));
        let owner = snapshot.members.iter().find(|m| m.owner).unwrap();
        for address in [owner.id.as_str(), "owner", " OWNER "] {
            let (found, position) = directory.resolve_member(address).unwrap().unwrap();
            assert_eq!(found.id, owner.id, "{address}");
            assert_eq!(position, snapshot.position);
        }
        assert!(directory.resolve_member("nobody-here").unwrap().is_none());
    }

    #[test]
    fn an_ambiguous_handle_or_an_unreadable_projection_fails_closed() {
        let office = Arc::new(Office::for_test(TestOffice::default()));
        let mut snapshot = office.org_snapshot().unwrap();
        let mut twin = snapshot.members.iter().find(|m| m.owner).unwrap().clone();
        twin.id = "01900000-0000-7000-8000-0000000000ff".into();
        snapshot.members.push(twin);
        let directory = OfficeDirectory::new(Box::new(move || Ok(snapshot.clone())));
        assert!(directory.resolve_member("owner").is_err());
        let failing = OfficeDirectory::new(Box::new(|| {
            Err(org_office::OfficeError::Storage("disk".into()))
        }));
        assert!(failing.resolve_member("owner").is_err());
    }
}
