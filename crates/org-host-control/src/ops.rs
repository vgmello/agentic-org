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

use std::fmt;

use org_proto::{Call, Empty, Grant, ListOfficesResult};
use serde::{Deserialize, Serialize};

use crate::listen::{
    Hsts, InvalidListenConfig, ListenFailure, ListenMode, ListenOrigin, Uncovered,
};
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
    /// A state this build doesn't know. It is never sent: serializing it fails.
    #[serde(other, skip_serializing)]
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
/// `sendmsg` with a 1-byte marker ([`HOST_SOCK_MARKER`]) and the descriptor. The host caps
/// channels per office and keeps one slot for local channels.
pub struct OfficeOpenChannel;

/// The byte that precedes the descriptor on `host.sock` (X-02 §5.2): the one data byte of the
/// `sendmsg` that follows an `office.open_channel` response line and carries the channel's fd. The
/// CLI reads that connection only with `recvmsg` until it arrives. Defined here, not taken from
/// `org-office-launch`'s `FD_MARKER` (the worker handoff's), so this crate's dependencies stay
/// `org-proto`, serde and `sha2`.
pub const HOST_SOCK_MARKER: u8 = 0x00;

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
/// `not_found` for an unknown op or another account's. The result and the refusal are the op's
/// own (an `org_proto::ApiError`, or a [`TlsRefusal`] for `tls.set`), so both decode as JSON.
pub struct OperationGet;

#[derive(Debug, Serialize, Deserialize)]
pub struct OperationGetParams {
    pub session: SessionHandle,
    pub operation_id: String,
}

impl Call for OperationGet {
    const METHOD: &'static str = "operation.get";
    type Params = OperationGetParams;
    type Result = HostOpResult<serde_json::Value, serde_json::Value>;
}

/// `tls.fetch` (**control connection only**; refused on `host.sock`): what the gateway serves, at
/// start and after each `tls.renew` event. `mode`, `origin` and `hsts` come from the **per-bind
/// snapshot** ([`crate::listen::ListenConfig`]) of the listener this gateway was given, never a live re-read;
/// `cert` and `key` are the stored pair. Before answering `ok` the host re-runs its pre-spawn
/// check on that snapshot ([`ListenOrigin`]'s two steps, and in `https` coverage of the
/// snapshot's origin by the stored pair), in the pre-spawn check's order, and answers
/// `unavailable` with the reason on a failure. A correct host never does: the form is defence in
/// depth. If the stored pair can't be read (the credential item was lost), the host answers the
/// plain error `storage_error` instead and reports `tls_missing` in status; the gateway treats
/// that error as it treats `unavailable`.
///
/// The gateway, at start: on `ok` it checks `origin` against `mode` and its listener with
/// [`ListenOrigin::check`] and refuses to start on a failure; on `unavailable` it refuses to
/// start. After `tls.renew`: `ok` with the same `mode`, `origin` and `hsts` swaps the pair for new
/// handshakes; `ok` with different ones is a host bug, and it exits without swapping;
/// `unavailable` keeps the current pair.
pub struct TlsFetch;

/// `tls.fetch`'s tagged result:
/// - `{"ok": {"mode": "https", "cert", "key", "origin", "hsts"}}`;
/// - `{"ok": {"mode": "http_loopback", "origin", "hsts"}}`, with no certificate fields;
/// - `{"unavailable": <ListenFailure>}`: exactly a [`ListenFailure`].
///
/// Not `Clone`: the key is a secret. The `ok` object decodes as one flat object, not through
/// serde's internally tagged enums, so this decode adds no buffer of its own between the JSON and
/// [`TlsPrivateKey`]. The JSON itself is the envelope's raw result, a `serde_json::Value`: built
/// with `serde_json::to_value` on the host's side and parsed from the response line on the
/// gateway's, so the key also sits there, unzeroed, on both sides (§6.5's raw-JSON exception).
/// `https` needs both `cert` and `key`, non-empty strings; `http_loopback` takes neither, so a
/// `cert` or `key` there is refused whatever its value, `null` included; any other `mode` is
/// refused. `origin` must be canonical; its fit to the listener is the gateway's check, not the
/// decoder's.
#[derive(Debug)]
pub enum TlsFetchResult {
    Https {
        /// The certificate chain, PEM.
        cert: String,
        key: TlsPrivateKey,
        origin: ListenOrigin,
        hsts: Hsts,
    },
    /// Plain HTTP on `127.0.0.1` or `::1`: no pair.
    HttpLoopback { origin: ListenOrigin, hsts: Hsts },
    /// The host can't serve the snapshot (a host bug).
    Unavailable(ListenFailure),
}

