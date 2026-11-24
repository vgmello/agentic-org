//! Canonical UUIDv7 idempotency key generator for `org-command`.

use std::fs::File;
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use org_command::KeySource;

/// Generates canonical lowercase RFC 9562 UUIDv7 strings.
#[derive(Debug, Default)]
pub struct UuidV7KeySource;

/// Formats a canonical lowercase RFC 9562 UUIDv7 string, returning `None` if the
/// system clock or OS entropy source cannot produce a valid key (fail-closed).
pub fn try_new_uuid_v7() -> Option<String> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis();
    if ms > 0xffff_ffff_ffff {
        return None;
    }
    let ms = ms as u64;
    let mut rand = [0u8; 10];
    File::open("/dev/urandom")
        .ok()?
        .read_exact(&mut rand)
        .ok()?;
    let mut bytes = [0u8; 16];
    bytes[0] = ((ms >> 40) & 0xff) as u8;
    bytes[1] = ((ms >> 32) & 0xff) as u8;
    bytes[2] = ((ms >> 24) & 0xff) as u8;
    bytes[3] = ((ms >> 16) & 0xff) as u8;
    bytes[4] = ((ms >> 8) & 0xff) as u8;
    bytes[5] = (ms & 0xff) as u8;
    bytes[6] = 0x70 | (rand[0] & 0x0f);
    bytes[7] = rand[1];
    bytes[8] = 0x80 | (rand[2] & 0x3f);
    bytes[9..16].copy_from_slice(&rand[3..10]);
    Some(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    ))
}

impl KeySource for UuidV7KeySource {
    fn new_key(&mut self) -> String {
        // Returning an empty string causes `org-command` to fail closed with `ErrorCode::Unavailable`.
        try_new_uuid_v7().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_distinct_canonical_uuid_v7_keys() {
        let mut keys = UuidV7KeySource;
        let a = keys.new_key();
        let b = keys.new_key();
        assert!(org_proto::is_uuid_v7(&a), "{a}");
        assert!(org_proto::is_uuid_v7(&b), "{b}");
        assert_ne!(a, b);
    }
}
