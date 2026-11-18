//! The refusing credentials adapter: no store is opened, so nothing is read, written,
//! created or locked.
//!
//! [`Store`] has no value, so the methods that would act on one cannot be reached.

use org_platform::Refused;

use super::{refusal, CredentialError, CredentialLocation, ItemName, Listing, Secret, WriteError};

pub(super) enum Store {}

pub(super) fn open(_location: CredentialLocation<'_>) -> Result<Store, CredentialError> {
    Err(refusal().into())
}

pub(super) fn protected_store() -> Result<(), Refused> {
    Err(refusal())
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
        let Refused::Unsupported { capability, os } = refusal();
        assert_eq!(capability, "credentials");
        assert_eq!(os, std::env::consts::OS);
    }
}
