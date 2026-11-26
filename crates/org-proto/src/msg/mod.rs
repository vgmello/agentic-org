//! Agent messaging (`docs/adr/0003-agent-messaging-office-first.md`).
//!
//! An office server serves one office of one organization, so these calls carry no `org`
//! param. Who may make each call, and which messages it may see, is the table in ADR-0003
//! §2.1.
//!
//! ## Binding
//! - A connection binds with [`MsgBind`] as a member (**member-bound**): an agent, by handle
//!   or id, with no credential. A person or the owner never binds here: they act through an
//!   owner grant on a host channel (X-02 §4.1), and a connection acting under a grant is
//!   **app-bound**. There is no app-wide binding and no app token: the retired `app_token`
//!   field is refused on presence ([`crate::retired`]).
//! - Binding fails closed (ADR-0003 §2.1.4, as amended by the agent-sessions design): an
//!   unknown member, a person or the owner, an agent not placed in this office, an offboarded
//!   agent or one whose status this office can't read, and a lookup that fails are all
//!   `not_authorized`. Only a malformed call is `invalid_request` (see [`MsgBind`] for the
//!   order). A paused agent may still bind: it keeps reading and answering its messages.
//! - Each `msg.bind` replaces the connection's previous binding and is checked only on its
//!   own params: an earlier binding grants nothing. A failed `msg.bind` leaves the connection
//!   unbound. Any re-bind, failed or not, ends the previous binding's held `msg.wait` and
//!   `msg.ask` calls with `not_bound`, and ends the connection's `subscribe` (subscribe again
//!   once bound).
//! - Until a connection binds, every other call here, and `who` and `status.set`
//!   ([`crate::directory`]), fails with `not_bound`. So does a call on a connection whose
//!   member was since moved out of this office or removed (ADR-0003 §3.1); it must bind again.
//! - A member-bound connection acts as its member. An agent sees only the messages it sent or
//!   received; an app-bound connection sees every message in the office.
//! - Calls of other modules follow their own rules: the organization commands
//!   ([`crate::org`]) need an app-bound connection (`org.snapshot`, a read, needs any
//!   binding).
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
//! A recipient or member (`to`, `from`, `member`, `participant`) is first trimmed of
//! leading and trailing ASCII whitespace (space, tab, line feed, form feed, carriage return;
//! HQ compares handles trimmed), and an empty result names no member. Other whitespace, such
//! as U+00A0, is text. It then resolves in this order (ADR-0003 §2.6):
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
//! So a client that prints a result re-serialized (e.g. `agent-os msg notify --format json`)
//! may show `unknown` for a value from a newer server.

mod bind;
mod events;
mod inbox;
mod message;
mod send;
mod spool;
mod triage;

pub use bind::{MsgBind, MsgBindParams, MsgBindResult};
pub use events::{
    MessageDeliveryFailed, MessageDeliveryFailedData, MessageReceived, MessageReceivedData,
    MessageStateChanged, MessageStateChangedData,
};
pub use inbox::{
    HeaderPage, HumanNeedsYouResult, MsgGet, MsgGetParams, MsgGetResult, MsgHumanNeedsYou,
    MsgHumanNeedsYouParams, MsgInbox, MsgInboxParams, MsgList, MsgListParams, MsgRead,
    MsgReadParams, MsgReadResult, MsgThreads, MsgThreadsParams, MsgWait, MsgWaitParams,
    MsgWaitResult, ThreadCursor, ThreadPage, ThreadSummary,
};
pub use message::{
    DeliveryFailedReason, Message, MessageHeader, MessageLifecycleState, MessagePriority,
    NotifyReason, SnoozeTarget,
};
pub use send::{
    MsgAsk, MsgAskParams, MsgAskResult, MsgReply, MsgReplyParams, MsgSend, MsgSendParams,
    SendResult,
};
pub use spool::SpooledSend;
pub use triage::{
    MsgDefer, MsgDeferParams, MsgDeferResult, MsgDone, MsgDoneParams, MsgDoneResult, MsgNotify,
    MsgNotifyParams, MsgUnsnooze, MsgUnsnoozeParams, MsgUnsnoozeResult, NotifyItem, NotifyResult,
};

use std::collections::BTreeMap;
use std::fmt;

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

/// Whether a message is urgent: it signals elevated attention for a busy agent (delivered at a
/// safe step boundary without aborting running tools, per `ADR-0035` / `doc-101`). True iff
/// `kind` is [`KIND_REQUEST`] or `priority` is `high` or `urgent`. An unknown priority isn't
/// urgent.
pub fn is_urgent(kind: &str, priority: MessagePriority) -> bool {
    kind == KIND_REQUEST || matches!(priority, MessagePriority::High | MessagePriority::Urgent)
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

#[cfg(test)]
mod tests;
