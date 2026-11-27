//! Wire contract for the one-call agent wake/resume dashboard (`agent.resume`, `TASK-220`).
//!
//! Returns a single bounded, structured snapshot for the bound caller: compact identity,
//! caller-scoped unread inbox summary, open tasks assigned to the caller with optional
//! caller-owned latest private working note pointer (`ADR-0031`), open asks sent by or
//! awaiting the caller, caller-owned active worktrees, and optional own budget status.
//!
//! `Debug` implementations redact private note titles/excerpts/tags and ask prompt excerpts
//! so private or message text never reaches logs.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ask::AskStatus;
use crate::budgets::{BudgetHoldState, BudgetStateSummary, BudgetUnit};
use crate::chat::DeliveredBecause;
use crate::external_auth::SessionMode;
use crate::msg::MessagePriority;
use crate::org::MemberKind;
use crate::private_notes::PrivateNoteKind;
use crate::tasks::{TaskPriority, TaskStatus};
use crate::Call;

/// Default maximum open assigned tasks included in `agent.resume`.
pub const RESUME_DEFAULT_TASKS_LIMIT: u32 = 20;
/// Hard ceiling on open assigned tasks included in `agent.resume`.
pub const RESUME_MAX_TASKS_LIMIT: u32 = 50;
/// Default maximum recent unread inbox items included in `agent.resume`.
pub const RESUME_DEFAULT_INBOX_LIMIT: u32 = 10;
/// Hard ceiling on recent unread inbox items included in `agent.resume`.
pub const RESUME_MAX_INBOX_LIMIT: u32 = 25;
/// Default maximum open asks included in `agent.resume`.
pub const RESUME_DEFAULT_ASKS_LIMIT: u32 = 10;
/// Hard ceiling on open asks included in `agent.resume`.
pub const RESUME_MAX_ASKS_LIMIT: u32 = 25;
/// Default maximum active worktrees included in `agent.resume`.
pub const RESUME_DEFAULT_WORKTREES_LIMIT: u32 = 10;
/// Hard ceiling on active worktrees included in `agent.resume`.
pub const RESUME_MAX_WORKTREES_LIMIT: u32 = 25;
/// Maximum UTF-8 byte budget for a private working note excerpt in `agent.resume`.
pub const RESUME_NOTE_EXCERPT_MAX_BYTES: usize = 240;
/// Maximum UTF-8 byte budget for an open ask prompt excerpt in `agent.resume`.
pub const RESUME_ASK_EXCERPT_MAX_BYTES: usize = 200;

/// `agent.resume`: single-call bounded wake/resume snapshot for the authenticated caller.
pub struct AgentResume;

/// Optional bounded section limits for `agent.resume`. All fields are optional and default
/// to conservative server-side caps.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentResumeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asks_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktrees_limit: Option<u32>,
}

/// Compact `whoami`-equivalent identity for the bound caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeIdentity {
    pub member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub name: String,
    pub kind: MemberKind,
    pub office_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<SessionMode>,
}

/// Compact header view of one unread inbox item belonging exclusively to the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeInboxItem {
    pub id: String,
    pub conversation_id: String,
    pub from_member_id: String,
    pub from_display_name: String,
    pub kind: String,
    pub priority: MessagePriority,
    pub urgent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub delivered_because: DeliveredBecause,
    pub created_at: String,
}

/// Bounded summary of the caller's unread inbox.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeInboxSummary {
    pub unread_count: u64,
    pub urgent_count: u64,
    #[serde(default)]
    pub items: Vec<ResumeInboxItem>,
    #[serde(default)]
    pub truncated: bool,
}

/// Bounded pointer and excerpt for the caller's own latest private working note on a task
/// (`ADR-0031`: strictly author-owned, never another agent's note).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeTaskNotePointer {
    pub note_id: String,
    pub revision: u64,
    #[serde(default)]
    pub kind: PrivateNoteKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub is_checkpoint: bool,
    pub updated_at: String,
}

impl fmt::Debug for ResumeTaskNotePointer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResumeTaskNotePointer")
            .field("note_id", &self.note_id)
            .field("revision", &self.revision)
            .field("kind", &self.kind)
            .field("title", &self.title.as_ref().map(|_| "<redacted>"))
            .field("excerpt", &"<redacted>")
            .field("tags", &format!("<{} redacted>", self.tags.len()))
            .field("is_checkpoint", &self.is_checkpoint)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

/// Compact summary of one active (open) task assigned to the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeTaskItem {
    pub id: String,
    pub key: String,
    pub title: String,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_note: Option<ResumeTaskNotePointer>,
}

/// Direction of an open ask relative to the calling employee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResumeAskDirection {
    /// An open ask sent to or addressed to the caller that still awaits an answer.
    AwaitingCaller,
    /// An open ask sent by the caller that still awaits an answer.
    SentByCaller,
    #[serde(other)]
    Unknown,
}

/// Bounded summary of one open structured ask sent by or awaiting the caller.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeOpenAsk {
    pub ask_id: String,
    pub conversation_id: String,
    pub direction: ResumeAskDirection,
    pub status: AskStatus,
    pub from_member_id: String,
    pub from_display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addressed_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocks: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub approval: bool,
    pub question_count: u32,
    pub prompt_excerpt: String,
    pub wait_until: String,
    pub created_at: String,
}

