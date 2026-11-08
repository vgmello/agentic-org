//! The normalized [`SessionEvent`] stream every runtime maps onto.
//!
//! Events are coarse: no raw runtime JSON rides along. Free text is reachable by
//! [`SessionEvent::redact`], which the office runs before anything is stored or streamed.

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{RateLimit, SessionId, Usage};

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
    /// The agent began a tool call.
    ToolStarted {
        /// The runtime's id for the call.
        tool_use_id: String,
        /// The tool.
        name: String,
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
    "tool_started",
    "tool_finished",
    "approval_requested",
    "approval_cancelled",
    "action_denied",
    "usage",
    "rate_limit",
    "turn_ended",
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
#[allow(dead_code)] // only its generated `deserialize` is used
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
    ToolStarted {
        tool_use_id: String,
        name: String,
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
            SessionEvent::ToolStarted {
                tool_use_id: _,
                name,
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
            SessionEvent::Notice { kind: _, text } => f(text),
            SessionEvent::ProtocolError { detail } => f(detail),
            SessionEvent::Truncated { len: _ } => {}
            SessionEvent::Unknown { type_name } => f(type_name),
            SessionEvent::Ended { exit: _ } => {}
        }
    }
}

pub(crate) fn redact_opt(s: &mut Option<String>, f: &mut dyn FnMut(&mut String)) {
    if let Some(s) = s {
        f(s);
    }
}

