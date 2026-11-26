//! Wire contracts for external ("bring your own agent", BYOA) agent sign-in and the headless
//! `agent-os` lifecycle (`TASK-137`, `doc-102`).
//!
//! Two layers authenticate an external agent:
//!
//! 1. A durable **lease** approved by a person with authority over the agent
//!    (`member.external_approve`), bound to the Ed25519 public key the workspace generated at
//!    `agent-os login`. The office keeps one active lease per agent (managed *or* external).
//! 2. A disposable, RAM-only **fast token** minted by a 1-RTT Ed25519 challenge-response
//!    (`auth.challenge` + `auth.exchange`) and presented on `msg.bind`, so the hot path costs one
//!    constant-time comparison: no key store, no database read and no signature per command.
//!
//! The `auth.*` methods are answered only on the office socket (same-UID peers), before a
//! connection binds; host channels refuse them. The `member.external_*` methods are operator
//! calls: they need a verified grant (a person or the owner) and are refused to every agent.

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::calls::Call;
use crate::Empty;

/// Default external lease lifetime when `--ttl` is not given: 24 hours.
pub const DEFAULT_LEASE_TTL_SECS: u64 = 24 * 60 * 60;
/// Shortest external lease an operator may approve: 5 minutes.
pub const MIN_LEASE_TTL_SECS: u64 = 5 * 60;
/// Longest external lease an operator may approve: 7 days.
pub const MAX_LEASE_TTL_SECS: u64 = 7 * 24 * 60 * 60;
/// Lifetime of one fast session token (refreshed by challenge-response): 1 hour.
pub const FAST_TOKEN_TTL_SECS: u64 = 60 * 60;
/// Lifetime of a pending approval code: 10 minutes.
pub const APPROVAL_CODE_TTL_SECS: u64 = 10 * 60;
/// Wrong approval-code attempts after which a pending login is locked out.
pub const MAX_APPROVAL_ATTEMPTS: u32 = 3;
/// Lifetime of a challenge nonce: 60 seconds, single use.
pub const CHALLENGE_TTL_SECS: u64 = 60;
/// Longest workspace label (bytes) an agent may present.
pub const MAX_WORKSPACE_LABEL_BYTES: usize = 128;
/// Length of an Ed25519 public key in bytes.
pub const ED25519_PUBLIC_KEY_LEN: usize = 32;

/// What a challenge nonce may be spent on; part of the signed message so a nonce issued for an
/// exchange can never authorize a logout and vice versa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengePurpose {
    Exchange,
    Logout,
}

impl ChallengePurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exchange => "exchange",
            Self::Logout => "logout",
        }
    }
}

/// The exact bytes an external agent signs with its workspace key: a domain-separated,
/// NUL-delimited tuple binding the purpose, office, member, lease session and nonce. Each field
/// is refused upstream when it contains a NUL, so the encoding is unambiguous.
pub fn challenge_message(
    purpose: ChallengePurpose,
    office_id: &str,
    member_id: &str,
    session_id: &str,
    nonce: &str,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(96 + office_id.len() + member_id.len() + session_id.len());
    out.extend_from_slice(b"agentic-org/external-auth/v1\0");
    for part in [purpose.as_str(), office_id, member_id, session_id, nonce] {
        out.extend_from_slice(part.as_bytes());
        out.push(0);
    }
    out
}

/// How a bound `agent-os` connection authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SessionMode {
    /// An office-launched run: token + kernel peer-PID ancestry.
    Managed,
    /// A BYOA workspace: token + active lease + same-UID socket peer.
    External,
    #[serde(other)]
    Unknown,
}

/// Where a pending external login stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExternalLoginState {
    Pending,
    Approved,
    Expired,
    /// Too many wrong approval codes: the request is void; log in again.
    Locked,
    #[serde(other)]
    Unknown,
}

// ---------------------------------------------------------------------------
// auth.* (office socket only, unbound)
// ---------------------------------------------------------------------------

pub struct AuthLoginRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthLoginRequestParams {
    /// `@handle`, handle or employee ID of the agent to sign in as.
    pub member: String,
    /// The workspace's Ed25519 public key (32 bytes, standard base64).
    pub pubkey_ed25519_b64: String,
    /// A short human label for the workspace (shown to the approver), e.g. `laptop:~/src/app`.
    pub workspace_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthLoginRequestResult {
    pub request_id: String,
    pub member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub office_id: String,
    /// The one-time code a person approves (`agentic-org agent approve <code>`).
    pub approval_code: String,
    pub expires_at: String,
    /// The copy-paste operator command that approves this login.
    pub approve_command: String,
    /// The web app path of the agent's profile, where the request can be approved.
    pub web_path: String,
}

impl Call for AuthLoginRequest {
    const METHOD: &'static str = "auth.login_request";
    type Params = AuthLoginRequestParams;
    type Result = AuthLoginRequestResult;
}

pub struct AuthLoginPoll;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthLoginPollParams {
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthLoginPollResult {
    pub state: ExternalLoginState,
    pub member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<String>,
}

impl Call for AuthLoginPoll {
    const METHOD: &'static str = "auth.login_poll";
    type Params = AuthLoginPollParams;
    type Result = AuthLoginPollResult;
}

pub struct AuthChallenge;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthChallengeParams {
    pub member: String,
    pub session_id: String,
    pub purpose: ChallengePurpose,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthChallengeResult {
    pub member_id: String,
    pub office_id: String,
    pub nonce: String,
    pub expires_at: String,
}

impl Call for AuthChallenge {
    const METHOD: &'static str = "auth.challenge";
    type Params = AuthChallengeParams;
    type Result = AuthChallengeResult;
}

/// A signed answer to an `auth.challenge` nonce (`auth.exchange`, `auth.logout`).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedChallengeParams {
    pub member: String,
    pub session_id: String,
    pub nonce: String,
    /// Ed25519 signature (64 bytes, standard base64) over [`challenge_message`].
    pub signature_ed25519_b64: String,
}

impl fmt::Debug for SignedChallengeParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignedChallengeParams")
            .field("member", &self.member)
            .field("session_id", &self.session_id)
            .field("nonce", &"<redacted>")
            .field("signature_ed25519_b64", &"<redacted>")
            .finish()
    }
}

