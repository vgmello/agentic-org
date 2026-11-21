//! Canonical UUIDv7 idempotency key generator for `org-command`.

use std::fs::File;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use org_command::KeySource;

static FALLBACK_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generates canonical lowercase RFC 9562 UUIDv7 strings.
#[derive(Debug, Default)]
pub struct UuidV7KeySource;

fn random_bytes(out: &mut [u8; 10]) {
    if let Ok(mut f) = File::open("/dev/urandom") {
        if f.read_exact(out).is_ok() {
            return;
        }
    }
    let seq = FALLBACK_COUNTER.fetch_add(1, Ordering::Relaxed);
    out[..8].copy_from_slice(&seq.to_be_bytes());
    out[8] = 0xa5;
    out[9] = 0x5a;
}

/// Formats a canonical lowercase RFC 9562 UUIDv7 string.
pub fn new_uuid_v7() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut rand = [0u8; 10];
    random_bytes(&mut rand);
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
    format!(
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
    )
}

impl KeySource for UuidV7KeySource {
    fn new_key(&mut self) -> String {
        new_uuid_v7()
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
