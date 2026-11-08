//! The app token: the per-office secret that lets the desktop app act for people (ADR-0003 §2.1).
//!
//! The host makes a fresh token for each office every time it starts and keeps it only in
//! memory. It passes it to the office's server over the startup pipe and gives it to the app over
//! `host.sock` after checking the app's code signature. It never reaches a file, argv, the
//! environment or a log.
//!
//! On the wire it is 64 lowercase hex digits. This type keeps it from leaking by accident:
//! - `Debug` and `Display` print `<redacted>`.
//! - Decode errors never echo the input.
//! - Equality is constant-time.
//! - The bytes are zeroed when it's dropped. It isn't `Clone`: share it by reference or `Arc`.
//!
//! Compare tokens with `==`, never through their strings: every token displays the same.
//!
//! It decodes from JSON only (it uses `deserialize_any`, which needs a self-describing format).
//!
//! Zeroing is defence in depth, not a guarantee that no copy of the token is left in memory. The
//! typed envelopes ([`Request`](crate::Request), [`Response`](crate::Response) and
//! [`ServerMessage`](crate::ServerMessage)) carry the token's hex through `serde_json::Value` and
//! frame buffers, and leave freed copies of it behind that nothing can zero. Reading those copies
//! needs local memory access as the same user.
//!
//! Other copies outside this type survive zeroing too: the array passed to
//! [`AppToken::from_bytes`] (zero it yourself), the raw line a token was read from, the encoded
//! output of a serialization and any reader or writer buffer it passed through, serde_json's
//! scratch buffer (escaped strings, `from_reader`), serde's buffering for `flatten` and tagged or
//! untagged enums, any other `serde_json::Value` hop, and stack slots left behind by moves. To
//! keep copies few, decode it with `serde_json::from_slice` from a line buffer you zero
//! afterwards, into a plain struct field.
//! Never log or `Debug`-print anything that holds it in raw form (an envelope, a `Value`, a line),
//! and never use it as a map key or an enum tag (serde echoes those in errors).

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// An app token's length in bytes (64 hex digits on the wire).
pub const APP_TOKEN_BYTES: usize = 32;

/// A per-office app token. See the module docs.
///
/// Zeroing on drop is defence in depth: it covers this token's own bytes only. The copies the
/// typed envelopes make of its hex (through `serde_json::Value` and frame buffers) can't be
/// zeroed. Reading them needs local memory access as the same user.
///
/// It isn't `Clone`:
///
/// ```compile_fail,E0599
/// let token = org_proto::AppToken::from_bytes([0; org_proto::APP_TOKEN_BYTES]);
/// let _copy = token.clone();
/// ```
pub struct AppToken([u8; APP_TOKEN_BYTES]);

impl AppToken {
    /// Wrap 32 random bytes. The caller makes them with a secure random source (e.g.
    /// `/dev/urandom`) and zeroes its own array afterwards: this only zeroes its own copy. This
    /// crate does no I/O.
    pub fn from_bytes(bytes: [u8; APP_TOKEN_BYTES]) -> Self {
        AppToken(bytes)
    }

    #[cfg(test)]
    fn expose_bytes(&self) -> &[u8; APP_TOKEN_BYTES] {
        &self.0
    }
}

impl PartialEq for AppToken {
    /// Constant-time: how long a comparison takes doesn't depend on where the tokens differ.
    fn eq(&self, other: &Self) -> bool {
        let mut diff = 0u8;
        for (a, b) in self.0.iter().zip(other.0.iter()) {
            diff |= a ^ b;
        }
        // Keep the compiler from short-circuiting the loop.
        std::hint::black_box(diff) == 0
    }
}

impl Eq for AppToken {}

impl Drop for AppToken {
    fn drop(&mut self) {
        zero(&mut self.0);
    }
}

impl fmt::Debug for AppToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AppToken(<redacted>)")
    }
}

impl fmt::Display for AppToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

const HEX: &[u8; 16] = b"0123456789abcdef";

impl Serialize for AppToken {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut buf = [0u8; APP_TOKEN_BYTES * 2];
        for (i, byte) in self.0.iter().enumerate() {
            buf[2 * i] = HEX[(byte >> 4) as usize];
            buf[2 * i + 1] = HEX[(byte & 0x0f) as usize];
        }
        let result = match std::str::from_utf8(&buf) {
            Ok(text) => s.serialize_str(text),
            Err(_) => unreachable!("hex digits are ASCII"),
        };
        zero(&mut buf);
        result
    }
}

impl<'de> Deserialize<'de> for AppToken {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // `deserialize_any`, so a value of another type reaches the visitor and fails with
        // BAD_TOKEN. `deserialize_str` would let serde_json build its own "invalid type" error,
        // which prints a number it parsed (e.g. an unquoted token's leading digits). This needs
        // a self-describing format, which JSON is.
        d.deserialize_any(TokenVisitor)
    }
}

struct TokenVisitor;

/// The one decode error. It never contains the input.
const BAD_TOKEN: &str = "an app token must be 64 lowercase hex digits";