pub struct AuthExchange;

/// Serde for a secret held in [`Zeroizing`] (a fast token): written as a plain JSON string, read
/// into memory that is wiped when it is dropped. `#[serde(with = "secret_string")]`.
pub mod secret_string {
    use serde::{Deserialize, Deserializer, Serializer};
    use zeroize::Zeroizing;

    pub fn serialize<S: Serializer>(value: &Zeroizing<String>, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_str(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<Zeroizing<String>, D::Error> {
        String::deserialize(input).map(Zeroizing::new)
    }
}

/// A fresh fast token, wiped from memory when dropped. Its `Debug` never shows the token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthExchangeResult {
    pub member_id: String,
    pub office_id: String,
    pub session_id: String,
    #[serde(with = "secret_string")]
    pub token: Zeroizing<String>,
    pub expires_at: String,
    pub lease_expires_at: String,
}

impl fmt::Debug for AuthExchangeResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthExchangeResult")
            .field("member_id", &self.member_id)
            .field("office_id", &self.office_id)
            .field("session_id", &self.session_id)
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("lease_expires_at", &self.lease_expires_at)
            .finish()
    }
}

impl Call for AuthExchange {
    const METHOD: &'static str = "auth.exchange";
    type Params = SignedChallengeParams;
    type Result = AuthExchangeResult;
}

pub struct AuthLogout;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthLogoutResult {
    pub member_id: String,
    pub session_id: String,
    pub revoked: bool,
}

impl Call for AuthLogout {
    const METHOD: &'static str = "auth.logout";
    type Params = SignedChallengeParams;
    type Result = AuthLogoutResult;
}

pub struct AuthWhoami;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthWhoamiResult {
    pub member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub office_id: String,
    pub mode: SessionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<String>,
}

impl Call for AuthWhoami {
    const METHOD: &'static str = "auth.whoami";
    type Params = Empty;
    type Result = AuthWhoamiResult;
}

// ---------------------------------------------------------------------------
// member.external_* (operator, grant-bound only)
// ---------------------------------------------------------------------------

/// One external lease as the operator sees it. Never carries a token or key material beyond the
/// public key's fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalSessionView {
    pub session_id: String,
    pub employee_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub workspace_label: String,
    /// First 16 hex digits of SHA-256 over the raw public key.
    pub pubkey_fingerprint: String,
    pub approved_by: String,
    pub approved_at: String,
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
    pub active: bool,
    /// The external lease this approval replaced, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_session_id: Option<String>,
    /// Whether approving parked a live managed session (`--takeover`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub took_over_managed: bool,
}

pub struct MemberExternalApprove;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberExternalApproveParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    /// The approval code `agent-os login` printed (`ABCD-EFGH`).
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u64>,
    /// Park a live managed turn first instead of refusing.
    #[serde(default)]
    pub takeover: bool,
}

impl Call for MemberExternalApprove {
    const METHOD: &'static str = "member.external_approve";
    type Params = MemberExternalApproveParams;
    type Result = ExternalSessionView;
}

/// One line of an agent's `external-history.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalHistoryEntry {
    pub at: String,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub struct MemberExternalSessions;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberExternalSessionsParams {
    /// `@handle`, handle or employee ID.
    pub employee_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberExternalSessionsResult {
    pub employee_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<ExternalSessionView>,
    #[serde(default)]
    pub history: Vec<ExternalHistoryEntry>,
}

impl Call for MemberExternalSessions {
    const METHOD: &'static str = "member.external_sessions";
    type Params = MemberExternalSessionsParams;
    type Result = MemberExternalSessionsResult;
}

pub struct MemberExternalRevoke;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberExternalRevokeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    /// `@handle`, handle or employee ID.
    pub employee_id: String,
    /// Revoke only this lease; refused as `not_found` when another lease is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberExternalRevokeResult {
    pub employee_id: String,
    pub revoked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

impl Call for MemberExternalRevoke {
    const METHOD: &'static str = "member.external_revoke";
    type Params = MemberExternalRevokeParams;
    type Result = MemberExternalRevokeResult;
}

#[cfg(test)]
#[path = "external_auth_tests.rs"]
mod tests;
