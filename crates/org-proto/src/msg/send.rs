//! Sending: `msg.send`, `msg.reply` and `msg.ask`.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::message::{Message, MessagePriority};
use super::{BodySize, Meta, MetaSize};
use crate::calls::Call;

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
    /// The recipient (see [Addressing](super#addressing)).
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
    /// Up to [`crate::attachments::MESSAGE_ATTACHMENTS_MAX`] of the caller's committed staged
    /// uploads, bound to the message (`TASK-98.2`). With files, `body` may be empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
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
            .field("attachments", &self.attachment_ids.len())
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
    /// Up to [`crate::attachments::MESSAGE_ATTACHMENTS_MAX`] of the caller's committed staged
    /// uploads, bound to the message (`TASK-98.2`). With files, `body` may be empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
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
            .field("attachments", &self.attachment_ids.len())
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
    /// The recipient (see [Addressing](super#addressing)).
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
    /// Up to [`crate::attachments::MESSAGE_ATTACHMENTS_MAX`] of the caller's committed staged
    /// uploads, bound to the message (`TASK-98.2`). With files, `body` may be empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
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
            .field("attachments", &self.attachment_ids.len())
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