impl TlsFetchResult {
    /// The snapshot's `mode`, `origin` and `hsts`, for `ok`: what the gateway compares after
    /// `tls.renew`. `None` for `unavailable`.
    pub fn listen(&self) -> Option<(ListenMode, &ListenOrigin, Hsts)> {
        match self {
            TlsFetchResult::Https { origin, hsts, .. } => Some((ListenMode::Https, origin, *hsts)),
            TlsFetchResult::HttpLoopback { origin, hsts } => {
                Some((ListenMode::HttpLoopback, origin, *hsts))
            }
            TlsFetchResult::Unavailable(_) => None,
        }
    }
}

#[derive(Serialize)]
struct TlsFetchRef<'a> {
    mode: ListenMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    cert: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<&'a TlsPrivateKey>,
    origin: &'a ListenOrigin,
    hsts: Hsts,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum TlsFetchOut<'a> {
    Ok(TlsFetchRef<'a>),
    Unavailable(&'a ListenFailure),
}

/// `cert` and `key` are present-means-`Some`, like `Request.principal`: the outer `Option` says
/// whether the key is there at all, so `http_loopback` refuses `"cert": null` as it refuses any
/// other value, and the inner one carries a `null`, which `https` refuses too.
#[derive(Deserialize)]
struct RawTlsFetch {
    mode: ListenMode,
    #[serde(default, deserialize_with = "present")]
    cert: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    key: Option<Option<TlsPrivateKey>>,
    origin: ListenOrigin,
    hsts: Hsts,
}

/// Decodes a present field as `Some`, even when its value is `null`. An absent one is `None`
/// through `default`.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum TlsFetchIn {
    Ok(RawTlsFetch),
    Unavailable(ListenFailure),
}

