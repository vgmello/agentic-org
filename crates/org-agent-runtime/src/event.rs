//! The normalized [`SessionEvent`] stream every runtime maps onto.
//!
//! Events are coarse: no raw runtime JSON rides along. Free text is reachable by
//! [`SessionEvent::redact`], which the office runs before anything is stored or streamed.

mod approval;
mod outcome;
mod redact;

#[cfg(test)]
mod approval_tests;
#[cfg(test)]
pub(crate) mod tests;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{RateLimit, SessionId, ToolCapability, Usage};

pub use approval::{ApprovalId, ApprovalKind, ApprovalRequest, Escalation};
pub use outcome::{Activity, LimitKind, NoticeKind, StartFailure, TurnOutcome};
pub(crate) use redact::redact_opt;
use redact::redact_value;

/// What a session reports. Serialized as `{"type": ..., "data": ...}` (`data` left out for
/// [`SessionEvent::Thinking`]); an unknown `type` decodes as [`SessionEvent::Unknown`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SessionEvent {
    /// The opening handshake finished; the session takes [`crate::Control`]s now.
    Ready {
        /// The runtime's version, as it reported it.
        runtime_version: Option<String>,
        /// The runtime's pid, if it reported one.
        pid: Option<u32>,
        /// The agents the runtime knows, by name. For a runtime with agents, the session has
        /// already checked that the spec's agent is among them
        /// ([`StartFailure::AgentMismatch`] otherwise); a runtime without agents reports none.
        agents: Vec<String>,
        /// The permission mode the runtime reports it's in, in its own words (e.g. Claude's
        /// `default` for manual). The session has already checked it matches the spec's
        /// ([`StartFailure::ModeMismatch`] otherwise). `None` only for a runtime that never
        /// reports its mode: then nothing could be checked. A runtime that does report one and
        /// leaves it out fails the start as a mismatch (with an empty `reported`).
        permission_mode: Option<String>,
    },
    /// The session couldn't start. [`SessionEvent::Ended`] follows once the process exits.
    StartFailed {
        /// Why.
        reason: StartFailure,
    },
    /// The runtime's id for the conversation, needed to resume it. Emitted once it's known,
    /// even if it was known up front. An id the runtime sends that isn't a valid
    /// [`SessionId`] becomes a [`SessionEvent::ProtocolError`] instead.
    SessionStarted {
        /// The runtime's session (thread, conversation) id.
        runtime_session_id: SessionId,
    },
    /// A turn began.
    TurnStarted {
        /// The runtime's turn id, where it has one (Codex needs it to steer and interrupt).
        turn_id: Option<String>,
        /// `false` for a turn the runtime began on its own (e.g. background work finishing
        /// after a turn ended), not in answer to a send.
        solicited: bool,
    },
    /// The session's activity changed.
    Status(Activity),
    /// Live text as it streams. For display only: never stored.
    TextDelta {
        /// The new text.
        text: String,
    },
    /// A finished piece of the agent's reply.
    AssistantText {
        /// The runtime's message id, where it has one. Several pieces can share one.
        message_id: Option<String>,
        /// The text.
        text: String,
    },
    /// The agent is thinking (reasoning). Coarse on purpose: no text and no token counts,
    /// only that it happened, for a live status.
    Thinking,
    /// The runtime reports its permission mode changed (e.g. after a permission-change
    /// approval), in its own words. `None` if it says the mode changed without naming it.
    ModeChanged {
        /// The new mode, in the runtime's words.
        mode: Option<String>,
    },
    /// The agent began a tool call.
    ToolStarted {
        /// The runtime's id for the call.
        tool_use_id: String,
        /// The tool.
        name: String,
        /// Runtime-neutral capability class of the tool, when classified by the adapter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capability: Option<ToolCapability>,
        /// The tool's input, as structured JSON.
        input: Value,
    },
    /// A tool call finished.
    ToolFinished {
        /// The call, as in [`SessionEvent::ToolStarted`].
        tool_use_id: String,
        /// Whether it failed.
        is_error: bool,
    },
    /// The runtime asks whether a tool call may go ahead. Answer with
    /// [`crate::Control::Answer`].
    ApprovalRequested(ApprovalRequest),
    /// The runtime withdrew an approval request (e.g. Claude, on interrupt). Don't answer it.
    ApprovalCancelled {
        /// The withdrawn request.
        id: ApprovalId,
    },
    /// The runtime denied an action without asking (its own policy, or no approval bridge).
    ActionDenied {
        /// The tool.
        tool: String,
        /// The call, where the runtime said which.
        tool_use_id: Option<String>,
        /// The runtime's explanation.
        message: Option<String>,
    },
    /// Token usage and cost.
    Usage(Usage),
    /// A rate-limit window's state.
    RateLimit(RateLimit),
    /// A turn ended.
    TurnEnded {
        /// How it ended.
        outcome: TurnOutcome,
        /// The runtime's error messages for the turn, if any.
        errors: Vec<String>,
    },
    /// A non-destructive mid-turn message (`Control::DeliverMidTurn`) was confirmed delivered
    /// by the runtime (`doc-101`, `ADR-0035`).
    MidTurnDelivered {
        /// Office-assigned correlation id echoed from [`crate::Control::DeliverMidTurn`].
        delivery_id: String,
    },
    /// A non-destructive mid-turn message (`Control::DeliverMidTurn`) was rejected or went
    /// unconsumed before the turn ended (`doc-101`, `ADR-0035`). The office keeps its items
    /// queued for the next turn boundary.
    MidTurnDeliveryFailed {
        /// Office-assigned correlation id echoed from [`crate::Control::DeliverMidTurn`].
        delivery_id: String,
        /// Human-readable runtime rejection or turn-end race explanation (redacted).
        reason: String,
    },
    /// Something worth showing that doesn't change the session's state (a retry, a warning).
    Notice {
        /// What kind.
        kind: NoticeKind,
        /// The text.
        text: String,
    },
    /// A line the session couldn't read (invalid JSON, a known message with a bad shape).
    /// The session goes on.
    ProtocolError {
        /// What was wrong. Never the line itself.
        detail: String,
    },
    /// A line too long to read ([`crate::MAX_LINE_BYTES`]) was dropped. The session goes on.
    Truncated {
        /// The dropped line's length in bytes.
        len: usize,
    },
    /// A message type this adapter doesn't know. Its body is dropped. Also what an unknown
    /// event type decodes as.
    Unknown {
        /// The message's type, as the runtime named it.
        type_name: String,
    },
    /// The process exited: the last event of every session, exactly once. Emitted only by
    /// [`crate::Session::on_exit`].
    Ended {
        /// How it exited.
        exit: ExitInfo,
    },
}

