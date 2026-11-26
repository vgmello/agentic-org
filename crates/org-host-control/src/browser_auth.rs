//! The browser-session identity every web-process call carries: the [`Digest32`] digests of
//! the browser's cookie and proof, the [`BrowserOrigin`] they are bound to, and the
//! [`BrowserAuth`] envelope of both. A leaf: the browser ops (`browser`), the organization ops
//! (`organization`) and the host ops (`ops`) build on it, re-exported at their old paths.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A 32-byte digest as 64 lowercase hex digits. Not secret: it is the stored form.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Digest32(pub [u8; 32]);
impl fmt::Debug for Digest32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Digest32(..)")
    }
}
impl Serialize for Digest32 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut text = String::with_capacity(64);
        for b in self.0 {
            text.push_str(&format!("{b:02x}"));
        }
        s.serialize_str(&text)
    }
}
impl<'de> Deserialize<'de> for Digest32 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        let bytes = text.as_bytes();
        let nibble = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        if bytes.len() != 64 {
            return Err(serde::de::Error::custom(
                "a digest is 64 lowercase hex digits",
            ));
        }
        let mut out = [0u8; 32];
        for (i, [high, low]) in bytes.as_chunks::<2>().0.iter().enumerate() {
            out[i] = match (nibble(*high), nibble(*low)) {
                (Some(h), Some(l)) => (h << 4) | l,
                _ => {
                    return Err(serde::de::Error::custom(
                        "a digest is 64 lowercase hex digits",
                    ))
                }
            };
        }
        Ok(Self(out))
    }
}

/// The browser origin a session is bound to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserOrigin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

/// The browser session a call acts for: the pair digests and the origin they are bound to.
/// When `local_authless` is set on a local plain-HTTP instance whose authentication is disabled,
/// the host authorizes the implicit local owner account and ignores the sentinel digests; the
/// host refuses `local_authless` whenever authentication is enabled or the listener is HTTPS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAuth {
    pub cookie_digest: Digest32,
    pub proof_digest: Digest32,
    pub origin: BrowserOrigin,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_authless: bool,
}

impl BrowserAuth {
    /// Constructs a local-authless browser auth envelope for an unauthenticated local HTTP HQ.
    pub fn local_authless(origin: BrowserOrigin) -> Self {
        Self {
            cookie_digest: Digest32([0u8; 32]),
            proof_digest: Digest32([0u8; 32]),
            origin,
            local_authless: true,
        }
    }
}