impl Serialize for TlsFetchResult {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            TlsFetchResult::Https {
                cert,
                key,
                origin,
                hsts,
            } => TlsFetchOut::Ok(TlsFetchRef {
                mode: ListenMode::Https,
                cert: Some(cert),
                key: Some(key),
                origin,
                hsts: *hsts,
            }),
            TlsFetchResult::HttpLoopback { origin, hsts } => TlsFetchOut::Ok(TlsFetchRef {
                mode: ListenMode::HttpLoopback,
                cert: None,
                key: None,
                origin,
                hsts: *hsts,
            }),
            TlsFetchResult::Unavailable(failed) => TlsFetchOut::Unavailable(failed),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for TlsFetchResult {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let raw = match TlsFetchIn::deserialize(d)? {
            TlsFetchIn::Ok(raw) => raw,
            TlsFetchIn::Unavailable(failed) => return Ok(TlsFetchResult::Unavailable(failed)),
        };
        let RawTlsFetch {
            mode,
            cert,
            key,
            origin,
            hsts,
        } = raw;
        match (mode, cert, key) {
            // `TlsPrivateKey` already refuses an empty key; the key test keeps the rule local.
            (ListenMode::Https, Some(Some(cert)), Some(Some(key)))
                if !cert.is_empty() && !key.expose().is_empty() =>
            {
                Ok(TlsFetchResult::Https {
                    cert,
                    key,
                    origin,
                    hsts,
                })
            }
            (ListenMode::HttpLoopback, None, None) => {
                Ok(TlsFetchResult::HttpLoopback { origin, hsts })
            }
            (ListenMode::Https, _, _) => {
                Err(D::Error::custom("https needs a non-empty cert and key"))
            }
            (ListenMode::HttpLoopback, _, _) => {
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
/// a TLS pair. First, as for every journaled op, the journal lookup: a known `operation_id` with
/// a stored result replays it before any check (a stored success replays even if setup has since
/// rewritten the origin; [`crate::journal`]). Otherwise, before it stores the pair and emits
/// `tls.renew`, the host checks, in this order, and answers the first failure (X-02 §6.2):
/// 1. the stored listener fields (`address`, `port`, `scheme`) and `origin`
///    ([`TlsRefusal::OriginInvalid`]);
/// 2. the stored `hsts` ([`TlsRefusal::HstsInvalid`]);
/// 3. that the certificate and the key parse ([`TlsRefusal::TlsInvalid`]);
/// 4. that the key matches the certificate ([`TlsRefusal::TlsKeyMismatch`]);
/// 5. that the certificate is valid now ([`TlsRefusal::TlsNotValid`]);
/// 6. in `https`, that it covers every `https` origin among the stored one and the running one
///    ([`TlsRefusal::TlsNameMismatch`]; Certificate coverage).
///
/// It never changes the scheme. Its journal fingerprint is SHA-256 of the certificate DER plus
/// SHA-256 of the key DER, computed from the secret type (`TlsSetParams`'s
/// [`crate::journal::Journaled`] implementation); the stored result echoes neither.
///
/// A failed check answers `{"status": "rejected", "refusal": <TlsRefusal>}` and stores no pair;
/// only [`TlsRefusal::TlsKeyMismatch`] is journaled ([`TlsRefusal::is_stored`]).
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
    type Result = HostOpResult<Empty, TlsRefusal>;
}

/// Why `tls.set` or `bootstrap` refused a TLS pair (X-02 §6.2, §6.3), with its wire form. These
/// aren't §9 error codes, and none carries a message.
///
/// A code this build doesn't know decodes as [`TlsRefusal::Unknown`]. It is never sent:
/// serializing it fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TlsRefusal {
    /// `{"code": "tls_name_mismatch", "uncovered": [...]}`: in `https`, the certificate doesn't
    /// cover these origins (the stored one first, then the running one).
    TlsNameMismatch { uncovered: Uncovered },
    /// `{"code": "tls_invalid"}`: the certificate or the key doesn't parse. Never stored: an
    /// unparseable pair has no fingerprint.
    TlsInvalid,
    /// `{"code": "tls_key_mismatch"}`: the key doesn't match the certificate.
    TlsKeyMismatch,
    /// `{"code": "tls_not_valid", "not_before", "not_after"}`: the certificate isn't valid now.
    /// Both are the certificate's validity bounds, as [`UtcTimestamp`]s.
    TlsNotValid {
        not_before: UtcTimestamp,
        not_after: UtcTimestamp,
    },
    /// `{"code": "origin_invalid"}`: the stored listen configuration's `origin` is invalid, or an
    /// `address`, `port` or `scheme` it is checked against is missing or malformed.
    OriginInvalid,
    /// `{"code": "hsts_invalid"}`: the stored listen configuration's `hsts` is invalid.
    HstsInvalid,
    #[serde(other, skip_serializing)]
    Unknown,
}

/// A certificate validity bound, as `tls_not_valid` carries it: RFC 3339 in one profile,
/// `YYYY-MM-DDTHH:MM:SSZ`. UTC with an uppercase `T` and `Z`, no fractional seconds and no offset
/// (`+00:00` is refused), and a real date and time: month 1-12, a day that exists in that month
/// (leap years by the Gregorian rule), hour 0-23, minute and second 0-59 (no leap second).
/// Decoding refuses any other form; it is never rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct UtcTimestamp(String);

impl UtcTimestamp {
    /// Accept `text` only in the profile above.
    pub fn parse(text: &str) -> Result<UtcTimestamp, InvalidTimestamp> {
        let b = text.as_bytes();
        if b.len() != 20 {
            return Err(InvalidTimestamp);
        }
        for (i, want) in [
            (4, b'-'),
            (7, b'-'),
            (10, b'T'),
            (13, b':'),
            (16, b':'),
            (19, b'Z'),
        ] {
            if b[i] != want {
                return Err(InvalidTimestamp);
            }
        }
        let number = |from: usize, to: usize| -> Result<u32, InvalidTimestamp> {
            b[from..to].iter().try_fold(0u32, |n, &c| {
                if c.is_ascii_digit() {
                    Ok(n * 10 + u32::from(c - b'0'))
                } else {
                    Err(InvalidTimestamp)
                }
            })
        };
        let year = number(0, 4)?;
        let month = number(5, 7)?;
        let day = number(8, 10)?;
        let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return Err(InvalidTimestamp),
        };
        if !(1..=days).contains(&day) || hour > 23 || minute > 59 || second > 59 {
            return Err(InvalidTimestamp);
        }
        Ok(UtcTimestamp(text.to_owned()))
    }

    /// The timestamp, as it is on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for UtcTimestamp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        UtcTimestamp::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A timestamp outside [`UtcTimestamp`]'s profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTimestamp;

impl fmt::Display for InvalidTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a timestamp must be a real UTC time as YYYY-MM-DDTHH:MM:SSZ")
    }
}

