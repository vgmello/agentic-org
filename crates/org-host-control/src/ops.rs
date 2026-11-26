//! The host ops (X-02 §6.1, §6.2), each a [`Call`]. Unless a doc says otherwise, an op is served
//! on both the gateway's control connection and `host.sock`.
//!
//! - **Open** (no session): `hello`, `account.verify`, and on `host.sock` only
//!   `cli.approval.create` / `cli.approval.poll`.
//! - **Session ops**: everything else here takes the `session` from `account.verify`. A session
//!   ends on `session.end`, a password change, unlinking, `account.revoke`, its TTL (12 h absolute,
//!   30 min idle by default), or EOF of the connection it was issued on.
//! - Refusals: `not_authorized` for a failed sign-in, a dead or unlinked session, or a peer not
//!   allowed the op; `not_found` for an organization or office the session isn't linked to;
//!   `unavailable` when the sign-in pool is full; `operation_reused` per [`crate::journal`].

use std::fmt;
use std::fmt::Write as _;

use org_proto::{Call, Empty, Grant, ListOfficesResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::listen::{
    Hsts, InvalidListenConfig, ListenFailure, ListenMode, ListenOrigin, Uncovered,
};
use crate::pem::{self, InvalidPem};
use crate::{
    HostOpResult, InvitationTokenSecret, OneTimeLinkUrl, Password, RecoveryCode, SessionHandle,
    TlsPrivateKey,
};

mod tls;

pub use tls::{
    InvalidTimestamp, TlsFetch, TlsFetchResult, TlsPairFingerprint, TlsRefusal, TlsSet,
    TlsSetParams, UtcTimestamp, WebServing, WebServingParams,
};

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

/// An organization the account is linked to: the employee it acts as there and its offices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedOrg {
    pub org_id: String,
    /// The organization's name, when the answer carries it (`account.verify` does). An
    /// answer without it reads as an empty name; an empty name is not written.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub employee_id: String,
    pub office_ids: Vec<String>,
}

impl Call for AccountVerify {
    const METHOD: &'static str = "account.verify";
    type Params = AccountVerifyParams;
    type Result = AccountVerifyResult;
}

/// `session.local` (open on `host.sock` only, plain HTTP only, local authless mode only):
/// issues a connection-bound native session for the implicit local owner account while
/// authentication is disabled (`ADR-0034`). Refused with `not_authorized` on the web
/// connection, on HTTPS instances, or whenever authentication is enabled.
pub struct SessionLocal;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionLocalParams {}

impl Call for SessionLocal {
    const METHOD: &'static str = "session.local";
    type Params = SessionLocalParams;
    type Result = AccountVerifyResult;
}

/// `org.create` (`host.sock` only; administrator session; journaled): admits an asynchronous
/// organization create or reports progress on `operation_id`, mirroring `organization.create`
/// without browser-session credentials.
pub struct OrgCreate;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrgCreateParams {
    pub session: SessionHandle,
    /// Canonical lowercase UUIDv7 minted by the CLI for this attempt.
    pub operation_id: String,
    pub plan: org_proto::selected_bootstrap::PlanV1,
}

impl Call for OrgCreate {
    const METHOD: &'static str = "org.create";
    type Params = OrgCreateParams;
    type Result = HostOpResult<crate::organization::OrganizationView>;
}

/// `org.lookup` (`host.sock` only; session): reports the state of one of the account's own
/// organization-create operations and resumes it if no creation thread is running.
pub struct OrgOperationLookup;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrgOperationLookupParams {
    pub session: SessionHandle,
    pub operation_id: String,
}

impl Call for OrgOperationLookup {
    const METHOD: &'static str = "org.lookup";
    type Params = OrgOperationLookupParams;
    type Result = HostOpResult<crate::organization::OrganizationView>;
}

/// `org.list` (`host.sock` only; session): lists organizations linked to the account and its
/// creates still in flight.
pub struct OrgList;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrgListParams {
    pub session: SessionHandle,
}

impl Call for OrgList {
    const METHOD: &'static str = "org.list";
    type Params = OrgListParams;
    type Result = crate::organization::ListResult;
}

/// `cli.approval.create` (open on `host.sock`): starts a browser-approved terminal sign-in
/// (FC-173 / SIGN-40 / SES-8). Returns a one-time approval URL (10-minute TTL) whose secret
/// travels only after `#`, bound to this `host.sock` connection.
pub struct CliApprovalCreate;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliApprovalCreateParams {}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliApprovalCreateResult {
    pub approval_id: String,
    pub url: OneTimeLinkUrl,
    pub expires_at_ms: i64,
}