impl Visitor<'_> for TokenVisitor {
    type Value = AppToken;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an app token")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<AppToken, E> {
        let text = text.as_bytes();
        if text.len() != APP_TOKEN_BYTES * 2 {
            return Err(E::custom(BAD_TOKEN));
        }
        // Built in place, so an early return zeroes the partial bytes through `Drop`.
        let mut token = AppToken([0u8; APP_TOKEN_BYTES]);
        for (i, pair) in text.as_chunks::<2>().0.iter().enumerate() {
            match (nibble(pair[0]), nibble(pair[1])) {
                (Some(hi), Some(lo)) => token.0[i] = (hi << 4) | lo,
                _ => return Err(E::custom(BAD_TOKEN)),
            }
        }
        Ok(token)
    }

    // Other scalars, 128-bit integers included (with serde_json's `arbitrary_precision` feature, a
    // huge number read from a `Value` arrives there; read from text it arrives as a map, which
    // fails as an invalid type without its digits): refuse without naming the value. Null, arrays
    // and objects fall to serde's default "invalid type" error, which names only the type. One
    // path stays out of reach: with `arbitrary_precision`, a `Value` holding a huge number decoded
    // into an `#[serde(untagged)]` enum is echoed by serde's own visitor, before this one runs.
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<AppToken, E> {
        Err(E::custom(BAD_TOKEN))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<AppToken, E> {
        Err(E::custom(BAD_TOKEN))
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<AppToken, E> {
        Err(E::custom(BAD_TOKEN))
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<AppToken, E> {
        Err(E::custom(BAD_TOKEN))
    }
    fn visit_i128<E: de::Error>(self, _: i128) -> Result<AppToken, E> {
        Err(E::custom(BAD_TOKEN))
    }
    fn visit_u128<E: de::Error>(self, _: u128) -> Result<AppToken, E> {
        Err(E::custom(BAD_TOKEN))
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// Overwrite with zeros in a way the optimizer can't remove.
// The crate denies unsafe code. This is the one exception: a plain write of zeros to bytes
// that are about to be dropped is a dead store the optimizer may remove, and safe Rust has no
// volatile write. `write_volatile` through an exclusive reference is sound.
#[allow(unsafe_code)]
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

    fn token() -> AppToken {
        let mut bytes = [0u8; APP_TOKEN_BYTES];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37).wrapping_add(0xa5);
        }
        AppToken::from_bytes(bytes)
    }

    fn hex_of(t: &AppToken) -> String {
        t.expose_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn wire_form_is_64_lowercase_hex_and_round_trips() {
        let t = token();
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, format!("\"{}\"", hex_of(&t)));
        assert_eq!(json.len(), 66);
        assert!(json[1..65]
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
        assert_eq!(serde_json::from_str::<AppToken>(&json).unwrap(), t);
    }

    #[test]
    fn debug_and_display_are_redacted() {
        let t = token();
        let secret = hex_of(&t);
        for shown in [format!("{t:?}"), format!("{t}"), format!("{:?}", Some(&t))] {
            assert!(!shown.contains(&secret[..8]), "{shown}");
            assert!(shown.contains("redacted"), "{shown}");
        }
    }

    #[test]
    fn decode_errors_never_echo_the_input() {
        let secret = hex_of(&token());
        let cases = [
            secret.to_uppercase(),         // uppercase isn't accepted
            format!("{}g", &secret[..63]), // a non-hex digit
            secret[..62].to_string(),      // too short
            format!("{secret}00"),         // too long
            format!("{}é", &secret[..62]), // non-ASCII
        ];
        for bad in cases {
            let err = serde_json::from_str::<AppToken>(&serde_json::to_string(&bad).unwrap())
                .unwrap_err()
                .to_string();
            assert!(!err.contains(&bad[..20]), "echoed: {err}");
            assert!(err.contains("64 lowercase hex digits"), "{err}");
        }
        for other in ["5", "-5", "1.5", "true", "null", "[1]", "{\"a\":1}"] {
            assert!(serde_json::from_str::<AppToken>(other).is_err(), "{other}");
        }
    }

    #[test]
    fn decode_errors_inside_a_struct_never_echo_it() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Start {
            app_token: AppToken,
        }
        let bad = format!("{}Z", &hex_of(&token())[..63]);
        let err = serde_json::from_str::<Start>(&format!("{{\"app_token\":\"{bad}\"}}"))
            .unwrap_err()
            .to_string();
        assert!(!err.contains(&bad[..20]), "echoed: {err}");
    }

    #[test]
    fn equality_compares_every_byte() {
        let a = token();
        assert_eq!(a, token());
        for i in [0, 15, 31] {
            let mut bytes = *a.expose_bytes();
            bytes[i] ^= 1;
            assert_ne!(a, AppToken::from_bytes(bytes), "byte {i}");
        }
    }

    #[test]
    fn decodes_in_every_shape_without_echoing() {
        use std::collections::HashMap;

        let t = token();
        let hex = hex_of(&t);
        // Escaped digits decode the same.
        let escaped = format!("\"\\u00{:x}{}\"", hex.as_bytes()[0], &hex[1..]);
        assert_eq!(serde_json::from_str::<AppToken>(&escaped).unwrap(), t);
        assert_eq!(
            serde_json::from_value::<AppToken>(serde_json::Value::String(hex.clone())).unwrap(),
            t
        );
        assert_eq!(
            serde_json::from_str::<Option<AppToken>>("null").unwrap(),
            None
        );

        #[derive(Debug, Deserialize)]
        struct Inner {
            app_token: AppToken,
        }
        #[derive(Debug, Deserialize)]
        struct Outer {
            #[serde(flatten)]
            inner: Inner,
        }
        #[derive(Debug, Deserialize)]
        #[serde(tag = "op")]
        enum Tagged {
            Start { app_token: AppToken },
        }
        #[derive(Debug, Deserialize)]
        #[serde(untagged)]
        enum Untagged {
            Start { app_token: AppToken },
        }

        let good = format!("{{\"op\":\"Start\",\"app_token\":\"{hex}\"}}");
        assert_eq!(
            serde_json::from_str::<Outer>(&good)
                .unwrap()
                .inner
                .app_token,
            t
        );
        let Tagged::Start { app_token } = serde_json::from_str::<Tagged>(&good).unwrap();
        assert_eq!(app_token, t);
        let Untagged::Start { app_token } = serde_json::from_str::<Untagged>(&good).unwrap();
        assert_eq!(app_token, t);
        let map: HashMap<String, AppToken> =
            serde_json::from_str(&format!("{{\"a\":\"{hex}\"}}")).unwrap();
        assert_eq!(map["a"], t);

        let bad = format!("{}G", &hex[..63]);
        let bad_line = format!("{{\"op\":\"Start\",\"app_token\":\"{bad}\"}}");
        let errors = [
            serde_json::from_str::<Outer>(&bad_line)
                .unwrap_err()
                .to_string(),
            serde_json::from_str::<Tagged>(&bad_line)
                .unwrap_err()
                .to_string(),
            serde_json::from_str::<Untagged>(&bad_line)
                .unwrap_err()
                .to_string(),
            serde_json::from_reader::<_, Inner>(bad_line.as_bytes())
                .unwrap_err()
                .to_string(),
        ];
        for err in errors {
            assert!(!err.contains(&bad[..20]), "echoed: {err}");
        }
    }

    #[test]
    fn unquoted_numbers_are_never_echoed() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Start {
            app_token: AppToken,
        }
        for digits in [
            "31415926535897932384",
            "1234567890123456",
            "27182818",
            "-27182818",
        ] {
            let err = serde_json::from_str::<Start>(&format!("{{\"app_token\":{digits}}}"))
                .unwrap_err()
                .to_string();
            assert!(!err.contains(&digits[..6]), "echoed: {err}");
            let value: serde_json::Value = serde_json::from_str(digits).unwrap();
            let err = serde_json::from_value::<AppToken>(value.clone())
                .unwrap_err()
                .to_string();
            assert!(!err.contains(&digits[..6]), "echoed: {err}");
            let err = AppToken::deserialize(&value).unwrap_err().to_string();
            assert!(!err.contains(&digits[..6]), "echoed: {err}");
        }
        // Every scalar fails with this module's own message, never serde's default one (which
        // would name the value).
        for scalar in [
            "5",
            "-5",
            "1.5",
            "true",
            "2718281.8",
            "31415926535897932384",
        ] {
            let err = serde_json::from_str::<AppToken>(scalar)
                .unwrap_err()
                .to_string();
            assert!(err.contains(BAD_TOKEN), "{scalar}: {err}");
            let value: serde_json::Value = serde_json::from_str(scalar).unwrap();
            let err = AppToken::deserialize(&value).unwrap_err().to_string();
            assert!(err.contains(BAD_TOKEN), "{scalar}: {err}");
        }
        // 128-bit integers (what serde_json's `arbitrary_precision` produces for huge numbers).
        use serde::de::value::{Error as ValueError, I128Deserializer, U128Deserializer};
        let err = AppToken::deserialize(U128Deserializer::<ValueError>::new(
            31415926535897932384u128,
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains(BAD_TOKEN) && !err.contains("314159"), "{err}");
        let err = AppToken::deserialize(I128Deserializer::<ValueError>::new(
            -31415926535897932384i128,
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains(BAD_TOKEN) && !err.contains("314159"), "{err}");
    }

    // Test-only: reading the bytes after `Drop` has run needs `ManuallyDrop::drop`, which is
    // unsafe. The library's only unsafe code is still `zero`.
    #[test]
    #[allow(unsafe_code)]
    fn drop_zeroes_the_token() {
        let mut t = std::mem::ManuallyDrop::new(AppToken::from_bytes([0xa5; APP_TOKEN_BYTES]));
        // SAFETY: dropped once and never used as a token again; only its bytes are read.
        unsafe { std::mem::ManuallyDrop::drop(&mut t) };
        assert_eq!(t.0, [0u8; APP_TOKEN_BYTES]);
    }

    #[test]
    fn zero_clears_bytes() {
        let mut bytes = [0xffu8; 8];
        zero(&mut bytes);
        assert_eq!(bytes, [0u8; 8]);
    }
}