impl std::error::Error for InvalidTimestamp {}

impl TlsRefusal {
    /// Whether the host journals this refusal from `op`, so a retry with the same `operation_id`
    /// replays it (X-02 §6.3):
    /// - `tls.set`: only `tls_key_mismatch` is stored. It depends on the pair alone, which the
    ///   fingerprint covers. `tls_invalid` isn't: an unparseable pair has no fingerprint. The
    ///   others depend on state outside the op that is expected to change (the stored and running
    ///   origins, the clock, the stored configuration), so a retry is checked again.
    ///   [`TlsRefusal::Unknown`] is never stored.
    /// - `bootstrap`: never. The host runs every bootstrap check before it creates the registry,
    ///   so a refused bootstrap leaves no record, and a re-run with the same `operation_id` is a
    ///   first bootstrap.
    pub fn is_stored(&self, op: TlsInstallOp) -> bool {
        match op {
            TlsInstallOp::TlsSet => matches!(self, TlsRefusal::TlsKeyMismatch),
            TlsInstallOp::Bootstrap => false,
        }
    }
}

impl From<InvalidListenConfig> for TlsRefusal {
    /// The invalid-configuration rule's refusal, as `bootstrap` and `tls.set` answer it.
    fn from(invalid: InvalidListenConfig) -> Self {
        match invalid {
            InvalidListenConfig::Listen(_) | InvalidListenConfig::Origin(_) => {
                TlsRefusal::OriginInvalid
            }
            InvalidListenConfig::Hsts => TlsRefusal::HstsInvalid,
        }
    }
}

/// The ops that install a TLS pair and can refuse it with a [`TlsRefusal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TlsInstallOp {
    /// `tls.set`.
    TlsSet,
    /// `bootstrap` (its `tls`, and its invalid-configuration check with or without `tls`).
    Bootstrap,
}

impl TlsInstallOp {
    /// The op's method.
    pub fn method(self) -> &'static str {
        match self {
            TlsInstallOp::TlsSet => TlsSet::METHOD,
            TlsInstallOp::Bootstrap => Bootstrap::METHOD,
        }
    }
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
/// normalized (NFC, case-folded) and unique. Its fingerprint leaves out `password`; a replay whose
/// password doesn't match the row's create-time verifier is `operation_reused`
/// ([`crate::journal`]).
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
/// `operation_id` and `hq_operation_id`; on a non-empty registry a different `operation_id` is
/// refused with `already_bootstrapped`, and the same one replays with a fresh session until
/// bootstrap has finished, then gets `bootstrap_finished`. Under
/// `https` a first bootstrap needs `tls`; a replay may omit it and compares a given pair by
/// fingerprint.
///
/// The journal lookup by `operation_id` comes first, as for every journaled op. **A replay then
/// checks the `login` and password as a sign-in** (X-02 §6.2 step 3, §6.3), never against a
/// fingerprint, which holds no secret: a `login` and password that don't sign in are the plain
/// error `not_authorized`, counted toward lockout as `account.verify` is, and setup answers it by
/// asking for the password again. Then the fingerprint (the normalized `login` and
/// `hq_operation_id`, [`crate::journal::Journaled`]): another `hq_operation_id` is the plain
/// error `operation_reused`. Then the pair: another one is `tls_mismatch`.
///
/// Its result is a [`BootstrapOutcome`] ([`HostOpResult`]'s wire form, **never `accepted`**):
/// `completed` with [`BootstrapResult`], or `rejected` with a [`BootstrapRefusal`], in the order
/// that type states.
/// On an empty registry the host checks the stored listen configuration with or without `tls`
/// (`origin_invalid`, `hsts_invalid`), and a first `tls` gets `tls.set`'s checks in `tls.set`'s
/// order (parse, key match, validity, coverage), all before it creates the registry. **No
/// bootstrap refusal is ever stored**: a re-run with the same `operation_id` after a refusal on
/// an empty registry is a first bootstrap.
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

