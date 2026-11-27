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
    Suspended,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suspended_on_asks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_for_member_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_for_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspended_since: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub pending_wake_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_wake_reason: Option<String>,
    /// Coarse, ephemeral live activity of the running turn (`TASK-103`); `None` between turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_phase: Option<ActivityPhase>,
    /// The running tool's name only, while [`ActivityPhase::RunningTool`]: never its input,
    /// output, diff or the agent's thinking (`doc-40` CHAT-4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tool_name: Option<String>,
    /// When the running tool started (RFC 3339 UTC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tool_started_at: Option<String>,
    /// The live runtime takes a person's steer into a running turn natively, without
    /// interrupting it. Otherwise `member.session_steer` interrupts the turn and sends the
    /// steer as the next turn (the warm process lives on either way).
    #[serde(default)]
    pub supports_steer: bool,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

/// Coarse live activity of a running turn (`TASK-103`). Ephemeral: never persisted, never
/// carrying tool inputs, outputs, diffs, file contents or thinking text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ActivityPhase {
    /// The model is reasoning (or the turn just began).
    Thinking,
    /// A tool call is running (its name only rides along).
    RunningTool,
    /// The model is writing its reply.
    Responding,
    /// A person interrupted the turn; it is winding down.
    Interrupting,
    #[serde(other)]
    Unknown,
}

/// `member.activity_changed` on the organization's `presence` feed: one agent's coalesced
/// coarse live activity, the latest per member (`TASK-103`). `phase: null` is idle. A
/// non-owner never gets the Chief of Staff's tool name or start time (her turns serve one
/// caller's private partition).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberActivityChangedData {
    pub member_id: String,
    pub phase: Option<ActivityPhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tool_started_at: Option<String>,
    #[serde(default)]
    pub supports_steer: bool,
    /// When this activity began (RFC 3339 UTC).
    pub changed_at: String,
}

impl MemberActivityChangedData {
    /// The same frame without the tool detail: what a caller who may not see the turn gets.
    pub fn without_tool_detail(mut self) -> Self {
        self.active_tool_name = None;
        self.active_tool_started_at = None;
        self
    }
}

/// The largest steer text `member.session_steer` takes, in UTF-8 bytes: one fenced message body.
pub const MAX_STEER_TEXT_BYTES: usize = 2_048;

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
    /// Optional prior runtime session ID to resume when workspace and spec hash still match
    /// (`TASK-92.1`, ADR-0028). Cannot be combined with `fresh: true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_runtime_session_id: Option<String>,
    /// Force a clean empty session, recycling any idle warm handle and ignoring hibernated
    /// resume state (`TASK-92.1`, ADR-0028).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fresh: bool,
    /// Optional per-turn model override (`TASK-136`); falls back to the agent profile default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional per-turn reasoning effort override (`low|medium|high|xhigh|max`, `TASK-136`);
    /// falls back to the agent profile default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
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

/// `member.session_interrupt`: destructively stop the agent's running turn while keeping its
/// warm process alive (unlike `member.session_stop`, which ends the process). Turns queued
/// behind it still run; an idle live session is left as it is (`TASK-103`).
pub struct MemberSessionInterrupt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSessionInterruptParams {
    pub command_id: String,
    pub employee_id: String,
}

impl Call for MemberSessionInterrupt {
    const METHOD: &'static str = "member.session_interrupt";
    type Params = MemberSessionInterruptParams;
    type Result = SessionView;
}

/// `member.session_steer`: a person's explicit mid-turn direction (`TASK-103`). Delivered into
/// the running turn natively where the runtime steers ([`SessionView::supports_steer`]);
/// otherwise the turn is interrupted and the steer runs as the next turn, ahead of queued
/// turns. An idle live session runs it as its next turn.
pub struct MemberSessionSteer;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSessionSteerParams {
    pub command_id: String,
    pub employee_id: String,
    /// At most [`MAX_STEER_TEXT_BYTES`]; never empty.
    pub text: String,
}

impl Call for MemberSessionSteer {
    const METHOD: &'static str = "member.session_steer";
    type Params = MemberSessionSteerParams;
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
// C4: Session discovery & read-only conversation log viewer (`REQ-05`, `SUB-6`, `FC-150`)
// ---------------------------------------------------------------------------

/// Conversation transcript source format parsed on the office.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TranscriptFormat {
    Claude,
    Codex,
    Agy,
    #[serde(other)]
    Unknown,
}

/// Normalized conversation entry kind shared across Claude, Codex, and Agy logs (`REQ-05`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TranscriptEntryKind {
    UserMessage,
    AssistantMessage,
    Thinking,
    ToolCall,
    ToolResult,
    FileEvent,
    Usage,
    Notice,
    SubagentRun,
    Raw,
    #[serde(other)]
    Unknown,
}

/// File operation classified for file-based visualization toggles (`REQ-05`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FileEventKind {
    Read,
    Write,
    Edit,
    Delete,
    Reference,
    #[serde(other)]
    Unknown,
}

/// Aggregated file touch entry for file-based visualization panels (`REQ-05`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTouchSummary {
    pub path: String,
    pub operations: Vec<FileEventKind>,
    pub count: u32,
    pub last_turn_index: u32,
}

/// Aggregated or per-entry token usage metrics extracted from a transcript.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Nested sub-agent conversation transcript attached to the parent turn that launched it (`SUB-6`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubagentTranscriptView {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
    pub format: TranscriptFormat,
    pub entry_count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<TranscriptEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileTouchSummary>,
    #[serde(default)]
    pub usage: TranscriptUsage,
}

/// One normalized, secret-redacted conversation log entry (`REQ-05`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub id: String,
    pub turn_index: u32,
    pub kind: TranscriptEntryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_output: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_action: Option<FileEventKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TranscriptUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent: Option<SubagentTranscriptView>,
}

/// Metadata summary of one discovered conversation session inside an agent's private home.
/// Never exposes raw host filesystem paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummaryView {
    pub session_id: String,
    pub format: TranscriptFormat,
    pub is_live: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub size_bytes: u64,
    pub subagent_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// Result of `member.session_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionListResult {
    pub employee_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_session_id: Option<String>,
    pub sessions: Vec<SessionSummaryView>,
}

/// Result of `member.session_transcript` (`REQ-05`, `SUB-6`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionTranscriptResult {
    pub employee_id: String,
    pub session_id: String,
    pub format: TranscriptFormat,
    pub is_live: bool,
    pub file_size_bytes: u64,
    pub total_entries: u64,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
    #[serde(default)]
    pub usage: TranscriptUsage,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileTouchSummary>,
    pub entries: Vec<TranscriptEntry>,
}

pub struct MemberSessionList;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSessionListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl Call for MemberSessionList {
    const METHOD: &'static str = "member.session_list";
    type Params = MemberSessionListParams;
    type Result = SessionListResult;
}

pub struct MemberSessionTranscript;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSessionTranscriptParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    /// Optional vendor/runtime session ID; omits to open the live or most recent session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Toggle tool call and tool result visibility (defaults to `true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_tool_calls: Option<bool>,
    /// Toggle file-based change/reference entries and file summary panel (defaults to `true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_file_changes: Option<bool>,
    /// Toggle nested sub-agent transcripts (`SUB-6`; defaults to `true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_subagents: Option<bool>,
    /// Entry offset cursor for pagination or live tailing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
    /// Maximum entries returned in this page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl Call for MemberSessionTranscript {
    const METHOD: &'static str = "member.session_transcript";
    type Params = MemberSessionTranscriptParams;
    type Result = SessionTranscriptResult;
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

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;
