//! Making app tokens (ADR-0003 §2.1), `std` only.
//!
//! The host makes a fresh token for each office every time it starts (and for an office it
//! creates) and keeps it only in memory, in that office's supervision: it reaches the office's
//! server in the first line of the control pipe, on every start within this host run, and never
//! a file, argv, the environment or a log. See `supervisor::child::StartLine`.

use std::fs::File;
use std::io::{self, Read};

use org_proto::AppToken;

/// A fresh app token: 32 bytes from `/dev/urandom`. The bytes read are zeroed afterwards (best
/// effort), whether or not the read succeeded; only the token keeps them.
pub(crate) fn generate() -> io::Result<AppToken> {
    let mut bytes = [0u8; 32];
    let read = File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes));
    let token = read.map(|()| AppToken::from_bytes(bytes));
    zero(&mut bytes);
    token
}

/// Overwrites with zeros in a way the optimizer can't remove (best effort, as `AppToken` does).
fn zero(bytes: &mut [u8]) {
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
        let (a, b) = (generate().unwrap(), generate().unwrap());
        assert!(a != b);
        assert!(a != AppToken::from_bytes([0; 32]));
    }

    #[test]
    fn zero_clears_every_byte() {
        let mut bytes = [0xa5u8; 32];
        zero(&mut bytes);
        assert_eq!(bytes, [0; 32]);
    }
}
