//! Cookie and proof values, their digests and the `Set-Cookie` attributes.
//!
//! Each value carries 256 bits from the operating system. The cookie travels
//! base64url without padding; the proof is 64 lowercase hex digits. The host
//! receives `SHA-256(domain ‖ value)` only, with a distinct domain per value so
//! a cookie can never stand in for a proof.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hyper::header::{HeaderMap, HeaderValue, COOKIE};
use org_host_control::browser::Digest32;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read};
use zeroize::Zeroizing;

const COOKIE_DOMAIN: &[u8] = b"agentic-org/browser-session/cookie/v1\0";
const PROOF_DOMAIN: &[u8] = b"agentic-org/browser-session/proof/v1\0";
pub const PROOF_HEADER: &str = "x-org-session-proof";

fn digest(domain: &[u8], value: &[u8]) -> Digest32 {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(value);
    Digest32(hash.finalize().into())
}

/// The instance-specific cookie name and its attributes for this listener.
#[derive(Clone, Debug)]
pub struct CookieName {
    name: String,
    secure: bool,
}
impl CookieName {
    /// `secure` is true over HTTPS; explicit loopback HTTP omits `Secure`.
    pub fn new(instance_id: &str, secure: bool) -> Self {
        let id: String = instance_id
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        Self {
            name: format!("org_session_{}", id.to_ascii_lowercase()),
            secure,
        }
    }
    pub fn as_str(&self) -> &str {
        &self.name
    }
    fn attributes(&self) -> &'static str {
        if self.secure {
            "; Path=/api/v1; HttpOnly; SameSite=Strict; Secure"
        } else {
            "; Path=/api/v1; HttpOnly; SameSite=Strict"
        }
    }
    /// The `Set-Cookie` value that stores `pair`'s cookie.
    pub fn set(&self, pair: &BrowserPair) -> HeaderValue {
        let text = Zeroizing::new(format!(
            "{}={}{}",
            self.name,
            pair.cookie.as_str(),
            self.attributes()
        ));
        let mut value = HeaderValue::from_str(&text).expect("cookie text is ASCII");
        value.set_sensitive(true);
        value
    }
    /// The `Set-Cookie` value that clears the cookie.
    pub fn clear(&self) -> HeaderValue {
        HeaderValue::from_str(&format!("{}={}; Max-Age=0", self.name, self.attributes()))
            .expect("cookie text is ASCII")
    }
    /// This instance's cookie from the request, if exactly one is present and
    /// well formed. Two copies of the name are refused (no ambiguity).
    fn read(&self, headers: &HeaderMap) -> Result<Option<String>, ()> {
        let mut found = None;
        for header in headers.get_all(COOKIE) {
            let text = header.to_str().map_err(|_| ())?;
            for part in text.split(';') {
                let Some((name, value)) = part.trim().split_once('=') else {
                    continue;
                };
                if name != self.name {
                    continue;
                }
                if found.is_some() {
                    return Err(());
                }
                found = Some(value.to_owned());
            }
        }
        Ok(found)
    }
}

/// A freshly minted pair; the raw values leave the process only in the
/// response that sets them.
pub struct BrowserPair {
    cookie: Zeroizing<String>,
    proof: Zeroizing<String>,
}
impl BrowserPair {
    pub fn mint() -> std::io::Result<Self> {
        let mut bytes = Zeroizing::new([0u8; 64]);
        File::open("/dev/urandom")?.read_exact(&mut *bytes)?;
        let cookie = Zeroizing::new(URL_SAFE_NO_PAD.encode(&bytes[..32]));
        let mut proof = Zeroizing::new(String::with_capacity(64));
        for b in &bytes[32..] {
            proof.push_str(&format!("{b:02x}"));
        }
        Ok(Self { cookie, proof })
    }
    pub fn proof(&self) -> &str {
        &self.proof
    }
    pub fn cookie_digest(&self) -> Digest32 {
        digest(COOKIE_DOMAIN, self.cookie.as_bytes())
    }
    pub fn proof_digest(&self) -> Digest32 {
        digest(PROOF_DOMAIN, self.proof.as_bytes())
    }
}

