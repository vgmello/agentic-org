//! Key custody: where the host keeps every office's database key.
//!
//! The host creates each office's key (`secrets::generate_db_key`), stores it here before the
//! office's first `--create`, loads it before every `--office` and `--discard`, and sends it on
//! the first line of the launch pipe (`org-office-launch`). Its servers never touch a key store.
//! A key is in the host's memory only from its creation or load until that first line is written.
//!
//! [`KeyStore`] is the host's own abstraction: the supervisor depends on it, never on a
//! particular store. The stores:
//! - **Release builds:** the Data Protection keychain, in the host's access group
//!   (`keychain`), so other code gets `errSecItemNotFound`, never a prompt.
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
#[cfg(target_os = "macos")]
mod keychain;
#[cfg(test)]
pub(crate) mod memory;
mod sweep;

pub(crate) use sweep::sweep_orphans;

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::Arc;

use org_proto::DbKey;

/// Where the host keeps every office's database key (see the module docs). Office ids are plain
/// ids ([`org_office_launch::is_plain_id`]); a store refuses any other
/// ([`KeyStoreError::NotPlainId`]).
pub(crate) trait KeyStore: Send + Sync {
    /// Stores `key` as office `office_id`'s key. Add-only: an existing item is
    /// [`KeyStoreError::Exists`] and is left as it is. On any other error nothing is stored.
    fn create(&self, office_id: &str, key: &DbKey) -> Result<(), KeyStoreError>;

    /// Office `office_id`'s key, or `None` if the store has no item for it. An item that isn't a
    /// key (the wrong length, not hex) is [`KeyStoreError::Corrupt`]. A `DbKey` keeps its bytes
    /// on the heap, so moving it moves a pointer; drop it as soon as its line is written.
    fn load(&self, office_id: &str) -> Result<Option<DbKey>, KeyStoreError>;

    /// Deletes office `office_id`'s key. A missing item is `Ok`.
    fn delete(&self, office_id: &str) -> Result<(), KeyStoreError>;

    /// The office ids the store holds a key for, never the keys themselves. Items whose name
    /// isn't a plain id are left out.
    fn list(&self) -> Result<Vec<String>, KeyStoreError>;
}

/// Why a key store call failed. It never holds key bytes, and neither does its `Display`.
#[derive(Debug)]
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
}

impl fmt::Display for KeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyStoreError::Exists => f.write_str("the key store already has a key for that office"),
            KeyStoreError::Corrupt => f.write_str("the stored item isn't a database key"),
            KeyStoreError::NotPlainId => f.write_str("the office id isn't a plain id"),
            KeyStoreError::Os(status) => write!(f, "the Keychain failed (OSStatus {status})"),
            KeyStoreError::Io(e) => write!(f, "the key folder: {e}"),
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
    NoKeychainGroup,
    /// A release build's access group isn't in its own `keychain-access-groups` entitlement.
    KeychainGroupNotEntitled { group: String },
}

/// The key store this host uses. Debug builds: the development folder `dev_key_dir`
/// (absolute, outside the office `home`), and [`StoreError::NoDevKeyDir`] without one, so a
/// debug host never writes a real Keychain item. Release builds: the Data Protection keychain
/// in the access group compiled in as `ORG_HOST_KEYCHAIN_GROUP`: [`StoreError::NoKeychainGroup`]
/// without one, and [`StoreError::KeychainGroupNotEntitled`] unless `own_access_groups` (the
/// host's own `keychain-access-groups` entitlement) lists it, so a misbuilt host fails at
/// startup rather than at its first `SecItemAdd`. A release build ignores `dev_key_dir`.
pub(crate) fn store_for(
    dev_key_dir: Option<&Path>,
    home: &Path,
    own_access_groups: impl FnOnce() -> Option<Vec<String>>,
) -> Result<Arc<dyn KeyStore>, StoreError> {
    #[cfg(debug_assertions)]
    {
        let _ = own_access_groups;
        dev_store(dev_key_dir, home)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (dev_key_dir, home);
        release_store(option_env!("ORG_HOST_KEYCHAIN_GROUP"), own_access_groups)
    }
}

