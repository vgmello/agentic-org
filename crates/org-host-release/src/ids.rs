//! Fresh Crockford Base32 ULIDs for private release-store staging and temporary control files.

use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

pub(crate) fn generate() -> io::Result<String> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut random = [0u8; 10];
    org_platform::confinement::fill_entropy(&mut random)?;
    let mut tail = [0u8; 16];
    tail[6..].copy_from_slice(&random);
    let mut value = (u128::from(now_ms & 0xFFFF_FFFF_FFFF) << 80) | u128::from_be_bytes(tail);
    let mut out = [0u8; 26];
    for slot in out.iter_mut().rev() {
        *slot = CROCKFORD[(value & 0x1F) as usize];
        value >>= 5;
    }
    Ok(String::from_utf8(out.to_vec()).expect("the Crockford alphabet is ASCII"))
}
