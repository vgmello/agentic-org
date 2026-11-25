//! The fixed-size secrets of the contract: [`OfficeSecretsKey`], an office's secrets key, and the
//! grant secrets of X-02 §4.1 ([`GrantMac`] and [`GrantKey`]). They share one implementation of
//! their guards.
//!
//! On the wire each is 64 lowercase hex digits. The types keep a secret from leaking by accident:
//! - `Debug` prints `<redacted>`, and there is no `Display`.
//! - Decode errors never echo the input.
//! - Equality is constant-time.
//! - The bytes are zeroed when it's dropped. None is `Clone`: share it by reference or `Arc`.
//!
//! Compare secrets with `==`, never through their strings: every one prints the same.
//!
//! They decode from JSON only (they use `deserialize_any`, which needs a self-describing format).
//!
//! [`OfficeSecretsKey`] travels only on the launch pipe from the host to the office's server
//! (`org-office-launch`, which re-exports it), never on a socket.
//!
//! Zeroing is defence in depth, not a guarantee that no copy of a secret is left in memory. The
//! typed envelopes ([`Request`](crate::Request), [`Response`](crate::Response) and
//! [`ServerMessage`](crate::ServerMessage)) carry a secret's hex through `serde_json::Value` and
//! frame buffers, and leave freed copies of it behind that nothing can zero. Reading those copies
//! needs local memory access as the same user.
//!
//! Other copies outside these types survive zeroing too: the array passed to `from_bytes` (zero
//! it yourself), the raw line a secret was read from, the encoded output of a serialization and
//! any reader or writer buffer it passed through, serde_json's scratch buffer (escaped strings,
//! `from_reader`), serde's buffering for `flatten` and tagged or untagged enums, and any other
//! `serde_json::Value` hop. The bytes themselves live on the heap (a `Box`), and decoding fills
//! them in place, so moving a secret moves only a pointer and leaves no copy of the bytes behind.
//! To keep copies few, decode it with `serde_json::from_slice` from a line buffer you zero
//! afterwards, into a plain struct field.
//! Never log or `Debug`-print anything that holds it in raw form (an envelope, a `Value`, a line),
//! and never use it as a map key or an enum tag (serde echoes those in errors).

use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// The length in bytes of every secret here.
const SECRET_BYTES: usize = 32;

/// An office secrets key's length in bytes (64 hex digits on the wire).
pub const OFFICE_SECRETS_KEY_BYTES: usize = SECRET_BYTES;

/// An office's secrets key: 32 random bytes that wrap the office's private keys
/// (`org_pki::wrap`). It never encrypts a database (ADR-0016).
///
/// The host creates each office's key from a secure random source, stores it in its own key
/// store, and sends it to the office's server on the launch pipe (`org-office-launch`) on every
/// create and start (a discard carries none). The server never creates, stores or deletes one.
/// It never reaches argv, the environment, a log or the office's folder.
///
/// Its guards:
/// - On the wire it is 64 lowercase hex digits; decode errors never echo the input, and it
///   decodes from JSON only.
/// - `Debug` prints `OfficeSecretsKey(<redacted>)`. There is no `Display`.
/// - Equality is constant-time.
/// - The bytes are zeroed when it's dropped. Share it by reference.
///
/// Zeroing is defence in depth: it covers this key's own bytes only. The copies listed in the
/// module docs (the array passed to [`OfficeSecretsKey::from_bytes`], line, reader and
/// writer buffers, serde's buffering) survive it, and so does any copy made from
/// [`OfficeSecretsKey::expose_bytes`]. Serializing it zeroes its own hex buffer.
///
/// It isn't `Clone`:
///
/// ```compile_fail,E0599
/// let key = org_proto::OfficeSecretsKey::from_bytes([0; org_proto::OFFICE_SECRETS_KEY_BYTES]);
/// let _copy = key.clone();
/// ```
pub struct OfficeSecretsKey(Box<[u8; OFFICE_SECRETS_KEY_BYTES]>);

impl OfficeSecretsKey {
    /// Wrap 32 random bytes. The caller makes them with a secure random source and zeroes its
    /// own array afterwards: this only zeroes its own copy. This crate does no I/O.
    pub fn from_bytes(bytes: [u8; OFFICE_SECRETS_KEY_BYTES]) -> Self {
        OfficeSecretsKey(Box::new(bytes))
    }

