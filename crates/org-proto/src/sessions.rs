//! Wire contracts for C3 agent sessions, supervision, grants, and browser-driven CLI
//! provider enrollment (`TASK-42`, `ADR-0025`, `ADR-0030`, `ADR-0032`).
//!
//! All method names use the `member.*` or `provider.*` families (preserving X-02's
//! host-channel refusal of `session.`, `agent.`, `run.`, and `approval.` prefixes).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::calls::Call;

// ---------------------------------------------------------------------------
// Session lifecycle & inspection records
// ---------------------------------------------------------------------------

/// High-level lifecycle state of an agent's managed session on this office.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SessionState {
    Idle,
    Starting,
    Running,
    WaitingApproval,
    Stopping,
    Stopped,
    StallLocked,
    RecoveryRequired,
    #[serde(other)]
    Unknown,
}

/// Summary of an active or reaped background task / sub-agent tracked under a session (ADR-0025).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskView {
    pub task_id: String,
    pub kind: String,
    pub label: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
}

/// Pending approval or escalation awaiting owner decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApprovalView {
    pub approval_id: String,
    pub tool: String,
    pub escalation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub requested_at: String,
}

/// Cumulative usage metrics across the primary agent and its sub-agents (SUB-4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionUsageSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub subagent_invocations: u32,
}

/// Wire view of an agent's session state on this office.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionView {
    pub employee_id: String,
    pub office_id: String,
    pub state: SessionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_session_id: Option<String>,
    pub private_home: String,
    pub same_user_isolation_notice: String,
    pub permission_preset: String,
    pub sandbox_enabled: bool,
    pub consecutive_stalls: u32,
    pub stall_locked: bool,
    pub recovery_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reply: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub background_tasks: Vec<BackgroundTaskView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_approvals: Vec<PendingApprovalView>,
    #[serde(default)]
    pub usage: SessionUsageSummary,
}

/// Result of `member.session_inspect` (also backing `agent-os session inspect` / CLI-9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInspectResult {
    pub session: SessionView,
    pub accepted_commands_count: u64,
    pub spec_hash: String,
}

/// One effective permission grant or deny rule returned by `member.session_grants` (CLI-10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantRuleView {
    pub effect: String,
    pub tool: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

/// Result of `member.session_grants` (also backing `agent-os grant list` / CLI-10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionGrantsResult {
    pub employee_id: String,
    pub permission_preset: String,
    pub permission_mode: String,
    pub sandbox_enabled: bool,
    pub writable_roots: Vec<String>,
    pub read_only_dirs: Vec<String>,
    pub denied_write_patterns: Vec<String>,
    pub denied_read_patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    pub rules: Vec<GrantRuleView>,
}

// ---------------------------------------------------------------------------
// Session RPC calls
// ---------------------------------------------------------------------------

