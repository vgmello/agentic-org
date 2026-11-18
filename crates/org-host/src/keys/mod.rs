//! Key custody: where the host keeps every office's secrets key.
//!
//! The host creates each office's key (`secrets::generate_office_secrets_key`), stores it here before the
//! office's first `--create`, loads it before every `--office` and `--discard`, and sends it on
//! the first line of the launch pipe (`org-office-launch`). Its servers never touch a key store.
//! A key is in the host's memory only from its creation or load until that first line is written.
//!
//! [`OfficeSecretsKeys`] is the host's own contract: the supervisor and provisioning depend on it,
//! never on a particular store or platform. The adapters:
//! - **Release builds on macOS:** the Data Protection keychain, in the host's access group
//!   (`keychain`), so other code gets `errSecItemNotFound`, never a prompt.
//! - **Release builds on any other operating system:** `unsupported`, which refuses every call
//!   with a reported reason ([`KeyStoreError::Unsupported`]). It is chosen before any Keychain
//!   precondition is checked, so such a host never reports a macOS-only reason (no access group,
//!   not hardened). `platform` is the only place that chooses between these two by
//!   `cfg(target_os)`, and it owns the Keychain preconditions (the access group, its entitlement
//!   and the host's own hardening).
//! - **Debug builds:** plain files in a development folder (`dev`), so a debug host and the tests
//!   never write a real Keychain item. A debug host without that folder refuses to start.
//! - **Unit tests:** an in-memory store with fault injection (`memory`).
//!
//! The rules every store follows, and the host relies on:
//! - `create` only adds: it never replaces an existing item ([`KeyStoreError::Exists`]), and on
//!   any other error it leaves no item behind.
//! - `delete` of a missing item is `Ok`.
//! - The host deletes an office's key **only** after that office's `--discard` reported
//!   `discarded`: a database left without its key could never be checked or discarded. The one
//!   exception is the orphan-key sweep (`sweep`, release builds only): a key whose office has
//!   left nothing in the home, still so at a start 7 days or more after it was first found so.
//! - No error ever shows key bytes: [`KeyStoreError`] holds only codes and kinds.

#[cfg(debug_assertions)]
mod dev;
mod keychain;
#[cfg(test)]
pub(crate) mod memory;
mod platform;
mod sweep;
mod unsupported;

pub(crate) use sweep::sweep_orphans;

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::Arc;

use org_proto::OfficeSecretsKey;

use crate::code_signing::Signing;

/// Where the host keeps every office's secrets key (see the module docs). Office ids are plain
/// ids ([`org_office_launch::is_plain_id`]); a store refuses any other
/// ([`KeyStoreError::NotPlainId`]).
pub(crate) trait OfficeSecretsKeys: Send + Sync {
    /// Stores `key` as office `office_id`'s key. Add-only: an existing item is
    /// [`KeyStoreError::Exists`] and is left as it is. On any other error nothing is stored.
    fn create(&self, office_id: &str, key: &OfficeSecretsKey) -> Result<(), KeyStoreError>;

    /// Office `office_id`'s key, or `None` if the store has no item for it. An item that isn't a
    /// key (the wrong length, not hex) is [`KeyStoreError::Corrupt`]. An `OfficeSecretsKey` keeps its bytes
    /// on the heap, so moving it moves a pointer; drop it as soon as its line is written.
    fn load(&self, office_id: &str) -> Result<Option<OfficeSecretsKey>, KeyStoreError>;

    /// Deletes office `office_id`'s key. A missing item is `Ok`.
    fn delete(&self, office_id: &str) -> Result<(), KeyStoreError>;

    /// The office ids the store holds a key for, never the keys themselves. Items whose name
    /// isn't a plain id are left out.
    fn list(&self) -> Result<Vec<String>, KeyStoreError>;
}

/// Why a key store call failed. It never holds key bytes, and neither does its `Display`.
/// Each adapter constructs only the variants it can meet; a build without that adapter
/// leaves them unused, which is not dead code to remove.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum KeyStoreError {
    /// `create`: the store already has an item for that office (it was left as it is).
    Exists,
    /// `load`: the item isn't a key.
    Corrupt,
    /// The office id isn't a plain id.
    NotPlainId,
    /// The Keychain failed with this `OSStatus`.
    Os(i32),
    /// The development store's file I/O failed.
    Io(io::Error),
    /// This operating system has no office secrets key adapter (`unsupported`).
    Unsupported,
}

