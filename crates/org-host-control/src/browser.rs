//! Browser-session host ops (A3 plan B4; ADR-0013), served only on the admitted web process's
//! control connection. Only digests of the browser's cookie and proof travel: the web process
//! mints both 32-byte values and sends `SHA-256(domain ‖ value)` as 64 lowercase hex digits.
//! Passwords travel as [`Password`] (redacted, zeroized). Every op is open (no host handle).
//!
//! Refusals: `invalid_request` with `field`/`rule` for a username or password rule;
//! `not_found` for a link that doesn't redeem; one identical `not_authorized` for an unknown
//! login, a wrong password or a session that isn't live; `rate_limited` with
//! `retry_after_ms` while a throttle key is locked; `unavailable` when the session bound is full.
use std::fmt;
use std::net::IpAddr;

use org_proto::{Call, Empty};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ops::{LinkedOrg, OfficeOpenChannelResult, SessionGrantResult};
use crate::organization::BrowserAuth;
use crate::status::{Administrator, InstanceMode, WebState};
use crate::{LinkToken, Password};

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

/// `link.register`: redeem the setup registration link and create the administrator.
pub struct LinkRegister;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkRegisterParams {
    pub link: LinkToken,
    pub login: String,
    pub password: Password,
    /// The browser's peer address as the web process saw it; a refused link
    /// counts against it on the same per-address throttle as sign-in.
    pub client_address: IpAddr,
    pub cookie_digest: Digest32,
    pub proof_digest: Digest32,
    pub origin: BrowserOrigin,
}
impl Call for LinkRegister {
    const METHOD: &'static str = "link.register";
    type Params = LinkRegisterParams;
    type Result = BrowserSession;
}

/// `session.sign_in`: check a password and create a browser session. `client_address` is the
/// browser's peer address as the web process saw it; the host throttles per login and per
/// address (IPv6 by /64).
pub struct SessionSignIn;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSignInParams {
    pub login: String,
    pub password: Password,
    pub client_address: IpAddr,
    pub cookie_digest: Digest32,
    pub proof_digest: Digest32,
    pub origin: BrowserOrigin,
}
impl Call for SessionSignIn {
    const METHOD: &'static str = "session.sign_in";
    type Params = SessionSignInParams;
    type Result = BrowserSession;
}

/// `session.validate`: the live session a cookie and proof name, from this origin.
pub struct SessionValidate;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionValidateParams {
    pub cookie_digest: Digest32,
    pub proof_digest: Digest32,
    pub origin: BrowserOrigin,
}
impl Call for SessionValidate {
    const METHOD: &'static str = "session.validate";
    type Params = SessionValidateParams;
    type Result = BrowserSession;
}

/// `session.end` on the web connection: delete the browser session, then push
/// [`SESSION_ENDED`]. Ending an unknown session is `{}` too. (On `host.sock` the same method
/// name ends a connection-bound native session by handle: [`crate::ops::SessionEnd`].)
pub struct BrowserSessionEnd;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserSessionEndParams {
    pub session_id: String,
}
impl Call for BrowserSessionEnd {
    const METHOD: &'static str = "session.end";
    type Params = BrowserSessionEndParams;
    type Result = Empty;
}

/// A browser session's view. Never holds a digest or a host handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSession {
    pub session_id: String,
    pub account: BrowserAccount,
    pub admin: bool,
    pub orgs: Vec<LinkedOrg>,
    pub server_time_ms: u64,
    pub idle_expires_at_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserAccount {
    pub id: String,
    pub login: String,
}

/// `instance.view`: the instance's display state behind the browser's `GET /instance`. The web
/// process validates the browser session first; the host serves this only on the web
/// connection. No path, socket, handle or certificate material; TLS state is not reported
/// until TLS lands.
pub struct InstanceView;
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceViewParams {}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceViewResult {
    /// The instance's registered name.
    pub name: String,
    pub status: InstanceMode,
    pub web_status: WebState,
    pub administrator: Administrator,
    /// The listener's certificate, for an `https` instance only: served (`ok`), fewer than
    /// 30 days left (`expiring`) or unservable (`missing`). Never certificate material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<org_proto::web::TlsStatus>,
}
impl Call for InstanceView {
    const METHOD: &'static str = "instance.view";
    type Params = InstanceViewParams;
    type Result = InstanceViewResult;
}

/// `browser.open_channel`: open an authenticated Web office channel on the admitted web
/// process's control connection. The descriptor travels on the web process's transfer
/// datagram socket (`TransferRecord::Channel { kind: Web, .. }`) and must be acknowledged
/// with `fds.received` before any grant is requested on it.
pub struct BrowserOpenChannel;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserOpenChannelParams {
    pub auth: BrowserAuth,
    pub office_id: String,
}
impl Call for BrowserOpenChannel {
    const METHOD: &'static str = "browser.open_channel";
    type Params = BrowserOpenChannelParams;
    type Result = OfficeOpenChannelResult;
}

/// `browser.grant`: mint a short-lived HMAC-signed office grant for a live browser session
/// over an already-acknowledged Web channel on this connection.
pub struct BrowserSessionGrant;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserSessionGrantParams {
    pub auth: BrowserAuth,
    pub office_id: String,
    pub channel_id: String,
}
impl Call for BrowserSessionGrant {
    const METHOD: &'static str = "browser.grant";
    type Params = BrowserSessionGrantParams;
    type Result = SessionGrantResult;
}

/// Host→web event: the session no longer exists; drop it from every cache.
pub const SESSION_ENDED: &str = "session.ended";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEnded {
    pub session_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_channel_and_grant_wire_shapes_refuse_unknown_fields() {
        let auth = BrowserAuth {
            cookie_digest: Digest32([0x11; 32]),
            proof_digest: Digest32([0x22; 32]),
            origin: BrowserOrigin {
                scheme: "http".into(),
                host: "127.0.0.1".into(),
                port: 8080,
            },
        };
        let open = BrowserOpenChannelParams {
            auth: auth.clone(),
            office_id: "0193a5c0-0000-7000-8000-000000000001".into(),
        };
        let wire = serde_json::to_value(&open).unwrap();
        let decoded: BrowserOpenChannelParams = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded.office_id, open.office_id);
        let mut extra = wire;
        extra["extra"] = true.into();
        assert!(serde_json::from_value::<BrowserOpenChannelParams>(extra).is_err());

        let grant = BrowserSessionGrantParams {
            auth,
            office_id: "0193a5c0-0000-7000-8000-000000000001".into(),
            channel_id: "ch-1".into(),
        };
        let wire = serde_json::to_value(&grant).unwrap();
        let decoded: BrowserSessionGrantParams = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded.channel_id, "ch-1");
        let mut extra = wire;
        extra["session"] = "handle".into();
        assert!(serde_json::from_value::<BrowserSessionGrantParams>(extra).is_err());

        assert_eq!(BrowserOpenChannel::METHOD, "browser.open_channel");
        assert_eq!(BrowserSessionGrant::METHOD, "browser.grant");
    }
}