pub struct MemberSessionStart;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSessionStartParams {
    pub command_id: String,
    pub employee_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

impl Call for MemberSessionStart {
    const METHOD: &'static str = "member.session_start";
    type Params = MemberSessionStartParams;
    type Result = SessionView;
}

pub struct MemberSessionStop;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSessionStopParams {
    pub command_id: String,
    pub employee_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Call for MemberSessionStop {
    const METHOD: &'static str = "member.session_stop";
    type Params = MemberSessionStopParams;
    type Result = SessionView;
}

pub struct MemberSessionInspect;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSessionInspectParams {
    /// Target agent employee ID. When omitted on a bound agent connection, inspects self.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
}

impl Call for MemberSessionInspect {
    const METHOD: &'static str = "member.session_inspect";
    type Params = MemberSessionInspectParams;
    type Result = SessionInspectResult;
}

pub struct MemberSessionGrants;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSessionGrantsParams {
    /// Target agent employee ID. When omitted on a bound agent connection, inspects self.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
}

impl Call for MemberSessionGrants {
    const METHOD: &'static str = "member.session_grants";
    type Params = MemberSessionGrantsParams;
    type Result = SessionGrantsResult;
}

pub struct MemberSessionApprovalDecide;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSessionApprovalDecideParams {
    pub command_id: String,
    pub employee_id: String,
    pub approval_id: String,
    /// One of `"allow_once"`, `"allow_always"`, `"deny"`.
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Call for MemberSessionApprovalDecide {
    const METHOD: &'static str = "member.session_approval";
    type Params = MemberSessionApprovalDecideParams;
    type Result = SessionView;
}

pub struct MemberSessionClearStall;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSessionClearStallParams {
    pub command_id: String,
    pub employee_id: String,
}

impl Call for MemberSessionClearStall {
    const METHOD: &'static str = "member.session_clear_stall";
    type Params = MemberSessionClearStallParams;
    type Result = SessionView;
}

// ---------------------------------------------------------------------------
// Provider CLI enrollment & wrapped credential custody (ADR-0032 / ENROLL-1..6)
// ---------------------------------------------------------------------------

/// State of a browser-driven CLI enrollment session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EnrollmentState {
    PendingBrowser,
    AwaitingCode,
    Completed,
    Cancelled,
    Expired,
    Failed,
    #[serde(other)]
    Unknown,
}

/// Public view of a CLI enrollment flow. Never carries raw OAuth tokens or secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentSessionView {
    pub login_id: String,
    pub connection_id: String,
    pub provider: String,
    pub state: EnrollmentState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_code: Option<String>,
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Non-secret metadata for a wrapped provider credential stored on this office.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCredentialView {
    pub connection_id: String,
    pub office_id: String,
    pub enrolled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrolled_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_hint: Option<String>,
}

pub struct ProviderEnrollmentStart;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEnrollmentStartParams {
    pub command_id: String,
    pub connection_id: String,
    /// Optional provider selector (`"anthropic"` / `"openai"` / `"claude"` / `"codex"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

impl Call for ProviderEnrollmentStart {
    const METHOD: &'static str = "provider.enrollment_start";
    type Params = ProviderEnrollmentStartParams;
    type Result = EnrollmentSessionView;
}

pub struct ProviderEnrollmentStatus;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEnrollmentStatusParams {
    pub connection_id: String,
    pub login_id: String,
}

impl Call for ProviderEnrollmentStatus {
    const METHOD: &'static str = "provider.enrollment_status";
    type Params = ProviderEnrollmentStatusParams;
    type Result = EnrollmentSessionView;
}

pub struct ProviderEnrollmentSubmit;

/// Submits a pasted callback/auth code for an enrollment flow. `Debug` redacts `code`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEnrollmentSubmitParams {
    pub command_id: String,
    pub connection_id: String,
    pub login_id: String,
    pub code: String,
}

impl fmt::Debug for ProviderEnrollmentSubmitParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderEnrollmentSubmitParams")
            .field("command_id", &self.command_id)
            .field("connection_id", &self.connection_id)
            .field("login_id", &self.login_id)
            .field("code", &"<redacted>")
            .finish()
    }
}

impl Call for ProviderEnrollmentSubmit {
    const METHOD: &'static str = "provider.enrollment_submit";
    type Params = ProviderEnrollmentSubmitParams;
    type Result = EnrollmentSessionView;
}

pub struct ProviderEnrollmentCancel;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEnrollmentCancelParams {
    pub command_id: String,
    pub connection_id: String,
    pub login_id: String,
}

impl Call for ProviderEnrollmentCancel {
    const METHOD: &'static str = "provider.enrollment_cancel";
    type Params = ProviderEnrollmentCancelParams;
    type Result = EnrollmentSessionView;
}

pub struct ProviderCredentialStatus;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCredentialStatusParams {
    pub connection_id: String,
}

impl Call for ProviderCredentialStatus {
    const METHOD: &'static str = "provider.credential_status";
    type Params = ProviderCredentialStatusParams;
    type Result = ProviderCredentialView;
}

pub struct ProviderCredentialRevoke;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCredentialRevokeParams {
    pub command_id: String,
    pub connection_id: String,
}

impl Call for ProviderCredentialRevoke {
    const METHOD: &'static str = "provider.credential_revoke";
    type Params = ProviderCredentialRevokeParams;
    type Result = ProviderCredentialView;
}
