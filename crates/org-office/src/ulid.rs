//! Opaque, sortable 26-character ULIDs (Crockford Base32), `std` only.
//!
//! No process-wide monotonic state: organization ids are created rarely and a collision is
//! caught by the database's primary key, so a time prefix plus 80 random bits is enough.
//! Ids are therefore NOT ordered within one millisecond; order by insertion, not by id.

use std::fs::File;
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A fresh ULID: 48 bits of Unix milliseconds followed by 80 random bits.
pub(crate) fn generate() -> std::io::Result<String> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    Ok(encode(now_ms, random_bytes()?))
}

/// `N` bytes from the operating system's random source.
pub(crate) fn random_bytes<const N: usize>() -> std::io::Result<[u8; N]> {
    let mut bytes = [0u8; N];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn encode(ms: u64, random: [u8; 10]) -> String {
    let mut value = (u128::from(ms & 0xFFFF_FFFF_FFFF) << 80) | {
        let mut tail = [0u8; 16];
        tail[6..].copy_from_slice(&random);
        u128::from_be_bytes(tail)
    };
    let mut out = [0u8; 26];
    for slot in out.iter_mut().rev() {
        *slot = CROCKFORD[(value & 0x1F) as usize];
        value >>= 5;
    }
    String::from_utf8(out.to_vec()).expect("Crockford alphabet is ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_known_vector() {
        assert_eq!(encode(0, [0; 10]), "00000000000000000000000000");
        assert_eq!(
            encode(0xFFFF_FFFF_FFFF, [0xFF; 10]),
            "7ZZZZZZZZZZZZZZZZZZZZZZZZZ"
        );
    }

    #[test]
    fn sorts_by_time_and_is_unique() {
        let earlier = encode(1_000, [9; 10]);
        let later = encode(2_000, [1; 10]);
        assert!(earlier < later);
        assert_ne!(generate().unwrap(), generate().unwrap());
        assert_eq!(generate().unwrap().len(), 26);
    }
}
