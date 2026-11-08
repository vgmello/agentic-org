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

/// The Unix milliseconds a ULID was made at, or `None` if `id` is not a ULID in its canonical
/// form: `org-proto`'s shared rule, so a command id and a change id are judged alike.
pub(crate) use org_proto::ulid_time_ms as time_ms;

/// A ULID made at `ms` whose random part is `n` repeated (for tests).
#[cfg(test)]
pub(crate) fn from_ms(ms: u64, n: u8) -> String {
    encode(ms, [n; 10])
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
    fn reads_the_time_of_a_canonical_ulid_only() {
        assert_eq!(
            time_ms("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            Some(1_469_922_850_259)
        );
        assert_eq!(time_ms(&encode(42, [7; 10])), Some(42));
        assert_eq!(
            time_ms(&encode(0xFFFF_FFFF_FFFF, [0xFF; 10])),
            Some(0xFFFF_FFFF_FFFF)
        );
        for bad in [
            "01arz3ndektsv4rrffq69g5fav", // lower case
            "01ARZ3NDEKTSV4RRFFQ69G5FAU", // U is not in the alphabet
            "01ARZ3NDEKTSV4RRFFQ69G5FA",  // short
            "81ARZ3NDEKTSV4RRFFQ69G5FAV", // too big
            "",
            "has space",
        ] {
            assert_eq!(time_ms(bad), None, "{bad}");
        }
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