impl fmt::Debug for ResumeOpenAsk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResumeOpenAsk")
            .field("ask_id", &self.ask_id)
            .field("conversation_id", &self.conversation_id)
            .field("direction", &self.direction)
            .field("status", &self.status)
            .field("from_member_id", &self.from_member_id)
            .field("from_display_name", &self.from_display_name)
            .field("addressed_to", &self.addressed_to)
            .field("blocks", &self.blocks)
            .field("required", &self.required)
            .field("approval", &self.approval)
            .field("question_count", &self.question_count)
            .field("prompt_excerpt", &"<redacted>")
            .field("wait_until", &self.wait_until)
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// Compact summary of one active task worktree belonging to the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeWorktreeItem {
    pub task_id: String,
    pub task_key: String,
    pub repository_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_key: Option<String>,
    pub branch: String,
    pub worktree_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_head: Option<String>,
}

/// Compact summary of one configured own-agent budget period (daily or monthly).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeBudgetPeriod {
    pub state_summary: BudgetStateSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_state: Option<BudgetHoldState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<BudgetUnit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_amount: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_amount: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_key: Option<String>,
}

/// Compact summary of the caller's configured daily/monthly budget status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeBudgetSummary {
    pub status: BudgetStateSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily: Option<ResumeBudgetPeriod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monthly: Option<ResumeBudgetPeriod>,
}

/// Bounded structured wake/resume snapshot returned by `agent.resume`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentResumeResult {
    pub identity: ResumeIdentity,
    pub unread_inbox: ResumeInboxSummary,
    #[serde(default)]
    pub active_tasks: Vec<ResumeTaskItem>,
    pub active_tasks_total: u64,
    #[serde(default)]
    pub open_asks: Vec<ResumeOpenAsk>,
    #[serde(default)]
    pub active_worktrees: Vec<ResumeWorktreeItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<ResumeBudgetSummary>,
}

impl Call for AgentResume {
    const METHOD: &'static str = "agent.resume";
    type Params = AgentResumeParams;
    type Result = AgentResumeResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_wire_shapes_round_trip_and_redact_sensitive_text_in_debug() {
        let note = ResumeTaskNotePointer {
            note_id: "0193a5c0-0000-7000-8000-000000000400".into(),
            revision: 3,
            kind: PrivateNoteKind::Note,
            title: Some("SECRET-TITLE".into()),
            excerpt: "SECRET-EXCERPT-BODY".into(),
            tags: vec!["checkpoint".into(), "SECRET-TAG".into()],
            is_checkpoint: true,
            updated_at: "2026-11-27T12:00:00.000Z".into(),
        };
        let note_dbg = format!("{note:?}");
        assert!(!note_dbg.contains("SECRET-TITLE"));
        assert!(!note_dbg.contains("SECRET-EXCERPT-BODY"));
        assert!(!note_dbg.contains("SECRET-TAG"));

        let ask = ResumeOpenAsk {
            ask_id: "0193a5c0-0000-7000-8000-000000000500".into(),
            conversation_id: "0193a5c0-0000-7000-8000-000000000600".into(),
            direction: ResumeAskDirection::AwaitingCaller,
            status: AskStatus::Waiting,
            from_member_id: "0193a5c0-0000-7000-8000-000000000001".into(),
            from_display_name: "Owner".into(),
            addressed_to: Some("0193a5c0-0000-7000-8000-000000000003".into()),
            blocks: Some("T-A1".into()),
            required: true,
            approval: false,
            question_count: 1,
            prompt_excerpt: "SECRET-ASK-PROMPT".into(),
            wait_until: "2026-11-27T12:10:00.000Z".into(),
            created_at: "2026-11-27T12:00:00.000Z".into(),
        };
        assert!(!format!("{ask:?}").contains("SECRET-ASK-PROMPT"));

        let res = AgentResumeResult {
            identity: ResumeIdentity {
                member_id: "0193a5c0-0000-7000-8000-000000000003".into(),
                handle: Some("ada".into()),
                name: "Ada".into(),
                kind: MemberKind::Agent,
                office_id: "0193a5c0-0000-7000-8000-000000000010".into(),
                session_mode: Some(SessionMode::Managed),
            },
            unread_inbox: ResumeInboxSummary::default(),
            active_tasks: vec![ResumeTaskItem {
                id: "0193a5c0-0000-7000-8000-000000000100".into(),
                key: "T-A1".into(),
                title: "Implement resume".into(),
                status: TaskStatus::InProgress,
                priority: TaskPriority::High,
                revision: 2,
                project_id: Some("0193a5c0-0000-7000-8000-000000000200".into()),
                project_key: Some("P-1".into()),
                blocked_reason: None,
                latest_note: Some(note),
            }],
            active_tasks_total: 1,
            open_asks: vec![ask],
            active_worktrees: Vec::new(),
            budget: None,
        };
        let encoded = serde_json::to_value(&res).unwrap();
        assert!(encoded.get("budget").is_none());
        let decoded: AgentResumeResult = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, res);
    }
}
