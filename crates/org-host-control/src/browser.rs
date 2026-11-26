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
use serde::{Deserialize, Serialize};

pub use crate::browser_auth::{BrowserAuth, BrowserOrigin, Digest32};
use crate::listen::Hsts;
use crate::ops::{LinkedOrg, OfficeOpenChannelResult, SessionGrantResult};
use crate::status::{Administrator, InstanceMode, WebState};
use crate::{LinkToken, Password, TlsPrivateKey};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_org_id: Option<String>,
}

/// `instance.view`: the instance's display state behind the browser's `GET /instance`. The web
/// process validates the browser session first; the host serves this only on the web
/// connection. No path, socket, handle or certificate material; TLS state is not reported
/// until TLS lands.
pub struct InstanceView;
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceViewParams {}
fn default_auth_enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceViewResult {
    /// The instance's registered name.
    pub name: String,
    pub status: InstanceMode,
    pub web_status: WebState,
    pub administrator: Administrator,
    /// Whether authentication is currently enforced on this instance (`false` on default local HTTP HQ).
    #[serde(default = "default_auth_enabled")]
    pub auth_enabled: bool,
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

/// `browser.office_restart`: restart an `offline` or `unavailable` office on behalf of a
/// live browser session (admitted web connection only; administrator or organization owner).
/// Journaled under `office.restart` by `operation_id`.
pub struct BrowserOfficeRestart;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserOfficeRestartParams {
    pub auth: BrowserAuth,
    pub operation_id: String,
    pub org_id: String,
    pub office_id: String,
}
impl Call for BrowserOfficeRestart {
    const METHOD: &'static str = "browser.office_restart";
    type Params = BrowserOfficeRestartParams;
    type Result = crate::HostOpResult<Empty>;
}
impl crate::journal::Journaled for BrowserOfficeRestartParams {
    const METHOD: &'static str = crate::ops::OfficeRestart::METHOD;
    fn operation_id(&self) -> &str {
        &self.operation_id
    }
    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        let BrowserOfficeRestartParams {
            auth: _,
            operation_id: _,
            org_id,
            office_id,
        } = self;
        #[derive(Serialize)]
        struct Fields<'a> {
            office_id: &'a str,
            org_id: &'a str,
        }
        serde_json::to_string(&Fields { office_id, org_id })
    }
}

/// `browser.cli_approval_decide`: approve or decline a pending terminal sign-in link on behalf
/// of a signed-in browser session (FC-173 / SIGN-40; admitted web connection only).
pub struct BrowserCliApprovalDecide;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserCliApprovalDecideParams {
    pub auth: BrowserAuth,
    pub link: LinkToken,
    pub approve: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliApprovalDecision {
    Approved,
    Declined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserCliApprovalDecideResult {
    pub status: CliApprovalDecision,
}

impl Call for BrowserCliApprovalDecide {
    const METHOD: &'static str = "browser.cli_approval_decide";
    type Params = BrowserCliApprovalDecideParams;
    type Result = BrowserCliApprovalDecideResult;
}

/// `browser.local_session`: return the implicit local owner session on a local plain-HTTP
/// instance whose authentication is disabled (ADR-0034; admitted web connection only).
/// Refused with `not_authorized` whenever authentication is enabled or the listener is HTTPS.
pub struct BrowserLocalSession;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserLocalSessionParams {
    pub origin: BrowserOrigin,
}

impl Call for BrowserLocalSession {
    const METHOD: &'static str = "browser.local_session";
    type Params = BrowserLocalSessionParams;
    type Result = BrowserSession;
}

/// `browser.auth_enable`: opt-in enablement of password/session authentication on a local
/// authless instance (ADR-0034; admitted web connection only). Upgrades the local owner account,
/// persists `auth_enabled = true`, broadcasts `auth.mode_changed`, and returns an active session.
pub struct BrowserAuthEnable;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAuthEnableParams {
    pub login: String,
    pub password: Password,
    pub client_address: IpAddr,
    pub cookie_digest: Digest32,
    pub proof_digest: Digest32,
    pub origin: BrowserOrigin,
}

impl Call for BrowserAuthEnable {
    const METHOD: &'static str = "browser.auth_enable";
    type Params = BrowserAuthEnableParams;
    type Result = BrowserSession;
}

/// `browser.tls_enable`: opt-in HTTPS enablement or live certificate replacement from inside
/// the app (ADR-0033 / TASK-62; admitted web connection only).
pub struct BrowserTlsEnable;

#[derive(Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum TlsEnableMode {
    SelfSigned,
    Supplied { cert: String, key: TlsPrivateKey },
}

impl fmt::Debug for TlsEnableMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelfSigned => f.write_str("TlsEnableMode::SelfSigned"),
            Self::Supplied { .. } => {
                f.write_str("TlsEnableMode::Supplied { cert: <redacted>, key: <redacted> }")
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserTlsEnableParams {
    pub auth: BrowserAuth,
    pub mode: TlsEnableMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hsts: Option<Hsts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserTlsEnableResult {
    pub scheme: org_proto::web::ListenScheme,
    pub origin: String,
    pub fingerprint: String,
    pub source: String,
    pub restart_required: bool,
}

impl Call for BrowserTlsEnable {
    const METHOD: &'static str = "browser.tls_enable";
    type Params = BrowserTlsEnableParams;
    type Result = BrowserTlsEnableResult;
}

/// Host→web event payload for [`crate::event::AUTH_MODE_CHANGED`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthModeChanged {
    pub auth_enabled: bool,
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
            local_authless: false,
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
