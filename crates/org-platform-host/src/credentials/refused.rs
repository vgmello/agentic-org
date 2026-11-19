//! The refusing credentials adapter: no store is opened, so nothing is read, written,
//! created or locked.
//!
//! [`Store`] has no value, so the methods that would act on one cannot be reached.

use org_platform::Refused;

use super::{
    refusal, CredentialError, CredentialLocation, ItemName, ItemStore, Listing, ProtectedStore,
    ReleaseDirectory, ReleaseItems, Secret, WriteError,
};

pub(super) enum Store {}

/// No release directory is opened, so there is none to open a store below.
pub(super) enum ReleaseRoot {}

pub(super) fn open(_location: CredentialLocation<'_>) -> Result<Store, CredentialError> {
    Err(refusal().into())
}

pub(super) fn protected_store() -> Result<ProtectedStore, Refused> {
    Err(refusal())
}

pub(super) fn release_directory(
    _location: &ReleaseDirectory<'_>,
    _item_stores: &[ItemStore<'_>],
    _account_data: &[std::os::fd::BorrowedFd<'_>],
) -> Result<ReleaseRoot, CredentialError> {
    Err(refusal().into())
}

/// The test entry of the release directory refuses as the host's does.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn release_directory_below(
    _base: &std::path::Path,
    _location: &ReleaseDirectory<'_>,
    _item_stores: &[ItemStore<'_>],
    _account_data: &[std::os::fd::BorrowedFd<'_>],
) -> Result<ReleaseRoot, CredentialError> {
    Err(refusal().into())
}

impl ReleaseRoot {
    pub(super) fn store(&self, _items: &ReleaseItems<'_>) -> Result<Store, CredentialError> {
        match *self {}
    }
}

impl Store {
    pub(super) fn add(&self, _name: &ItemName, _value: &[u8]) -> Result<(), WriteError> {
        match *self {}
    }

    pub(super) fn replace(&self, _name: &ItemName, _value: &[u8]) -> Result<(), WriteError> {
        match *self {}
    }

    pub(super) fn load(&self, _name: &ItemName) -> Result<Option<Secret>, CredentialError> {
        match *self {}
    }

    pub(super) fn delete(&self, _name: &ItemName) -> Result<(), CredentialError> {
        match *self {}
    }

    pub(super) fn list(&self, _is_item: &dyn Fn(&str) -> bool) -> Result<Listing, CredentialError> {
        match *self {}
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::credentials::{DirectoryStore, KeychainItems, Sharing};

    #[test]
    fn every_location_refuses_and_nothing_is_made() {
        let fixture = tempfile::tempdir().unwrap();
        let anchor = fixture.path().canonicalize().unwrap();
        let locations = [
            CredentialLocation::Keychain(KeychainItems {
                access_group: "TEAMID1234.org.agentic.host",
                service: "org.agentic.office-key",
            }),
            CredentialLocation::Directory(DirectoryStore {
                anchor: &anchor,
                below: &["store"],
                create: true,
                sharing: Sharing::Exclusive,
                suffix: "",
            }),
            CredentialLocation::Directory(DirectoryStore {
                anchor: Path::new("relative"),
                below: &[],
                create: false,
                sharing: Sharing::Shared,
                suffix: ".key",
            }),
        ];
        for location in locations {
            let error = open(location).err();
            assert_eq!(error, Some(CredentialError::Refused(refusal())));
        }
        assert_eq!(std::fs::read_dir(&anchor).unwrap().count(), 0);
        assert_eq!(protected_store(), Err(refusal()));
        // A release directory is refused before its path is read: nothing is opened,
        // checked, locked or made, whatever is asked of it.
        let data_root = std::fs::File::open(&anchor).unwrap();
        for (path, mount_point_required, expect_items) in [
            (anchor.join("store"), false, false),
            (anchor.clone(), true, true),
            (std::path::PathBuf::from("relative/../path"), false, true),
        ] {
            let location = ReleaseDirectory {
                path: &path,
                data_root: std::os::fd::AsFd::as_fd(&data_root),
                mount_point_required,
                expect_items,
            };
            let stores = [ItemStore {
                directory: "office-keys",
                suffix: ".key",
                is_item: |_| true,
            }];
            for item_stores in [&stores[..], &[]] {
                let refused = release_directory(&location, item_stores, &[]);
                assert_eq!(refused.err(), Some(CredentialError::Refused(refusal())));
                let refused = release_directory_below(&anchor, &location, item_stores, &[]);
                assert_eq!(refused.err(), Some(CredentialError::Refused(refusal())));
            }
        }
        assert_eq!(std::fs::read_dir(&anchor).unwrap().count(), 0);
        let Refused::Unsupported { capability, os } = refusal();
        assert_eq!(capability, "credentials");
        assert_eq!(os, std::env::consts::OS);
    }
}