/// `bootstrap`'s completed result: a session for the first account.
#[derive(Debug, Serialize, Deserialize)]
pub struct BootstrapResult {
    pub session: SessionHandle,
}

impl Call for Bootstrap {
    const METHOD: &'static str = "bootstrap";
    type Params = BootstrapParams;
    type Result = BootstrapOutcome;
}

/// `bootstrap`'s result (X-02 §6.3): [`HostOpResult`]'s wire form, tagged by `status`, without
/// `accepted`, which bootstrap never answers. So `{"status": "accepted"}` doesn't decode, and
/// neither does any other status: bootstrap's result shapes are fixed, so there is no `Unknown`.
/// Not `Clone`: the session is a secret.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BootstrapOutcome {
    Completed { result: BootstrapResult },
    Rejected { refusal: BootstrapRefusal },
}

/// Why `bootstrap` refused (X-02 §6.3, "Result shapes fixed by move 1"): a [`TlsRefusal`], or
/// one of bootstrap's own four, `{"code": "tls_required"}`, `{"code": "tls_mismatch"}`,
/// `{"code": "already_bootstrapped"}` and `{"code": "bootstrap_finished"}`. None is ever stored
/// ([`BootstrapRefusal::is_stored`]).
///
/// **The host decides in this order**, and answers the first that applies. Steps 1 to 3 act on
/// the journal lookup's answer (no record, a finished one, or a replay), and that lookup comes
/// first, as for every journaled op:
/// 1. on a non-empty registry, an `operation_id` other than the recorded bootstrap's (or a
///    registry setup has no record of) is [`BootstrapRefusal::AlreadyBootstrapped`], whatever
///    else holds, even after bootstrap has finished;
/// 2. the recorded `operation_id` gets [`BootstrapRefusal::BootstrapFinished`] once bootstrap has
///    finished;
/// 3. otherwise the recorded `operation_id` replays, checked in this order: the `login` and
///    password as a sign-in (a `login` and password that don't sign in are the plain error
///    `not_authorized`), then the fingerprint (the normalized `login` and `hq_operation_id`:
///    another `hq_operation_id` is the plain error `operation_reused`), then the pair (one other
///    than the stored one is [`BootstrapRefusal::TlsMismatch`]). A replay doesn't re-check the
///    listen configuration;
/// 4. on an empty registry: the stored listener fields and `origin`
///    ([`TlsRefusal::OriginInvalid`]; a missing listen configuration is one), then `hsts`
///    ([`TlsRefusal::HstsInvalid`]), then
///    [`BootstrapRefusal::TlsRequired`], then `tls.set`'s order for the pair (parse, key match,
///    validity, coverage). Every refusal here but `tls_required` is a [`BootstrapRefusal::Tls`].
///
/// It is one flat object tagged by `code`, like [`TlsRefusal`]. A code neither type knows decodes
/// as `Tls(TlsRefusal::Unknown)`; it is never sent: serializing it fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapRefusal {
    /// `{"code": "tls_required"}`: a first bootstrap under `https` without `tls`.
    TlsRequired,
    /// `{"code": "tls_mismatch"}`: a replay with a pair other than the stored one (compared by
    /// fingerprint). Setup tells the operator to re-run `setup --resume` with the original pair,
    /// or without `tls` and then `agentic-org tls renew`.
    TlsMismatch,
    /// `{"code": "already_bootstrapped"}`: a different `operation_id` on a non-empty registry
    /// (whether or not bootstrap has finished), or a registry setup has no record of. Setup
    /// aborts and tells the operator to resume by signing in (`agentic-org setup --resume`) or to
    /// repair or uninstall.
    AlreadyBootstrapped,
    /// `{"code": "bootstrap_finished"}`: the recorded bootstrap's own `operation_id`, after the
    /// registry records that bootstrap has finished (setup's `exit` followed the HQ link), so
    /// bootstrap mode is refused. A distinct result, not an abort: setup goes on to start the
    /// service host (X-02 §6.2 step 5).
    BootstrapFinished,
    /// The listen configuration or the pair failed one of `tls.set`'s checks.
    Tls(TlsRefusal),
}

