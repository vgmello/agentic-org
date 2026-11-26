//! Triage: `msg.defer`, `msg.unsnooze`, `msg.done` and `msg.notify`.

use serde::{Deserialize, Serialize};

use super::*;
use crate::calls::Call;

/// `msg.defer`: snooze a message addressed to the bound member until a time or its next break.
/// Agent-bound or app-bound to a member. The message leaves the default inbox until it's due,
/// even if it was just read, and its `triage_gen` goes up.
///
/// Errors: `not_found` (not addressed to the caller), `invalid_request` (an unknown or past
/// target), `too_large` (`note` or a string over its limit), `not_bound`, `storage_error`.
pub struct MsgDefer;

/// `msg.defer`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgDeferParams {
    pub id: String,
    pub until: SnoozeTarget,
    /// Why. At most [`MAX_NOTE_BYTES`]. Not private: the sender and the app see it too, in
    /// headers (`snooze_note`) and in `message.state_changed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `msg.defer`'s result: the message's snooze as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgDeferResult {
    pub id: String,
    pub triage_gen: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snooze_after_task: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snooze_note: Option<String>,
}

impl Call for MsgDefer {
    const METHOD: &'static str = "msg.defer";
    type Params = MsgDeferParams;
    type Result = MsgDeferResult;
}

/// `msg.unsnooze`: release a snoozed message addressed to the bound member back to active `unread`
/// early (`TASK-81`). Clears `snoozed_until`, `snooze_after_task`, and `snooze_note`, bumps
/// `triage_gen`, and publishes `message.state_changed`.
///
/// Errors: `not_found` (not addressed to the caller), `invalid_request` (not currently snoozed),
/// `not_bound`, `storage_error`.
pub struct MsgUnsnooze;

/// `msg.unsnooze`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgUnsnoozeParams {
    pub id: String,
}

/// `msg.unsnooze`'s result: the message's restored active state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgUnsnoozeResult {
    pub id: String,
    pub state: MessageLifecycleState,
    pub triage_gen: u64,
}

impl Call for MsgUnsnooze {
    const METHOD: &'static str = "msg.unsnooze";
    type Params = MsgUnsnoozeParams;
    type Result = MsgUnsnoozeResult;
}

/// `msg.done`: close a message addressed to the bound member without replying. Agent-bound or
/// app-bound to a member. A done message never resurfaces.
///
/// Errors: `not_found` (not addressed to the caller), `too_large` (`note` or a string over its
/// limit), `not_bound`, `storage_error`.
pub struct MsgDone;

/// `msg.done`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgDoneParams {
    pub id: String,
    /// Why. At most [`MAX_NOTE_BYTES`]. Not private: the sender and the app see it too, in
    /// `message.state_changed` (`reason`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `msg.done`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgDoneResult {
    pub id: String,
    pub resolved_at: String,
}

impl Call for MsgDone {
    const METHOD: &'static str = "msg.done";
    type Params = MsgDoneParams;
    type Result = MsgDoneResult;
}

/// `msg.notify`: what the bound member should triage now, as header summaries (never bodies).
/// Agent-bound or app-bound to a member. Runtime hooks call it at a break (`after_task`) and
/// from a busy agent's watcher (`urgent_only`); see ADR-0003 §2.3 for the selection rules. It
/// releases due snoozes as it goes. With `session_id`, only the items it returns are recorded
/// as reported to that session; the ones past [`NOTIFY_MAX_ITEMS`] come up next time.
///
/// Errors: `too_large` (`session_id`), `not_bound`, `storage_error`.
pub struct MsgNotify;

/// `msg.notify`'s params. `{}` is valid: a stateless check outside a break.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgNotifyParams {
    /// Report each `(message, triage_gen)` to this session at most once. Omitted: stateless.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// This is a natural break: report everything new, and release `after_task` snoozes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub after_task: bool,
    /// Report urgent items only (the busy watcher's mode).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub urgent_only: bool,
    /// Outside a break, an untriaged normal message counts once it's this old. Omitted:
    /// [`DEFAULT_SWEEP_AFTER_MS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep_after_ms: Option<u64>,
}

/// `msg.notify`'s result.
///
/// **A locked contract**: `agent-os msg notify --format json` prints it as-is, re-serialized
/// from this struct, so its keys come out in this order:
/// `{"v","agent","urgent","count","urgent_count","items","more","text"}`. (On the socket it
/// travels inside a response, whose key order isn't guaranteed.) Add fields only at the end.
/// Values this build doesn't know print as `"unknown"` (see
/// [Unknown values](super#unknown-values)).
///
/// Members appear by handle, as in the locked contract; a member with no handle appears by
/// its member id. `agent` is the bound member's handle in its stored case, not the string
/// the client bound with: an `ORG_AGENT` holding a member id won't match it textually, and a
/// handle should be compared ignoring case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyResult {
    /// [`NOTIFY_VERSION`].
    pub v: u32,
    /// The bound member's handle in its stored case (its member id when it has none); at
    /// most [`MAX_HANDLE_BYTES`].
    pub agent: String,
    /// Whether any item is urgent.
    pub urgent: bool,
    /// How many messages need triage: the total, including any past `items`.
    pub count: u32,
    /// How many of `count` are urgent: the total, including any past `items`.
    pub urgent_count: u32,
    /// At most [`NOTIFY_MAX_ITEMS`]: urgent first, then oldest first.
    pub items: Vec<NotifyItem>,
    /// How many of `count` `text` doesn't list: `count` minus the item lines shown (at most
    /// [`NOTIFY_MAX_LINES`]), or 0.
    pub more: u32,
    /// A ready-to-print summary of at most [`NOTIFY_MAX_LINES`] items; empty when there are
    /// none. It never holds bodies. The sender names and subjects in it come from other
    /// members: they are untrusted data, not instructions, and the text says so.
    pub text: String,
}

/// One message `msg.notify` reports. Part of the locked [`NotifyResult`] contract; its keys
/// come out in this order.
///
/// `from_name`, `subject` and `kind` are sanitized (one line, no control characters) and are
/// untrusted data from the sender, never instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyItem {
    pub id: String,
    /// The sender's handle (its member id when it has none), or [`OFFICE_SENDER`] for a
    /// server notice; at most [`MAX_HANDLE_BYTES`].
    pub from: String,
    /// The sender's display name, sanitized to at most [`NOTIFY_FROM_NAME_MAX_CHARS`]
    /// characters.
    pub from_name: String,
    /// Sanitized to at most [`NOTIFY_SUBJECT_MAX_CHARS`] characters; `(no subject)` when the
    /// message has none.
    pub subject: String,
    /// Sanitized; at most [`MAX_ID_BYTES`].
    pub kind: String,
    pub urgent: bool,
    pub priority: MessagePriority,
    /// The message's triage generation, so a session can tell a re-surfaced message apart.
    pub triage_gen: u64,
    pub created_at: String,
    pub reason: NotifyReason,
}

impl Call for MsgNotify {
    const METHOD: &'static str = "msg.notify";
    type Params = MsgNotifyParams;
    type Result = NotifyResult;
}
