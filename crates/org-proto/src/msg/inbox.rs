//! Reading: `msg.wait`, `msg.read`, `msg.inbox`, `msg.list`, `msg.get`,
//! `msg.human_needs_you` and `msg.threads`.

use serde::{Deserialize, Serialize};

use super::message::{Message, MessageHeader};
use crate::calls::Call;

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
    /// Only messages from this member (see [Addressing](super#addressing)).
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
    /// Only messages from this member (see [Addressing](super#addressing)).
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
    /// Only messages this member sent or received (see [Addressing](super#addressing)).
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
    /// Only threads this member is in (see [Addressing](super#addressing)).
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
