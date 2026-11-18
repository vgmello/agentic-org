//! The office secrets key (ADR-0016 decision 3).
//!
//! **The host makes and keeps it** (ADR-0002, key custody) and sends it to the server on the
//! launch pipe. It is never a database key: the office databases are plain SQLite. The worker
//! uses it only to wrap and unwrap the office's private keys (`org_pki::wrap`). An office
//! started without it still serves its data; only what needs a wrapped key is unavailable.

use std::fmt;
use std::sync::Arc;

use org_proto::OfficeSecretsKey;

/// The 256-bit office secrets key. `Debug` never prints it, and there is no `Display`.
///
/// It is the host's [`OfficeSecretsKey`] (zeroed when dropped), shared by reference count: the store keeps
/// one copy for its pool of read connections for as long as the office runs, and the memory is
/// wiped when the last holder drops. Cloning shares that one copy; nothing else copies the bytes
/// but the connection's key statement, which is wiped as soon as it has run.
#[derive(Clone)]
pub struct Key(Arc<OfficeSecretsKey>);

impl Key {
    /// Takes the key the host sent. Best effort: the caller's `OfficeSecretsKey` is consumed, but a Rust move
    /// is a bitwise copy, so a stack frame the key passed through may keep the bytes until it is
    /// reused; the heap copy shared by the stores is the one that is wiped when the last holder
    /// drops.
    pub fn from_db_key(key: OfficeSecretsKey) -> Self {
        Self(Arc::new(key))
    }

    /// A key from raw bytes, for tests (the host makes the real ones).
    #[cfg(test)]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self::from_db_key(OfficeSecretsKey::from_bytes(bytes))
    }

    /// The raw key, for the worker to wrap and unwrap the office's private keys
    /// (`org_pki::wrap`). Never log, persist or send it.
    pub fn expose_bytes(&self) -> &[u8; 32] {
        self.0.expose_bytes()
    }
}

impl PartialEq for Key {
    /// Constant-time (the `OfficeSecretsKey`'s own).
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
        assert_eq!(key.expose_bytes(), &[1; 32]);
        assert_eq!(Arc::strong_count(&key.0), 1);
        let shared = key.clone();
        assert_eq!(Arc::strong_count(&key.0), 2);
        drop(shared);
    }
}
