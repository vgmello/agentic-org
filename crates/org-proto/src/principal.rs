//! Human authority on host-made channels (X-02 §4.1): a request's `principal`, carrying a grant
//! the host issued and MAC'd for one channel of one office.
//!
//! - The host signs a person in, and for each request batch issues a [`Grant`] with
//!   `session.grant`: who is acting ([`GrantClaims`]), bound to an office and to one channel the
//!   host made for it, valid until `exp`, with a [`GrantMac`] over [`grant_mac_input`].
//! - The client sends it in every request on that channel, as [`Principal`] in
//!   [`crate::Request::principal`].
//! - The worker checks it, in order: the channel is host-made and the grant's `channel_id` is this
//!   connection's; the MAC is valid under the current key (constant time); `exp` hasn't passed;
//!   `office_id` is this office and `org_id` its organization (otherwise `not_found`); the session
//!   isn't revoked; `member` exists, is the owner and isn't removed. Every other failure is
//!   `not_authorized`. Neither is stored.
//!
//! `principal` on an office socket gets `not_authorized`. This module holds the shapes and the
//! MAC input as pure bytes; the HMAC is computed by the host and the worker.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::token::GrantMac;

/// The domain separator that starts every [`grant_mac_input`], followed by a `0x00` byte.
pub const GRANT_MAC_DOMAIN: &str = "org-grant/1";

/// What a grant says: who acts, for which office and channel, until when. The MAC covers exactly
/// these fields, through [`grant_mac_input`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GrantClaims {
    /// The signed-in account (the host's id for it).
    pub account: String,
    /// The organization member the account acts as.
    pub member: String,
    pub org_id: String,
    pub office_id: String,
    /// The host-made channel this grant is bound to.
    pub channel_id: String,
    /// The session's non-secret id, distinct from its handle. Revocation names it.
    pub session_id: String,
    /// When the grant expires: Unix milliseconds, 5 minutes after issue.
    pub exp: u64,
}

/// The bytes a grant's MAC is computed over (X-02 §4.1):
///
/// `"org-grant/1" ‖ 0x00 ‖ lp(account) ‖ lp(member) ‖ lp(org_id) ‖ lp(office_id) ‖ lp(channel_id)
/// ‖ lp(session_id) ‖ u64_be(exp)`
///
/// where `lp(x)` is `u32_be(len(x)) ‖ utf8(x)`, `len` in bytes. The length prefixes make the
/// encoding unambiguous: no two different claims give the same bytes.
///
/// `None` only when a field is longer than `u32::MAX` bytes, which no request can carry (a frame
/// is at most [`crate::MAX_FRAME_BYTES`]); a verifier treats it as a failed check.
pub fn grant_mac_input(claims: &GrantClaims) -> Option<Vec<u8>> {
    let fields = [
        &claims.account,
        &claims.member,
        &claims.org_id,
        &claims.office_id,
        &claims.channel_id,
        &claims.session_id,
    ];
    let capacity = GRANT_MAC_DOMAIN.len()
        + 1
        + fields.iter().map(|f| 4 + f.len()).sum::<usize>()
        + std::mem::size_of::<u64>();
    let mut out = Vec::with_capacity(capacity);
    out.extend_from_slice(GRANT_MAC_DOMAIN.as_bytes());
    out.push(0x00);
    for field in fields {
        let len = u32::try_from(field.len()).ok()?;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(field.as_bytes());
    }
    out.extend_from_slice(&claims.exp.to_be_bytes());
    Some(out)
}

/// A grant: its [`GrantClaims`] and their [`GrantMac`]. On the wire it is one flat object,
/// `{"account", "member", "org_id", "office_id", "channel_id", "session_id", "exp", "mac"}`,
/// with `exp` an integer and `mac` 64 lowercase hex digits.
///
/// It isn't `Clone`, because its MAC isn't. Its `Debug` omits the MAC.
#[derive(PartialEq, Eq)]
pub struct Grant {
    pub claims: GrantClaims,
    pub mac: GrantMac,
}

impl Grant {
    pub fn new(claims: GrantClaims, mac: GrantMac) -> Self {
        Self { claims, mac }
    }
}

impl fmt::Debug for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = &self.claims;
        f.debug_struct("Grant")
            .field("account", &c.account)
            .field("member", &c.member)
            .field("org_id", &c.org_id)
            .field("office_id", &c.office_id)
            .field("channel_id", &c.channel_id)
            .field("session_id", &c.session_id)
            .field("exp", &c.exp)
            .finish_non_exhaustive()
    }
}