    /// The raw key, only for storing it in the host's key store and for wrapping or unwrapping
    /// an office's private keys (`org_pki::wrap`), nothing else. Don't copy the bytes beyond
    /// their immediate use: borrow them for the one call that needs them.
    /// `*key.expose_bytes()` copies the array, and any copy is the caller's to zero. Never log
    /// or format it.
    pub fn expose_bytes(&self) -> &[u8; OFFICE_SECRETS_KEY_BYTES] {
        &self.0
    }
}

impl PartialEq for OfficeSecretsKey {
    /// Constant-time: how long a comparison takes doesn't depend on where the keys differ.
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(&self.0, &other.0)
    }
}

impl Eq for OfficeSecretsKey {}

impl OfficeSecretsKey {
    /// Zero the bytes in place (what `Drop` does; tests call it to check the bytes).
    fn wipe(&mut self) {
        #[cfg(test)]
        tests::WIPES.with(|wipes| wipes.set(wipes.get() + 1));
        zero(&mut *self.0);
    }
}

impl Drop for OfficeSecretsKey {
    fn drop(&mut self) {
        self.wipe();
    }
}

impl fmt::Debug for OfficeSecretsKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OfficeSecretsKey(<redacted>)")
    }
}

impl Serialize for OfficeSecretsKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serialize_hex(&self.0, s)
    }
}

impl<'de> Deserialize<'de> for OfficeSecretsKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // `deserialize_any`, so a value of another type reaches the visitor and fails with
        // BAD_SECRETS_KEY. `deserialize_str` would let serde_json build its own "invalid type"
        // error, which prints a number it parsed (e.g. an unquoted key's leading digits). This
        // needs a self-describing format, which JSON is.
        d.deserialize_any(SecretVisitor::<OfficeSecretsKey>(PhantomData))
    }
}

/// The one decode error. It never contains the input.
const BAD_SECRETS_KEY: &str = "an office secrets key must be 64 lowercase hex digits";

impl Secret for OfficeSecretsKey {
    const EXPECTING: &'static str = "an office secrets key";
    const BAD: &'static str = BAD_SECRETS_KEY;

    fn zeroed() -> Self {
        OfficeSecretsKey(Box::new([0u8; SECRET_BYTES]))
    }
    fn bytes_mut(&mut self) -> &mut [u8; SECRET_BYTES] {
        &mut self.0
    }
}

/// A grant's MAC length in bytes: an HMAC-SHA256 tag (64 hex digits on the wire).
pub const GRANT_MAC_BYTES: usize = SECRET_BYTES;

/// A per-worker grant key's length in bytes (64 hex digits on the wire).
pub const GRANT_KEY_BYTES: usize = SECRET_BYTES;

/// A grant's MAC: `HMAC-SHA256(grant_key, grant_mac_input(claims))` (X-02 §4.1; see
/// [`crate::grant_mac_input`]). The host computes it and the worker checks it; this crate does
/// no cryptography.
///
/// On the wire it is exactly 64 lowercase hex digits; anything else is refused, and decode
/// errors never echo the input. It has the guards of [`OfficeSecretsKey`]: `Debug` prints
/// `GrantMac(<redacted>)`, there is no `Display`, and equality is constant-time. Worker verification
/// uses the HMAC implementation's constant-time tag verifier. The bytes are zeroed on drop,
/// and it isn't `Clone`:
///
/// ```compile_fail,E0599
/// let mac = org_proto::GrantMac::from_bytes([0; org_proto::GRANT_MAC_BYTES]);
/// let _copy = mac.clone();
/// ```
pub struct GrantMac(Box<[u8; GRANT_MAC_BYTES]>);

impl GrantMac {
    /// Wrap a computed tag. The caller zeroes its own array afterwards.
    pub fn from_bytes(bytes: [u8; GRANT_MAC_BYTES]) -> Self {
        GrantMac(Box::new(bytes))
    }

    /// Borrow the tag for verification only. Never log it; zero any copy you make.
    pub fn expose_bytes(&self) -> &[u8; GRANT_MAC_BYTES] {
        &self.0
    }
}

