//! Making the host's secrets, `std` only: database keys, plus non-secret unique names drawn from
//! the same source. The host makes no app tokens: a v3 launch handoff carries none.
//!
//! The host makes an office's database key once, when it creates the office, and stores it in its
//! key store (`keys`) before the office's server starts. It is in the host's memory only from its
//! creation (or a load from the store) until the first line that carries it is written.

use std::fs::File;
use std::io::{self, Read};

use org_proto::{DbKey, DB_KEY_BYTES};

/// A fresh database key: 32 bytes from `/dev/urandom`.
pub(crate) fn generate_db_key() -> io::Result<DbKey> {
    random(DbKey::from_bytes)
}

/// A fresh non-secret unique name: 16 lowercase hex characters (8 bytes) from `/dev/urandom`,
/// for per-spawn file names such as a rendezvous socket. It is not a key and carries no authority.
pub(crate) fn generate_unique_name() -> io::Result<String> {
    random(|bytes| bytes[..8].iter().map(|b| format!("{b:02x}")).collect())
}

/// 32 bytes from `/dev/urandom`, wrapped by `wrap`. The bytes read are zeroed afterwards (best
/// effort), whether or not the read succeeded; only the wrapped secret keeps them.
fn random<T>(wrap: impl FnOnce([u8; 32]) -> T) -> io::Result<T> {
    const _: () = assert!(DB_KEY_BYTES == 32);
    let mut bytes = [0u8; 32];
    let read = File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes));
    let secret = read.map(|()| wrap(bytes));
    zero(&mut bytes);
    secret
}

/// Overwrites with zeros in a way the optimizer can't remove (best effort, as `AppToken` does).
pub(crate) fn zero(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        // SAFETY: `b` is a valid, aligned, exclusive reference to a u8.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_random() {
        let (a, b) = (generate_db_key().unwrap(), generate_db_key().unwrap());
        assert!(a != b);
        assert!(a != DbKey::from_bytes([0; 32]));
    }

    #[test]
    fn unique_names_are_random_lowercase_hex() {
        let (a, b) = (
            generate_unique_name().unwrap(),
            generate_unique_name().unwrap(),
        );
        assert_ne!(a, b);
        for name in [&a, &b] {
            assert_eq!(name.len(), 16);
            assert!(
                name.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')),
                "{name}"
            );
        }
    }

    #[test]
    fn zero_clears_every_byte() {
        let mut bytes = [0xa5u8; 32];
        zero(&mut bytes);
        assert_eq!(bytes, [0; 32]);
    }
}
