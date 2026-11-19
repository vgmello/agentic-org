//! GNU/Linux: the directory store, and the release directory.
//!
//! There is no Keychain, so a Keychain location is refused. A release build's
//! protected store is a release directory (`linux/release.rs`): a checked, locked
//! directory outside the data root, below which each item store is a directory store.

mod release;

use std::fs::File;
use std::io;
use std::sync::Arc;

use org_platform::Refused;

use super::{
    directory, refusal, CredentialError, CredentialLocation, ItemName, Listing, ProtectedStore,
    Secret, WriteError,
};

#[cfg(any(test, feature = "test-support"))]
pub(super) use release::release_directory_below;
pub(super) use release::{check_unused_release_directory, release_directory, ReleaseRoot};

/// One open directory store.
pub(super) struct Store {
    directory: directory::Store,
    /// The release directory this store is below, if it is below one: its lock is
    /// held for as long as the store lives. Dropped after the store itself.
    _root: Option<Arc<release::Root>>,
}

pub(super) fn open(location: CredentialLocation<'_>) -> Result<Store, CredentialError> {
    match location {
        CredentialLocation::Keychain(_) => Err(refusal().into()),
        CredentialLocation::Directory(location) => {
            directory::Store::open(&location).map(|directory| Store {
                directory,
                _root: None,
            })
        }
    }
}

pub(super) fn protected_store() -> Result<ProtectedStore, Refused> {
    Ok(ProtectedStore::ReleaseDirectory)
}

/// Nothing more: the directory flush is this system's last step of a publication.
pub(super) fn flush_published(_file: &File) -> io::Result<()> {
    Ok(())
}

impl Store {
    pub(super) fn add(&self, name: &ItemName, value: &[u8]) -> Result<(), WriteError> {
        self.directory.add(name, value)
    }

    pub(super) fn replace(&self, name: &ItemName, value: &[u8]) -> Result<(), WriteError> {
        self.directory.replace(name, value)
    }

    pub(super) fn load(&self, name: &ItemName) -> Result<Option<Secret>, CredentialError> {
        self.directory.load(name)
    }

    pub(super) fn delete(&self, name: &ItemName) -> Result<(), CredentialError> {
        self.directory.delete(name)
    }

    pub(super) fn list(&self, is_item: &dyn Fn(&str) -> bool) -> Result<Listing, CredentialError> {
        self.directory.list(is_item)
    }
}

#[cfg(test)]
mod tests {
    use crate::credentials::{
        self, CredentialError, CredentialLocation, KeychainItems, ProtectedStore, Refused,
    };

    #[test]
    fn linux_keeps_a_releases_secrets_in_a_release_directory_and_refuses_a_keychain_location() {
        let refusal = credentials::refusal();
        assert_eq!(
            refusal.to_string(),
            "credentials is not available on linux in this release; nothing was changed"
        );
        assert_eq!(
            credentials::protected_store(),
            Ok(ProtectedStore::ReleaseDirectory)
        );
        let location = CredentialLocation::Keychain(KeychainItems {
            access_group: "TEAMID1234.org.agentic.host",
            service: "org.agentic.office-key",
        });
        assert_eq!(
            credentials::credentials(location).unwrap_err(),
            CredentialError::Refused(refusal)
        );
        let Refused::Unsupported { capability, .. } = credentials::refusal();
        assert_eq!(capability, "credentials");
    }
}
