//! The key that encrypts the office databases.
//!
//! **The host makes and keeps it** (ADR-0002, key custody): it creates the key, holds it in its
//! own Keychain item, and sends it to the server on the launch pipe. `org-office` never makes,
//! stores, loads or deletes a key: `Office::create`, `open` and `discard` are GIVEN one, so a
//! server started by anyone but the host has none and cannot open the databases.

use std::fmt;
use std::sync::Arc;

use org_proto::DbKey;

/// A 256-bit database key. `Debug` never prints it, and there is no `Display`.
///
/// It is the host's [`DbKey`] (zeroed when dropped), shared by reference count: the store keeps
/// one copy for its pool of read connections for as long as the office runs, and the memory is
/// wiped when the last holder drops. Cloning shares that one copy; nothing else copies the bytes
/// but the connection's key statement, which is wiped as soon as it has run.
#[derive(Clone)]
pub struct Key(Arc<DbKey>);

impl Key {
    /// Takes the key the host sent. Best effort: the caller's `DbKey` is consumed, but a Rust move
    /// is a bitwise copy, so a stack frame the key passed through may keep the bytes until it is
    /// reused; the heap copy shared by the stores is the one that is wiped when the last holder
    /// drops.
    pub fn from_db_key(key: DbKey) -> Self {
        Self(Arc::new(key))
    }

    /// A key from raw bytes, for tests (the host makes the real ones).
    #[cfg(test)]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self::from_db_key(DbKey::from_bytes(bytes))
    }

    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        self.0.expose_bytes()
    }
}

impl PartialEq for Key {
    /// Constant-time (the `DbKey`'s own).
    fn eq(&self, other: &Self) -> bool {
        *self.0 == *other.0
    }
}

impl Eq for Key {}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key(<redacted>)")
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
    fn keys_compare_by_value_and_clones_share_one_copy() {
        let key = Key::from_bytes([1; 32]);
        assert_eq!(key, key.clone());
        assert_ne!(key, Key::from_bytes([2; 32]));
        assert_eq!(key.as_bytes(), &[1; 32]);
        assert_eq!(Arc::strong_count(&key.0), 1);
        let shared = key.clone();
        assert_eq!(Arc::strong_count(&key.0), 2);
        drop(shared);
    }
}