impl Call for CliApprovalCreate {
    const METHOD: &'static str = "cli.approval.create";
    type Params = CliApprovalCreateParams;
    type Result = CliApprovalCreateResult;
}

/// `cli.approval.poll` (open on `host.sock`, same connection that created `approval_id`):
/// checks approval status; once approved in a signed-in browser, issues a connection-bound
/// native session (`SES-8`) on this connection without a terminal password.
pub struct CliApprovalPoll;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliApprovalPollParams {
    pub approval_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum CliApprovalPollResult {
    Pending,
    Approved {
        session: SessionHandle,
        account: String,
        admin: bool,
        orgs: Vec<LinkedOrg>,
    },
    Declined,
    Expired,
}

impl Call for CliApprovalPoll {
    const METHOD: &'static str = "cli.approval.poll";
    type Params = CliApprovalPollParams;
    type Result = CliApprovalPollResult;
}

/// The params of an op that names only its session.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionParams {
    pub session: SessionHandle,
}

/// Live account/session facts, on its issuing control connection. Inspection
/// rechecks protected registry authority; it never recreates an expired session.
pub struct SessionInspect;
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionInspectResult {
    pub account: String,
    pub login: String,
    pub admin: bool,
    pub orgs: Vec<LinkedOrg>,
    pub server_time_ms: u64,
    pub idle_expires_at_ms: u64,
}
impl Call for SessionInspect {
    const METHOD: &'static str = "session.inspect";
    type Params = SessionParams;
    type Result = SessionInspectResult;
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

/// Lifecycle state returned by `office.join` / `office.re_enroll` (`TASK-52`, `doc-67`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfficeJoinState {
    JoinPending,
    Joined,
}

/// Result of `office.join` or `office.re_enroll` (`TASK-52`, `doc-67`, D25).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeJoinResult {
    pub office_id: String,
    pub org_id: String,
    pub org_name: String,
    pub office_name: String,
    pub hq_office_id: String,
    pub state: OfficeJoinState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_code: Option<String>,
}

/// `office.join` (host.sock, admin/owner session; journaled): enrolls this host as a satellite
/// office using a secret one-time invitation token (`doc-67`, `TASK-52`).
pub struct OfficeJoin;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeJoinParams {
    /// Required when the host has an administrator account; omitted on a fresh satellite-only
    /// instance where the verified local peer joins with explicit consent (`TASK-52` H-3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionHandle>,
    pub operation_id: String,
    pub invitation: InvitationTokenSecret,
    /// Explicit destination consent (maps to interactive prompt or `--yes`).
    pub consent: bool,
}

impl Call for OfficeJoin {
    const METHOD: &'static str = "office.join";
    type Params = OfficeJoinParams;
    type Result = HostOpResult<OfficeJoinResult>;
}

/// `office.remove_pending` (host.sock, admin/owner session; journaled): cancels and cleans up
/// a pending satellite join before enrollment completes (`doc-67`, `TASK-52`).
pub struct OfficeRemovePending;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeRemovePendingParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionHandle>,
    pub operation_id: String,
    pub office_id: String,
}

impl Call for OfficeRemovePending {
    const METHOD: &'static str = "office.remove_pending";
    type Params = OfficeRemovePendingParams;
    type Result = HostOpResult<Empty>;
}

/// `office.re_enroll` (host.sock, admin session; journaled): one-command re-enrollment of
/// an existing satellite onto a replacement HQ after R1 Git import (D25, `TASK-52`).
pub struct OfficeReEnroll;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeReEnrollParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionHandle>,
    pub operation_id: String,
    pub office_id: String,
    pub invitation: InvitationTokenSecret,
    pub consent: bool,
}

impl Call for OfficeReEnroll {
    const METHOD: &'static str = "office.re_enroll";
    type Params = OfficeReEnrollParams;
    type Result = HostOpResult<OfficeJoinResult>;
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

/// Local descriptor ownership receipt, on the connection that opened it. A
/// receipt cannot authorize another call or nominate an office/channel.
pub struct FdsReceived;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FdsReceivedParams {
    pub of: String,
}
impl Call for FdsReceived {
    const METHOD: &'static str = "fds.received";
    type Params = FdsReceivedParams;
    type Result = Empty;
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

/// `recovery.release` (`host.sock`; admin, the signed `agentic-org` peer, OS user presence, on
/// the HQ machine): show the organization's recovery code **once** and mark it `delivered`.
/// Before acknowledgement, showing it again needs `redeliver: true`, which is audited without the
/// code and emits `organization.changed`. The CLI prints it only to a TTY.
pub struct RecoveryRelease;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

#[cfg(test)]
mod linked_org_tests;
