//! The host ops (X-02 §6.1, §6.2), each a [`Call`]. Unless a doc says otherwise, an op is served
//! on both the gateway's control connection and `host.sock`.
//!
//! - **Open** (no session): `hello`, `account.verify`, and on `host.sock` only
//!   `account.reset_password`.
//! - **Session ops**: everything else here takes the `session` from `account.verify`. A session
//!   ends on `session.end`, a password change, unlinking, `account.revoke`, its TTL (12 h absolute,
//!   30 min idle by default), or EOF of the connection it was issued on.
//! - Refusals: `not_authorized` for a failed sign-in, a dead or unlinked session, or a peer not
//!   allowed the op; `not_found` for an organization or office the session isn't linked to;
//!   `unavailable` when the sign-in pool is full; `operation_reused` per [`crate::journal`].

use org_proto::{Call, Empty, Grant, ListOfficesResult};
use serde::{Deserialize, Serialize};

use crate::{HostOpResult, Password, RecoveryCode, SessionHandle, TlsPrivateKey};

/// `account.verify` (open): sign in. Argon2id on a bounded pool; an unknown login hashes against
/// a dummy, and every failure is the same `not_authorized`. Lockout is per login and per socket;
/// `host.sock` has its own budget.
pub struct AccountVerify;

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountVerifyParams {
    pub login: String,
    pub password: Password,
}

/// `account.verify`'s result: the session and what the account may act on.
#[derive(Debug, Serialize, Deserialize)]
pub struct AccountVerifyResult {
    pub session: SessionHandle,
    pub account: String,
    pub admin: bool,
    pub orgs: Vec<LinkedOrg>,
}

/// An organization the account is linked to: the member it acts as there and its offices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedOrg {
    pub org_id: String,
    pub member_id: String,
    pub office_ids: Vec<String>,
}

impl Call for AccountVerify {
    const METHOD: &'static str = "account.verify";
    type Params = AccountVerifyParams;
    type Result = AccountVerifyResult;
}

/// The params of an op that names only its session.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionParams {
    pub session: SessionHandle,
}

/// `session.renew`: extend the session's idle timer, without the password.
pub struct SessionRenew;

impl Call for SessionRenew {
    const METHOD: &'static str = "session.renew";
    type Params = SessionParams;
    type Result = Empty;
}

/// `session.end`: end the session. The host revokes its grants on every worker it issued them
/// for (a `revoked` transfer record).
pub struct SessionEnd;

impl Call for SessionEnd {
    const METHOD: &'static str = "session.end";
    type Params = SessionParams;
    type Result = Empty;
}

/// `session.grant`: a grant for one request batch on one channel (X-02 §4.1). The session must be
/// live and linked to the office's organization, and `channel_id` a live channel the host made
/// for that office **on this same connection** (`office.open_channel`). The grant expires 5
/// minutes after issue; the gateway renews early.
pub struct SessionGrant;

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionGrantParams {
    pub session: SessionHandle,
    pub office_id: String,
    pub channel_id: String,
}

/// `session.grant`'s result. Not `Clone`: the grant's MAC is a secret.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionGrantResult {
    pub grant: Grant,
}

impl Call for SessionGrant {
    const METHOD: &'static str = "session.grant";
    type Params = SessionGrantParams;
    type Result = SessionGrantResult;
}

/// `organization.create` (admin; journaled, §6.3): create an organization and its HQ office. It
/// answers `accepted` while the create is in flight and `completed` with the organization once
/// the worker reported `created`; a retry with the same `operation_id` uses the same IDs, key and
/// recovery code.
pub struct OrganizationCreate;

#[derive(Debug, Serialize, Deserialize)]
pub struct OrganizationCreateParams {
    pub session: SessionHandle,
    pub operation_id: String,
    pub name: String,
}

impl Call for OrganizationCreate {
    const METHOD: &'static str = "organization.create";
    type Params = OrganizationCreateParams;
    type Result = HostOpResult<org_proto::Organization>;
}

/// `organization.cancel` (the same account, or an admin): mark a `reserved` create `abandoned`.
/// `conflict` while an attempt is in flight.
pub struct OrganizationCancel;