/// How a runtime process ended, as the pump saw it when it reaped the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExitInfo {
    /// The exit code, if it exited normally.
    pub code: Option<i32>,
    /// The signal that killed it, if one did.
    pub signal: Option<i32>,
}

/// Every [`SessionEvent`] tag, for telling a known type from an unknown one when decoding.
const KNOWN_EVENT_TYPES: &[&str] = &[
    "ready",
    "start_failed",
    "session_started",
    "turn_started",
    "status",
    "text_delta",
    "assistant_text",
    "thinking",
    "mode_changed",
    "tool_started",
    "tool_finished",
    "approval_requested",
    "approval_cancelled",
    "action_denied",
    "usage",
    "rate_limit",
    "turn_ended",
    "mid_turn_delivered",
    "mid_turn_delivery_failed",
    "notice",
    "protocol_error",
    "truncated",
    "unknown",
    "ended",
];

/// The derived decoder for the known event types; [`SessionEvent`]'s own decoder adds the
/// fallback to [`SessionEvent::Unknown`], which serde can't express for a variant with data.
#[derive(Deserialize)]
#[serde(
    remote = "SessionEvent",
    tag = "type",
    content = "data",
    rename_all = "snake_case"
)]
enum KnownSessionEvent {
    Ready {
        runtime_version: Option<String>,
        pid: Option<u32>,
        agents: Vec<String>,
        permission_mode: Option<String>,
    },
    StartFailed {
        reason: StartFailure,
    },
    SessionStarted {
        runtime_session_id: SessionId,
    },
    TurnStarted {
        turn_id: Option<String>,
        solicited: bool,
    },
    Status(Activity),
    TextDelta {
        text: String,
    },
    AssistantText {
        message_id: Option<String>,
        text: String,
    },
    Thinking,
    ModeChanged {
        mode: Option<String>,
    },
    ToolStarted {
        tool_use_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capability: Option<ToolCapability>,
        input: Value,
    },
    ToolFinished {
        tool_use_id: String,
        is_error: bool,
    },
    ApprovalRequested(ApprovalRequest),
    ApprovalCancelled {
        id: ApprovalId,
    },
    ActionDenied {
        tool: String,
        tool_use_id: Option<String>,
        message: Option<String>,
    },
    Usage(Usage),
    RateLimit(RateLimit),
    TurnEnded {
        outcome: TurnOutcome,
        errors: Vec<String>,
    },
    MidTurnDelivered {
        delivery_id: String,
    },
    MidTurnDeliveryFailed {
        delivery_id: String,
        reason: String,
    },
    Notice {
        kind: NoticeKind,
        text: String,
    },
    ProtocolError {
        detail: String,
    },
    Truncated {
        len: usize,
    },
    Unknown {
        type_name: String,
    },
    Ended {
        exit: ExitInfo,
    },
}

