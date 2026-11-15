//! Making the host's secrets, `std` only: app tokens (ADR-0003 §2.1) and database keys.
//!
//! The host makes a fresh app token for each office every time it starts (and for an office it
//! creates) and keeps it only in memory, in that office's supervision: it reaches the office's
//! server in the first line of the control pipe, on every start within this host run, and never
//! a file, argv, the environment or a log. See `supervisor::child`.
//!
//! The host makes an office's database key once, when it creates the office, and stores it in its
//! key store (`keys`) before the office's server starts. It is in the host's memory only from its
//! creation (or a load from the store) until the first line that carries it is written.

use std::fs::File;
use std::io::{self, Read};

use org_proto::{AppToken, DbKey, APP_TOKEN_BYTES, DB_KEY_BYTES};

/// A fresh app token: 32 bytes from `/dev/urandom`.
pub(crate) fn generate_app_token() -> io::Result<AppToken> {
    random(AppToken::from_bytes)
}

/// A fresh database key: 32 bytes from `/dev/urandom`. It goes straight into the create message,
/// which the host keeps in a `Box` (`supervisor::child::CreateLine`).
pub(crate) fn generate_db_key() -> io::Result<DbKey> {
    random(DbKey::from_bytes)
}

/// 32 bytes from `/dev/urandom`, wrapped by `wrap`. The bytes read are zeroed afterwards (best
/// effort), whether or not the read succeeded; only the wrapped secret keeps them.
fn random<T>(wrap: impl FnOnce([u8; 32]) -> T) -> io::Result<T> {
    const _: () = assert!(APP_TOKEN_BYTES == 32 && DB_KEY_BYTES == 32);
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
    fn tokens_are_random() {
        let (a, b) = (generate_app_token().unwrap(), generate_app_token().unwrap());
        assert!(a != b);
        assert!(a != AppToken::from_bytes([0; 32]));
    }

    #[test]
    fn keys_are_random() {
        let (a, b) = (generate_db_key().unwrap(), generate_db_key().unwrap());
        assert!(a != b);
        assert!(a != DbKey::from_bytes([0; 32]));
    }

    #[test]
    fn zero_clears_every_byte() {
        let mut bytes = [0xa5u8; 32];
        zero(&mut bytes);
        assert_eq!(bytes, [0; 32]);
    }
}