#[derive(Debug, Serialize, Deserialize)]
pub struct OrganizationCancelParams {
    pub session: SessionHandle,
    pub operation_id: String,
}

impl Call for OrganizationCancel {
    const METHOD: &'static str = "organization.cancel";
    type Params = OrganizationCancelParams;
    type Result = Empty;
}

/// `organization.list`: the organizations the session's account is linked to, each with its
/// recovery state.
pub struct OrganizationList;

/// One linked organization: `org-proto`'s [`org_proto::Organization`] fields, plus `recovery`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedOrganization {
    #[serde(flatten)]
    pub organization: org_proto::Organization,
    pub recovery: RecoveryState,
}

/// Where an organization's recovery code is (X-02 §6.4). Unknown values decode as
/// [`RecoveryState::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RecoveryState {
    /// Staged by the host, never shown.
    Pending,
    /// Shown once with `recovery.release`, not yet acknowledged.
    Delivered,
    /// Acknowledged: the staged copy is erased.
    Acknowledged,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationListResult {
    pub organizations: Vec<LinkedOrganization>,
}

impl Call for OrganizationList {
    const METHOD: &'static str = "organization.list";
    type Params = SessionParams;
    type Result = OrganizationListResult;
}

/// `office.list`: the offices of the organizations the session is linked to, or of `org_id`.
pub struct OfficeList;

#[derive(Debug, Serialize, Deserialize)]
pub struct OfficeListParams {
    pub session: SessionHandle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
}

impl Call for OfficeList {
    const METHOD: &'static str = "office.list";
    type Params = OfficeListParams;
    type Result = ListOfficesResult;
}

/// `office.restart` (an account linked to the office's organization; journaled): start an
/// `offline` or `unavailable` office now. Refused only as OAT:191 says.
pub struct OfficeRestart;

#[derive(Debug, Serialize, Deserialize)]
pub struct OfficeRestartParams {
    pub session: SessionHandle,
    pub operation_id: String,
    pub office_id: String,
}

impl Call for OfficeRestart {
    const METHOD: &'static str = "office.restart";
    type Params = OfficeRestartParams;
    type Result = HostOpResult<Empty>;
}

/// `office.open_channel` (a live session linked to the office's organization, on both sockets):
/// a new host-made channel to the office's worker. The answer carries only `channel_id`. On the
/// control connection the descriptor arrives on the gateway's transfer socket in a `channel`
/// record with the same `channel_id`; on `host.sock` the response line is followed by one
/// `sendmsg` with a 1-byte marker and the descriptor. The host caps channels per office and keeps
/// one slot for local channels.
pub struct OfficeOpenChannel;

#[derive(Debug, Serialize, Deserialize)]
pub struct OfficeOpenChannelParams {
    pub session: SessionHandle,
    pub office_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeOpenChannelResult {
    pub channel_id: String,
}

impl Call for OfficeOpenChannel {
    const METHOD: &'static str = "office.open_channel";
    type Params = OfficeOpenChannelParams;
    type Result = OfficeOpenChannelResult;
}

/// `operation.get` (the same account): a journaled op's stored result, by its `operation_id`.
/// `not_found` for an unknown op or another account's.
pub struct OperationGet;

#[derive(Debug, Serialize, Deserialize)]
pub struct OperationGetParams {
    pub session: SessionHandle,
    pub operation_id: String,
}

impl Call for OperationGet {
    const METHOD: &'static str = "operation.get";
    type Params = OperationGetParams;
    type Result = HostOpResult<serde_json::Value>;
}

/// `tls.fetch` (**control connection only**; refused on `host.sock`): the gateway's TLS pair, at
/// start and after each `tls.renew` event.
pub struct TlsFetch;

/// `tls.fetch`'s result, by the configured listen scheme: `{"mode": "https", "cert", "key"}` or
/// `{"mode": "http_loopback"}`. Not `Clone`: the key is a secret.
///
/// It decodes as one flat object, not through serde's internally tagged enums, so the key goes
/// straight from the JSON into [`TlsPrivateKey`] without an intermediate buffer. `https` needs
/// both `cert` and `key`; `http_loopback` takes neither; any other `mode` is refused.
#[derive(Debug)]
pub enum TlsFetchResult {
    Https {
        /// The certificate chain, PEM.
        cert: String,
        key: TlsPrivateKey,
    },
    /// Plain HTTP on a loopback address: no pair.
    HttpLoopback,
}

/// The configured listen scheme, `tls.fetch`'s `mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TlsMode {
    Https,
    HttpLoopback,
}

