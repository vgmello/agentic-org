//! Durable autonomous wake-up queue, required-ask suspension state, and hourly wake quota
//! (`TASK-47`, `ADR-0020`, `ADR-0023`, `ADR-0028`, `ADR-0030`).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::credentials::rfc3339_now;

/// Default maximum agent-initiated turns per rolling hour per target agent (`ADR-0023` d5).
pub const DEFAULT_HOURLY_WAKE_LIMIT: usize = 12;

/// Default rolling window for agent-initiated wake limits: 1 hour (`ADR-0023` d5).
pub const DEFAULT_WAKE_WINDOW: Duration = Duration::from_secs(3600);

/// Default grace period after required-ask wind-down notice before forced suspension (`ADR-0020` d10).
pub const DEFAULT_SUSPEND_GRACE: Duration = Duration::from_secs(120);

/// What caused an autonomous agent turn start (`ADR-0023` d8, `ADR-0028` d7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeKind {
    DirectMessage,
    Mention,
    Ask,
    Handoff,
    TaskAssignment,
    AskAnswer,
    WatcherAnswer,
    Recovery,
}

impl WakeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DirectMessage => "message",
            Self::Mention => "mention",
            Self::Ask => "ask",
            Self::Handoff => "handoff",
            Self::TaskAssignment => "assignment",
            Self::AskAnswer | Self::WatcherAnswer => "answer",
            Self::Recovery => "recovery",
        }
    }
}

/// Why a pending start or required-ask resume could not run immediately (`ADR-0030` d2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeHoldReason {
    Busy,
    Paused,
    Suspended,
    HourlyLimit,
    BudgetHold,
    StallLocked,
    RecoveryRequired,
    AdmissionRefused,
}

impl WakeHoldReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Paused => "paused",
            Self::Suspended => "suspended",
            Self::HourlyLimit => "hourly_limit",
            Self::BudgetHold => "budget_hold",
            Self::StallLocked => "stall_locked",
            Self::RecoveryRequired => "recovery_required",
            Self::AdmissionRefused => "admission_refused",
        }
    }
}

/// One durable pending-start work item queued for an agent (`ADR-0030` d2..3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingWakeItem {
    pub item_id: String,
    pub kind: WakeKind,
    pub sender_id: String,
    pub sender_name: String,
    pub sender_is_person: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub source_id: String,
    pub summary: String,
    pub enqueued_at_ms: u64,
}

/// Durable per-agent pending-start queue persisted in `pending-wakes.v1.json` (`LIVE-4`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingWakeQueueRecord {
    pub version: u32,
    pub employee_id: String,
    #[serde(default)]
    pub items: Vec<PendingWakeItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_reason: Option<WakeHoldReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held_since_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    pub updated_at: String,
}

impl PendingWakeQueueRecord {
    pub fn empty(employee_id: &str) -> Self {
        Self {
            version: 1,
            employee_id: employee_id.to_owned(),
            items: Vec::new(),
            hold_reason: None,
            held_since_ms: None,
            retry_after_ms: None,
            updated_at: rfc3339_now(),
        }
    }

    pub fn push_dedup(&mut self, item: PendingWakeItem) -> bool {
        if self
            .items
            .iter()
            .any(|existing| existing.item_id == item.item_id)
        {
            return false;
        }
        self.items.push(item);
        self.updated_at = rfc3339_now();
        true
    }

    pub fn has_person_item(&self) -> bool {
        self.items.iter().any(|i| i.sender_is_person)
    }

    pub fn set_hold(&mut self, reason: WakeHoldReason, now_ms: u64, retry_after_ms: Option<u64>) {
        if self.hold_reason != Some(reason) {
            self.held_since_ms = Some(now_ms);
        }
        self.hold_reason = Some(reason);
        self.retry_after_ms = retry_after_ms;
        self.updated_at = rfc3339_now();
    }
}

/// One open required ask tracked for an active or suspended agent (`ADR-0020` d9, `ASK-11..12`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuspendedAskEntry {
    pub ask_id: String,
    pub conversation_id: String,
    pub question_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_for_member_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_for_name: Option<String>,
    #[serde(default)]
    pub asked_by_subagent: bool,
    pub opened_at_ms: u64,
}

/// An answer to a required ask saved durably if resume is temporarily held (`ASK-8`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingAskAnswer {
    pub ask_id: String,
    pub question_summary: String,
    pub answer_summary: String,
    pub answered_by: String,
    pub answered_at: String,
    #[serde(default)]
    pub asked_by_subagent: bool,
}

/// Durable required-ask suspension record persisted in `suspended-asks.v1.json` (`ASK-6`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuspendedStateRecord {
    pub version: u32,
    pub employee_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_session_id: Option<String>,
    pub suspended_since: String,
    #[serde(default)]
    pub asks: Vec<SuspendedAskEntry>,
    #[serde(default)]
    pub reaped_background_tasks: Vec<String>,
    #[serde(default)]
    pub pending_resume_answers: Vec<PendingAskAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_reason: Option<WakeHoldReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held_since_ms: Option<u64>,
    pub updated_at: String,
}

