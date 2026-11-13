//! Making the host's secrets, `std` only: app tokens (ADR-0003 §2.1, launch v2, retired with
//! it), database keys, grant keys and recovery codes.
//!
//! The host makes an office's database key once, when it creates the office, and stores it in its
//! key store (`keys`) before the office's server starts. It is in the host's memory only from its
//! creation (or a load from the store) until the handoff that carries it is written.
//!
//! A grant key (X-02 §4.1) is made for every start of a server and kept in memory, with that
//! running server, for as long as it runs; a recovery code (§6.4) for every create, returned
//! once with its result. Neither reaches a file, argv, the environment or a log.

use std::fs::File;
use std::io::{self, Read};

use org_proto::{
    AppToken, DbKey, GrantKey, RecoveryCode, APP_TOKEN_BYTES, DB_KEY_BYTES, GRANT_KEY_BYTES,
};

/// The recovery code's alphabet (Crockford Base32) and shape: 8 groups of 4, 160 bits.
const CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CODE_GROUPS: usize = 8;

/// A fresh app token: 32 bytes from `/dev/urandom`.
pub(crate) fn generate_app_token() -> io::Result<AppToken> {
    random(AppToken::from_bytes)
}

/// A fresh grant key: 32 bytes from `/dev/urandom`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "sent by the v3 spawn, next branch")
)]
pub(crate) fn generate_grant_key() -> io::Result<GrantKey> {
    random(GrantKey::from_bytes)
}

/// A fresh recovery code, `XXXX-XXXX-…` (8 groups of 4 Crockford Base32 characters, 160 bits
/// from `/dev/urandom`), built in place: no copy of it is left behind.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "sent by the v3 create, next branch")
)]
pub(crate) fn generate_recovery_code() -> io::Result<RecoveryCode> {
    let mut bytes = [0u8; 20];
    let read = File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes));
    let mut code = String::with_capacity(CODE_GROUPS * 5 - 1);
    if read.is_ok() {
        for index in 0..CODE_GROUPS * 4 {
            if index > 0 && index % 4 == 0 {
                code.push('-');
            }
            // 5 bits from the 40 of each group of 5 bytes.
            let chunk = &bytes[index / 8 * 5..index / 8 * 5 + 5];
            let bits = chunk.iter().fold(0u64, |acc, b| acc << 8 | u64::from(*b));
            let value = (bits >> (35 - 5 * (index % 8))) & 0x1f;
            code.push(char::from(CODE_ALPHABET[value as usize]));
        }
    }
    zero(&mut bytes);
    read?;
    RecoveryCode::new(code).map_err(io::Error::other)
}

/// A fresh database key: 32 bytes from `/dev/urandom`.
pub(crate) fn generate_db_key() -> io::Result<DbKey> {
    random(DbKey::from_bytes)
}

/// 32 bytes from `/dev/urandom`, wrapped by `wrap`. The bytes read are zeroed afterwards (best
/// effort), whether or not the read succeeded; only the wrapped secret keeps them.
fn random<T>(wrap: impl FnOnce([u8; 32]) -> T) -> io::Result<T> {
    const _: () = assert!(APP_TOKEN_BYTES == 32 && GRANT_KEY_BYTES == 32 && DB_KEY_BYTES == 32);
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
    fn grant_keys_are_random() {
        let (a, b) = (generate_grant_key().unwrap(), generate_grant_key().unwrap());
        assert!(a.expose_bytes() != b.expose_bytes());
        assert!(*a.expose_bytes() != [0; 32]);
    }

    #[test]
    fn recovery_codes_are_random_and_shaped() {
        let (a, b) = (
            generate_recovery_code().unwrap(),
            generate_recovery_code().unwrap(),
        );
        assert_ne!(a.expose(), b.expose());
        let groups: Vec<&str> = a.expose().split('-').collect();
        assert_eq!(groups.len(), 8);
        assert!(groups
            .iter()
            .all(|g| g.len() == 4 && g.bytes().all(|c| CODE_ALPHABET.contains(&c))));
        assert_eq!(format!("{a:?}"), "RecoveryCode(<redacted>)");
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