#[derive(Serialize)]
struct TlsFetchRef<'a> {
    mode: TlsMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    cert: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<&'a TlsPrivateKey>,
}

#[derive(Deserialize)]
struct RawTlsFetch {
    mode: TlsMode,
    #[serde(default)]
    cert: Option<String>,
    #[serde(default)]
    key: Option<TlsPrivateKey>,
}

impl Serialize for TlsFetchResult {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            TlsFetchResult::Https { cert, key } => TlsFetchRef {
                mode: TlsMode::Https,
                cert: Some(cert),
                key: Some(key),
            },
            TlsFetchResult::HttpLoopback => TlsFetchRef {
                mode: TlsMode::HttpLoopback,
                cert: None,
                key: None,
            },
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for TlsFetchResult {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let raw = RawTlsFetch::deserialize(d)?;
        match (raw.mode, raw.cert, raw.key) {
            (TlsMode::Https, Some(cert), Some(key)) => Ok(TlsFetchResult::Https { cert, key }),
            (TlsMode::HttpLoopback, None, None) => Ok(TlsFetchResult::HttpLoopback),
            (TlsMode::Https, _, _) => Err(D::Error::custom("https needs a cert and a key")),
            (TlsMode::HttpLoopback, _, _) => {
                Err(D::Error::custom("http_loopback takes no cert or key"))
            }
        }
    }
}

impl Call for TlsFetch {
    const METHOD: &'static str = "tls.fetch";
    type Params = Empty;
    type Result = TlsFetchResult;
}

/// `tls.set` (`host.sock` only; admin, the signed `agentic-org` peer on macOS; journaled): install
/// a TLS pair. The host checks that the key matches the certificate and that the certificate is
/// valid now before it stores the pair and emits `tls.renew`. It never changes the scheme. Its
/// journal fingerprint is SHA-256 of the certificate DER plus SHA-256 of the key, computed by the
/// host; the stored result echoes neither.
pub struct TlsSet;

#[derive(Debug, Serialize, Deserialize)]
pub struct TlsSetParams {
    pub session: SessionHandle,
    pub operation_id: String,
    pub cert: String,
    pub key: TlsPrivateKey,
}

impl Call for TlsSet {
    const METHOD: &'static str = "tls.set";
    type Params = TlsSetParams;
    type Result = HostOpResult<Empty>;
}

/// `account.set_password` (`host.sock`): change the password of the session's own account. A
/// password change ends the account's sessions (X-02 §4.1).
pub struct AccountSetPassword;

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountSetPasswordParams {
    pub session: SessionHandle,
    pub current: Password,
    pub new: Password,
}

impl Call for AccountSetPassword {
    const METHOD: &'static str = "account.set_password";
    type Params = AccountSetPasswordParams;
    type Result = Empty;
}

/// `account.reset_password` (`host.sock`, no session): the way back from a forgotten password.
/// macOS: the signed `agentic-org` peer and OS user presence. Linux: a root peer
/// (`sudo agentic-org account reset-password`). Audited; it ends all of that account's sessions
/// and clears the login's lockout.
pub struct AccountResetPassword;

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountResetPasswordParams {
    pub login: String,
    pub new: Password,
}

impl Call for AccountResetPassword {
    const METHOD: &'static str = "account.reset_password";
    type Params = AccountResetPasswordParams;
    type Result = Empty;
}