impl KeyStoreError {
    /// Whether the office's key is to be treated as missing: only an item that isn't a key.
    /// Every other error, a refused platform included, is a store failure.
    pub(crate) fn means_key_missing(&self) -> bool {
        matches!(self, Self::Corrupt)
    }
}

impl fmt::Display for KeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyStoreError::Exists => f.write_str("the key store already has a key for that office"),
            KeyStoreError::Corrupt => f.write_str("the stored item isn't an office secrets key"),
            KeyStoreError::NotPlainId => f.write_str("the office id isn't a plain id"),
            KeyStoreError::Os(status) => write!(f, "the Keychain failed (OSStatus {status})"),
            KeyStoreError::Io(e) => write!(f, "the key folder: {e}"),
            KeyStoreError::Unsupported => f.write_str(
                "this operating system has no office secrets key store (only macOS is implemented)",
            ),
        }
    }
}

impl std::error::Error for KeyStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            KeyStoreError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for KeyStoreError {
    fn from(e: io::Error) -> Self {
        KeyStoreError::Io(e)
    }
}

/// Why no key store could be chosen (the host maps it to its own startup error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoreError {
    /// A debug build has no development key folder (absolute, outside the office home).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    NoDevKeyDir,
    /// A release build has no Keychain access group compiled in.
    // Keychain preconditions: only the macOS adapter (`platform`) checks them.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    NoKeychainGroup,
    /// A release build's access group isn't in its own `keychain-access-groups` entitlement.
    // Keychain preconditions: only the macOS adapter (`platform`) checks them.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    KeychainGroupNotEntitled { group: String },
    /// A release build's own signature can't be read, or it isn't signed with the hardened
    /// runtime, or is entitled to let other code into it (`code_signing::REJECTED_ENTITLEMENTS`):
    /// any same-user process could then read the keys out of it.
    #[cfg_attr(any(debug_assertions, not(target_os = "macos")), allow(dead_code))]
    NotHardened,
}

/// The key store this host uses. Debug builds: the development folder `dev_key_dir`
/// (absolute, outside the office `home`), and [`StoreError::NoDevKeyDir`] without one, so a
/// debug host never writes a real Keychain item. Release builds on an operating system with no
/// key store: the refusing adapter ([`KeyStoreError::Unsupported`] on every call), before any of
/// the checks below. Release builds on macOS: the Data Protection keychain in the access group compiled in as `ORG_HOST_KEYCHAIN_GROUP`: [`StoreError::NoKeychainGroup`]
/// without one. `own_signing` (the host's own signing information, read once) must show both
/// that group in its `keychain-access-groups` entitlement
/// ([`StoreError::KeychainGroupNotEntitled`] otherwise, so a misbuilt host fails at startup
/// rather than at its first `SecItemAdd`) and the hardened runtime with none of the
/// entitlements that let other code in ([`StoreError::NotHardened`] otherwise: an injectable
/// host would give every key away). A release build ignores `dev_key_dir`.
pub(crate) fn store_for(
    dev_key_dir: Option<&Path>,
    home: &Path,
    instance_id: Option<&str>,
    own_signing: impl FnOnce() -> Option<Signing>,
) -> Result<Arc<dyn OfficeSecretsKeys>, StoreError> {
    #[cfg(debug_assertions)]
    {
        let _ = own_signing;
        let scoped = dev_key_dir.map(|dir| match instance_id {
            Some(id) => dir.join(id),
            None => dir.to_owned(),
        });
        dev_store(scoped.as_deref(), home)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (dev_key_dir, home);
        platform::release_keys(
            option_env!("ORG_HOST_KEYCHAIN_GROUP"),
            instance_id,
            own_signing,
        )
    }
}

