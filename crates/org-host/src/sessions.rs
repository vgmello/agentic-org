//! Browser sessions (A3, plan B3; SES-4, SES-6, D8).
//!
//! The web process mints a 32-byte random cookie value and a 32-byte proof for
//! each browser session. The host never sees either: it stores and compares only
//! the domain-separated digests defined on [`SessionDigest`], so a copied
//! registry yields no usable cookie.
//!
//! `BrowserSessions` is the consumer-owned, engine-independent host-storage
//! contract (ADR-0018). The SQLite registry implements it in `hostdb::sessions`;
//! `memory` is a second implementation that proves the seam, and
//! `contract_tests` runs one suite against both. The shared rules (host-wide
//! bound, idle lifetime, digest comparison) live here so adapters only store.
//!
//! Time is host wall milliseconds supplied by the caller, so tests inject it.
//! Adapters make it monotone: a call never observes an earlier time than one
//! already applied. A negative time is refused as `Unavailable`.

use sha2::{Digest as _, Sha256};

#[cfg(test)]
pub(crate) mod contract_tests;
#[cfg(test)]
pub(crate) mod memory;

/// Host-wide bound on stored browser sessions (D8). Expired sessions are reaped
/// before a new one is refused.
pub const SESSION_LIMIT: usize = 4096;
/// A session ends after 30 days without activity.
pub const IDLE_MS: i64 = 30 * 24 * 60 * 60 * 1000;
const COOKIE_DOMAIN: &[u8] = b"agentic-org/session-cookie/v1";
const PROOF_DOMAIN: &[u8] = b"agentic-org/session-proof/v1";

/// The only form in which a cookie value or proof reaches the host.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SessionDigest(pub [u8; 32]);
impl SessionDigest {
    /// `SHA-256("agentic-org/session-cookie/v1" ‖ value)`. The domain is a
    /// fixed-length constant, so the concatenation is unambiguous.
    pub fn cookie(value: &[u8]) -> Self {
        Self(digest(COOKIE_DOMAIN, value))
    }
    /// `SHA-256("agentic-org/session-proof/v1" ‖ value)`.
    pub fn proof(value: &[u8]) -> Self {
        Self(digest(PROOF_DOMAIN, value))
    }
    /// Constant-time equality: the time taken does not depend on where the
    /// digests first differ.
    pub fn ct_eq(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}
impl std::fmt::Debug for SessionDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionDigest(..)")
    }
}
fn digest(domain: &[u8], value: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(value);
    hash.finalize().into()
}

/// The browser origin a session is bound to. A session presented from any
/// other origin is not found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    scheme: Scheme,
    host: String,
    port: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Http,
    Https,
}
impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "http" => Some(Self::Http),
            "https" => Some(Self::Https),
            _ => None,
        }
    }
}
/// The origin is not a bounded lowercase host with a nonzero port.
#[derive(Debug, PartialEq, Eq)]
pub struct InvalidOrigin;
impl Origin {
    pub fn new(scheme: Scheme, host: &str, port: u16) -> Result<Self, InvalidOrigin> {
        let valid = !host.is_empty()
            && host.len() <= 255
            && port != 0
            && host.bytes().all(|b| {
                b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || matches!(b, b'.' | b'-' | b':' | b'[' | b']')
            });
        valid
            .then(|| Self {
                scheme,
                host: host.to_owned(),
                port,
            })
            .ok_or(InvalidOrigin)
    }
    pub fn scheme(&self) -> Scheme {
        self.scheme
    }
    pub fn host(&self) -> &str {
        &self.host
    }
    pub fn port(&self) -> u16 {
        self.port
    }
}

/// A session to store. Its ID is a fresh UUIDv7.
pub struct NewSession {
    pub id: String,
    pub account_id: String,
    pub cookie: SessionDigest,
    pub proof: SessionDigest,
    pub origin: Origin,
}
impl NewSession {
    pub fn new(
        account_id: &str,
        cookie: SessionDigest,
        proof: SessionDigest,
        origin: Origin,
    ) -> Result<Self, StoreError> {
        Ok(Self {
            id: crate::ids::uuid_v7().map_err(|_| StoreError::Unavailable)?,
            account_id: account_id.to_owned(),
            cookie,
            proof,
            origin,
        })
    }
}

/// A stored session as `find` returns it. Digests stay inside the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: String,
    pub account_id: String,
    pub created_ms: i64,
    pub last_activity_ms: i64,
    pub origin: Origin,
}

/// A session that no longer exists. The caller pushes `session.ended` for it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ended {
    pub id: String,
    pub account_id: String,
}

/// The result of `create`. Both arms carry the sessions reaped as expired in
/// the same transaction, so the caller can push their end. `Full` carries no
/// account information: every account gets the same `unavailable` reply (SES-6).
#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    Created { reaped: Vec<Ended> },
    Full { reaped: Vec<Ended> },
}

/// Storage failed. A failure is never reported as a valid session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    Unavailable,
}

/// Whether a session last active at `last_activity_ms` is still live at `now_ms`.
pub fn is_live(last_activity_ms: i64, now_ms: i64) -> bool {
    now_ms < last_activity_ms.saturating_add(IDLE_MS)
}

/// The host-storage contract for browser sessions. Every mutating call runs in
/// one transaction.
pub trait BrowserSessions {
    /// Reaps expired sessions, then stores `new` unless `SESSION_LIMIT` live
    /// sessions remain. An unknown account or a duplicate digest is
    /// `Unavailable`; nothing is stored.
    fn create(&self, new: &NewSession, now_ms: i64) -> Result<Admission, StoreError>;
    /// The live session whose cookie digest is `cookie`, if its proof digest
    /// matches `proof` (constant time) and it is bound to `origin`.
    fn find(
        &self,
        cookie: &SessionDigest,
        proof: &SessionDigest,
        origin: &Origin,
        now_ms: i64,
    ) -> Result<Option<SessionRecord>, StoreError>;
    /// Raises each session's last activity to the given time if later, never
    /// past the store's monotone time (at least `now_ms`), which it advances.
    /// A caller cannot extend a session beyond host time. Unknown IDs are ignored.
    fn touch(&self, activity: &[(&str, i64)], now_ms: i64) -> Result<(), StoreError>;
    /// Deletes one session.
    fn end(&self, id: &str) -> Result<Option<Ended>, StoreError>;
    /// Deletes every session of `account_id` except `except`, sorted by ID.
    fn end_account(&self, account_id: &str, except: Option<&str>)
        -> Result<Vec<Ended>, StoreError>;
    /// Deletes every expired session, sorted by ID.
    fn reap(&self, now_ms: i64) -> Result<Vec<Ended>, StoreError>;
}
