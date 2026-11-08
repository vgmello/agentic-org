//! Monotonic, lexicographically sortable 26-char Crockford Base32 ULID generator (`std`-only).

use std::fs::File;
use std::io::Read;
use std::process;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

struct MonotonicState {
    last_ms: u64,
    rand_hi: u16, // top 16 bits of 80-bit randomness
    rand_lo: u64, // bottom 64 bits of 80-bit randomness
}

static STATE: Mutex<MonotonicState> = Mutex::new(MonotonicState {
    last_ms: 0,
    rand_hi: 0,
    rand_lo: 0,
});

fn os_random_80(now_ns: u128) -> (u16, u64) {
    let mut buf = [0u8; 10];
    if let Ok(mut f) = File::open("/dev/urandom") {
        if f.read_exact(&mut buf).is_ok() {
            let hi = u16::from_be_bytes([buf[0], buf[1]]);
            let lo = u64::from_be_bytes([
                buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8], buf[9],
            ]);
            return (hi, lo);
        }
    }
    let seed = (now_ns as u64)
        .wrapping_mul(6364136223846793005)
        .wrapping_add(u64::from(process::id()) << 32);
    let hi = ((seed >> 48) as u16) ^ (process::id() as u16);
    (hi, seed)
}

/// Generate a fresh 26-character monotonic ULID string.
pub fn generate_ulid() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let now_ms = (dur.as_millis() as u64) & 0x0000_FFFF_FFFF_FFFF;
    let now_ns = dur.as_nanos();

    let (hi, lo) = {
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if now_ms == st.last_ms && (st.rand_hi != 0 || st.rand_lo != 0) {
            let (new_lo, carry) = st.rand_lo.overflowing_add(1);
            st.rand_lo = new_lo;
            if carry {
                st.rand_hi = st.rand_hi.wrapping_add(1);
            }
        } else {
            let (r_hi, r_lo) = os_random_80(now_ns);
            st.last_ms = now_ms;
            st.rand_hi = r_hi;
            st.rand_lo = r_lo;
        }
        (st.rand_hi, st.rand_lo)
    };

    encode_ulid(now_ms, hi, lo)
}

pub fn encode_ulid(time_ms_48: u64, rand_hi_16: u16, rand_lo_64: u64) -> String {
    let mut val: u128 = ((time_ms_48 & 0x0000_FFFF_FFFF_FFFF) as u128) << 80;
    val |= (u128::from(rand_hi_16)) << 64;
    val |= u128::from(rand_lo_64);

    let mut out = [b'0'; 26];
    for i in (0..26).rev() {
        let idx = (val & 0x1F) as usize;
        out[i] = CROCKFORD[idx];
        val >>= 5;
    }
    String::from_utf8(out.to_vec()).expect("valid ascii ulid")
}

/// Validate that a string is a well-formed 26-char Crockford Base32 ULID.
pub fn is_valid_ulid(s: &str) -> bool {
    if s.len() != 26 {
        return false;
    }
    let bytes = s.as_bytes();
    // First char in a 128-bit ULID cannot exceed '7'
    if bytes[0] > b'7' {
        return false;
    }
    bytes.iter().all(|b| CROCKFORD.contains(b))
}
