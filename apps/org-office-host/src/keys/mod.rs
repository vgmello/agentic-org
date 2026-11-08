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
//!   `discarded`: a database left without its key could never be checked or discarded.
//! - No error ever shows key bytes: [`KeyStoreError`] holds only codes and kinds.

#[cfg(debug_assertions)]
mod dev;
#[cfg(target_os = "macos")]
mod keychain;
#[cfg(test)]
pub(crate) mod memory;

use std::fmt;
use std::io;
use std::sync::Arc;

use org_proto::DbKey;

use crate::error::HostError;
use crate::Config;

/// Where the host keeps every office's database key (see the module docs). Office ids are plain
/// ids ([`org_office_launch::is_plain_id`]); a store refuses any other
/// ([`KeyStoreError::NotPlainId`]).
pub(crate) trait KeyStore: Send + Sync {
    /// Stores `key` as office `office_id`'s key. Add-only: an existing item is
    /// [`KeyStoreError::Exists`] and is left as it is. On any other error nothing is stored.
    fn create(&self, office_id: &str, key: &DbKey) -> Result<(), KeyStoreError>;

    /// Office `office_id`'s key, or `None` if the store has no item for it. An item that isn't a
    /// key (the wrong length, not hex) is [`KeyStoreError::Corrupt`]. The key is on the heap, so
    /// moving it moves a pointer; drop it as soon as its line is written.
    fn load(&self, office_id: &str) -> Result<Option<Box<DbKey>>, KeyStoreError>;

    /// Deletes office `office_id`'s key. A missing item is `Ok`.
    fn delete(&self, office_id: &str) -> Result<(), KeyStoreError>;
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

/// The key store this host uses. Debug builds: the development folder `config.dev_key_dir`
/// (absolute), and [`HostError::NoDevKeyDir`] without one, so a debug host never writes a real
/// Keychain item. Release builds: the Data Protection keychain in the access group compiled in
/// as `ORG_HOST_KEYCHAIN_GROUP`, and [`HostError::NoKeychainGroup`] without one.
pub(crate) fn store_for(config: &Config) -> Result<Arc<dyn KeyStore>, HostError> {
    #[cfg(debug_assertions)]
    {
        dev_store(config.dev_key_dir.as_deref(), &config.home)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = config;
        release_store(option_env!("ORG_HOST_KEYCHAIN_GROUP"))
    }
}

/// The development store in `dir`, which must be absolute and outside the office `home`: inside
/// `offices/` it would look like an unfinished office (and be discarded with every key in it),
/// and anywhere in the home it would put plain-text keys there.
#[cfg(debug_assertions)]
fn dev_store(
    dir: Option<&std::path::Path>,
    home: &std::path::Path,
) -> Result<Arc<dyn KeyStore>, HostError> {
    let inside = |dir: &std::path::Path| {
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
        _ => Err(HostError::NoDevKeyDir),
    }
}

/// The release store for the access group `group` (`ORG_HOST_KEYCHAIN_GROUP` at build time).
// TODO(host-keys follow-up): at startup, check `group` against the host's own
// `keychain-access-groups` entitlement (and fail clearly if it isn't there), instead of
// finding out at the first `SecItemAdd`.
#[cfg_attr(debug_assertions, allow(dead_code))]
fn release_store(group: Option<&str>) -> Result<Arc<dyn KeyStore>, HostError> {
    match group {
        #[cfg(target_os = "macos")]
        Some(group) if !group.trim().is_empty() => Ok(Arc::new(keychain::Keychain::new(group))),
        _ => Err(HostError::NoKeychainGroup),
    }
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
        for group in [None, Some(""), Some("  ")] {
            assert!(
                matches!(release_store(group), Err(HostError::NoKeychainGroup)),
                "{group:?}"
            );
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
                matches!(dev_store(dir, home), Err(HostError::NoDevKeyDir)),
                "{dir:?}"
            );
        }
        assert!(dev_store(Some(Path::new("/tmp/org-keys-never-made")), home).is_ok());
    }
}
