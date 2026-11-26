//! Message values: priority, lifecycle state, notify and failure reasons, snooze
//! targets, headers and whole messages.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::*;

/// How soon a message needs attention. An omitted priority means `normal`, except that a
/// `request` (an `ask`) defaults to `urgent`; the server decides. Unknown values decode as
/// [`MessagePriority::Unknown`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MessagePriority {
    /// Normal priority delivery.
    #[default]
    Normal,
    /// Elevated priority: surfaced promptly to a busy recipient at a safe step boundary.
    High,
    /// Highest priority: surfaced promptly to a busy recipient at a safe step boundary.
    Urgent,
    /// A value this build doesn't know (sent by a newer peer). Not urgent. Refused as input:
    /// `msg.send`, `msg.reply`, `msg.ask` and spool ingest refuse it (`invalid_request`).
    #[serde(other)]
    Unknown,
}

/// Where a message is in its recipient's inbox. Unknown values decode as
/// [`MessageLifecycleState::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MessageLifecycleState {
    /// Not yet read or triaged. An ask to a person or the owner stays `unread` past its
    /// deadline, flagged `expired_waiting_person`.
    Unread,
    /// Deferred with `msg.defer`; hidden from the default inbox until it's due.
    Snoozed,
    /// Read (or answered).
    Read,
    /// Closed with `msg.done`; never resurfaces.
    Done,
    /// A request to an agent whose deadline passed before the agent acted on it. (An ask to a
    /// person or the owner never becomes `expired`: see
    /// [`MessageHeader::expired_waiting_person`].)
    Expired,
    /// Couldn't be delivered (e.g. the recipient was removed); its sender got a
    /// `delivery_failed`.
    DeadLetter,
    /// A value this build doesn't know (sent by a newer peer).
    #[serde(other)]
    Unknown,
}

/// Why `msg.notify` reports an item. Unknown values decode as [`NotifyReason::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NotifyReason {
    /// Unread and not snoozed: urgent, or reported at a break (`after_task`).
    New,
    /// Its time-based snooze (`until`) is due, so it was released.
    Due,
    /// It was snoozed until the next break, and this is that break (`after_task`).
    AfterTask,
    /// Normal, untriaged and older than `sweep_after_ms`, so it's surfaced mid-task.
    Sweep,
    /// A value this build doesn't know (sent by a newer peer).
    #[serde(other)]
    Unknown,
}

/// Why a message couldn't be delivered. Unknown values decode as
/// [`DeliveryFailedReason::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeliveryFailedReason {
    /// The recipient isn't (any more) an agent or person of the organization.
    UnknownRecipient,
    /// A request to an agent whose deadline passed before the agent acted on it.
    Expired,
    /// A value this build doesn't know (sent by a newer peer).
    #[serde(other)]
    Unknown,
}

/// When a deferred message comes back (`msg.defer`'s `until`). On the wire:
/// `{"type":"until","at":"2026-11-08T10:00:00.000Z"}` or `{"type":"after_task"}`. Unknown
/// types decode as [`SnoozeTarget::Unknown`], which a server refuses (`invalid_request`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SnoozeTarget {
    /// Due at `at`, an RFC 3339 UTC time in the future.
    Until { at: String },
    /// Due at the recipient's next natural break (`msg.notify` with `after_task`).
    AfterTask,
    /// A type this build doesn't know (sent by a newer peer).
    #[serde(other)]
    Unknown,
}

/// Everything about a message except its `body` and `meta`. Inbox listings, events and
/// "Needs you" rows carry headers, so they stay small and never leak content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageHeader {
    /// Client-made (a ULID).
    pub id: String,
    /// The id of the thread's first message; a message that starts a thread has its own id.
    pub thread_id: String,
    /// The message this one answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    /// The sender's member id, or [`OFFICE_SENDER`] (`office`) for a `delivery_failed` notice
    /// from the server.
    pub from_member_id: String,
    /// The sender's display name; at most [`MAX_HANDLE_BYTES`].
    pub from_display_name: String,
    pub to_member_id: String,
    /// The recipient's name as addressed at send time; at most [`MAX_HANDLE_BYTES`].
    pub to_display_name: String,
    /// [`KIND_MSG`], [`KIND_REQUEST`], [`KIND_REPLY`], [`KIND_DELIVERY_FAILED`] or a custom
    /// tag.
    pub kind: String,
    pub priority: MessagePriority,
    /// Always [`is_urgent`]`(kind, priority)`; sent so clients don't recompute it.
    pub urgent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub created_at: String,
    /// An ask's deadline: `created_at + ttl_ms`, set once by the server. Absent on other
    /// kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// An ask's `ttl_ms`. Absent on other kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
    /// The body's size, as JSON-encoded bytes.
    pub body_bytes: u64,
    /// The meta's size, as compact JSON; 0 when there's none.
    #[serde(default)]
    pub meta_bytes: u64,
    /// The commit order at this office. Paging cursors use it; it differs between offices.
    pub local_seq: u64,
    pub state: MessageLifecycleState,
    /// Bumped each time the message is deferred or released from a snooze, so `msg.notify`
    /// reports it again at most once per generation.
    #[serde(default)]
    pub triage_gen: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<String>,
    /// Snoozed until the recipient's next break.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snooze_after_task: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snooze_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_at: Option<String>,
    /// An ask to a person or the owner whose deadline passed. Its state stays `unread`: it
    /// stays in their Needs you, counts in `total_urgent_unread`, and a late reply is still
    /// delivered. (The asker's `msg.ask` got `timeout`.)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub expired_waiting_person: bool,
}

/// A whole message: its header, `body` and `meta`. The header is nested, never flattened.
///
/// Its `Debug` prints the header and only the sizes of `body` and `meta`, never their
/// content.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub header: MessageHeader,
    pub body: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: Meta,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message")
            .field("header", &self.header)
            .field("body", &BodySize(&self.body))
            .field("meta", &MetaSize(&self.meta))
            .finish()
    }
}
