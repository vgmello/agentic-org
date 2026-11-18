//! GNU/Linux: the directory store. There is no protected store for a release yet, so a
//! Keychain location is refused; increment X3 writes the release store (the design's
//! ancestor, private-group, filesystem, mount-point, disjointness and empty-store
//! checks).

use std::fs::File;
use std::io;

use org_platform::Refused;

use super::{
    directory, refusal, CredentialError, CredentialLocation, ItemName, Listing, Secret, WriteError,
};

pub(super) struct Store(directory::Store);

pub(super) fn open(location: CredentialLocation<'_>) -> Result<Store, CredentialError> {
    match location {
        CredentialLocation::Keychain(_) => Err(refusal().into()),
        CredentialLocation::Directory(location) => directory::Store::open(&location).map(Store),
    }
}

pub(super) fn protected_store() -> Result<(), Refused> {
    Err(refusal())
}

/// Nothing more: the directory flush is this system's last step of a publication.
pub(super) fn flush_published(_file: &File) -> io::Result<()> {
    Ok(())
}

impl Store {
    pub(super) fn add(&self, name: &ItemName, value: &[u8]) -> Result<(), WriteError> {
        self.0.add(name, value)
    }

    pub(super) fn replace(&self, name: &ItemName, value: &[u8]) -> Result<(), WriteError> {
        self.0.replace(name, value)
    }

    pub(super) fn load(&self, name: &ItemName) -> Result<Option<Secret>, CredentialError> {
        self.0.load(name)
    }

    pub(super) fn delete(&self, name: &ItemName) -> Result<(), CredentialError> {
        self.0.delete(name)
    }

    pub(super) fn list(&self, is_item: &dyn Fn(&str) -> bool) -> Result<Listing, CredentialError> {
        self.0.list(is_item)
    }
}

#[cfg(test)]
mod tests {
    use crate::credentials::{self, CredentialError, CredentialLocation, KeychainItems, Refused};

    #[test]
    fn linux_has_no_protected_store_and_refuses_a_keychain_location() {
        let refusal = credentials::refusal();
        assert_eq!(
            refusal.to_string(),
            "credentials is not available on linux in this release; nothing was changed"
        );
        assert_eq!(credentials::protected_store(), Err(refusal.clone()));
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