impl<'de> Deserialize<'de> for SessionEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if let Some(type_name) = value.get("type").and_then(Value::as_str) {
            if !KNOWN_EVENT_TYPES.contains(&type_name) {
                return Ok(SessionEvent::Unknown {
                    type_name: type_name.to_owned(),
                });
            }
        }
        KnownSessionEvent::deserialize(value).map_err(de::Error::custom)
    }
}

impl SessionEvent {
    /// Runs `f` over every free-text string in the event, so the office can redact secrets
    /// before it stores or streams anything.
    ///
    /// It reaches every string that carries runtime text: messages, details, names, versions,
    /// tool input and approval input **at any depth, object keys included**. It leaves out the
    /// correlation ids the office must keep intact to act on the event (the runtime session,
    /// turn, message, tool-use and approval ids), and numbers and booleans.
    ///
    /// `f` may change a string in place. If it makes two keys of one JSON object equal, the
    /// later one wins.
    pub fn redact(&mut self, f: &mut dyn FnMut(&mut String)) {
        // Every pattern names every field (no `..`), so a new field can't be added without
        // deciding here whether it's redacted.
        match self {
            SessionEvent::Ready {
                runtime_version,
                pid: _,
                agents,
                permission_mode,
            } => {
                redact_opt(runtime_version, f);
                for item in agents.iter_mut() {
                    f(item);
                }
                redact_opt(permission_mode, f);
            }
            SessionEvent::StartFailed { reason } => reason.redact(f),
            SessionEvent::SessionStarted {
                runtime_session_id: _,
            } => {}
            SessionEvent::TurnStarted {
                turn_id: _,
                solicited: _,
            } => {}
            SessionEvent::Status(_) => {}
            SessionEvent::TextDelta { text } => f(text),
            SessionEvent::AssistantText {
                message_id: _,
                text,
            } => f(text),
            SessionEvent::Thinking => {}
            SessionEvent::ModeChanged { mode } => redact_opt(mode, f),
            SessionEvent::ToolStarted {
                tool_use_id: _,
                name,
                capability: _,
                input,
            } => {
                f(name);
                redact_value(input, f);
            }
            SessionEvent::ToolFinished {
                tool_use_id: _,
                is_error: _,
            } => {}
            SessionEvent::ApprovalRequested(request) => request.redact(f),
            SessionEvent::ApprovalCancelled { id: _ } => {}
            SessionEvent::ActionDenied {
                tool,
                tool_use_id: _,
                message,
            } => {
                f(tool);
                redact_opt(message, f);
            }
            SessionEvent::Usage(usage) => usage.redact(f),
            SessionEvent::RateLimit(limit) => limit.redact(f),
            SessionEvent::TurnEnded { outcome, errors } => {
                outcome.redact(f);
                for item in errors.iter_mut() {
                    f(item);
                }
            }
            SessionEvent::MidTurnDelivered { delivery_id: _ } => {}
            SessionEvent::MidTurnDeliveryFailed {
                delivery_id: _,
                reason,
            } => f(reason),
            SessionEvent::Notice { kind: _, text } => f(text),
            SessionEvent::ProtocolError { detail } => f(detail),
            SessionEvent::Truncated { len: _ } => {}
            SessionEvent::Unknown { type_name } => f(type_name),
            SessionEvent::Ended { exit: _ } => {}
        }
    }
}