/// The key a worker checks grants with (X-02 §4.1). The host makes a new one from a secure
/// random source at every worker start and sends it over the rendezvous connection, never on the
/// launch pipe; a new key invalidates every older grant.
///
/// It has the guards of [`OfficeSecretsKey`]: 64 lowercase hex digits on the wire, decode errors that never
/// echo the input, `Debug` printing `GrantKey(<redacted>)`, no `Display`, constant-time equality,
/// zeroed on drop, and no `Clone`:
///
/// ```compile_fail,E0599
/// let key = org_proto::GrantKey::from_bytes([0; org_proto::GRANT_KEY_BYTES]);
/// let _copy = key.clone();
/// ```
pub struct GrantKey(Box<[u8; GRANT_KEY_BYTES]>);

impl GrantKey {
    /// Wrap 32 random bytes. The caller makes them with a secure random source and zeroes its
    /// own array afterwards. This crate does no I/O.
    pub fn from_bytes(bytes: [u8; GRANT_KEY_BYTES]) -> Self {
        GrantKey(Box::new(bytes))
    }

    /// The raw key, only to key the HMAC. Borrow it for that one call; any copy is the caller's
    /// to zero. Never log or format it.
    pub fn expose_bytes(&self) -> &[u8; GRANT_KEY_BYTES] {
        &self.0
    }
}

/// The guards [`GrantMac`] and [`GrantKey`] share with [`OfficeSecretsKey`]: constant-time equality,
/// zeroing on drop, a redacted `Debug`, hex encoding and the one decoding visitor.
macro_rules! hex_secret {
    ($ty:ident, $expecting:literal, $bad:literal) => {
        impl PartialEq for $ty {
            /// Constant-time: how long a comparison takes doesn't depend on where they differ.
            fn eq(&self, other: &Self) -> bool {
                constant_time_eq(&self.0, &other.0)
            }
        }

        impl Eq for $ty {}

        impl Drop for $ty {
            fn drop(&mut self) {
                #[cfg(test)]
                tests::WIPES.with(|wipes| wipes.set(wipes.get() + 1));
                zero(&mut *self.0);
            }
        }

        impl fmt::Debug for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($ty), "(<redacted>)"))
            }
        }

        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                serialize_hex(&self.0, s)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                // `deserialize_any`, for the reason given on `OfficeSecretsKey`'s decoder.
                d.deserialize_any(SecretVisitor::<$ty>(PhantomData))
            }
        }

        impl Secret for $ty {
            const EXPECTING: &'static str = $expecting;
            const BAD: &'static str = $bad;

            fn zeroed() -> Self {
                $ty(Box::new([0u8; SECRET_BYTES]))
            }
            fn bytes_mut(&mut self) -> &mut [u8; SECRET_BYTES] {
                &mut self.0
            }
        }
    };
}

hex_secret!(
    GrantMac,
    "a grant mac",
    "a grant mac must be 64 lowercase hex digits"
);
hex_secret!(
    GrantKey,
    "a grant key",
    "a grant key must be 64 lowercase hex digits"
);

/// What every fixed-size secret here ([`OfficeSecretsKey`], [`GrantMac`], [`GrantKey`]) shares,
/// so they decode through one visitor. Each type zeroes itself on drop.
trait Secret: Sized {
    /// What the visitor expects, for serde's messages.
    const EXPECTING: &'static str;
    /// The one decode error. It never contains the input.
    const BAD: &'static str;
    /// All zeros, to decode into in place.
    fn zeroed() -> Self;
    fn bytes_mut(&mut self) -> &mut [u8; SECRET_BYTES];
}

