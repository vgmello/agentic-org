//! Agent messaging (`docs/adr/0003-agent-messaging-office-first.md`).
//!
//! An office server serves one office of one organization, so these calls carry no `org`
//! param. Who may make each call, and which messages it may see, is the table in ADR-0003
//! §2.1.
//!
//! ## Binding
//! - A connection binds with [`MsgBind`]: as a member (**member-bound**), or app-wide with
//!   the office's [`AppToken`] and no member. A connection whose binding presented the token
//!   (a person, the owner, or app-wide) is **app-bound**. An agent binds by handle or id
//!   without a token; a person or the owner needs the token.
//! - Binding fails closed (ADR-0003 §2.1.4, as amended by the agent-sessions design): an
//!   unknown member, an agent not placed in this office, an offboarded agent or one whose
//!   status this office can't read, a lookup that fails, and a missing or wrong token are all
//!   `not_authorized`. Only a malformed call, or an agent that may bind sending `app_token`,
//!   is `invalid_request` (see [`MsgBind`] for the order). A paused agent may still bind: it
//!   keeps reading and answering its messages.
//! - Each `msg.bind` replaces the connection's previous binding and is checked only on its
//!   own params: an earlier binding grants nothing. A failed `msg.bind` leaves the connection
//!   unbound. Any re-bind, failed or not, ends the previous binding's held `msg.wait` and
//!   `msg.ask` calls with `not_bound`.
//! - Until a connection binds, every other call here, and `who` and `status.set`
//!   ([`crate::directory`]), fails with `not_bound`. So does a call on a connection whose
//!   member was since moved out of this office or removed (ADR-0003 §3.1); it must bind again.
//! - A member-bound connection acts as its member. An agent sees only the messages it sent or
//!   received; an app-bound connection sees every message in the office.
//! - An app-wide binding may call only `who`, `msg.list`, `msg.get`, `msg.threads` and
//!   `msg.human_needs_you`. Calls that act as a member (`msg.send`, `msg.inbox`,
//!   `status.set`, ...) fail on it with `not_bound`.
//!
//! ## Events
//! Message events ([`MessageReceived`], [`MessageStateChanged`], [`MessageDeliveryFailed`])
//! go to app-bound connections, and to other member-bound connections only for messages
//! their member sent or received; never to unbound connections. Servers filter them per
//! connection.
//!
//! ## Content stays out of logs
//! Bodies and `meta` are never in `Debug` output (the types here that hold them print only
//! their sizes) and never in events (events carry a [`MessageHeader`]).
//!
//! A server answers params that fail to decode (for `msg.*`, `who` and `status.set`) with
//! `invalid_request` and a fixed message. It never echoes or logs serde's error text, which
//! can quote a body, meta or note value. Clients likewise never log a
//! [`crate::CallError::Decode`] (its `Display` or `Debug`) for a `msg.*` result: it can
//! quote a body or meta value.
//!
//! ## Addressing
//! A recipient or member (`to`, `from`, `member`, `participant`) resolves in this order
//! (ADR-0003 §2.6):
//! 1. [`OWNER_HANDLE`](crate::OWNER_HANDLE) (`owner`, ignoring case) is the organization's
//!    owner. It comes before ids, so a member whose id is `owner` can't capture it. The
//!    owner is addressed only as `owner`: the owner's name is not a handle.
//! 2. An exact member id (case-sensitive). It beats a handle.
//! 3. A handle, ignoring case: an agent's
//!    [`messaging_name`](crate::org::AgentProfile::messaging_name) or a person's `name`. The
//!    handles form one namespace, so a handle names at most one member.
//! 4. Anything else names no member. Display names never resolve.
//!
//! [`OFFICE_SENDER`] (`office`, ignoring case) is reserved for the server and refused before
//! any of this: a `to`, `from`, `member` or `participant` of `office` is `invalid_request`.
//! It appears only as the sender of a `delivery_failed` notice.
//!
//! A recipient (`msg.send`, `msg.ask`, and the sender a `msg.reply` answers) must be in the
//! organization: an [`Offboarded`](crate::org::AgentStatus::Offboarded) agent, an agent
//! whose status is [`Unknown`](crate::org::AgentStatus::Unknown), or a name that matches no
//! member is `unknown_recipient`. A [`Paused`](crate::org::AgentStatus::Paused) agent still
//! receives: its messages wait in its inbox.
//!
//! ## Limits and their errors
//! String limits are measured on the JSON encoding of the value as serde_json encodes it
//! (compact, non-ASCII unescaped): a string with its quotes and escapes
//! ([`json_string_len`]), and `meta` as compact JSON. So a message within its limits always
//! fits in [`crate::MAX_FRAME_BYTES`], and so do a page of at most [`MAX_PAGE_LIMIT`] headers
//! and a `msg.notify` result (at most [`NOTIFY_MAX_ITEMS`] sanitized items).
//!
//! One rule for errors: a string, body, meta or note over its byte limit is `too_large`; a
//! number out of range (a `limit`, `timeout_ms`, `wait_ms` or `ttl_ms`) is
//! `invalid_request`.
//!
//! ## Differences from the ADR-0003 text
//! Deliberate, and this crate is the contract where they differ (the `who` item is in
//! [`crate::directory`]):
//! - Headers carry `state` (the ADR's inbox list calls it `status`).
//! - `who` has no `kind: owner`; find the owner with `member: "owner"`.
//! - `reply_to` on `msg.send` is checked for app-bound callers too, not only agent-bound
//!   ones.
//! - `msg.bind` takes `member`, also read as `agent` (the ADR's name).
//! - `msg.send` and [`SpooledSend`] have no `ttl_ms` or `expires_at`: only asks have a
//!   deadline, and asks are never spooled.
//! - `msg.ask`'s `ttl_ms` is optional (a default by recipient), and its result is
//!   `{reply?, expires_at}`: no reply means `wait_ms` passed, so re-ask.
//! - [`Message`] nests its header (`{header, body, meta}`) instead of flattening it.
//! - Member references, handles and display names have their own limit,
//!   [`MAX_HANDLE_BYTES`], rather than the 128 bytes of ids.
//! - A `msg.notify` result holds at most [`NOTIFY_MAX_ITEMS`] items; `count` and
//!   `urgent_count` still count them all.
//!
//! ## Unknown values
//! Enum values this build doesn't know decode as `Unknown` and re-serialize as `"unknown"`.
//! So a client that prints a result re-serialized (e.g. `org-agent msg notify --format json`)
//! may show `unknown` for a value from a newer server.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::calls::Call;
use crate::events::EventKind;
use crate::token::AppToken;

/// A plain message (`msg.send`'s usual kind).
pub const KIND_MSG: &str = "msg";
/// A request: an `ask` that waits for a reply. Always urgent (see [`is_urgent`]). Only
/// `msg.ask` sends it: `msg.send` and spool ingest refuse it (`invalid_request`).
pub const KIND_REQUEST: &str = "request";
/// A reply (`msg.reply`, or a `msg.send` with `reply_to`).
pub const KIND_REPLY: &str = "reply";
/// A notice from the server that a message couldn't be delivered (see
/// [`MessageDeliveryFailed`]). Clients may not send this kind, or reply to such a notice:
/// `invalid_request`.
///
/// Every other kind is a custom tag the server stores and passes on unread.
pub const KIND_DELIVERY_FAILED: &str = "delivery_failed";
/// The sender of messages the server itself sends (`delivery_failed` notices), in place of a
/// member id or handle. Reserved: no client may bind as, send to or address `office`.
pub const OFFICE_SENDER: &str = "office";

/// The largest `body`, in JSON-encoded bytes (256 KiB).
pub const MAX_BODY_BYTES: usize = 256 * 1024;
/// The largest `meta`, as compact JSON (64 KiB).
pub const MAX_META_BYTES: usize = 64 * 1024;
/// The largest `subject`, in JSON-encoded bytes.
pub const MAX_SUBJECT_BYTES: usize = 1024;
/// The largest snooze note (`msg.defer`) or done note (`msg.done`), in JSON-encoded bytes.
pub const MAX_NOTE_BYTES: usize = 1024;
/// The largest id, kind, session id or timestamp (e.g. `ThreadCursor::last_msg_at`,
/// `SnoozeTarget::Until { at }`), in JSON-encoded bytes.
pub const MAX_ID_BYTES: usize = 128;
/// The largest member or team reference (`to`, `from`, `member`, `participant`, `who`'s `team`),
/// handle or display name, in JSON-encoded bytes: 64 characters of up to 4 bytes each, plus the
/// quotes. A person's handle is their name, which follows the organization-name rule of up to 64
/// characters.
pub const MAX_HANDLE_BYTES: usize = 258;
/// The longest a single socket call may hold (`msg.wait`'s `timeout_ms`, `msg.ask`'s
/// `wait_ms`): one hour. A longer wait is a loop of calls. More is `invalid_request`.
pub const MAX_WAIT_MS: u64 = 3_600_000;
/// The longest `ttl_ms` an ask may have: 24 hours. More is `invalid_request`.
pub const MAX_TTL_MS: u64 = 86_400_000;
/// The most `msg.wait` and `msg.ask` calls one connection may have held at once. Another one
/// is refused with `unavailable` until one ends.
pub const MAX_HELD_CALLS_PER_CONNECTION: usize = 16;
/// The most items one page of `msg.inbox`, `msg.list`, `msg.threads` or
/// `msg.human_needs_you` holds; a larger `limit` is refused (`invalid_request`).
pub const MAX_PAGE_LIMIT: u32 = 100;
/// The page size when `limit` is omitted.
pub const DEFAULT_PAGE_LIMIT: u32 = 50;
/// An `ask`'s deadline when `ttl_ms` is omitted and the recipient is an agent: 2 minutes.
pub const DEFAULT_ASK_TTL_MS_AGENT: u64 = 120_000;
/// An `ask`'s deadline when `ttl_ms` is omitted and the recipient is a person or the owner:
/// 24 hours, since people answer on human timescales.
pub const DEFAULT_ASK_TTL_MS_PERSON: u64 = 86_400_000;
/// `msg.notify`'s `sweep_after_ms` when omitted: 5 minutes.
pub const DEFAULT_SWEEP_AFTER_MS: u64 = 300_000;
/// The most items `msg.notify`'s `text` lists; the rest are counted in `more`.
pub const NOTIFY_MAX_LINES: usize = 10;
/// The most items a [`NotifyResult`] holds: urgent first, then oldest first.
pub const NOTIFY_MAX_ITEMS: usize = 100;
/// The longest [`NotifyItem::subject`], in characters, after sanitizing.
pub const NOTIFY_SUBJECT_MAX_CHARS: usize = 80;
/// The longest [`NotifyItem::from_name`], in characters, after sanitizing.
pub const NOTIFY_FROM_NAME_MAX_CHARS: usize = 48;
/// The `v` of a [`NotifyResult`] this build writes.
pub const NOTIFY_VERSION: u32 = 1;
/// The `v` of a [`SpooledSend`] this build writes.
pub const SPOOL_VERSION: u32 = 1;

