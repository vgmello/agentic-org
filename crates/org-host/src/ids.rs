//! Organization and office ids: 26-character ULIDs (Crockford Base32), `std` only.
//!
//! No process-wide monotonic state: the host makes ids only on `organization.create`, which is
//! rare and serialized, and a clash of 80 random bits is not a practical concern. Ids are
//! therefore not ordered within one millisecond.

use std::fs::File;
use std::io::{self, Read};
use std::time::{SystemTime, UNIX_EPOCH};

use org_proto::location::OFFICE_SOCKET_HANDLE_CHARS;

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The last 12 characters of an office id (its random part): its socket's and log's name.
pub(crate) fn handle_of(office_id: &str) -> &str {
    &office_id[office_id.len().saturating_sub(OFFICE_SOCKET_HANDLE_CHARS)..]
}

/// A fresh ULID: 48 bits of Unix milliseconds followed by 80 bits from `/dev/urandom`.
pub(crate) fn generate() -> io::Result<String> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut random = [0u8; 10];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    Ok(encode(now_ms, random))
}

/// Canonical selected entity identity. Legacy ULID generation remains unchanged.
pub(crate) fn uuid_v7() -> io::Result<String> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::other("clock unavailable"))?
        .as_millis();
    if ms > 0xffff_ffff_ffff {
        return Err(io::Error::other("UUID clock exhausted"));
    }
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    bytes[..6].copy_from_slice(&(ms as u64).to_be_bytes()[2..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}

fn encode(ms: u64, random: [u8; 10]) -> String {
    let mut tail = [0u8; 16];
    tail[6..].copy_from_slice(&random);
    let mut value = (u128::from(ms & 0xFFFF_FFFF_FFFF) << 80) | u128::from_be_bytes(tail);
    let mut out = [0u8; 26];
    for slot in out.iter_mut().rev() {
        *slot = CROCKFORD[(value & 0x1F) as usize];
        value >>= 5;
    }
    String::from_utf8(out.to_vec()).expect("the Crockford alphabet is ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_known_vectors() {
        assert_eq!(encode(0, [0; 10]), "00000000000000000000000000");
        assert_eq!(
            encode(0xFFFF_FFFF_FFFF, [0xFF; 10]),
            "7ZZZZZZZZZZZZZZZZZZZZZZZZZ"
        );
    }

    #[test]
    fn ids_are_plain_unique_and_time_ordered() {
        let (a, b) = (generate().unwrap(), generate().unwrap());
        assert_ne!(a, b);
        assert_eq!(a.len(), 26);
        assert!(org_office_launch::is_plain_id(&a));
        assert!(encode(1_000, [9; 10]) < encode(2_000, [1; 10]));
    }
}