/// A grant as written, borrowing it. Plain fields, no `flatten`, so the MAC is never buffered
/// by serde.
#[derive(Serialize)]
struct GrantRef<'a> {
    account: &'a str,
    member: &'a str,
    org_id: &'a str,
    office_id: &'a str,
    channel_id: &'a str,
    session_id: &'a str,
    exp: u64,
    mac: &'a GrantMac,
}

/// A grant as read: the MAC decodes straight into its secret type.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGrant {
    account: String,
    member: String,
    org_id: String,
    office_id: String,
    channel_id: String,
    session_id: String,
    exp: u64,
    mac: GrantMac,
}

impl Serialize for Grant {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let c = &self.claims;
        GrantRef {
            account: &c.account,
            member: &c.member,
            org_id: &c.org_id,
            office_id: &c.office_id,
            channel_id: &c.channel_id,
            session_id: &c.session_id,
            exp: c.exp,
            mac: &self.mac,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Grant {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw: RawGrant = d.deserialize_any(ObjectOnly::new("a grant object"))?;
        Ok(Grant {
            claims: GrantClaims {
                account: raw.account,
                member: raw.member,
                org_id: raw.org_id,
                office_id: raw.office_id,
                channel_id: raw.channel_id,
                session_id: raw.session_id,
                exp: raw.exp,
            },
            mac: raw.mac,
        })
    }
}

/// Who a request acts for, on a host-made channel: `{"grant": {...}}` (X-02 §4.1). It travels in
/// [`crate::Request::principal`]. Not `Clone`; its `Debug` omits the MAC.
///
/// It decodes only from a JSON object (a grant too), and a value of any other type is refused
/// without being quoted, so a MAC sent in the wrong place never reaches an error message.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Principal {
    pub grant: Grant,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPrincipal {
    grant: Grant,
}

impl<'de> Deserialize<'de> for Principal {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw: RawPrincipal = d.deserialize_any(ObjectOnly::new("a principal object"))?;
        Ok(Principal { grant: raw.grant })
    }
}

/// Decodes `T` from a map only. Every other JSON type is refused with a fixed message that never
/// quotes the value (serde's default "invalid type" error would print a string or a number).
pub(crate) struct ObjectOnly<T> {
    expecting: &'static str,
    marker: std::marker::PhantomData<T>,
}

impl<T> ObjectOnly<T> {
    pub(crate) fn new(expecting: &'static str) -> Self {
        Self {
            expecting,
            marker: std::marker::PhantomData,
        }
    }

    fn refuse<E: serde::de::Error>(&self) -> E {
        E::custom(format_args!("expected {}", self.expecting))
    }
}

impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for ObjectOnly<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.expecting)
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
        T::deserialize(serde::de::value::MapAccessDeserializer::new(map))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, _: A) -> Result<T, A::Error> {
        Err(self.refuse())
    }
    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<T, E> {
        Err(self.refuse())
    }
    fn visit_bytes<E: serde::de::Error>(self, _: &[u8]) -> Result<T, E> {
        Err(self.refuse())
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<T, E> {
        Err(self.refuse())
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<T, E> {
        Err(self.refuse())
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<T, E> {
        Err(self.refuse())
    }
    fn visit_i128<E: serde::de::Error>(self, _: i128) -> Result<T, E> {
        Err(self.refuse())
    }
    fn visit_u128<E: serde::de::Error>(self, _: u128) -> Result<T, E> {
        Err(self.refuse())
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<T, E> {
        Err(self.refuse())
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<T, E> {
        Err(self.refuse())
    }
    fn visit_none<E: serde::de::Error>(self) -> Result<T, E> {
        Err(self.refuse())
    }
}

/// [`crate::Request::principal`] found a principal it can't read. It carries no detail, so it can
/// never quote a secret; a server answers it with `not_authorized`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MalformedPrincipal;

impl fmt::Display for MalformedPrincipal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the request's principal isn't a valid grant")
    }
}

impl std::error::Error for MalformedPrincipal {}

/// Who made a change, on `org.changed` and `work.changed` (X-02 §4.2, for Activity): the member
/// and the kind of principal that acted. Absent on events from servers that predate it, and on
/// changes no principal made.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Actor {
    pub member: String,
    pub kind: ActorKind,
}

/// The kind of principal behind a change. Unknown values decode as [`ActorKind::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ActorKind {
    /// A person acting through a grant ([`Principal`]): in v1.0, the owner.
    Human,
    /// An agent acting on its bound connection (ADR-0003 `msg.bind`).
    Agent,
    /// Not known to this build. Never sent: serializing it fails.
    #[serde(other, skip_serializing)]
    Unknown,
}
