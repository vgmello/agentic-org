//! One-time links (standalone-server design section 6, decision D4).
//!
//! A link carries a 32-byte random secret, base64url without padding, after the
//! URL's `#`. The host stores only `SHA-256("agentic-org/link/v1" ‖ instance_id ‖
//! kind ‖ token)`, so a stored row cannot be replayed and is bound to this
//! instance. Issuing cancels the previous pending link of the same kind and
//! subject in the same transaction, so at most one is ever redeemable.
//!
//! `OneTimeLinks` is the engine-independent host-storage contract (ADR-0018).
//! The SQLite registry implements it in `hostdb::links`; `memory` is a second
//! implementation that proves the seam, and `contract_tests` runs against both.

use std::fs::File;
use std::io::{self, Read};

use sha2::{Digest as _, Sha256};

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
pub(crate) mod memory;

/// How long a registration link stays redeemable.
pub(crate) const REGISTRATION_TTL_MS: i64 = 24 * 60 * 60 * 1000;
const DOMAIN: &[u8] = b"agentic-org/link/v1";
const TOKEN_BYTES: usize = 32;
const SUBJECT_MAX: usize = 128;

/// What a link authorizes. The kind is part of the digest, so a token of one
/// kind never redeems another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkKind {
    /// Creates an administrator while none exists (setup and `admin reset`).
    Registration,
}
impl LinkKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Registration => "registration",
        }
    }
    pub(crate) fn ttl_ms(self) -> i64 {
        match self {
            Self::Registration => REGISTRATION_TTL_MS,
        }
    }
}

/// The thing a link is for, such as the single initial administrator. At most
/// one link per kind and subject is pending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Subject(String);
impl Subject {
    pub(crate) fn new(value: &str) -> Result<Self, Error> {
        let valid = !value.is_empty()
            && value.len() <= SUBJECT_MAX
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'));
        valid.then(|| Self(value.to_owned())).ok_or(Error::Invalid)
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// The link secret. It is returned once, to the caller that issued it, and is
/// never stored or logged.
pub(crate) struct Token([u8; TOKEN_BYTES]);
impl Token {
    fn generate() -> io::Result<Self> {
        let mut bytes = [0u8; TOKEN_BYTES];
        File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        Ok(Self(bytes))
    }
    /// Only exactly 32 bytes in canonical unpadded base64url parse.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let bytes = base64url_decode(text)?;
        let bytes: [u8; TOKEN_BYTES] = bytes.try_into().ok()?;
        let token = Self(bytes);
        (token.encode() == text).then_some(token)
    }
    /// The text that goes after `#` in the link.
    pub(crate) fn encode(&self) -> String {
        base64url_encode(&self.0)
    }
}
impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(..)")
    }
}

/// The only form in which a link is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinkDigest(pub(crate) [u8; 32]);
impl LinkDigest {
    pub(crate) fn of(instance_id: &str, kind: LinkKind, token: &Token) -> Self {
        let mut hash = Sha256::new();
        hash.update(DOMAIN);
        hash.update(instance_id.as_bytes());
        hash.update(kind.as_str().as_bytes());
        hash.update(token.0);
        Self(hash.finalize().into())
    }
}

/// A newly issued link. `token` is the secret; show it once.
#[derive(Debug)]
pub(crate) struct Issued {
    pub(crate) link_id: String,
    pub(crate) token: Token,
    pub(crate) expires_at_ms: i64,
}

/// A link that can still be redeemed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pending {
    pub(crate) link_id: String,
    pub(crate) issued_at_ms: i64,
    pub(crate) expires_at_ms: i64,
}

/// Why a token does not redeem. The caller shows one generic message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    Unknown,
    Used,
    Cancelled,
    Expired,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Invalid,
    Io(io::ErrorKind),
    Store,
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e.kind())
    }
}

/// The new row an adapter writes, already hashed. Adapters never see a token.
pub(crate) struct NewLink<'a> {
    pub(crate) link_id: &'a str,
    pub(crate) kind: LinkKind,
    pub(crate) subject: &'a Subject,
    pub(crate) digest: LinkDigest,
    pub(crate) issued_at_ms: i64,
    pub(crate) expires_at_ms: i64,
}

/// The host-storage contract for one-time links. Adapters implement the
/// storage primitives; the shared rules live in the provided methods.
pub(crate) trait OneTimeLinks {
    /// The instance every digest is bound to.
    fn instance_id(&self) -> &str;
    /// Cancel the pending link of the same kind and subject (ended at
    /// `issued_at_ms`) and insert `link`, in one transaction.
    fn replace_pending(&self, link: NewLink<'_>) -> Result<(), Error>;
    /// Mark the pending, unexpired link with this digest used at `now_ms`.
    fn redeem(
        &self,
        kind: LinkKind,
        digest: LinkDigest,
        now_ms: i64,
    ) -> Result<Result<String, Refused>, Error>;
    /// The pending link of this kind and subject, expired or not.
    fn pending_row(&self, kind: LinkKind, subject: &Subject) -> Result<Option<Pending>, Error>;

    fn issue(&self, kind: LinkKind, subject: &Subject, now_ms: i64) -> Result<Issued, Error> {
        let token = Token::generate()?;
        let link_id = crate::ids::uuid_v7()?;
        let expires_at_ms = now_ms.checked_add(kind.ttl_ms()).ok_or(Error::Invalid)?;
        self.replace_pending(NewLink {
            link_id: &link_id,
            kind,
            subject,
            digest: LinkDigest::of(self.instance_id(), kind, &token),
            issued_at_ms: now_ms,
            expires_at_ms,
        })?;
        Ok(Issued {
            link_id,
            token,
            expires_at_ms,
        })
    }
    /// Redeems a token once. A malformed token is `Unknown`.
    fn consume(
        &self,
        kind: LinkKind,
        token: &str,
        now_ms: i64,
    ) -> Result<Result<String, Refused>, Error> {
        let Some(token) = Token::parse(token) else {
            return Ok(Err(Refused::Unknown));
        };
        self.redeem(
            kind,
            LinkDigest::of(self.instance_id(), kind, &token),
            now_ms,
        )
    }
    /// The redeemable link of this kind and subject, if any.
    fn pending(
        &self,
        kind: LinkKind,
        subject: &Subject,
        now_ms: i64,
    ) -> Result<Option<Pending>, Error> {
        Ok(self
            .pending_row(kind, subject)?
            .filter(|p| now_ms < p.expires_at_ms))
    }
}

/// The state transition every adapter applies when a digest is presented.
pub(crate) fn redeem_rule(state: &str, expires_at_ms: i64, now_ms: i64) -> Result<(), Refused> {
    match state {
        "used" => Err(Refused::Used),
        "cancelled" => Err(Refused::Cancelled),
        "pending" if now_ms >= expires_at_ms => Err(Refused::Expired),
        "pending" => Ok(()),
        _ => Err(Refused::Unknown),
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64url_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 0x3f) as usize] as char);
        }
    }
    out
}
fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    for chunk in text.as_bytes().chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            let v = ALPHABET.iter().position(|a| a == c)? as u32;
            n |= v << (18 - 6 * i);
        }
        out.extend_from_slice(&n.to_be_bytes()[1..chunk.len()]);
    }
    Some(out)
}