/// The digests of what a request presented: this instance's cookie and the
/// `X-Org-Session-Proof` header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentedPair {
    pub cookie_digest: Digest32,
    pub proof_digest: Digest32,
}
impl PresentedPair {
    /// `None` unless the request carries exactly one well-formed cookie and one
    /// well-formed proof. A cookie without a proof is never recovered.
    pub fn read(name: &CookieName, headers: &HeaderMap) -> Option<Self> {
        let cookie = name.read(headers).ok()??;
        let decoded = URL_SAFE_NO_PAD.decode(cookie.as_bytes()).ok()?;
        if decoded.len() != 32 {
            return None;
        }
        let mut proofs = headers.get_all(PROOF_HEADER).iter();
        let proof = proofs.next()?.to_str().ok()?;
        if proofs.next().is_some()
            || proof.len() != 64
            || !proof
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return None;
        }
        Some(Self {
            cookie_digest: digest(COOKIE_DOMAIN, cookie.as_bytes()),
            proof_digest: digest(PROOF_DOMAIN, proof.as_bytes()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    #[test]
    fn minted_pair_round_trips_through_the_request_headers() {
        let name = CookieName::new("01ARZ3NDEKTSV4RRFFQ69G5FAV", true);
        let pair = BrowserPair::mint().unwrap();
        let set = name.set(&pair);
        let set = set.to_str().unwrap();
        assert!(set.starts_with("org_session_01arz3ndektsv4rrffq69g5fav="));
        assert!(set.ends_with("; Path=/api/v1; HttpOnly; SameSite=Strict; Secure"));
        assert!(!set.contains("Domain"));
        let cookie = set.split(';').next().unwrap();
        let presented = PresentedPair::read(
            &name,
            &headers(&[
                ("cookie", &format!("other=1; {cookie}")),
                (PROOF_HEADER, pair.proof()),
            ]),
        )
        .unwrap();
        assert_eq!(presented.cookie_digest, pair.cookie_digest());
        assert_eq!(presented.proof_digest, pair.proof_digest());
        assert_ne!(pair.cookie_digest(), pair.proof_digest());
        assert_eq!(pair.proof().len(), 64);
        assert!(!CookieName::new("x", false)
            .set(&pair)
            .to_str()
            .unwrap()
            .contains("Secure"));
    }

    #[test]
    fn cookie_without_proof_or_with_a_malformed_or_doubled_value_is_absent() {
        let name = CookieName::new("01ARZ3NDEKTSV4RRFFQ69G5FAV", false);
        let pair = BrowserPair::mint().unwrap();
        let set = name.set(&pair);
        let cookie = set.to_str().unwrap().split(';').next().unwrap().to_owned();
        let upper = pair.proof().to_ascii_uppercase();
        for case in [
            vec![("cookie", cookie.as_str())],
            vec![(PROOF_HEADER, pair.proof())],
            vec![("cookie", cookie.as_str()), (PROOF_HEADER, upper.as_str())],
            vec![
                ("cookie", cookie.as_str()),
                (PROOF_HEADER, &pair.proof()[1..]),
            ],
            vec![
                ("cookie", cookie.as_str()),
                (PROOF_HEADER, pair.proof()),
                (PROOF_HEADER, pair.proof()),
            ],
            vec![
                ("cookie", &format!("{cookie}; {cookie}")),
                (PROOF_HEADER, pair.proof()),
            ],
            vec![
                ("cookie", "org_session_01arz3ndektsv4rrffq69g5fav=short"),
                (PROOF_HEADER, pair.proof()),
            ],
        ] {
            assert_eq!(
                PresentedPair::read(&name, &headers(&case)),
                None,
                "{case:?}"
            );
        }
    }
}
