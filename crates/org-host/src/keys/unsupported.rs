//! The office secrets key adapter for an operating system with no implementation yet (every
//! release build other than macOS; Linux arrives with X1). It exists so such a build compiles and
//! refuses at run time with a reported reason. It never falls back to another platform's store or
//! to plain files: every call is [`KeyStoreError::Unsupported`], so the host creates no office,
//! starts none with a key it cannot keep, and deletes nothing.

// macOS builds select the Keychain instead (`keys::platform`); the refusal is still tested there.
#![cfg_attr(target_os = "macos", allow(dead_code))]

use org_proto::OfficeSecretsKey;

use super::{KeyStoreError, OfficeSecretsKeys};

/// Refuses every call (see the module docs).
pub(crate) struct UnsupportedKeys;

impl OfficeSecretsKeys for UnsupportedKeys {
    fn create(&self, _office_id: &str, _key: &OfficeSecretsKey) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unsupported)
    }

    fn load(&self, _office_id: &str) -> Result<Option<OfficeSecretsKey>, KeyStoreError> {
        Err(KeyStoreError::Unsupported)
    }

    fn delete(&self, _office_id: &str) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unsupported)
    }

    fn list(&self) -> Result<Vec<String>, KeyStoreError> {
        Err(KeyStoreError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_call_is_refused_with_its_reason_and_nothing_is_kept() {
        let store = UnsupportedKeys;
        let key = OfficeSecretsKey::from_bytes([7; 32]);
        let id = "01JAAAAAAAAAAAAAAAAAAAAAAA";
        let refusals = [
            store.create(id, &key).unwrap_err(),
            store.load(id).unwrap_err(),
            store.delete(id).unwrap_err(),
            store.list().unwrap_err(),
        ];
        for refusal in refusals {
            assert!(matches!(refusal, KeyStoreError::Unsupported), "{refusal:?}");
            assert!(
                !refusal.means_key_missing(),
                "a refusal is not a missing key"
            );
            assert_eq!(
                refusal.to_string(),
                "this operating system has no office secrets key store (only macOS is implemented)"
            );
        }
        // A create that was refused stored nothing: the next load is still refused, not a key.
        assert!(store.load(id).is_err());
    }
}