/// The development store in `dir`, which must be absolute and outside the office `home`: inside
/// `offices/` it would look like an unfinished office (and be discarded with every key in it),
/// and anywhere in the home it would put plain-text keys there.
#[cfg(debug_assertions)]
fn dev_store(dir: Option<&Path>, home: &Path) -> Result<Arc<dyn OfficeSecretsKeys>, StoreError> {
    let inside = |dir: &Path| {
        let lexical = dir.starts_with(home);
        let canonical = match (dir.canonicalize(), home.canonicalize()) {
            (Ok(dir), Ok(home)) => dir.starts_with(home),
            _ => false,
        };
        lexical || canonical
    };
    match dir {
        Some(dir) if dir.is_absolute() && !inside(dir) => {
            Ok(Arc::new(dev::DevKeys::new(dir.to_path_buf())))
        }
        _ => Err(StoreError::NoDevKeyDir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(debug_assertions)]
    #[test]
    fn instance_key_namespaces_and_orphan_sweeps_are_independent() {
        use std::time::{Duration, UNIX_EPOCH};
        let fixture = tempfile::tempdir().unwrap();
        let home_a = fixture.path().join("a");
        let home_b = fixture.path().join("b");
        std::fs::create_dir(&home_a).unwrap();
        std::fs::create_dir(&home_b).unwrap();
        let key_base = fixture.path().join("keys");
        let a = store_for(Some(&key_base), &home_a, Some("INSTANCEA"), || {
            panic!("debug never signs")
        })
        .unwrap();
        let b = store_for(Some(&key_base), &home_b, Some("INSTANCEB"), || {
            panic!("debug never signs")
        })
        .unwrap();
        let office = "SAMEOFFICE";
        a.create(office, &OfficeSecretsKey::from_bytes([1; 32]))
            .unwrap();
        b.create(office, &OfficeSecretsKey::from_bytes([2; 32]))
            .unwrap();
        assert_eq!(a.list().unwrap(), vec![office]);
        assert_eq!(b.list().unwrap(), vec![office]);
        let before = b.load(office).unwrap().unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        assert!(sweep_orphans(&*a, &home_a, |ids| Ok(ids.to_vec()), now).is_empty());
        assert_eq!(
            sweep_orphans(
                &*a,
                &home_a,
                |ids| Ok(ids.to_vec()),
                now + Duration::from_secs(8 * 86400)
            ),
            vec![office]
        );
        assert!(a.load(office).unwrap().is_none());
        assert_eq!(b.load(office).unwrap().unwrap(), before);
        assert!(!home_b.join("orphan-keys.json").exists());
        b.delete(office).unwrap();
        assert!(b.list().unwrap().is_empty());
    }

    #[test]
    fn only_a_corrupt_item_means_the_key_is_missing() {
        assert!(KeyStoreError::Corrupt.means_key_missing());
        for error in [
            KeyStoreError::Exists,
            KeyStoreError::NotPlainId,
            KeyStoreError::Os(-25308),
            KeyStoreError::Io(io::Error::from(io::ErrorKind::PermissionDenied)),
            KeyStoreError::Unsupported,
        ] {
            assert!(!error.means_key_missing(), "{error:?}");
        }
    }

    #[test]
    fn errors_never_show_more_than_codes() {
        let shown = [
            KeyStoreError::Exists,
            KeyStoreError::Corrupt,
            KeyStoreError::NotPlainId,
            KeyStoreError::Os(-25308),
            KeyStoreError::Io(io::Error::from(io::ErrorKind::PermissionDenied)),
            KeyStoreError::Unsupported,
        ]
        .map(|e| e.to_string());
        assert!(shown[3].contains("-25308"), "{}", shown[3]);
        for text in shown {
            assert!(!text.is_empty());
            assert!(!text.bytes().any(|b| b == b'\n'), "{text}");
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_debug_build_needs_an_absolute_dev_key_dir_outside_the_home() {
        use std::path::Path;
        let home = Path::new("/tmp/org-home-never-made");
        for dir in [
            None,
            Some(Path::new("relative/keys")),
            Some(home),
            Some(Path::new("/tmp/org-home-never-made/offices/keys")),
        ] {
            assert!(
                matches!(dev_store(dir, home), Err(StoreError::NoDevKeyDir)),
                "{dir:?}"
            );
        }
        assert!(dev_store(Some(Path::new("/tmp/org-keys-never-made")), home).is_ok());
    }
}