/// The development store in `dir`, which must be absolute and outside the office `home`: inside
/// `offices/` it would look like an unfinished office (and be discarded with every key in it),
/// and anywhere in the home it would put plain-text keys there.
#[cfg(debug_assertions)]
fn dev_store(dir: Option<&Path>, home: &Path) -> Result<Arc<dyn KeyStore>, StoreError> {
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

/// The release store for the access group `group` (`ORG_HOST_KEYCHAIN_GROUP` at build time),
/// once `entitled` (the host's own `keychain-access-groups`) shows the host may use it.
///
/// The live entitlement check can't run in the tests: an unsigned (or ad-hoc signed) test
/// binary has no `keychain-access-groups`, so it would always refuse. [`group_entitled`], the
/// decision, is tested on synthetic entitlements; the reading is `app_auth`'s, tested on
/// synthetic signing information. Check it live on the first signed build.
#[cfg_attr(debug_assertions, allow(dead_code))]
fn release_store(
    group: Option<&str>,
    entitled: impl FnOnce() -> Option<Vec<String>>,
) -> Result<Arc<dyn KeyStore>, StoreError> {
    let group = match group {
        Some(group) if !group.trim().is_empty() => group,
        _ => return Err(StoreError::NoKeychainGroup),
    };
    if !group_entitled(group, entitled().as_deref()) {
        return Err(StoreError::KeychainGroupNotEntitled {
            group: group.to_owned(),
        });
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Arc::new(keychain::Keychain::new(group)))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(StoreError::NoKeychainGroup)
    }
}

/// Whether the access group `group` is one of the host's own `entitled` groups (exactly). No
/// entitlement, or one that couldn't be read, entitles nothing.
#[cfg_attr(debug_assertions, allow(dead_code))]
fn group_entitled(group: &str, entitled: Option<&[String]>) -> bool {
    entitled.is_some_and(|groups| groups.iter().any(|g| g == group))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_never_show_more_than_codes() {
        let shown = [
            KeyStoreError::Exists,
            KeyStoreError::Corrupt,
            KeyStoreError::NotPlainId,
            KeyStoreError::Os(-25308),
            KeyStoreError::Io(io::Error::from(io::ErrorKind::PermissionDenied)),
        ]
        .map(|e| e.to_string());
        assert!(shown[3].contains("-25308"), "{}", shown[3]);
        for text in shown {
            assert!(!text.is_empty());
            assert!(!text.bytes().any(|b| b == b'\n'), "{text}");
        }
    }

    #[test]
    fn a_release_build_without_an_access_group_has_no_store() {
        let mut asked = false;
        for group in [None, Some(""), Some("  ")] {
            let store = release_store(group, || {
                asked = true;
                Some(vec!["G".to_owned()])
            });
            assert!(
                matches!(store, Err(StoreError::NoKeychainGroup)),
                "{group:?}"
            );
        }
        assert!(!asked, "the entitlement was read without a group");
    }

    #[test]
    fn a_release_build_needs_its_access_group_in_its_own_entitlement() {
        const GROUP: &str = "TEAMID1234.org.agentic.host";
        let groups = |list: &[&str]| Some(list.iter().map(|g| (*g).to_owned()).collect());
        for entitled in [
            None,
            groups(&[]),
            groups(&["TEAMID1234.other"]),
            groups(&[
                "TEAMID1234.org.agentic.host.x",
                "teamid1234.org.agentic.host",
            ]),
        ] {
            assert!(!group_entitled(GROUP, entitled.as_deref()), "{entitled:?}");
            let err = release_store(Some(GROUP), || entitled.clone())
                .err()
                .unwrap();
            assert!(
                matches!(&err, StoreError::KeychainGroupNotEntitled { group } if group == GROUP),
                "{entitled:?}: {err:?}"
            );
        }
        let entitled = groups(&["TEAMID1234.other", GROUP]);
        assert!(group_entitled(GROUP, entitled.as_deref()));
        // Choosing the store touches no Keychain item.
        assert!(release_store(Some(GROUP), || entitled).is_ok());
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