/// Constant-time equality of two secrets.
fn constant_time_eq(a: &[u8; SECRET_BYTES], b: &[u8; SECRET_BYTES]) -> bool {
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    // Keep the compiler from short-circuiting the loop.
    std::hint::black_box(diff) == 0
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// A secret as 64 lowercase hex digits, through a stack buffer that is zeroed afterwards.
fn serialize_hex<S: Serializer>(bytes: &[u8; SECRET_BYTES], s: S) -> Result<S::Ok, S::Error> {
    let mut buf = [0u8; SECRET_BYTES * 2];
    for (i, byte) in bytes.iter().enumerate() {
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

struct SecretVisitor<T>(PhantomData<T>);

impl<T: Secret> Visitor<'_> for SecretVisitor<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(T::EXPECTING)
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<T, E> {
        let text = text.as_bytes();
        if text.len() != SECRET_BYTES * 2 {
            return Err(E::custom(T::BAD));
        }
        // Built in place, so an early return zeroes the partial bytes through `Drop`.
        let mut secret = T::zeroed();
        for (i, pair) in text.as_chunks::<2>().0.iter().enumerate() {
            match (nibble(pair[0]), nibble(pair[1])) {
                (Some(hi), Some(lo)) => secret.bytes_mut()[i] = (hi << 4) | lo,
                _ => return Err(E::custom(T::BAD)),
            }
        }
        Ok(secret)
    }

    // Other scalars, 128-bit integers included (with serde_json's `arbitrary_precision` feature, a
    // huge number read from a `Value` arrives there; read from text it arrives as a map, which
    // fails as an invalid type without its digits): refuse without naming the value. Null, arrays
    // and objects fall to serde's default "invalid type" error, which names only the type. One
    // path stays out of reach: with `arbitrary_precision`, a `Value` holding a huge number decoded
    // into an `#[serde(untagged)]` enum is echoed by serde's own visitor, before this one runs.
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<T, E> {
        Err(E::custom(T::BAD))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<T, E> {
        Err(E::custom(T::BAD))
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<T, E> {
        Err(E::custom(T::BAD))
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<T, E> {
        Err(E::custom(T::BAD))
    }
    fn visit_i128<E: de::Error>(self, _: i128) -> Result<T, E> {
        Err(E::custom(T::BAD))
    }
    fn visit_u128<E: de::Error>(self, _: u128) -> Result<T, E> {
        Err(E::custom(T::BAD))
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// Overwrite with zeros in a way the optimizer can't remove. Used for [`OfficeSecretsKey`] and the grant secrets,
/// and re-exported as [`crate::zero_bytes`] for other crates' secret buffers.
// The crate denies unsafe code. This is the one exception: a plain write of zeros to bytes
// that are about to be dropped is a dead store the optimizer may remove, and safe Rust has no
// volatile write. `write_volatile` through an exclusive reference is sound.
#[expect(
    unsafe_code,
    reason = "best-effort secret zeroing requires ptr::write_volatile so the optimizer cannot elide dead stores"
)]
pub fn zero(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        // SAFETY: `b` is a valid, aligned, exclusive reference to a u8.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// How many times `wipe` ran on this thread (tests run in parallel, one per thread).
        pub(super) static WIPES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// L3c: dropping a key runs `wipe`, once.
    #[test]
    fn dropping_a_key_wipes_it() {
        let wipes = || WIPES.with(std::cell::Cell::get);
        let before = wipes();
        drop(OfficeSecretsKey::from_bytes(
            [0x5a; OFFICE_SECRETS_KEY_BYTES],
        ));
        assert_eq!(wipes(), before + 1);
        // Moved, not copied: one wipe, when the one value is dropped.
        let key = OfficeSecretsKey::from_bytes([1; OFFICE_SECRETS_KEY_BYTES]);
        let moved = key;
        assert_eq!(wipes(), before + 1);
        drop(moved);
        assert_eq!(wipes(), before + 2);
    }

    /// The grant secrets wipe like the others, once each, and moving them copies nothing.
    #[test]
    fn dropping_a_grant_mac_or_key_wipes_it() {
        let wipes = || WIPES.with(std::cell::Cell::get);
        let before = wipes();
        drop(GrantMac::from_bytes([0xa5; GRANT_MAC_BYTES]));
        assert_eq!(wipes(), before + 1);
        let key = GrantKey::from_bytes([0x5a; GRANT_KEY_BYTES]);
        let moved = key;
        assert_eq!(wipes(), before + 1);
        drop(moved);
        assert_eq!(wipes(), before + 2);
        // A decode that fails part-way wipes the bytes it built.
        assert!(serde_json::from_str::<GrantKey>(&format!("\"{}zz\"", "00".repeat(31))).is_err());
        assert_eq!(wipes(), before + 3);
    }

    #[test]
    fn decode_errors_inside_a_struct_never_echo_it() {
        #[derive(Debug, Deserialize)]
        #[expect(
            dead_code,
            reason = "field exists solely to exercise OfficeSecretsKey deserialization error formatting"
        )]
        struct Start {
            office_secrets_key: OfficeSecretsKey,
        }
        let bad = format!("{}Z", &key_hex(&office_secrets_key())[..63]);
        let err = serde_json::from_str::<Start>(&format!("{{\"office_secrets_key\":\"{bad}\"}}"))
            .unwrap_err()
            .to_string();
        assert!(!err.contains(&bad[..20]), "echoed: {err}");
    }

    #[test]
    fn decodes_in_every_shape_without_echoing() {
        use std::collections::HashMap;

        let t = office_secrets_key();
        let hex = key_hex(&t);
        // Escaped digits decode the same.
        let escaped = format!("\"\\u00{:x}{}\"", hex.as_bytes()[0], &hex[1..]);
        assert_eq!(
            serde_json::from_str::<OfficeSecretsKey>(&escaped).unwrap(),
            t
        );
        assert_eq!(
            serde_json::from_value::<OfficeSecretsKey>(serde_json::Value::String(hex.clone()))
                .unwrap(),
            t
        );
        assert_eq!(
            serde_json::from_str::<Option<OfficeSecretsKey>>("null").unwrap(),
            None
        );

        #[derive(Debug, Deserialize)]
        struct Inner {
            office_secrets_key: OfficeSecretsKey,
        }
        #[derive(Debug, Deserialize)]
        struct Outer {
            #[serde(flatten)]
            inner: Inner,
        }
        #[derive(Debug, Deserialize)]
        #[serde(tag = "op")]
        enum Tagged {
            Start {
                office_secrets_key: OfficeSecretsKey,
            },
        }
        #[derive(Debug, Deserialize)]
        #[serde(untagged)]
        enum Untagged {
            Start {
                office_secrets_key: OfficeSecretsKey,
            },
        }

        let good = format!("{{\"op\":\"Start\",\"office_secrets_key\":\"{hex}\"}}");
        assert_eq!(
            serde_json::from_str::<Outer>(&good)
                .unwrap()
                .inner
                .office_secrets_key,
            t
        );
        let Tagged::Start { office_secrets_key } = serde_json::from_str::<Tagged>(&good).unwrap();
        assert_eq!(office_secrets_key, t);
        let Untagged::Start { office_secrets_key } =
            serde_json::from_str::<Untagged>(&good).unwrap();
        assert_eq!(office_secrets_key, t);
        let map: HashMap<String, OfficeSecretsKey> =
            serde_json::from_str(&format!("{{\"a\":\"{hex}\"}}")).unwrap();
        assert_eq!(map["a"], t);

        let bad = format!("{}G", &hex[..63]);
        let bad_line = format!("{{\"op\":\"Start\",\"office_secrets_key\":\"{bad}\"}}");
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
        #[expect(
            dead_code,
            reason = "field exists solely to exercise OfficeSecretsKey deserialization error formatting"
        )]
        struct Start {
            office_secrets_key: OfficeSecretsKey,
        }
        for digits in [
            "31415926535897932384",
            "1234567890123456",
            "27182818",
            "-27182818",
        ] {
            let err =
                serde_json::from_str::<Start>(&format!("{{\"office_secrets_key\":{digits}}}"))
                    .unwrap_err()
                    .to_string();
            assert!(!err.contains(&digits[..6]), "echoed: {err}");
            let value: serde_json::Value = serde_json::from_str(digits).unwrap();
            let err = serde_json::from_value::<OfficeSecretsKey>(value.clone())
                .unwrap_err()
                .to_string();
            assert!(!err.contains(&digits[..6]), "echoed: {err}");
            let err = OfficeSecretsKey::deserialize(&value)
                .unwrap_err()
                .to_string();
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
            let err = serde_json::from_str::<OfficeSecretsKey>(scalar)
                .unwrap_err()
                .to_string();
            assert!(err.contains(BAD_SECRETS_KEY), "{scalar}: {err}");
            let value: serde_json::Value = serde_json::from_str(scalar).unwrap();
            let err = OfficeSecretsKey::deserialize(&value)
                .unwrap_err()
                .to_string();
            assert!(err.contains(BAD_SECRETS_KEY), "{scalar}: {err}");
        }
        // 128-bit integers (what serde_json's `arbitrary_precision` produces for huge numbers).
        use serde::de::value::{Error as ValueError, I128Deserializer, U128Deserializer};
        let err = OfficeSecretsKey::deserialize(U128Deserializer::<ValueError>::new(
            31415926535897932384u128,
        ))
        .unwrap_err()
        .to_string();
        assert!(
            err.contains(BAD_SECRETS_KEY) && !err.contains("314159"),
            "{err}"
        );
        let err = OfficeSecretsKey::deserialize(I128Deserializer::<ValueError>::new(
            -31415926535897932384i128,
        ))
        .unwrap_err()
        .to_string();
        assert!(
            err.contains(BAD_SECRETS_KEY) && !err.contains("314159"),
            "{err}"
        );
    }

    fn office_secrets_key() -> OfficeSecretsKey {
        let mut bytes = [0u8; OFFICE_SECRETS_KEY_BYTES];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(53).wrapping_add(0x3c);
        }
        OfficeSecretsKey::from_bytes(bytes)
    }

    fn key_hex(k: &OfficeSecretsKey) -> String {
        k.expose_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn an_office_secrets_key_is_64_lowercase_hex_and_round_trips() {
        let k = office_secrets_key();
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(json, format!("\"{}\"", key_hex(&k)));
        assert_eq!(json.len(), 66);
        assert!(json[1..65]
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
        let back = serde_json::from_str::<OfficeSecretsKey>(&json).unwrap();
        assert_eq!(back, k);
        assert_eq!(back.expose_bytes(), k.expose_bytes());
        assert_eq!(
            serde_json::from_str::<Option<OfficeSecretsKey>>("null").unwrap(),
            None
        );
    }

    #[test]
    fn an_office_secrets_key_of_the_wrong_length_or_not_hex_is_refused_without_echoing() {
        let secret = key_hex(&office_secrets_key());
        let cases = [
            secret.to_uppercase(),         // uppercase isn't accepted
            format!("{}g", &secret[..63]), // a non-hex digit
            secret[..62].to_string(),      // too short
            format!("{secret}00"),         // too long
            format!("{}é", &secret[..62]), // non-ASCII
        ];
        for bad in cases {
            let err =
                serde_json::from_str::<OfficeSecretsKey>(&serde_json::to_string(&bad).unwrap())
                    .unwrap_err()
                    .to_string();
            assert!(!err.contains(&bad[..20]), "echoed: {err}");
            assert!(err.contains(BAD_SECRETS_KEY), "{err}");
        }
        for other in ["5", "-5", "1.5", "true", "31415926535897932384"] {
            let err = serde_json::from_str::<OfficeSecretsKey>(other)
                .unwrap_err()
                .to_string();
            assert!(err.contains(BAD_SECRETS_KEY), "{other}: {err}");
        }
        for other in ["null", "[1]", "{\"a\":1}", "\"\""] {
            assert!(
                serde_json::from_str::<OfficeSecretsKey>(other).is_err(),
                "{other}"
            );
        }
    }

    #[test]
    fn an_office_secrets_key_is_redacted_in_debug() {
        let k = office_secrets_key();
        let secret = key_hex(&k);
        for shown in [
            format!("{k:?}"),
            format!("{:?}", Some(&k)),
            format!("{k:#?}"),
        ] {
            assert!(!shown.contains(&secret[..8]), "{shown}");
            assert!(shown.contains("OfficeSecretsKey(<redacted>)"), "{shown}");
        }
    }

    #[test]
    fn office_secrets_key_equality_compares_every_byte() {
        let a = office_secrets_key();
        assert_eq!(a, office_secrets_key());
        for i in [0, 15, 31] {
            let mut bytes = *a.expose_bytes();
            bytes[i] ^= 1;
            assert_ne!(a, OfficeSecretsKey::from_bytes(bytes), "byte {i}");
        }
    }

    /// `Drop` calls `wipe`.
    #[test]
    fn drop_zeroes_the_office_secrets_key() {
        let mut k = OfficeSecretsKey::from_bytes([0x5a; OFFICE_SECRETS_KEY_BYTES]);
        k.wipe();
        assert_eq!(*k.0, [0u8; OFFICE_SECRETS_KEY_BYTES]);
    }

    #[test]
    fn zero_clears_bytes() {
        let mut bytes = [0xffu8; 8];
        zero(&mut bytes);
        assert_eq!(bytes, [0u8; 8]);
    }
}