impl SuspendedStateRecord {
    /// Returns `true` when `item` is an ask or handoff from an employee this suspended agent is
    /// actively waiting on, breaking mutual-suspension deadlocks (`ADR-0030` d7 / `LIVE-8`).
    pub fn allows_deadlock_breaker(&self, item: &PendingWakeItem) -> bool {
        if !matches!(item.kind, WakeKind::Ask | WakeKind::Handoff) {
            return false;
        }
        self.asks
            .iter()
            .any(|ask| ask.waiting_for_member_id.as_deref() == Some(item.sender_id.as_str()))
    }

    pub fn primary_waiting_target(&self) -> (Option<String>, Option<String>) {
        self.asks
            .first()
            .map(|a| (a.waiting_for_member_id.clone(), a.waiting_for_name.clone()))
            .unwrap_or((None, None))
    }

    pub fn waiting_status_line(&self) -> String {
        match self
            .asks
            .first()
            .and_then(|a| a.waiting_for_name.as_deref())
        {
            Some(name) if !name.is_empty() => format!("Waiting for an answer from {name}"),
            _ => "Waiting for a required answer".to_owned(),
        }
    }
}

/// Durable rolling hourly quota of agent-initiated starts (`ADR-0023` d5 / `WAKE-4..5`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeQuotaRecord {
    pub version: u32,
    pub employee_id: String,
    #[serde(default)]
    pub agent_wake_timestamps_ms: Vec<u64>,
    pub updated_at: String,
}

impl WakeQuotaRecord {
    pub fn empty(employee_id: &str) -> Self {
        Self {
            version: 1,
            employee_id: employee_id.to_owned(),
            agent_wake_timestamps_ms: Vec::new(),
            updated_at: rfc3339_now(),
        }
    }

    /// Prunes expired timestamps and checks whether one more agent-initiated start fits inside
    /// `limit`. Returns `Err(retry_after_ms)` when the hourly limit is reached.
    pub fn check_limit(&mut self, now_ms: u64, window_ms: u64, limit: usize) -> Result<(), u64> {
        let cutoff = now_ms.saturating_sub(window_ms);
        self.agent_wake_timestamps_ms.retain(|&ts| ts > cutoff);
        if limit == 0 {
            return Err(now_ms.saturating_add(window_ms));
        }
        if self.agent_wake_timestamps_ms.len() < limit {
            return Ok(());
        }
        let oldest = self
            .agent_wake_timestamps_ms
            .iter()
            .copied()
            .min()
            .unwrap_or(now_ms);
        Err(oldest.saturating_add(window_ms))
    }

    pub fn record_start(&mut self, now_ms: u64, window_ms: u64) {
        let cutoff = now_ms.saturating_sub(window_ms);
        self.agent_wake_timestamps_ms.retain(|&ts| ts > cutoff);
        self.agent_wake_timestamps_ms.push(now_ms);
        self.updated_at = rfc3339_now();
    }
}

/// Result of attempting to enqueue/dispatch a wake trigger for one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WakeOutcome {
    Started,
    Queued { reason: WakeHoldReason },
    Ignored,
}

/// Composes one coalesced turn objective covering 1..N queued wake items (`LIVE-5`, `LIVE-8`).
pub fn compose_coalesced_wake_objective(
    items: &[PendingWakeItem],
    still_open_asks: &[SuspendedAskEntry],
) -> String {
    let mut lines = Vec::new();
    if items.len() == 1 {
        let item = &items[0];
        lines.push(format!(
            "Incoming {} from {} ({}):",
            item.kind.as_str(),
            item.sender_name,
            item.source_id
        ));
        lines.push(item.summary.trim().to_owned());
    } else {
        lines.push(format!(
            "You have {} waiting items that arrived while you were busy or held:",
            items.len()
        ));
        for (idx, item) in items.iter().enumerate() {
            lines.push(format!(
                "{}. [{}] from {} ({}): {}",
                idx + 1,
                item.kind.as_str(),
                item.sender_name,
                item.source_id,
                item.summary.trim()
            ));
        }
    }
    append_still_open_asks(&mut lines, still_open_asks);
    lines.join("\n")
}

/// Composes the resume input when one or more required asks are answered (`ASK-5`, `ASK-7`,
/// `ASK-11`, `ASK-12`, `ADR-0020` d12). Always includes the question alongside its answer.
pub fn compose_required_ask_resume_objective(
    answers: &[PendingAskAnswer],
    reaped_background_tasks: &[String],
    still_open_asks: &[SuspendedAskEntry],
) -> String {
    let mut lines = vec!["Your required ask was answered; resuming your session.".to_owned()];
    for ans in answers {
        let origin = if ans.asked_by_subagent {
            " (asked by your sub-agent)"
        } else {
            ""
        };
        lines.push(format!(
            "- Ask `{}`{origin}\n  Question: {}\n  Answer from {}: {}",
            ans.ask_id,
            ans.question_summary.trim(),
            ans.answered_by,
            ans.answer_summary.trim()
        ));
    }
    if !reaped_background_tasks.is_empty() {
        lines.push(format!(
            "Background work stopped during suspension: {}",
            reaped_background_tasks.join(", ")
        ));
    }
    append_still_open_asks(&mut lines, still_open_asks);
    lines.join("\n")
}

fn append_still_open_asks(lines: &mut Vec<String>, still_open_asks: &[SuspendedAskEntry]) {
    if still_open_asks.is_empty() {
        return;
    }
    lines.push("Your own required ask(s) remain open:".to_owned());
    for ask in still_open_asks {
        lines.push(format!(
            "- Ask `{}`: {}",
            ask.ask_id,
            ask.question_summary.trim()
        ));
    }
}