/// A message's `meta`: free-form JSON the server stores and passes on. At most
/// [`MAX_META_BYTES`] as compact JSON.
pub type Meta = BTreeMap<String, serde_json::Value>;

/// The exact length in bytes of `serde_json::to_string(s)`: the string's JSON encoding with
/// its quotes and escapes. Computed without allocating, so a server can check a limit before
/// copying anything.
///
/// `"` and `\` take 2 bytes, as do `\n`, `\r`, `\t`, backspace (0x08) and form feed (0x0C);
/// other characters below 0x20 take 6 (`\u00XX`); everything else takes its UTF-8 length.
pub fn json_string_len(s: &str) -> usize {
    2 + s
        .bytes()
        .map(|b| match b {
            b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 0x08 | 0x0c => 2,
            0x00..=0x1f => 6,
            _ => 1,
        })
        .sum::<usize>()
}

/// Whether a message is urgent: it interrupts a busy agent instead of waiting for its next
/// break. True iff `kind` is [`KIND_REQUEST`] or `priority` is `high` or `urgent`. An unknown
/// priority isn't urgent.
pub fn is_urgent(kind: &str, priority: MessagePriority) -> bool {
    kind == KIND_REQUEST || matches!(priority, MessagePriority::High | MessagePriority::Urgent)
}

/// How soon a message needs attention. An omitted priority means `normal`, except that a
/// `request` (an `ask`) defaults to `urgent`; the server decides. Unknown values decode as
/// [`MessagePriority::Unknown`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MessagePriority {
    /// Waits for the recipient's next break.
    #[default]
    Normal,
    /// Urgent: interrupts a busy recipient.
    High,
    /// Urgent: interrupts a busy recipient.
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

/// Prints a body as `<N bytes>`, N being its UTF-8 length (not its JSON-encoded size).
struct BodySize<'a>(&'a str);

impl fmt::Debug for BodySize<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{} bytes>", self.0.len())
    }
}

/// Prints a meta as `<N keys>`.
struct MetaSize<'a>(&'a Meta);

impl fmt::Debug for MetaSize<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{} keys>", self.0.len())
    }
}

/// `msg.bind`: bind this connection, as a member or app-wide (see
/// [Binding](self#binding)). Every other `msg.*` call (and `who`, `status.set`) needs it
/// first.
///
/// Any local client may call it. An agent binds without a token (an accepted risk until the
/// office runner spawns every agent, ADR-0003 M-A). A person or the owner needs `app_token`.
/// The desktop app may bind app-wide, with `app_token` and no `member`; that binding may call
/// only `who`, `msg.list`, `msg.get`, `msg.threads` and `msg.human_needs_you`.
///
/// It fails closed, checking in this order:
/// 1. The input alone: params that don't decode (including a malformed `app_token`, or both
///    `member` and `agent`), `office` as `member`, or neither `member` nor `app_token`, are
///    `invalid_request`; a `member` over [`MAX_HANDLE_BYTES`] is `too_large`. None of this
///    depends on who the member is.
/// 2. The member: one that isn't found, an agent not placed in this office, an
///    [`Offboarded`](crate::org::AgentStatus::Offboarded) agent, one whose status is
///    [`Unknown`](crate::org::AgentStatus::Unknown), or a lookup that fails, is
///    `not_authorized`, whatever else the call sends. A
///    [`Paused`](crate::org::AgentStatus::Paused) agent may still bind.
/// 3. The credentials: a missing or wrong `app_token` for a person or the owner, or a wrong
///    one for an app-wide binding, is `not_authorized`. An agent (one that passed step 2)
///    with an `app_token` is `invalid_request`: an agent binds without one.
///
/// Each call replaces the previous binding and is checked only on its own params. A failed
/// call leaves the connection unbound. Any re-bind, failed or not, ends the previous
/// binding's held `msg.wait` and `msg.ask` calls with `not_bound`.
///
/// Errors: `not_authorized` (step 2, and a missing or wrong token in step 3),
/// `invalid_request` (step 1, and an active or paused agent with `app_token` in step 3),
/// `too_large` (step 1).
pub struct MsgBind;

/// `msg.bind`'s params. Its `Debug` shows no token (see [`AppToken`]), and it isn't `Clone`.
///
/// Office-run agents will also send a session token here (a later additive field, per the
/// agent-sessions design).
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgBindParams {
    /// Who to act as (see [Addressing](self#addressing)); at most [`MAX_HANDLE_BYTES`]. Also
    /// read from `agent`, the ADR's name for it; params with both don't decode
    /// (`invalid_request`).
    #[serde(default, alias = "agent", skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// Required to bind a person or the owner (`not_authorized` without it), or to bind
    /// app-wide. Refused with an active or paused agent (`invalid_request`); see [`MsgBind`]
    /// for the order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_token: Option<AppToken>,
}

/// `msg.bind`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgBindResult {
    /// The bound member; absent for an app-wide binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    pub office_id: String,
}

impl Call for MsgBind {
    const METHOD: &'static str = "msg.bind";
    type Params = MsgBindParams;
    type Result = MsgBindResult;
}

/// `msg.send`: send a message from the bound member, fire and forget. Agent-bound or
/// app-bound to a member. A request goes through `msg.ask` instead, so `msg.send` has no
/// deadline.
///
/// With `reply_to`, the parent must be one the caller sent or received (else `not_found`), and
/// the message joins the parent's thread.
///
/// Errors: `unknown_recipient` (no such member, or an offboarded or `Unknown`-status agent;
/// a paused agent still receives), `not_found` (`reply_to` isn't visible to the caller),
/// `already_exists` (the id was used for different content), `too_large` (a string, body or
/// meta over its limit), `invalid_request` (kind `request` (use `msg.ask`) or
/// `delivery_failed`, an `Unknown` priority, a `reply_to` that is a `delivery_failed` notice,
/// or `to` is `office`), `not_bound`, `storage_error`.
pub struct MsgSend;

/// `msg.send`'s params. Its `Debug` prints only the sizes of `body` and `meta`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgSendParams {
    /// Client-made (a ULID). A retry with the same id and content returns the first result;
    /// the same id with different content is `already_exists`.
    pub id: String,
    /// The recipient (see [Addressing](self#addressing)).
    pub to: String,
    /// Usually [`KIND_MSG`]; [`KIND_REQUEST`] and [`KIND_DELIVERY_FAILED`] are refused.
    pub kind: String,
    /// Omitted: `normal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<MessagePriority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub body: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
}

impl fmt::Debug for MsgSendParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MsgSendParams")
            .field("id", &self.id)
            .field("to", &self.to)
            .field("kind", &self.kind)
            .field("priority", &self.priority)
            .field("subject", &self.subject)
            .field("body", &BodySize(&self.body))
            .field("meta", &MetaSize(&self.meta))
            .field("reply_to", &self.reply_to)
            .finish()
    }
}

/// The result of `msg.send` and `msg.reply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendResult {
    pub id: String,
    pub thread_id: String,
    /// The commit order at this office.
    pub local_seq: u64,
    /// The commit order in this office's outgoing stream (ADR-0003 §3.1).
    pub origin_seq: u64,
}

impl Call for MsgSend {
    const METHOD: &'static str = "msg.send";
    type Params = MsgSendParams;
    type Result = SendResult;
}

/// `msg.reply`: answer a message addressed to the bound member. Agent-bound or app-bound to a
/// member. The reply goes to the message's sender, in its thread, with kind `reply`, and the
/// answered message becomes `read`. Replying to a request past its deadline is allowed (a
/// late reply).
///
/// Errors: `not_found` (`to_id` isn't addressed to the caller), `unknown_recipient` (its
/// sender has since been offboarded or removed, or has an `Unknown` status), `already_exists`,
/// `too_large` (a string, body or meta over its limit), `invalid_request` (`to_id` is a
/// `delivery_failed` notice, or an `Unknown` priority), `not_bound`, `storage_error`.
pub struct MsgReply;

/// `msg.reply`'s params. Its `Debug` prints only the sizes of `body` and `meta`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgReplyParams {
    /// Client-made (a ULID); retries work as in `msg.send`.
    pub id: String,
    /// The message being answered; it must be addressed to the caller.
    pub to_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<MessagePriority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub body: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: Meta,
}