/// Redacts every string in a JSON value, object keys included.
fn redact_value(value: &mut Value, f: &mut dyn FnMut(&mut String)) {
    match value {
        Value::String(s) => f(s),
        Value::Array(items) => items.iter_mut().for_each(|v| redact_value(v, f)),
        Value::Object(map) => {
            for (mut key, mut item) in std::mem::take(map) {
                f(&mut key);
                redact_value(&mut item, f);
                map.insert(key, item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Why a session couldn't start. Unknown kinds decode as [`StartFailure::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum StartFailure {
    /// The runtime doesn't know the spec's agent. (Claude would silently run its default
    /// agent instead, so the session refuses to go on.)
    AgentMismatch {
        /// The agent the spec asked for.
        requested: String,
        /// The agents the runtime reported.
        available: Vec<String>,
    },
    /// The runtime reports a different permission mode from the one the spec asked for, so
    /// the session refuses to go on rather than run looser than asked.
    ModeMismatch {
        /// The mode the adapter asked for, in the runtime's words.
        requested: String,
        /// The mode the runtime reported, in its words.
        reported: String,
    },
    /// Resuming the conversation failed (e.g. its transcript was pruned). The office starts
    /// a fresh one.
    ResumeFailed {
        /// What the runtime said.
        detail: String,
    },
    /// The runtime isn't signed in.
    AuthRequired,
    /// The runtime didn't finish the opening handshake in time.
    ProtocolTimeout,
    /// Anything else.
    Other {
        /// What happened.
        detail: String,
    },
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

impl StartFailure {
    fn redact(&mut self, f: &mut dyn FnMut(&mut String)) {
        match self {
            StartFailure::AgentMismatch {
                requested,
                available,
            } => {
                f(requested);
                for item in available.iter_mut() {
                    f(item);
                }
            }
            StartFailure::ModeMismatch {
                requested,
                reported,
            } => {
                f(requested);
                f(reported);
            }
            StartFailure::ResumeFailed { detail } | StartFailure::Other { detail } => f(detail),
            StartFailure::AuthRequired | StartFailure::ProtocolTimeout | StartFailure::Unknown => {}
        }
    }
}

/// What the session is doing. Unknown values decode as [`Activity::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Activity {
    /// A turn is running.
    Busy,
    /// Between turns.
    Idle,
    /// A turn is waiting for an approval answer.
    WaitingOnApproval,
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

/// A runtime's request to approve one tool call. The office answers it from the agent's
/// policy, or asks a person in Needs you.
///
/// Only yes-or-no requests become approvals. A runtime request that needs **content** back
/// (Codex `tool/requestUserInput`, `mcpServer/elicitation/request`) has no answer in
/// [`crate::Decision`], so adapters **decline it themselves** in v1 and never raise it here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// The request's id. Pass it back unchanged in [`crate::Control::Answer`].
    pub id: ApprovalId,
    /// What kind of action it is.
    pub kind: ApprovalKind,
    /// The tool.
    pub tool: String,
    /// The tool's name for people, where the runtime gives one.
    pub display_name: Option<String>,
    /// The tool's input, as structured JSON: what will actually run if allowed.
    pub input: Value,
    /// The agent's own explanation. **Untrusted**: the agent wrote it, so it may try to
    /// persuade. Show it labelled as such, and never decide on it.
    pub justification_untrusted: Option<String>,
    /// The path that triggered the request, where the runtime says.
    pub blocked_path: Option<String>,
    /// The tool call this is about, where the runtime says.
    pub tool_use_id: Option<String>,
}

impl ApprovalRequest {
    fn redact(&mut self, f: &mut dyn FnMut(&mut String)) {
        let ApprovalRequest {
            id: _,
            kind: _,
            tool,
            display_name,
            input,
            justification_untrusted,
            blocked_path,
            tool_use_id: _,
        } = self;
        f(tool);
        redact_opt(display_name, f);
        redact_value(input, f);
        redact_opt(justification_untrusted, f);
        redact_opt(blocked_path, f);
    }
}

/// What an [`ApprovalRequest`] asks to do. Unknown values decode as [`ApprovalKind::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApprovalKind {
    /// Run a shell command.
    Command,
    /// Change files.
    FileChange,
    /// Widen the session's own permissions (e.g. Codex `item/permissions/requestApproval`).
    Permissions,
    /// Any other tool call (e.g. an MCP tool).
    Other,
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

/// An approval request's id: **opaque** to the office.
///
/// The office stores it, compares it, and hands it back in [`crate::Control::Answer`]; it
/// never builds one or reads meaning into it. Adapters make it from their runtime's request
/// id ([`ApprovalId::new`]) and read it back ([`ApprovalId::as_str`]) to answer. It serializes
/// as a plain string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApprovalId(String);

impl ApprovalId {
    /// For adapters: wraps the runtime's own request id (as a string, whatever its type on
    /// the wire).
    pub fn new(id: impl Into<String>) -> Self {
        ApprovalId(id.into())
    }

    /// For adapters: the runtime's request id, to answer it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How a turn ended. Unknown values decode as [`TurnOutcome::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TurnOutcome {
    /// The agent finished.
    Completed,
    /// The turn was interrupted (by the office, or by a deny that stopped it).
    Interrupted,
    /// The turn failed.
    Failed {
        /// Why, as the runtime said.
        reason: String,
    },
    /// The model refused.
    Refused,
    /// A per-turn limit stopped it.
    Limit {
        /// Which limit.
        kind: LimitKind,
    },
    /// The account's quota ran out.
    Quota,
    /// The runtime lost its sign-in during the turn (e.g. Antigravity's "authentication
    /// required").
    SignedOut,
    /// The runtime ended the turn in a way this build doesn't know.
    #[serde(other)]
    Unknown,
}

impl TurnOutcome {
    fn redact(&mut self, f: &mut dyn FnMut(&mut String)) {
        match self {
            TurnOutcome::Failed { reason } => f(reason),
            TurnOutcome::Completed
            | TurnOutcome::Interrupted
            | TurnOutcome::Refused
            | TurnOutcome::Limit { kind: _ }
            | TurnOutcome::Quota
            | TurnOutcome::SignedOut
            | TurnOutcome::Unknown => {}
        }
    }
}

/// Which per-turn limit ended a turn. Unknown values decode as [`LimitKind::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LimitKind {
    /// The model's output limit.
    MaxTokens,
    /// The runtime's limit on agentic turns.
    MaxTurns,
    /// The runtime's own time limit.
    Timeout,
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

/// What kind of [`SessionEvent::Notice`]. Unknown values decode as [`NoticeKind::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NoticeKind {
    /// The runtime is retrying (e.g. an API call).
    Retry,
    /// A warning.
    Warning,
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{CostBasis, LimitStatus, UsageScope, Window};
    use serde_json::json;

    /// One of every event, with every string field set, so tests cover them all.
    pub(crate) fn every_event() -> Vec<SessionEvent> {
        let events = vec![
            SessionEvent::Ready {
                runtime_version: Some("s:version".into()),
                pid: Some(42),
                agents: vec!["s:agent".into()],
                permission_mode: Some("s:mode".into()),
            },
            SessionEvent::StartFailed {
                reason: StartFailure::AgentMismatch {
                    requested: "s:requested".into(),
                    available: vec!["s:available".into()],
                },
            },
            SessionEvent::StartFailed {
                reason: StartFailure::ModeMismatch {
                    requested: "s:mode_requested".into(),
                    reported: "s:mode_reported".into(),
                },
            },
            SessionEvent::StartFailed {
                reason: StartFailure::ResumeFailed {
                    detail: "s:resume".into(),
                },
            },
            SessionEvent::StartFailed {
                reason: StartFailure::Other {
                    detail: "s:other".into(),
                },
            },
            SessionEvent::StartFailed {
                reason: StartFailure::AuthRequired,
            },
            SessionEvent::StartFailed {
                reason: StartFailure::ProtocolTimeout,
            },
            SessionEvent::SessionStarted {
                runtime_session_id: SessionId::try_from("id:session").unwrap(),
            },
            SessionEvent::TurnStarted {
                turn_id: Some("id:turn".into()),
                solicited: false,
            },
            SessionEvent::Status(Activity::WaitingOnApproval),
            SessionEvent::TextDelta {
                text: "s:delta".into(),
            },
            SessionEvent::AssistantText {
                message_id: Some("id:message".into()),
                text: "s:text".into(),
            },
            SessionEvent::Thinking,
            SessionEvent::ToolStarted {
                tool_use_id: "id:tool_use".into(),
                name: "s:tool".into(),
                input: json!({
                    "s:key": ["s:nested", {"s:deep": "s:deeper"}, 1, true, null],
                    "type": "s:typed",
                }),
            },
            SessionEvent::ToolFinished {
                tool_use_id: "id:tool_use".into(),
                is_error: true,
            },
            SessionEvent::ApprovalRequested(ApprovalRequest {
                id: ApprovalId::new("id:approval"),
                kind: ApprovalKind::Command,
                tool: "s:tool".into(),
                display_name: Some("s:display".into()),
                input: json!({"command": "s:command", "s:k": {"s:k2": ["s:v"]}}),
                justification_untrusted: Some("s:why".into()),
                blocked_path: Some("s:path".into()),
                tool_use_id: Some("id:tool_use".into()),
            }),
            SessionEvent::ApprovalCancelled {
                id: ApprovalId::new("id:approval"),
            },
            SessionEvent::ActionDenied {
                tool: "s:tool".into(),
                tool_use_id: Some("id:tool_use".into()),
                message: Some("s:denied".into()),
            },
            SessionEvent::Usage(Usage {
                model: Some("s:model".into()),
                input: 1,
                output: 2,
                cache_read: 3,
                cache_write: 4,
                reasoning: 5,
                cost_usd: Some(0.25),
                cost_basis: CostBasis::ListPriceEstimate,
                scope: UsageScope::TurnDelta,
            }),
            SessionEvent::RateLimit(RateLimit {
                window: Window::Other {
                    id: "s:window".into(),
                    mins: Some(60),
                },
                used_fraction: Some(0.5),
                resets_at: Some("s:resets".into()),
                status: LimitStatus::Warning,
            }),
            SessionEvent::TurnEnded {
                outcome: TurnOutcome::Failed {
                    reason: "s:reason".into(),
                },
                errors: vec!["s:error".into()],
            },
            SessionEvent::TurnEnded {
                outcome: TurnOutcome::Limit {
                    kind: LimitKind::MaxTurns,
                },
                errors: vec![],
            },
            SessionEvent::TurnEnded {
                outcome: TurnOutcome::SignedOut,
                errors: vec![],
            },
            SessionEvent::Notice {
                kind: NoticeKind::Retry,
                text: "s:notice".into(),
            },
            SessionEvent::ProtocolError {
                detail: "s:protocol".into(),
            },
            SessionEvent::Truncated { len: 5_000_000 },
            SessionEvent::Unknown {
                type_name: "s:type".into(),
            },
            SessionEvent::Ended {
                exit: ExitInfo {
                    code: None,
                    signal: Some(15),
                },
            },
        ];
        // Exhaustive: a new variant fails to compile here until it has a sample above.
        for e in &events {
            match e {
                SessionEvent::Ready { .. }
                | SessionEvent::StartFailed { .. }
                | SessionEvent::SessionStarted { .. }
                | SessionEvent::TurnStarted { .. }
                | SessionEvent::Status(_)
                | SessionEvent::TextDelta { .. }
                | SessionEvent::AssistantText { .. }
                | SessionEvent::Thinking
                | SessionEvent::ToolStarted { .. }
                | SessionEvent::ToolFinished { .. }
                | SessionEvent::ApprovalRequested(_)
                | SessionEvent::ApprovalCancelled { .. }
                | SessionEvent::ActionDenied { .. }
                | SessionEvent::Usage(_)
                | SessionEvent::RateLimit(_)
                | SessionEvent::TurnEnded { .. }
                | SessionEvent::Notice { .. }
                | SessionEvent::ProtocolError { .. }
                | SessionEvent::Truncated { .. }
                | SessionEvent::Unknown { .. }
                | SessionEvent::Ended { .. } => {}
            }
        }
        events
    }

    fn tag(e: &SessionEvent) -> String {
        serde_json::to_value(e).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn every_event_round_trips_through_serde() {
        let mut tags = std::collections::BTreeSet::new();
        for event in every_event() {
            let json = serde_json::to_string(&event).unwrap();
            let back: SessionEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(back, event, "{json}");
            tags.insert(tag(&event));
        }
        // Every variant's tag is a known one, and every known tag has a sample.
        let known: std::collections::BTreeSet<String> =
            KNOWN_EVENT_TYPES.iter().map(|s| s.to_string()).collect();
        assert_eq!(tags, known);
    }

    #[test]
    fn event_wire_shapes() {
        let cases = [
            (SessionEvent::Thinking, json!({"type": "thinking"})),
            (
                SessionEvent::Ended {
                    exit: ExitInfo {
                        code: Some(0),
                        signal: None,
                    },
                },
                json!({"type": "ended", "data": {"exit": {"code": 0, "signal": null}}}),
            ),
            (
                SessionEvent::ApprovalRequested(ApprovalRequest {
                    id: ApprovalId::new("7"),
                    kind: ApprovalKind::FileChange,
                    tool: "Edit".into(),
                    display_name: None,
                    input: json!({}),
                    justification_untrusted: None,
                    blocked_path: None,
                    tool_use_id: None,
                }),
                json!({"type": "approval_requested", "data": {
                    "id": "7", "kind": "file_change", "tool": "Edit", "display_name": null,
                    "input": {}, "justification_untrusted": null, "blocked_path": null,
                    "tool_use_id": null}}),
            ),
            (
                SessionEvent::Status(Activity::Busy),
                json!({"type": "status", "data": "busy"}),
            ),
            (
                SessionEvent::TextDelta { text: "hi".into() },
                json!({"type": "text_delta", "data": {"text": "hi"}}),
            ),
            (
                SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Limit {
                        kind: LimitKind::MaxTokens,
                    },
                    errors: vec![],
                },
                json!({"type": "turn_ended", "data": {
                    "outcome": {"type": "limit", "kind": "max_tokens"}, "errors": []}}),
            ),
            (
                SessionEvent::ApprovalCancelled {
                    id: ApprovalId::new("7"),
                },
                json!({"type": "approval_cancelled", "data": {"id": "7"}}),
            ),
        ];
        for (event, expected) in cases {
            assert_eq!(serde_json::to_value(&event).unwrap(), expected);
        }
    }

    #[test]
    fn unknown_event_types_decode_as_unknown() {
        let e: SessionEvent =
            serde_json::from_value(json!({"type": "reasoning_summary", "data": {"n": 3}})).unwrap();
        assert_eq!(
            e,
            SessionEvent::Unknown {
                type_name: "reasoning_summary".into()
            }
        );
        let e: SessionEvent = serde_json::from_value(json!({"type": "new_unit"})).unwrap();
        assert_eq!(
            e,
            SessionEvent::Unknown {
                type_name: "new_unit".into()
            }
        );
        // A known type with a bad body is an error, not Unknown.
        assert!(serde_json::from_value::<SessionEvent>(
            json!({"type": "text_delta", "data": {"txt": 1}})
        )
        .is_err());
        assert!(serde_json::from_value::<SessionEvent>(json!({"data": {}})).is_err());
        assert!(serde_json::from_value::<SessionEvent>(json!("ended")).is_err());
    }

    #[test]
    fn unknown_enum_values_decode_as_unknown() {
        let a: Activity = serde_json::from_str(r#""napping""#).unwrap();
        assert_eq!(a, Activity::Unknown);
        let k: LimitKind = serde_json::from_str(r#""max_budget""#).unwrap();
        assert_eq!(k, LimitKind::Unknown);
        let n: NoticeKind = serde_json::from_str(r#""info""#).unwrap();
        assert_eq!(n, NoticeKind::Unknown);
        let k: ApprovalKind = serde_json::from_str(r#""network""#).unwrap();
        assert_eq!(k, ApprovalKind::Unknown);
        let o: TurnOutcome = serde_json::from_value(json!({"type": "paused", "why": "x"})).unwrap();
        assert_eq!(o, TurnOutcome::Unknown);
        let o: TurnOutcome =
            serde_json::from_value(json!({"type": "limit", "kind": "max_budget"})).unwrap();
        assert_eq!(
            o,
            TurnOutcome::Limit {
                kind: LimitKind::Unknown
            }
        );
        let s: StartFailure =
            serde_json::from_value(json!({"type": "binary_missing", "path": "/x"})).unwrap();
        assert_eq!(s, StartFailure::Unknown);
        // Nested inside an event too.
        let e: SessionEvent =
            serde_json::from_value(json!({"type": "status", "data": "napping"})).unwrap();
        assert_eq!(e, SessionEvent::Status(Activity::Unknown));
    }

    #[test]
    fn redact_visits_every_free_text_string() {
        for mut event in every_event() {
            let before = serde_json::to_value(&event).unwrap();
            let mut visited = Vec::new();
            event.redact(&mut |s| {
                visited.push(s.clone());
                s.insert_str(0, "R:");
            });
            let after = serde_json::to_value(&event).unwrap();
            // Every string in the serialized event that a sample marked as free text ("s:")
            // was visited and changed; every id ("id:") was left alone.
            let mut strings = Vec::new();
            collect_strings(&before, &mut strings);
            for s in strings.iter().filter(|s| s.starts_with("s:")) {
                assert!(visited.contains(s), "{s} not visited in {before}");
            }
            assert!(
                visited.iter().all(|s| !s.starts_with("id:")),
                "an id was visited: {visited:?}"
            );
            let mut redacted = Vec::new();
            collect_strings(&after, &mut redacted);
            assert!(
                redacted.iter().all(|s| !s.starts_with("s:")),
                "unredacted text left in {after}"
            );
        }
    }

    #[test]
    fn redact_reaches_nested_values_and_keys() {
        let mut event = SessionEvent::ToolStarted {
            tool_use_id: "toolu_1".into(),
            name: "Bash".into(),
            input: json!({
                "command": "echo TOKEN=abc",
                "env": {"TOKEN=abc": ["x", {"y": "TOKEN=abc"}]},
            }),
        };
        event.redact(&mut |s| *s = s.replace("TOKEN=abc", "TOKEN=***"));
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("abc"), "{json}");
        assert!(json.contains("toolu_1"));
    }

    /// Every string in a JSON value, object keys included.
    fn collect_strings(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => out.push(s.clone()),
            Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
            Value::Object(m) => {
                for (k, x) in m {
                    out.push(k.clone());
                    collect_strings(x, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn approval_id_is_opaque() {
        let id = ApprovalId::new("req-0001");
        // It's a plain string on the wire, and comes back unchanged.
        assert_eq!(serde_json::to_value(&id).unwrap(), json!("req-0001"));
        let back: ApprovalId = serde_json::from_value(json!("req-0001")).unwrap();
        assert_eq!(back, id);
        assert_eq!(back.as_str(), "req-0001");
        // Compared by value only; a numeric wire id is the adapter's to stringify.
        assert_ne!(ApprovalId::new("1"), ApprovalId::new("01"));
        assert!(serde_json::from_value::<ApprovalId>(json!(1)).is_err());
    }
}
