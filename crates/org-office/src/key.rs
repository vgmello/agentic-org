//! The key that encrypts the office database, and where it is kept.
//!
//! `org-office` never talks to a keychain: the server binary supplies a [`KeyProvider`]
//! (the macOS Keychain in production). Tests supply `InMemoryKeys`, which exists only in
//! test builds and with the `dev-keys` feature (off by default), so it can't reach a release.

use std::error::Error;
use std::fmt;

use crate::ulid;

#[cfg(any(test, feature = "dev-keys"))]
pub use self::in_memory::InMemoryKeys;

// A release build (no debug assertions) must not carry in-memory keys, even if some crate
// in the workspace turns the feature on and Cargo unifies it into the server's build.
#[cfg(all(feature = "dev-keys", not(debug_assertions)))]
compile_error!("the `dev-keys` feature (InMemoryKeys) must not be enabled in release builds");

/// A 256-bit database key. `Debug` never prints it, and there is no `Display`.
/// Its memory is not wiped on drop; the key is needed for as long as the office runs.
#[derive(Clone, PartialEq, Eq)]
pub struct Key([u8; 32]);

impl Key {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// A fresh random key.
    pub fn generate() -> std::io::Result<Self> {
        ulid::random_bytes().map(Self)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key(<redacted>)")
    }
}

pub type KeyError = Box<dyn Error + Send + Sync>;

/// Where an office's key lives between runs. A dumb store: the office decides when to
/// create a key (only `Office::create` does), so a provider never invents one.
///
/// Errors must not contain the key.
pub trait KeyProvider {
    /// The key saved for `office_id`, or `None` if there is none.
    fn load(&self, office_id: &str) -> Result<Option<Key>, KeyError>;

    /// Saves `key` for `office_id`. Must not return `Ok` until the key is durable: the
    /// office creates its database only after this succeeds.
    fn store(&self, office_id: &str, key: &Key) -> Result<(), KeyError>;
}

#[cfg(any(test, feature = "dev-keys"))]
mod in_memory {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::{Key, KeyError, KeyProvider};

    /// Keeps keys in memory only. **For tests and dev**: nothing survives the process, so a
    /// database it encrypted can't be reopened by a later process, unless built with
    /// [`InMemoryKeys::fixed`]. Not compiled into release builds (see the module docs).
    #[derive(Default)]
    pub struct InMemoryKeys {
        fixed: Option<Key>,
        saved: Mutex<HashMap<String, Key>>,
    }

    impl InMemoryKeys {
        pub fn new() -> Self {
            Self::default()
        }

        /// Answers `key` for every office, so separate processes agree. Never use it for real data.
        /// It holds a key for every office id, so `Office::create` refuses to use it
        /// (`KeyExists`); it is for opening a database already encrypted with `key`.
        pub fn fixed(key: Key) -> Self {
            Self {
                fixed: Some(key),
                saved: Mutex::default(),
            }
        }

        fn saved(&self) -> std::sync::MutexGuard<'_, HashMap<String, Key>> {
            self.saved
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }
    }

    impl KeyProvider for InMemoryKeys {
        fn load(&self, office_id: &str) -> Result<Option<Key>, KeyError> {
            Ok(self.saved().get(office_id).cloned().or(self.fixed.clone()))
        }

        fn store(&self, office_id: &str, key: &Key) -> Result<(), KeyError> {
            self.saved().insert(office_id.to_owned(), key.clone());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_key() {
        let key = Key::from_bytes([0xAB; 32]);
        assert_eq!(format!("{key:?}"), "Key(<redacted>)");
    }

    #[test]
    fn generated_keys_differ() {
        assert_ne!(Key::generate().unwrap(), Key::generate().unwrap());
    }

    #[test]
    fn in_memory_keys_round_trip_and_fixed_answers_everywhere() {
        let keys = InMemoryKeys::new();
        assert!(keys.load("a").unwrap().is_none());
        keys.store("a", &Key::from_bytes([1; 32])).unwrap();
        assert_eq!(keys.load("a").unwrap(), Some(Key::from_bytes([1; 32])));
        assert!(keys.load("b").unwrap().is_none());

        let fixed = InMemoryKeys::fixed(Key::from_bytes([7; 32]));
        assert_eq!(
            fixed.load("anything").unwrap(),
            Some(Key::from_bytes([7; 32]))
        );
    }
}