impl fmt::Debug for MsgReplyParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MsgReplyParams")
            .field("id", &self.id)
            .field("to_id", &self.to_id)
            .field("priority", &self.priority)
            .field("subject", &self.subject)
            .field("body", &BodySize(&self.body))
            .field("meta", &MetaSize(&self.meta))
            .finish()
    }
}

impl Call for MsgReply {
    const METHOD: &'static str = "msg.reply";
    type Params = MsgReplyParams;
    type Result = SendResult;
}

/// `msg.ask`: send a request (kind `request`) and hold the call until it's answered or
/// `wait_ms` passes. Agent-bound or app-bound to a member.
///
/// Re-asking with the same id and content never re-sends: it waits again for the same request
/// (idempotent re-ask, ADR-0003 §3.3.4), keeping its `expires_at`. That's how a client waits
/// past [`MAX_WAIT_MS`], or recovers from a dropped connection.
///
/// Errors: `unknown_recipient` (no such member, or an offboarded or `Unknown`-status agent;
/// a paused agent still receives), `already_exists` (the id was used for different content),
/// `too_large` (a string, body or meta over its limit), `invalid_request` (`wait_ms` or
/// `ttl_ms` out of range, an `Unknown` priority, or `to` is `office`), `timeout`
/// (`expires_at` passed without a reply), `unknown_recipient` or `timeout` when a
/// `delivery_failed` answers the request, `unavailable` (the connection already holds
/// [`MAX_HELD_CALLS_PER_CONNECTION`] calls), `not_bound` (also when a re-bind ends the call),
/// `storage_error`.
pub struct MsgAsk;

/// `msg.ask`'s params. Its `Debug` prints only the sizes of `body` and `meta`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgAskParams {
    /// Client-made (a ULID); reuse it to re-ask.
    pub id: String,
    /// The recipient (see [Addressing](self#addressing)).
    pub to: String,
    /// Omitted: `urgent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<MessagePriority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub body: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: Meta,
    /// The request's deadline, at most [`MAX_TTL_MS`]. Omitted: the server's default by
    /// recipient, [`DEFAULT_ASK_TTL_MS_AGENT`] for an agent and [`DEFAULT_ASK_TTL_MS_PERSON`]
    /// for a person or the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
    /// How long this call holds, at most [`MAX_WAIT_MS`].
    pub wait_ms: u64,
}

impl fmt::Debug for MsgAskParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MsgAskParams")
            .field("id", &self.id)
            .field("to", &self.to)
            .field("priority", &self.priority)
            .field("subject", &self.subject)
            .field("body", &BodySize(&self.body))
            .field("meta", &MetaSize(&self.meta))
            .field("ttl_ms", &self.ttl_ms)
            .field("wait_ms", &self.wait_ms)
            .finish()
    }
}

/// `msg.ask`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgAskResult {
    /// The reply. `None` means `wait_ms` passed while the request is still live: re-ask with
    /// the same id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<Message>,
    /// The request's deadline.
    pub expires_at: String,
}

impl Call for MsgAsk {
    const METHOD: &'static str = "msg.ask";
    type Params = MsgAskParams;
    type Result = MsgAskResult;
}

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
/// **A locked contract**: `org-agent msg notify --format json` prints it as-is, re-serialized
/// from this struct, so its keys come out in this order:
/// `{"v","agent","urgent","count","urgent_count","items","more","text"}`. (On the socket it
/// travels inside a response, whose key order isn't guaranteed.) Add fields only at the end.
/// Values this build doesn't know print as `"unknown"` (see
/// [Unknown values](self#unknown-values)).
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

/// `msg.wait`: hold until the bound member has an unread message, and return the oldest
/// unsnoozed one **without** marking it read. Agent-bound or app-bound to a member.
///
/// Errors: `invalid_request` (`timeout_ms` over [`MAX_WAIT_MS`], or `from` is `office`),
/// `unavailable` (the connection already holds [`MAX_HELD_CALLS_PER_CONNECTION`] calls),
/// `not_found` (`from` names no one), `too_large` (`from`), `not_bound` (also when a re-bind
/// ends the call), `storage_error`.
pub struct MsgWait;

/// `msg.wait`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgWaitParams {
    /// How long to hold, at most [`MAX_WAIT_MS`].
    pub timeout_ms: u64,
    /// Only messages from this member (see [Addressing](self#addressing)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Only urgent messages.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub urgent_only: bool,
}

/// `msg.wait`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgWaitResult {
    /// `None` when `timeout_ms` passed first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
}

impl Call for MsgWait {
    const METHOD: &'static str = "msg.wait";
    type Params = MsgWaitParams;
    type Result = MsgWaitResult;
}

/// `msg.read`: claim and mark read a message addressed to the bound member: the one named, or
/// else the oldest unsnoozed unread one. Agent-bound or app-bound to a member.
///
/// Errors: `not_found` (`id` isn't addressed to the caller), `too_large` (`id`), `not_bound`,
/// `storage_error`.
pub struct MsgRead;

/// `msg.read`'s params. `{}` reads the next message.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgReadParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// `msg.read`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgReadResult {
    /// `None` when no `id` was given and the inbox is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
}

impl Call for MsgRead {
    const METHOD: &'static str = "msg.read";
    type Params = MsgReadParams;
    type Result = MsgReadResult;
}

/// `msg.inbox`: one page of headers of messages addressed to the bound member, newest first
/// (by `local_seq`). Agent-bound or app-bound to a member.
///
/// By default it shows only unread, unsnoozed messages, following the triage spec (ADR-0003
/// §2.3: the inbox hides snoozed messages until they're due; `--all` shows them).
///
/// Errors: `invalid_request` (`limit` over [`MAX_PAGE_LIMIT`], or `from` is `office`),
/// `not_found` (`from` names no one), `too_large` (`from` or `thread_id`), `not_bound`,
/// `storage_error`.
pub struct MsgInbox;

/// `msg.inbox`'s params. `{}` is the first page of the default inbox.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgInboxParams {
    /// Add snoozed, read and done messages. By default only unread, unsnoozed ones.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_all: bool,
    /// Only messages from this member (see [Addressing](self#addressing)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Only messages with `local_seq` below this: the previous page's `next_cursor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
    /// Omitted: [`DEFAULT_PAGE_LIMIT`]. At most [`MAX_PAGE_LIMIT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// A page of headers, newest first (by `local_seq`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderPage {
    pub headers: Vec<MessageHeader>,
    /// Pass as `cursor` for the next page; absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
}

impl Call for MsgInbox {
    const METHOD: &'static str = "msg.inbox";
    type Params = MsgInboxParams;
    type Result = HeaderPage;
}

/// `msg.list`: one page of message headers by thread or participant, newest first. Agent-bound
/// or app-bound. An app-bound connection sees every message in the office. An agent sees only
/// messages it sent or received, and `participant` must be omitted or name itself.
///
/// Errors: `invalid_request` (an agent's `participant` names someone else, `participant` is
/// `office`, or `limit` over [`MAX_PAGE_LIMIT`]), `not_found`, `too_large` (`participant` or
/// `thread_id`), `not_bound`, `storage_error`.
pub struct MsgList;

/// `msg.list`'s params.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Only messages this member sent or received (see [Addressing](self#addressing)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participant: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unread_only: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_snoozed: bool,
    /// Only messages with `local_seq` below this: the previous page's `next_cursor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
    /// Omitted: [`DEFAULT_PAGE_LIMIT`]. At most [`MAX_PAGE_LIMIT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl Call for MsgList {
    const METHOD: &'static str = "msg.list";
    type Params = MsgListParams;
    type Result = HeaderPage;
}

/// `msg.get`: one whole message, without changing its state. Agent-bound or app-bound. An
/// app-bound connection may get any message in the office; an agent only one it sent or
/// received.
///
/// Errors: `not_found` (no such message, or not visible to the caller), `too_large` (`id`),
/// `not_bound`, `storage_error`.
pub struct MsgGet;

/// `msg.get`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgGetParams {
    pub id: String,
}

/// `msg.get`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgGetResult {
    pub message: Message,
}

impl Call for MsgGet {
    const METHOD: &'static str = "msg.get";
    type Params = MsgGetParams;
    type Result = MsgGetResult;
}

/// `msg.human_needs_you`: what a person or the owner needs to answer, for the menu bar's
/// "Needs you" rows and indicator dot: their urgent unread messages, including asks whose
/// deadline passed while waiting for them (`expired_waiting_person`). **App-bound only.**
///
/// Errors: `not_authorized` (bound as an agent), `invalid_request` (`member_id` is an agent,
/// or `limit` over [`MAX_PAGE_LIMIT`]), `not_found`, `too_large` (`member_id`), `not_bound`,
/// `storage_error`.
pub struct MsgHumanNeedsYou;

/// `msg.human_needs_you`'s params. `{}` is the owner's queue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgHumanNeedsYouParams {
    /// A person's or the owner's id. Omitted: the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    /// Omitted: [`DEFAULT_PAGE_LIMIT`]. At most [`MAX_PAGE_LIMIT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// `msg.human_needs_you`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanNeedsYouResult {
    pub items: Vec<MessageHeader>,
    /// Every urgent unread message for the member, including those past `limit` and asks
    /// flagged `expired_waiting_person`; the dot's count.
    pub total_urgent_unread: u32,
}

impl Call for MsgHumanNeedsYou {
    const METHOD: &'static str = "msg.human_needs_you";
    type Params = MsgHumanNeedsYouParams;
    type Result = HumanNeedsYouResult;
}

