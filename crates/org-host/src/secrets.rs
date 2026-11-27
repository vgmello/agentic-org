//! Making the host's secrets, `std` only: office secrets keys, plus non-secret unique names drawn from
//! the same source.
//!
//! The host makes an office's secrets key once, when it creates the office, and stores it in its
//! key store (`keys`) before the office's server starts. It is in the host's memory only from its
//! creation (or a load from the store) until the first line that carries it is written.

use std::io;

use org_proto::{OfficeSecretsKey, OFFICE_SECRETS_KEY_BYTES};

/// A fresh office secrets key: 32 bytes from OS CSPRNG (`org_platform::confinement::fill_entropy`).
pub(crate) fn generate_office_secrets_key() -> io::Result<OfficeSecretsKey> {
    random(OfficeSecretsKey::from_bytes)
}

/// A fresh non-secret unique name: 16 lowercase hex characters (8 bytes) from OS CSPRNG,
/// for per-spawn file names such as a rendezvous socket. It is not a key and carries no authority.
pub(crate) fn generate_unique_name() -> io::Result<String> {
    random(|bytes| bytes[..8].iter().map(|b| format!("{b:02x}")).collect())
}

/// 32 bytes from OS CSPRNG, wrapped by `wrap`. The bytes read are zeroed afterwards (best
/// effort), whether or not the read succeeded; only the wrapped secret keeps them.
fn random<T>(wrap: impl FnOnce([u8; 32]) -> T) -> io::Result<T> {
    const _: () = assert!(OFFICE_SECRETS_KEY_BYTES == 32);
    let mut bytes = [0u8; 32];
    let read = org_platform::confinement::fill_entropy(&mut bytes);
    let secret = read.map(|()| wrap(bytes));
    zero(&mut bytes);
    secret
}

/// Overwrites with zeros in a way the optimizer can't remove (best effort via [`org_proto::zero_bytes`]).
pub(crate) fn zero(bytes: &mut [u8]) {
    org_proto::zero_bytes(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_random() {
        let (a, b) = (
            generate_office_secrets_key().unwrap(),
            generate_office_secrets_key().unwrap(),
        );
        assert!(a != b);
        assert!(a != OfficeSecretsKey::from_bytes([0; 32]));
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