impl BootstrapRefusal {
    /// Always `false`. On an empty registry the host runs every check before it creates the
    /// registry, so a refused bootstrap leaves no record ([`TlsRefusal::is_stored`] with
    /// [`TlsInstallOp::Bootstrap`]); on a non-empty one, `tls_mismatch`, `already_bootstrapped`
    /// and `bootstrap_finished` answer the request without journaling it.
    pub fn is_stored(&self) -> bool {
        match self {
            BootstrapRefusal::TlsRequired
            | BootstrapRefusal::TlsMismatch
            | BootstrapRefusal::AlreadyBootstrapped
            | BootstrapRefusal::BootstrapFinished => false,
            BootstrapRefusal::Tls(refusal) => refusal.is_stored(TlsInstallOp::Bootstrap),
        }
    }
}

/// Bootstrap's own codes.
#[derive(Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
enum BootstrapOwnRefusal {
    TlsRequired,
    TlsMismatch,
    AlreadyBootstrapped,
    BootstrapFinished,
}

/// Bootstrap's own codes first: [`TlsRefusal`] would take any other code as its `Unknown`.
#[derive(Deserialize)]
#[serde(untagged)]
enum BootstrapRefusalWire {
    Own(BootstrapOwnRefusal),
    Tls(TlsRefusal),
}

impl Serialize for BootstrapRefusal {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            BootstrapRefusal::TlsRequired => BootstrapOwnRefusal::TlsRequired.serialize(s),
            BootstrapRefusal::TlsMismatch => BootstrapOwnRefusal::TlsMismatch.serialize(s),
            BootstrapRefusal::AlreadyBootstrapped => {
                BootstrapOwnRefusal::AlreadyBootstrapped.serialize(s)
            }
            BootstrapRefusal::BootstrapFinished => {
                BootstrapOwnRefusal::BootstrapFinished.serialize(s)
            }
            BootstrapRefusal::Tls(refusal) => refusal.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for BootstrapRefusal {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match BootstrapRefusalWire::deserialize(d)? {
            BootstrapRefusalWire::Own(BootstrapOwnRefusal::TlsRequired) => {
                BootstrapRefusal::TlsRequired
            }
            BootstrapRefusalWire::Own(BootstrapOwnRefusal::TlsMismatch) => {
                BootstrapRefusal::TlsMismatch
            }
            BootstrapRefusalWire::Own(BootstrapOwnRefusal::AlreadyBootstrapped) => {
                BootstrapRefusal::AlreadyBootstrapped
            }
            BootstrapRefusalWire::Own(BootstrapOwnRefusal::BootstrapFinished) => {
                BootstrapRefusal::BootstrapFinished
            }
            BootstrapRefusalWire::Tls(refusal) => BootstrapRefusal::Tls(refusal),
        })
    }
}

/// `exit` (bootstrap-mode host only): setup is done with the bootstrap-mode host, which exits and
/// releases `host.lock`. Never served on `host.sock`.
pub struct Exit;

impl Call for Exit {
    const METHOD: &'static str = "exit";
    type Params = Empty;
    type Result = Empty;
}