/// `msg.threads`: one page of thread summaries, most recently active first, for the app's
/// Inbox view. **App-bound only.**
///
/// Errors: `not_authorized` (bound as an agent), `invalid_request` (`participant` is
/// `office`, or `limit` over [`MAX_PAGE_LIMIT`]), `not_found`, `too_large` (`participant` or
/// a cursor field), `not_bound`, `storage_error`.
pub struct MsgThreads;

/// `msg.threads`'s params. `{}` is the first page of every thread in the office.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgThreadsParams {
    /// Only threads this member is in (see [Addressing](self#addressing)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participant: Option<String>,
    /// The previous page's `next_cursor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ThreadCursor>,
    /// Omitted: [`DEFAULT_PAGE_LIMIT`]. At most [`MAX_PAGE_LIMIT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Where a page of threads ends: threads order by `(last_msg_at, thread_id)`, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThreadCursor {
    pub last_msg_at: String,
    pub thread_id: String,
}

/// One thread, without any message content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub thread_id: String,
    /// The first message's subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub created_at: String,
    pub last_msg_id: String,
    pub last_msg_at: String,
    pub message_count: u64,
    /// The member ids of everyone who sent or received a message in it. May include
    /// [`OFFICE_SENDER`] (`office`) when the server sent a `delivery_failed` notice in it.
    /// Unbounded today; paging it would come as an additive change.
    pub participants: Vec<String>,
}

/// `msg.threads`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadPage {
    pub threads: Vec<ThreadSummary>,
    /// Pass as `cursor` for the next page; absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<ThreadCursor>,
}

impl Call for MsgThreads {
    const METHOD: &'static str = "msg.threads";
    type Params = MsgThreadsParams;
    type Result = ThreadPage;
}

/// `message.received`: a message was committed for a recipient at this office. Carries only
/// the header, so frames stay small and bodies never reach subscribers.
///
/// Sent to app-bound connections, and to other member-bound connections only for messages
/// their member sent or received; never to unbound ones (see [Events](self#events)).
pub struct MessageReceived;

/// The data of a `message.received` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageReceivedData {
    pub header: MessageHeader,
}

impl EventKind for MessageReceived {
    const NAME: &'static str = "message.received";
    type Data = MessageReceivedData;
}

/// `message.state_changed`: a message's state changed (read, snoozed, released, done,
/// expired...), or an ask to a person or the owner passed its deadline
/// (`expired_waiting_person`).
///
/// Sent to app-bound connections, and to other member-bound connections only for messages
/// their member sent or received; never to unbound ones (see [Events](self#events)).
pub struct MessageStateChanged;

/// The data of a `message.state_changed` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageStateChangedData {
    pub id: String,
    pub thread_id: String,
    pub to_member_id: String,
    pub state: MessageLifecycleState,
    pub triage_gen: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snooze_after_task: bool,
    /// A snooze or done note, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// As [`MessageHeader::expired_waiting_person`]: the state stays `unread`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub expired_waiting_person: bool,
    pub updated_at: String,
}

impl EventKind for MessageStateChanged {
    const NAME: &'static str = "message.state_changed";
    type Data = MessageStateChangedData;
}

/// `message.delivery_failed`: a message couldn't be delivered. The server also sends its
/// sender a `delivery_failed` message (from [`OFFICE_SENDER`], replying to the original), which
/// answers a held `msg.ask`.
///
/// Sent to app-bound connections, and to other member-bound connections only for messages
/// their member sent or received; never to unbound ones (see [Events](self#events)).
pub struct MessageDeliveryFailed;

/// The data of a `message.delivery_failed` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDeliveryFailedData {
    /// The `delivery_failed` message's id.
    pub id: String,
    /// The message that couldn't be delivered.
    pub original_id: String,
    pub thread_id: String,
    /// The original's sender, who gets the notice.
    pub from_member_id: String,
    pub recipient_id: String,
    /// The recipient's name as addressed at send time; at most [`MAX_HANDLE_BYTES`].
    pub recipient_name: String,
    pub reason: DeliveryFailedReason,
}

impl EventKind for MessageDeliveryFailed {
    const NAME: &'static str = "message.delivery_failed";
    type Data = MessageDeliveryFailedData;
}

/// A `msg.send` written to the office's spool by `org-agent` while its office server is
/// unreachable (ADR-0003 §3.5), and ingested by the office when it starts. Only
/// fire-and-forget sends are spooled, never asks, so it has no deadline.
///
/// `from` is unauthenticated (whoever wrote the file), so ingest applies every `msg.send`
/// rule as if `from` had bound as an agent without a token: the limits apply, kinds `request`
/// and `delivery_failed`, an `Unknown` priority and an `office` recipient are refused, the
/// recipient rules of [Addressing](self#addressing) apply, and `reply_to` must be visible to
/// `from`. On top of that, `from` must resolve to an **active agent that the office doesn't
/// run** (one with no run mode, per the agent-sessions design): agents the office runs will
/// bind with a session token (see [`MsgBindParams`]), which a file can't carry. A `from` that
/// is a person, the owner, `office`, a `run:` address (agent-sessions design), an office-run
/// agent, an agent whose status isn't [`Active`](crate::org::AgentStatus::Active)
/// ([`Paused`](crate::org::AgentStatus::Paused),
/// [`Offboarded`](crate::org::AgentStatus::Offboarded) or
/// [`Unknown`](crate::org::AgentStatus::Unknown)), or a handle or id that names no member is
/// refused.
///
/// A `created_at` in the future is clamped to the ingest time. A backdated `created_at` is
/// accepted: like `from`, it's the writer's word (the agent-identity risk, ADR-0003 M-A). A
/// file whose `office_id` isn't this office, with an unknown `v`, or that fails a rule, is
/// moved to `spool/dead/` and never delivered. The office logs the file's id and the reason,
/// never its body or meta.
///
/// Its `Debug` prints only the sizes of `body` and `meta`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpooledSend {
    /// [`SPOOL_VERSION`].
    pub v: u32,
    /// The office it was spooled for; any other office refuses it.
    pub office_id: String,
    /// The sender, as the writer claims (see above).
    pub from: String,
    /// Client-made (a ULID); ingest dedups on it like a `msg.send` retry.
    pub id: String,
    pub to: String,
    pub kind: String,
    pub priority: MessagePriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub body: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    /// When `org-agent` wrote it, as the writer claims (see above).
    pub created_at: String,
}