/// `account.create` (`host.sock`; admin, the signed `agentic-org` peer and OS user presence on
/// macOS; journaled): add an account. `admin: false` with `owner_of` makes an owner without the
/// create right (dogfood 1.6, SRV-24). The actor is always the session, never a param. Logins are
/// normalized (NFC, case-folded) and unique.
pub struct AccountCreate;

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountCreateParams {
    pub session: SessionHandle,
    pub operation_id: String,
    pub login: String,
    pub password: Password,
    pub admin: bool,
    /// The organization the new account owns (it links to that organization's owner member).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_of: Option<String>,
}

/// `account.create`'s completed result: the new account's id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountCreated {
    pub account: String,
}

impl Call for AccountCreate {
    const METHOD: &'static str = "account.create";
    type Params = AccountCreateParams;
    type Result = HostOpResult<AccountCreated>;
}

/// `recovery.release` (`host.sock`; admin, the signed `agentic-org` peer, OS user presence, on
/// the HQ machine): show the organization's recovery code **once** and mark it `delivered`.
/// Before acknowledgement, showing it again needs `redeliver: true`, which is audited without the
/// code and emits `organization.changed`. The CLI prints it only to a TTY.
pub struct RecoveryRelease;

#[derive(Debug, Serialize, Deserialize)]
pub struct RecoveryReleaseParams {
    pub session: SessionHandle,
    pub org_id: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub redeliver: bool,
}

/// `recovery.release`'s result. Not `Clone`: the code is a secret.
#[derive(Debug, Serialize, Deserialize)]
pub struct RecoveryReleaseResult {
    pub code: RecoveryCode,
}

impl Call for RecoveryRelease {
    const METHOD: &'static str = "recovery.release";
    type Params = RecoveryReleaseParams;
    type Result = RecoveryReleaseResult;
}

/// `recovery.acknowledge` (`host.sock`; as `recovery.release`): erase the staged code and emit
/// `organization.changed { recovery: acknowledged }`.
pub struct RecoveryAcknowledge;

#[derive(Debug, Serialize, Deserialize)]
pub struct RecoveryAcknowledgeParams {
    pub session: SessionHandle,
    pub org_id: String,
}

impl Call for RecoveryAcknowledge {
    const METHOD: &'static str = "recovery.acknowledge";
    type Params = RecoveryAcknowledgeParams;
    type Result = Empty;
}

/// `subscribe` (control connection): follow the host's events ([`crate::event`]) on this
/// connection. The control connection is its authority.
pub struct Subscribe;

impl Call for Subscribe {
    const METHOD: &'static str = "subscribe";
    type Params = Empty;
    type Result = Empty;
}

/// `bootstrap` (bootstrap-mode host only, on its connection to setup's listener): create the
/// registry and the first account, and answer with a session for it. The host stores
/// `operation_id` and `hq_operation_id`; the same `operation_id` on a non-empty registry replays
/// (a fresh session, after the password is checked), and a different one aborts. Under `https` a
/// first bootstrap needs `tls`; a replay may omit it and compares a given pair by fingerprint.
pub struct Bootstrap;

#[derive(Debug, Serialize, Deserialize)]
pub struct BootstrapParams {
    pub operation_id: String,
    pub hq_operation_id: String,
    pub login: String,
    pub password: Password,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<TlsPair>,
}

/// A TLS pair, as `bootstrap` carries it. Not `Clone`: the key is a secret.
#[derive(Debug, Serialize, Deserialize)]
pub struct TlsPair {
    /// The certificate chain, PEM.
    pub cert: String,
    pub key: TlsPrivateKey,
}

/// `bootstrap`'s result: a session for the first account.
#[derive(Debug, Serialize, Deserialize)]
pub struct BootstrapResult {
    pub session: SessionHandle,
}

impl Call for Bootstrap {
    const METHOD: &'static str = "bootstrap";
    type Params = BootstrapParams;
    type Result = BootstrapResult;
}

/// `exit` (bootstrap-mode host only): setup is done with the bootstrap-mode host, which exits and
/// releases `host.lock`. Never served on `host.sock`.
pub struct Exit;

impl Call for Exit {
    const METHOD: &'static str = "exit";
    type Params = Empty;
    type Result = Empty;
}