impl fmt::Debug for SpooledSend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpooledSend")
            .field("v", &self.v)
            .field("office_id", &self.office_id)
            .field("from", &self.from)
            .field("id", &self.id)
            .field("to", &self.to)
            .field("kind", &self.kind)
            .field("priority", &self.priority)
            .field("subject", &self.subject)
            .field("body", &BodySize(&self.body))
            .field("meta", &MetaSize(&self.meta))
            .field("reply_to", &self.reply_to)
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use serde::de::DeserializeOwned;
    use serde_json::{json, Value};

    use super::*;
    use crate::{ApiError, CallError, ErrorCode, Request, Response, MAX_FRAME_BYTES};

    /// Asserts `value` serializes to exactly `expected` and decodes back to `value`.
    fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T, expected: Value) {
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
        assert_eq!(&serde_json::from_value::<T>(expected).unwrap(), value);
    }

    /// Asserts `{}` decodes as the default.
    fn empty_decodes<T: DeserializeOwned + Default + PartialEq + Debug>() {
        assert_eq!(
            serde_json::from_value::<T>(json!({})).unwrap(),
            T::default()
        );
    }

    const BODY: &str = "zebra-crossing-body-7731";
    const META_VALUE: &str = "meta-secret-4482";

    fn meta() -> Meta {
        Meta::from([("ticket".to_string(), json!(META_VALUE))])
    }

    fn header() -> MessageHeader {
        MessageHeader {
            id: "01M6WRN71MBX8FHS08MB0Q9Z24".into(),
            thread_id: "01M6WRN71MBX8FHS08MB0Q9Z24".into(),
            reply_to: None,
            from_member_id: "01M6WA".into(),
            from_display_name: "ada".into(),
            to_member_id: "01M6WB".into(),
            to_display_name: "remy".into(),
            kind: KIND_REQUEST.into(),
            priority: MessagePriority::Urgent,
            urgent: true,
            subject: Some("Quick check".into()),
            created_at: "2026-11-08T05:03:51.348Z".into(),
            expires_at: Some("2026-11-08T05:05:51.348Z".into()),
            ttl_ms: Some(120_000),
            body_bytes: 26,
            meta_bytes: 0,
            local_seq: 42,
            state: MessageLifecycleState::Unread,
            triage_gen: 0,
            snoozed_until: None,
            snooze_after_task: false,
            snooze_note: None,
            read_at: None,
            expired_waiting_person: false,
        }
    }

    fn header_json() -> Value {
        json!({
            "id": "01M6WRN71MBX8FHS08MB0Q9Z24",
            "thread_id": "01M6WRN71MBX8FHS08MB0Q9Z24",
            "from_member_id": "01M6WA",
            "from_display_name": "ada",
            "to_member_id": "01M6WB",
            "to_display_name": "remy",
            "kind": "request",
            "priority": "urgent",
            "urgent": true,
            "subject": "Quick check",
            "created_at": "2026-11-08T05:03:51.348Z",
            "expires_at": "2026-11-08T05:05:51.348Z",
            "ttl_ms": 120000,
            "body_bytes": 26,
            "meta_bytes": 0,
            "local_seq": 42,
            "state": "unread",
            "triage_gen": 0
        })
    }

    fn message() -> Message {
        Message {
            header: header(),
            body: BODY.into(),
            meta: meta(),
        }
    }

    fn message_json() -> Value {
        json!({"header": header_json(), "body": BODY, "meta": {"ticket": META_VALUE}})
    }

    fn token_bytes() -> [u8; 32] {
        let mut bytes = [0u8; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(29).wrapping_add(0x3c);
        }
        bytes
    }

    fn token_hex() -> String {
        token_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn method_and_event_names_are_exact_and_unique() {
        let names = [
            MsgBind::METHOD,
            MsgSend::METHOD,
            MsgReply::METHOD,
            MsgAsk::METHOD,
            MsgDefer::METHOD,
            MsgDone::METHOD,
            MsgNotify::METHOD,
            MsgWait::METHOD,
            MsgRead::METHOD,
            MsgInbox::METHOD,
            MsgList::METHOD,
            MsgGet::METHOD,
            MsgHumanNeedsYou::METHOD,
            MsgThreads::METHOD,
            crate::directory::Who::METHOD,
            crate::directory::StatusSet::METHOD,
            MessageReceived::NAME,
            MessageStateChanged::NAME,
            MessageDeliveryFailed::NAME,
            crate::directory::MemberPresenceChanged::NAME,
        ];
        assert_eq!(
            names,
            [
                "msg.bind",
                "msg.send",
                "msg.reply",
                "msg.ask",
                "msg.defer",
                "msg.done",
                "msg.notify",
                "msg.wait",
                "msg.read",
                "msg.inbox",
                "msg.list",
                "msg.get",
                "msg.human_needs_you",
                "msg.threads",
                "who",
                "status.set",
                "message.received",
                "message.state_changed",
                "message.delivery_failed",
                "member.presence_changed",
            ]
        );
        let unique: std::collections::BTreeSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
    }

    #[test]
    fn header_and_message_wire_shapes() {
        check(&header(), header_json());
        check(&message(), message_json());

        // Every optional field set.
        let mut full = header();
        full.reply_to = Some("01M6WP".into());
        full.meta_bytes = 12;
        full.triage_gen = 2;
        full.state = MessageLifecycleState::Snoozed;
        full.snoozed_until = Some("2026-11-08T07:00:00.000Z".into());
        full.snooze_after_task = true;
        full.snooze_note = Some("after the build".into());
        full.read_at = Some("2026-11-08T05:04:00.000Z".into());
        full.expired_waiting_person = true;
        let mut expected = header_json();
        let obj = expected.as_object_mut().unwrap();
        obj.insert("reply_to".into(), json!("01M6WP"));
        obj.insert("meta_bytes".into(), json!(12));
        obj.insert("triage_gen".into(), json!(2));
        obj.insert("state".into(), json!("snoozed"));
        obj.insert("snoozed_until".into(), json!("2026-11-08T07:00:00.000Z"));
        obj.insert("snooze_after_task".into(), json!(true));
        obj.insert("snooze_note".into(), json!("after the build"));
        obj.insert("read_at".into(), json!("2026-11-08T05:04:00.000Z"));
        obj.insert("expired_waiting_person".into(), json!(true));
        check(&full, expected);

        // A message without meta omits it, and `meta_bytes`/`triage_gen` may be missing.
        let bare = Message {
            meta: Meta::new(),
            ..message()
        };
        check(&bare, json!({"header": header_json(), "body": BODY}));
        let mut old = header_json();
        old.as_object_mut().unwrap().remove("meta_bytes");
        old.as_object_mut().unwrap().remove("triage_gen");
        assert_eq!(
            serde_json::from_value::<MessageHeader>(old).unwrap(),
            header()
        );
    }

    #[test]
    fn bind_wire_shape() {
        let params = MsgBindParams {
            member: Some("owner".into()),
            app_token: Some(AppToken::from_bytes(token_bytes())),
        };
        check(
            &params,
            json!({"member": "owner", "app_token": token_hex()}),
        );
        check(
            &MsgBindParams {
                member: Some("ac-software-engineer-remy".into()),
                app_token: None,
            },
            json!({"member": "ac-software-engineer-remy"}),
        );
        empty_decodes::<MsgBindParams>();
        // Both names at once don't decode (a server answers `invalid_request`).
        assert!(
            serde_json::from_value::<MsgBindParams>(json!({"member": "a", "agent": "b"})).is_err()
        );
        // `agent` (the ADR's name) is read as `member`; `member` is what's written.
        assert_eq!(
            serde_json::from_value::<MsgBindParams>(json!({"agent": "x"})).unwrap(),
            MsgBindParams {
                member: Some("x".into()),
                app_token: None,
            }
        );
        check(
            &MsgBindResult {
                member_id: Some("01M6WB".into()),
                office_id: "01M6O".into(),
            },
            json!({"member_id": "01M6WB", "office_id": "01M6O"}),
        );
        check(
            &MsgBindResult {
                member_id: None,
                office_id: "01M6O".into(),
            },
            json!({"office_id": "01M6O"}),
        );
    }

    fn send_params() -> MsgSendParams {
        MsgSendParams {
            id: "01M6WS".into(),
            to: "remy".into(),
            kind: KIND_MSG.into(),
            priority: None,
            subject: None,
            body: BODY.into(),
            meta: Meta::new(),
            reply_to: None,
        }
    }

    #[test]
    fn send_has_no_deadline_fields() {
        // Only `msg.ask` has a deadline; an old client's `ttl_ms` on a send is ignored.
        let p: MsgSendParams = serde_json::from_value(json!({
            "id": "01M6WS", "to": "remy", "kind": "msg", "body": BODY, "ttl_ms": 60000,
            "expires_at": "2026-11-08T05:02:00.000Z"
        }))
        .unwrap();
        assert_eq!(p, send_params());
        let s: SpooledSend = serde_json::from_value(json!({
            "v": 1, "office_id": "01M6O", "from": "ac-software-engineer-remy",
            "id": "01M6WS", "to": "ada", "kind": "msg", "priority": "normal",
            "body": BODY, "meta": {"ticket": META_VALUE},
            "expires_at": "2026-11-08T05:02:00.000Z", "ttl_ms": 120000,
            "created_at": "2026-11-08T05:00:00.000Z"
        }))
        .unwrap();
        assert_eq!(s, spooled());
    }

    #[test]
    fn send_wire_shape() {
        check(
            &send_params(),
            json!({"id": "01M6WS", "to": "remy", "kind": "msg", "body": BODY}),
        );
        let full = MsgSendParams {
            priority: Some(MessagePriority::High),
            subject: Some("Build".into()),
            meta: meta(),
            reply_to: Some("01M6WP".into()),
            ..send_params()
        };
        check(
            &full,
            json!({
                "id": "01M6WS", "to": "remy", "kind": "msg", "priority": "high",
                "subject": "Build", "body": BODY, "meta": {"ticket": META_VALUE},
                "reply_to": "01M6WP"
            }),
        );
        check(
            &SendResult {
                id: "01M6WS".into(),
                thread_id: "01M6WP".into(),
                local_seq: 43,
                origin_seq: 7,
            },
            json!({"id": "01M6WS", "thread_id": "01M6WP", "local_seq": 43, "origin_seq": 7}),
        );
    }

    fn reply_params() -> MsgReplyParams {
        MsgReplyParams {
            id: "01M6WR".into(),
            to_id: "01M6WRN71MBX8FHS08MB0Q9Z24".into(),
            priority: None,
            subject: None,
            body: BODY.into(),
            meta: meta(),
        }
    }

    #[test]
    fn reply_wire_shape() {
        check(
            &reply_params(),
            json!({
                "id": "01M6WR", "to_id": "01M6WRN71MBX8FHS08MB0Q9Z24", "body": BODY,
                "meta": {"ticket": META_VALUE}
            }),
        );
        let full = MsgReplyParams {
            priority: Some(MessagePriority::Normal),
            subject: Some("Re: Quick check".into()),
            meta: Meta::new(),
            ..reply_params()
        };
        check(
            &full,
            json!({
                "id": "01M6WR", "to_id": "01M6WRN71MBX8FHS08MB0Q9Z24", "priority": "normal",
                "subject": "Re: Quick check", "body": BODY
            }),
        );
    }

    fn ask_params() -> MsgAskParams {
        MsgAskParams {
            id: "01M6WQ".into(),
            to: "owner".into(),
            priority: None,
            subject: Some("Approve?".into()),
            body: BODY.into(),
            meta: meta(),
            ttl_ms: None,
            wait_ms: 3_600_000,
        }
    }

    #[test]
    fn ask_wire_shape() {
        check(
            &ask_params(),
            json!({
                "id": "01M6WQ", "to": "owner", "subject": "Approve?", "body": BODY,
                "meta": {"ticket": META_VALUE}, "wait_ms": 3600000
            }),
        );
        let full = MsgAskParams {
            priority: Some(MessagePriority::High),
            ttl_ms: Some(86_400_000),
            ..ask_params()
        };
        assert_eq!(serde_json::to_value(&full).unwrap()["ttl_ms"], 86_400_000);
        assert_eq!(serde_json::to_value(&full).unwrap()["priority"], "high");
        assert!(serde_json::from_value::<MsgAskParams>(json!({
            "id": "01M6WQ", "to": "owner", "body": "x"
        }))
        .is_err());

        check(
            &MsgAskResult {
                reply: None,
                expires_at: "2026-11-09T05:03:51.348Z".into(),
            },
            json!({"expires_at": "2026-11-09T05:03:51.348Z"}),
        );
        check(
            &MsgAskResult {
                reply: Some(message()),
                expires_at: "2026-11-09T05:03:51.348Z".into(),
            },
            json!({"reply": message_json(), "expires_at": "2026-11-09T05:03:51.348Z"}),
        );
    }

    #[test]
    fn defer_wire_shape_and_snooze_targets() {
        check(
            &MsgDeferParams {
                id: "01M6WQ".into(),
                until: SnoozeTarget::Until {
                    at: "2026-11-08T10:00:00.000Z".into(),
                },
                note: Some("after lunch".into()),
            },
            json!({
                "id": "01M6WQ",
                "until": {"type": "until", "at": "2026-11-08T10:00:00.000Z"},
                "note": "after lunch"
            }),
        );
        check(
            &MsgDeferParams {
                id: "01M6WQ".into(),
                until: SnoozeTarget::AfterTask,
                note: None,
            },
            json!({"id": "01M6WQ", "until": {"type": "after_task"}}),
        );
        check(
            &MsgDeferResult {
                id: "01M6WQ".into(),
                triage_gen: 1,
                snoozed_until: Some("2026-11-08T10:00:00.000Z".into()),
                snooze_after_task: false,
                snooze_note: Some("after lunch".into()),
            },
            json!({
                "id": "01M6WQ", "triage_gen": 1, "snoozed_until": "2026-11-08T10:00:00.000Z",
                "snooze_note": "after lunch"
            }),
        );
        check(
            &MsgDeferResult {
                id: "01M6WQ".into(),
                triage_gen: 2,
                snoozed_until: None,
                snooze_after_task: true,
                snooze_note: None,
            },
            json!({"id": "01M6WQ", "triage_gen": 2, "snooze_after_task": true}),
        );
    }

    #[test]
    fn done_wire_shape() {
        check(
            &MsgDoneParams {
                id: "01M6WQ".into(),
                note: None,
            },
            json!({"id": "01M6WQ"}),
        );
        check(
            &MsgDoneParams {
                id: "01M6WQ".into(),
                note: Some("handled in standup".into()),
            },
            json!({"id": "01M6WQ", "note": "handled in standup"}),
        );
        check(
            &MsgDoneResult {
                id: "01M6WQ".into(),
                resolved_at: "2026-11-08T06:00:00.000Z".into(),
            },
            json!({"id": "01M6WQ", "resolved_at": "2026-11-08T06:00:00.000Z"}),
        );
    }

    fn notify_result() -> NotifyResult {
        NotifyResult {
            v: NOTIFY_VERSION,
            agent: "agy-senior-dev".into(),
            urgent: true,
            count: 1,
            urgent_count: 1,
            items: vec![NotifyItem {
                id: "01M6WRN71MBX8FHS08MB0Q9Z24".into(),
                from: "ac-staff-engineer-ada".into(),
                from_name: "Ada".into(),
                subject: "Quick check".into(),
                kind: "request".into(),
                urgent: true,
                priority: MessagePriority::Urgent,
                triage_gen: 0,
                created_at: "2026-11-08T05:03:51.348Z".into(),
                reason: NotifyReason::New,
            }],
            more: 0,
            text: "1 new message(s), 1 urgent".into(),
        }
    }

    #[test]
    fn notify_wire_shape() {
        check(
            &MsgNotifyParams {
                session_id: Some("sess-1".into()),
                after_task: true,
                urgent_only: true,
                sweep_after_ms: Some(300_000),
            },
            json!({
                "session_id": "sess-1", "after_task": true, "urgent_only": true,
                "sweep_after_ms": 300000
            }),
        );
        check(&MsgNotifyParams::default(), json!({}));
        empty_decodes::<MsgNotifyParams>();
        check(
            &notify_result(),
            json!({
                "v": 1, "agent": "agy-senior-dev", "urgent": true, "count": 1,
                "urgent_count": 1,
                "items": [{
                    "id": "01M6WRN71MBX8FHS08MB0Q9Z24", "from": "ac-staff-engineer-ada",
                    "from_name": "Ada", "subject": "Quick check", "kind": "request",
                    "urgent": true, "priority": "urgent", "triage_gen": 0,
                    "created_at": "2026-11-08T05:03:51.348Z", "reason": "new"
                }],
                "more": 0, "text": "1 new message(s), 1 urgent"
            }),
        );
    }

    #[test]
    fn notify_result_keys_keep_the_locked_order() {
        assert_eq!(
            serde_json::to_string(&notify_result()).unwrap(),
            concat!(
                r#"{"v":1,"agent":"agy-senior-dev","urgent":true,"count":1,"urgent_count":1,"#,
                r#""items":[{"id":"01M6WRN71MBX8FHS08MB0Q9Z24","from":"ac-staff-engineer-ada","#,
                r#""from_name":"Ada","subject":"Quick check","kind":"request","urgent":true,"#,
                r#""priority":"urgent","triage_gen":0,"created_at":"2026-11-08T05:03:51.348Z","#,
                r#""reason":"new"}],"more":0,"text":"1 new message(s), 1 urgent"}"#,
            )
        );
    }

    /// Asserts each value's wire name both ways. `Unknown` re-serializes as `"unknown"`.
    fn wire_names<T: Serialize + DeserializeOwned + PartialEq + Debug + Copy>(table: &[(T, &str)]) {
        for &(value, name) in table {
            assert_eq!(serde_json::to_value(value).unwrap(), name, "{value:?}");
            assert_eq!(serde_json::from_value::<T>(name.into()).unwrap(), value);
        }
    }

    #[test]
    fn enum_wire_names() {
        wire_names(&[
            (MessagePriority::Normal, "normal"),
            (MessagePriority::High, "high"),
            (MessagePriority::Urgent, "urgent"),
            (MessagePriority::Unknown, "unknown"),
        ]);
        wire_names(&[
            (MessageLifecycleState::Unread, "unread"),
            (MessageLifecycleState::Snoozed, "snoozed"),
            (MessageLifecycleState::Read, "read"),
            (MessageLifecycleState::Done, "done"),
            (MessageLifecycleState::Expired, "expired"),
            (MessageLifecycleState::DeadLetter, "dead_letter"),
            (MessageLifecycleState::Unknown, "unknown"),
        ]);
        wire_names(&[
            (NotifyReason::New, "new"),
            (NotifyReason::Due, "due"),
            (NotifyReason::AfterTask, "after_task"),
            (NotifyReason::Sweep, "sweep"),
            (NotifyReason::Unknown, "unknown"),
        ]);
        wire_names(&[
            (DeliveryFailedReason::UnknownRecipient, "unknown_recipient"),
            (DeliveryFailedReason::Expired, "expired"),
            (DeliveryFailedReason::Unknown, "unknown"),
        ]);
        assert_eq!(
            serde_json::to_value(SnoozeTarget::Unknown).unwrap(),
            json!({"type": "unknown"})
        );
    }

    #[test]
    fn constants_are_pinned() {
        assert_eq!(
            [
                KIND_MSG,
                KIND_REQUEST,
                KIND_REPLY,
                KIND_DELIVERY_FAILED,
                OFFICE_SENDER
            ],
            ["msg", "request", "reply", "delivery_failed", "office"]
        );
        assert_eq!(
            [
                MAX_BODY_BYTES,
                MAX_META_BYTES,
                MAX_SUBJECT_BYTES,
                MAX_NOTE_BYTES,
                MAX_ID_BYTES,
                MAX_HANDLE_BYTES,
                MAX_HELD_CALLS_PER_CONNECTION,
                NOTIFY_MAX_LINES,
                NOTIFY_MAX_ITEMS,
                NOTIFY_SUBJECT_MAX_CHARS,
                NOTIFY_FROM_NAME_MAX_CHARS,
            ],
            [262_144, 65_536, 1024, 1024, 128, 258, 16, 10, 100, 80, 48]
        );
        assert_eq!(
            [
                MAX_WAIT_MS,
                MAX_TTL_MS,
                DEFAULT_ASK_TTL_MS_AGENT,
                DEFAULT_ASK_TTL_MS_PERSON,
                DEFAULT_SWEEP_AFTER_MS,
            ],
            [3_600_000, 86_400_000, 120_000, 86_400_000, 300_000]
        );
        assert_eq!([MAX_PAGE_LIMIT, DEFAULT_PAGE_LIMIT], [100, 50]);
        assert_eq!([NOTIFY_VERSION, SPOOL_VERSION], [1, 1]);
    }

    #[test]
    fn wait_and_read_wire_shapes() {
        check(
            &MsgWaitParams {
                timeout_ms: 60_000,
                from: None,
                urgent_only: false,
            },
            json!({"timeout_ms": 60000}),
        );
        check(
            &MsgWaitParams {
                timeout_ms: 60_000,
                from: Some("ada".into()),
                urgent_only: true,
            },
            json!({"timeout_ms": 60000, "from": "ada", "urgent_only": true}),
        );
        assert!(serde_json::from_value::<MsgWaitParams>(json!({})).is_err());
        check(&MsgWaitResult { message: None }, json!({}));
        check(
            &MsgWaitResult {
                message: Some(message()),
            },
            json!({"message": message_json()}),
        );

        check(&MsgReadParams::default(), json!({}));
        empty_decodes::<MsgReadParams>();
        check(
            &MsgReadParams {
                id: Some("01M6WQ".into()),
            },
            json!({"id": "01M6WQ"}),
        );
        check(&MsgReadResult { message: None }, json!({}));
        check(
            &MsgReadResult {
                message: Some(message()),
            },
            json!({"message": message_json()}),
        );
    }

    #[test]
    fn inbox_and_list_wire_shapes() {
        check(&MsgInboxParams::default(), json!({}));
        empty_decodes::<MsgInboxParams>();
        check(
            &MsgInboxParams {
                include_all: true,
                from: Some("ada".into()),
                thread_id: Some("01M6WP".into()),
                cursor: Some(40),
                limit: Some(20),
            },
            json!({
                "include_all": true, "from": "ada", "thread_id": "01M6WP", "cursor": 40,
                "limit": 20
            }),
        );
        check(
            &HeaderPage {
                headers: vec![header()],
                next_cursor: Some(42),
            },
            json!({"headers": [header_json()], "next_cursor": 42}),
        );
        check(
            &HeaderPage {
                headers: vec![],
                next_cursor: None,
            },
            json!({"headers": []}),
        );

        check(&MsgListParams::default(), json!({}));
        empty_decodes::<MsgListParams>();
        check(
            &MsgListParams {
                thread_id: Some("01M6WP".into()),
                participant: Some("owner".into()),
                unread_only: true,
                include_snoozed: true,
                cursor: Some(9),
                limit: Some(100),
            },
            json!({
                "thread_id": "01M6WP", "participant": "owner", "unread_only": true,
                "include_snoozed": true, "cursor": 9, "limit": 100
            }),
        );
    }

    #[test]
    fn get_wire_shape() {
        check(
            &MsgGetParams {
                id: "01M6WQ".into(),
            },
            json!({"id": "01M6WQ"}),
        );
        check(
            &MsgGetResult { message: message() },
            json!({"message": message_json()}),
        );
    }

    #[test]
    fn human_needs_you_wire_shape() {
        check(&MsgHumanNeedsYouParams::default(), json!({}));
        empty_decodes::<MsgHumanNeedsYouParams>();
        check(
            &MsgHumanNeedsYouParams {
                member_id: Some("01M6WO".into()),
                limit: Some(5),
            },
            json!({"member_id": "01M6WO", "limit": 5}),
        );
        check(
            &HumanNeedsYouResult {
                items: vec![header()],
                total_urgent_unread: 3,
            },
            json!({"items": [header_json()], "total_urgent_unread": 3}),
        );
    }

    #[test]
    fn threads_wire_shape() {
        check(&MsgThreadsParams::default(), json!({}));
        empty_decodes::<MsgThreadsParams>();
        let cursor = ThreadCursor {
            last_msg_at: "2026-11-08T05:03:51.348Z".into(),
            thread_id: "01M6WP".into(),
        };
        let cursor_json = json!({"last_msg_at": "2026-11-08T05:03:51.348Z", "thread_id": "01M6WP"});
        check(
            &MsgThreadsParams {
                participant: Some("owner".into()),
                cursor: Some(cursor.clone()),
                limit: Some(25),
            },
            json!({"participant": "owner", "cursor": cursor_json, "limit": 25}),
        );
        let summary = ThreadSummary {
            thread_id: "01M6WP".into(),
            subject: None,
            created_at: "2026-11-08T05:00:00.000Z".into(),
            last_msg_id: "01M6WQ".into(),
            last_msg_at: "2026-11-08T05:03:51.348Z".into(),
            message_count: 4,
            participants: vec!["01M6WA".into(), "01M6WO".into()],
        };
        check(
            &ThreadPage {
                threads: vec![summary],
                next_cursor: Some(cursor),
            },
            json!({
                "threads": [{
                    "thread_id": "01M6WP", "created_at": "2026-11-08T05:00:00.000Z",
                    "last_msg_id": "01M6WQ", "last_msg_at": "2026-11-08T05:03:51.348Z",
                    "message_count": 4, "participants": ["01M6WA", "01M6WO"]
                }],
                "next_cursor": cursor_json
            }),
        );
        check(
            &ThreadPage {
                threads: vec![],
                next_cursor: None,
            },
            json!({"threads": []}),
        );
    }

    #[test]
    fn event_wire_shapes() {
        check(
            &MessageReceivedData { header: header() },
            json!({"header": header_json()}),
        );
        check(
            &MessageStateChangedData {
                id: "01M6WQ".into(),
                thread_id: "01M6WP".into(),
                to_member_id: "01M6WB".into(),
                state: MessageLifecycleState::Done,
                triage_gen: 1,
                snoozed_until: None,
                snooze_after_task: false,
                reason: Some("handled".into()),
                expired_waiting_person: false,
                updated_at: "2026-11-08T06:00:00.000Z".into(),
            },
            json!({
                "id": "01M6WQ", "thread_id": "01M6WP", "to_member_id": "01M6WB",
                "state": "done", "triage_gen": 1, "reason": "handled",
                "updated_at": "2026-11-08T06:00:00.000Z"
            }),
        );
        // A person's ask past its deadline: still unread, flagged.
        check(
            &MessageStateChangedData {
                id: "01M6WQ".into(),
                thread_id: "01M6WQ".into(),
                to_member_id: "01M6WO".into(),
                state: MessageLifecycleState::Unread,
                triage_gen: 0,
                snoozed_until: None,
                snooze_after_task: false,
                reason: None,
                expired_waiting_person: true,
                updated_at: "2026-11-09T05:03:51.348Z".into(),
            },
            json!({
                "id": "01M6WQ", "thread_id": "01M6WQ", "to_member_id": "01M6WO",
                "state": "unread", "triage_gen": 0, "expired_waiting_person": true,
                "updated_at": "2026-11-09T05:03:51.348Z"
            }),
        );
        check(
            &MessageDeliveryFailedData {
                id: "01M6WF".into(),
                original_id: "01M6WQ".into(),
                thread_id: "01M6WP".into(),
                from_member_id: "01M6WA".into(),
                recipient_id: "01M6WB".into(),
                recipient_name: "remy".into(),
                reason: DeliveryFailedReason::Expired,
            },
            json!({
                "id": "01M6WF", "original_id": "01M6WQ", "thread_id": "01M6WP",
                "from_member_id": "01M6WA", "recipient_id": "01M6WB",
                "recipient_name": "remy", "reason": "expired"
            }),
        );
        let ev = crate::Event::new::<MessageReceived>(1, &MessageReceivedData { header: header() })
            .unwrap();
        assert_eq!(
            ev.data::<MessageReceived>().unwrap().unwrap().header,
            header()
        );
    }

    fn spooled() -> SpooledSend {
        SpooledSend {
            v: SPOOL_VERSION,
            office_id: "01M6O".into(),
            from: "ac-software-engineer-remy".into(),
            id: "01M6WS".into(),
            to: "ada".into(),
            kind: KIND_MSG.into(),
            priority: MessagePriority::Normal,
            subject: None,
            body: BODY.into(),
            meta: meta(),
            reply_to: None,
            created_at: "2026-11-08T05:00:00.000Z".into(),
        }
    }

    #[test]
    fn spooled_send_wire_shape() {
        check(
            &spooled(),
            json!({
                "v": 1, "office_id": "01M6O", "from": "ac-software-engineer-remy",
                "id": "01M6WS", "to": "ada", "kind": "msg", "priority": "normal",
                "body": BODY, "meta": {"ticket": META_VALUE},
                "created_at": "2026-11-08T05:00:00.000Z"
            }),
        );
        let full = SpooledSend {
            subject: Some("Hi".into()),
            meta: Meta::new(),
            reply_to: Some("01M6WP".into()),
            ..spooled()
        };
        check(
            &full,
            json!({
                "v": 1, "office_id": "01M6O", "from": "ac-software-engineer-remy",
                "id": "01M6WS", "to": "ada", "kind": "msg", "priority": "normal",
                "subject": "Hi", "body": BODY, "reply_to": "01M6WP",
                "created_at": "2026-11-08T05:00:00.000Z"
            }),
        );
    }

    #[test]
    fn unknown_values_decode_as_unknown_and_unknown_fields_are_ignored() {
        assert_eq!(
            serde_json::from_value::<MessagePriority>(json!("critical")).unwrap(),
            MessagePriority::Unknown
        );
        assert_eq!(
            serde_json::from_value::<MessageLifecycleState>(json!("archived")).unwrap(),
            MessageLifecycleState::Unknown
        );
        assert_eq!(
            serde_json::from_value::<NotifyReason>(json!("reminder")).unwrap(),
            NotifyReason::Unknown
        );
        assert_eq!(
            serde_json::from_value::<DeliveryFailedReason>(json!("blocked")).unwrap(),
            DeliveryFailedReason::Unknown
        );
        assert_eq!(
            serde_json::from_value::<SnoozeTarget>(json!({"type": "next_week", "x": 1})).unwrap(),
            SnoozeTarget::Unknown
        );

        let mut h = header_json();
        h["priority"] = json!("critical");
        h["state"] = json!("archived");
        h["reactions"] = json!(["+1"]);
        let decoded: MessageHeader = serde_json::from_value(h).unwrap();
        assert_eq!(decoded.priority, MessagePriority::Unknown);
        assert_eq!(decoded.state, MessageLifecycleState::Unknown);

        let mut m = message_json();
        m["attachments"] = json!([]);
        assert_eq!(serde_json::from_value::<Message>(m).unwrap(), message());
        let p: MsgNotifyParams =
            serde_json::from_value(json!({"after_task": true, "format": "json"})).unwrap();
        assert!(p.after_task);
    }

    #[test]
    fn priority_defaults_to_normal_and_snooze_target_needs_its_fields() {
        assert_eq!(MessagePriority::default(), MessagePriority::Normal);
        assert!(serde_json::from_value::<SnoozeTarget>(json!({"type": "until"})).is_err());
        assert!(serde_json::from_value::<SnoozeTarget>(json!({"at": "x"})).is_err());
    }

    #[test]
    fn debug_never_prints_bodies_meta_or_tokens() {
        let bind = MsgBindParams {
            member: Some("owner".into()),
            app_token: Some(AppToken::from_bytes(token_bytes())),
        };
        let shown = [
            format!("{:?}", message()),
            format!(
                "{:?}",
                MsgSendParams {
                    meta: meta(),
                    ..send_params()
                }
            ),
            format!("{:?}", reply_params()),
            format!("{:?}", ask_params()),
            format!("{:?}", spooled()),
            format!(
                "{:?}",
                MsgReadResult {
                    message: Some(message())
                }
            ),
            format!("{bind:?}"),
        ];
        let hex = token_hex();
        for s in &shown {
            assert!(!s.contains(BODY), "{s}");
            assert!(!s.contains(META_VALUE), "{s}");
            assert!(!s.contains(&hex[..16]), "{s}");
        }
        for s in &shown[..6] {
            assert!(s.contains(&format!("<{} bytes>", BODY.len())), "{s}");
            assert!(s.contains("<1 keys>"), "{s}");
        }
        assert!(shown[6].contains("redacted"), "{}", shown[6]);
    }

    #[test]
    fn json_string_len_matches_serde_json() {
        let mut cases: Vec<String> = vec![
            String::new(),
            "plain".into(),
            "\"quoted\"".into(),
            "back\\slash".into(),
            "\u{7f}".into(),
            "é漢字🦀".into(),
            "\u{2028}\u{2029}".into(),
            "/<>&'".into(),
        ];
        for c in 0u8..0x20 {
            cases.push(format!("a{}b", c as char));
        }
        cases.push((0u8..0x20).map(char::from).collect());
        for s in &cases {
            assert_eq!(
                json_string_len(s),
                serde_json::to_string(s).unwrap().len(),
                "{s:?}"
            );
        }
    }

    #[test]
    fn urgency_follows_kind_and_priority() {
        use MessagePriority::*;
        for (kind, priority, urgent) in [
            (KIND_MSG, Normal, false),
            (KIND_MSG, High, true),
            (KIND_MSG, Urgent, true),
            (KIND_MSG, Unknown, false),
            (KIND_REQUEST, Normal, true),
            (KIND_REQUEST, Unknown, true),
            (KIND_REPLY, Normal, false),
            (KIND_REPLY, High, true),
            ("custom", Urgent, true),
            ("custom", Normal, false),
        ] {
            assert_eq!(is_urgent(kind, priority), urgent, "{kind} {priority:?}");
        }
    }

    #[test]
    fn a_message_at_every_limit_fits_in_a_frame() {
        // Quotes double on the wire, so the body is at its limit with half as many characters.
        let body = "\"".repeat((MAX_BODY_BYTES - 2) / 2);
        assert_eq!(json_string_len(&body), MAX_BODY_BYTES);
        let meta = Meta::from([("k".to_string(), json!("m".repeat(MAX_META_BYTES - 8)))]);
        assert_eq!(serde_json::to_string(&meta).unwrap().len(), MAX_META_BYTES);
        let id = max_id();

        let params = MsgSendParams {
            id: id.clone(),
            to: max_handle(),
            kind: id.clone(),
            priority: Some(MessagePriority::Urgent),
            subject: Some("s".repeat(MAX_SUBJECT_BYTES - 2)),
            body: body.clone(),
            meta: meta.clone(),
            reply_to: Some(id),
        };
        let line =
            serde_json::to_string(&Request::new::<MsgSend>(u64::MAX, &params).unwrap()).unwrap();
        assert!(line.len() <= MAX_FRAME_BYTES, "{}", line.len());

        let result = MsgReadResult {
            message: Some(Message {
                header: header_at_limits(),
                body,
                meta,
            }),
        };
        let line =
            serde_json::to_string(&Response::ok::<MsgRead>(u64::MAX, &result).unwrap()).unwrap();
        assert!(line.len() <= MAX_FRAME_BYTES, "{}", line.len());
    }

    #[test]
    fn new_error_codes_round_trip() {
        for (code, name) in [
            (ErrorCode::Timeout, "timeout"),
            (ErrorCode::TooLarge, "too_large"),
            (ErrorCode::NotBound, "not_bound"),
        ] {
            assert_eq!(serde_json::to_value(code).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<ErrorCode>(name.into()).unwrap(),
                code
            );
            let line =
                serde_json::to_string(&Response::error(9, ApiError::new(code, "x"))).unwrap();
            match serde_json::from_str::<Response>(&line)
                .unwrap()
                .into_result::<MsgSend>()
            {
                Err(CallError::Api(e)) => assert_eq!(e.code, code),
                other => panic!("expected an api error, got {other:?}"),
            }
        }
    }

    #[test]
    fn typed_request_and_response_round_trip() {
        let req = Request::new::<MsgAsk>(3, &ask_params()).unwrap();
        assert!(req.is::<MsgAsk>());
        assert_eq!(req.params::<MsgAsk>().unwrap(), ask_params());
        let result = MsgAskResult {
            reply: Some(message()),
            expires_at: "2026-11-09T05:03:51.348Z".into(),
        };
        let line = serde_json::to_string(&Response::ok::<MsgAsk>(3, &result).unwrap()).unwrap();
        let back: Response = serde_json::from_str(&line).unwrap();
        assert_eq!(back.into_result::<MsgAsk>().unwrap(), result);
    }

    /// An id or kind at [`MAX_ID_BYTES`].
    fn max_id() -> String {
        let id = "i".repeat(MAX_ID_BYTES - 2);
        assert_eq!(json_string_len(&id), MAX_ID_BYTES);
        id
    }

    /// A handle or display name at [`MAX_HANDLE_BYTES`]: 64 four-byte characters.
    fn max_handle() -> String {
        let handle = "\u{1F980}".repeat(64);
        assert_eq!(json_string_len(&handle), MAX_HANDLE_BYTES);
        handle
    }

    /// A header with every string field at its limit.
    fn header_at_limits() -> MessageHeader {
        let id = max_id();
        let mut h = header();
        for field in [
            &mut h.id,
            &mut h.thread_id,
            &mut h.from_member_id,
            &mut h.to_member_id,
            &mut h.kind,
        ] {
            field.clone_from(&id);
        }
        h.from_display_name = max_handle();
        h.to_display_name = max_handle();
        h.reply_to = Some(id);
        h.expires_at = Some("2026-11-09T05:03:51.348Z".into());
        h.subject = Some("s".repeat(MAX_SUBJECT_BYTES - 2));
        h.snoozed_until = Some("2026-11-08T07:00:00.000Z".into());
        h.snooze_after_task = true;
        h.snooze_note = Some("n".repeat(MAX_NOTE_BYTES - 2));
        h.read_at = Some("2026-11-08T05:04:00.000Z".into());
        h.expired_waiting_person = true;
        h.local_seq = u64::MAX;
        h.body_bytes = u64::MAX;
        h.meta_bytes = u64::MAX;
        h.triage_gen = u64::MAX;
        h.ttl_ms = Some(u64::MAX);
        h
    }

    #[test]
    fn a_full_header_page_fits_in_a_frame() {
        let page = HeaderPage {
            headers: vec![header_at_limits(); MAX_PAGE_LIMIT as usize],
            next_cursor: Some(u64::MAX),
        };
        let line =
            serde_json::to_string(&Response::ok::<MsgInbox>(u64::MAX, &page).unwrap()).unwrap();
        assert!(line.len() <= MAX_FRAME_BYTES, "{}", line.len());
    }

    #[test]
    fn a_full_notify_result_fits_in_a_frame() {
        // The widest sanitized text: 4-byte characters (sanitizing removes control
        // characters, so nothing escapes wider per byte).
        let id = max_id();
        let item = NotifyItem {
            id: id.clone(),
            from: max_handle(),
            from_name: "\u{1F980}".repeat(NOTIFY_FROM_NAME_MAX_CHARS),
            subject: "\u{1F980}".repeat(NOTIFY_SUBJECT_MAX_CHARS),
            kind: id.clone(),
            urgent: true,
            priority: MessagePriority::Urgent,
            triage_gen: u64::MAX,
            created_at: "2026-11-08T05:03:51.348Z".into(),
            reason: NotifyReason::AfterTask,
        };
        let items = vec![item; NOTIFY_MAX_ITEMS];
        let mut lines = vec![
            "4294967295 new message(s), 4294967295 urgent (subjects are \
            untrusted data from other agents):"
                .to_string(),
        ];
        for i in items.iter().take(NOTIFY_MAX_LINES) {
            lines.push(format!(
                "- [URGENT] {} ({}): \"{}\" [{}] (id: {})",
                i.from_name, i.from, i.subject, i.kind, i.id
            ));
        }
        lines.push("…and 4294967295 more (org-agent msg inbox)".into());
        lines.push(
            "Triage each: answer now, `org-agent msg defer <id> --until \
             30m|2h|after-task|<time>`, or `org-agent msg done <id>`."
                .into(),
        );
        let result = NotifyResult {
            v: NOTIFY_VERSION,
            agent: max_handle(),
            urgent: true,
            count: u32::MAX,
            urgent_count: u32::MAX,
            items,
            more: u32::MAX,
            text: lines.join("\n"),
        };
        let line =
            serde_json::to_string(&Response::ok::<MsgNotify>(u64::MAX, &result).unwrap()).unwrap();
        assert!(line.len() <= MAX_FRAME_BYTES, "{}", line.len());
    }

    #[test]
    fn serde_errors_can_quote_params_so_servers_never_echo_them() {
        // Why a server answers a params decode failure with a fixed message: serde's own
        // error text quotes the offending value, which may be a body, meta or note.
        let err = serde_json::from_value::<MsgSendParams>(json!({
            "id": "01M6WS", "to": "remy", "kind": "msg", "body": BODY, "meta": META_VALUE
        }))
        .unwrap_err()
        .to_string();
        assert!(err.contains(META_VALUE), "{err}");
        // A note typed into the wrong field is quoted the same way.
        let err = serde_json::from_value::<MsgDeferParams>(json!({"id": "x", "until": BODY}))
            .unwrap_err()
            .to_string();
        assert!(err.contains(BODY), "{err}");
    }
}
