//! B6 Conversations, channels, direct messages, mentions, inbox delivery reasons,
//! reactions, read markers, handoff acceptance, and live event polling (`doc-90`, `ADR-0024`,
//! `doc-28` §3.3).
//!
//! All methods belong to the `chat.*`, `dm.*`, or `msg.*` families so X-02 host-channel
//! policy (`operations.json`) serves them with verified per-request grants while keeping
//! refused families (`channel.*`, `session.*`, `subscribe`) strictly closed.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ask::{AskAnswerRecord, AskWithdrawalRecord};
use crate::attachments::AttachmentMeta;
use crate::calls::Call;
use crate::directory::MemberPresenceChangedData;
use crate::inbox::InboxChangedData;
use crate::msg::{
    MessageHeader, MessageLifecycleState, MessagePriority, MessageReceivedData,
    MessageStateChangedData, Meta,
};
use crate::org::OrgChangedData;

mod watching;

pub use watching::{
    ChatChannelSummarize, ChatChannelSummarizeParams, ChatChannelSummarizeResult,
    ChatWatchingModelGet, ChatWatchingModelGetParams, ChatWatchingModelResult,
    ChatWatchingModelSet, ChatWatchingModelSetParams, MAX_SUMMARIZE_WINDOW,
};

/// Most characters in a headers-only `snippet` (`TASK-99` rung 4).
pub const SNIPPET_MAX_CHARS: usize = 160;

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// Conversation kind (`doc-90`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConversationKind {
    Dm,
    GroupDm,
    Channel,
    #[serde(other)]
    Unknown,
}

/// Optional automatic scope of a channel (`doc-90`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AutoScope {
    Organization,
    Team,
    Project,
    #[serde(other)]
    Unknown,
}

/// Why a message reached an employee's inbox (`doc-90`, `ADR-0024`, `WATCH-1/2/9`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeliveredBecause {
    Addressed,
    ThreadReply,
    Mention,
    AnswerCandidate,
    #[serde(other)]
    Unknown,
}

/// Per-viewer mute preference for a member in a conversation (`doc-90`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MuteMode {
    #[default]
    None,
    Muted,
    Digest,
    #[serde(other)]
    Unknown,
}

/// Supported message reaction kinds (`doc-90`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReactionKind {
    Seen,
    Looking,
    #[serde(other)]
    Unknown,
}

/// One member's membership state in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationMembershipView {
    pub employee_id: String,
    pub joined_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_at: Option<String>,
}

/// One pinned message reference on a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationPinView {
    pub message_id: String,
    pub pinned_by: String,
    pub pinned_at: String,
}

/// Summary of one conversation (`dm`, `group_dm`, or `channel`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub kind: ConversationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_scope: Option<AutoScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub archived: bool,
    pub revision: u64,
    pub created_at: String,
    pub members: Vec<ConversationMembershipView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<ConversationPinView>,
    /// Unread inbox items (addressed, mentions, thread replies) the caller holds here.
    #[serde(default)]
    pub unread_count: u64,
    /// Messages after the caller's read marker that the caller did not send: every such message
    /// when it has no marker yet (`TASK-99` rung 1). Channel posts reach no inbox (`ADR-0024`
    /// d6), so this, not `unread_count`, is what is new in a channel.
    #[serde(default)]
    pub new_message_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_at: Option<String>,
    #[serde(default)]
    pub is_member: bool,
}

impl ConversationSummary {
    /// Whether the caller has anything new here: `new_message_count > 0 || unread_count > 0`.
    pub fn has_unread(&self) -> bool {
        self.new_message_count > 0 || self.unread_count > 0
    }
}

/// One resolved `@handle` mention recorded by the office (`WATCH-2/9`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MentionedMember {
    pub employee_id: String,
    pub handle: String,
    pub display_name: String,
}

/// One reaction attached to a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReactionView {
    pub employee_id: String,
    pub reaction: ReactionKind,
    pub created_at: String,
}

/// Handoff acceptance record (`CHAT-18`, `doc-90`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffAcceptanceView {
    pub accepted_by: String,
    pub accepted_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One enriched inbox entry carrying its delivery reason (`WATCH-1/2/9`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatInboxItem {
    pub id: String,
    pub conversation_id: String,
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    pub from_member_id: String,
    pub from_display_name: String,
    pub kind: String,
    pub priority: MessagePriority,
    pub urgent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub created_at: String,
    pub state: MessageLifecycleState,
    pub triage_gen: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snooze_after_task: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snooze_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_at: Option<String>,
    pub delivered_because: DeliveredBecause,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_message_ids: Vec<String>,
    /// Optional human-readable link context (e.g. matched question text for `answer_candidate`,
    /// `ADR-0024` d11).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub local_seq: u64,
    /// The message's files (`TASK-98.2`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentMeta>,
}

/// One full message inside a conversation, including reactions and handoff acceptance.
/// Debug redacts body and meta contents.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessageView {
    pub id: String,
    pub conversation_id: String,
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub also_in_channel: bool,
    pub sender_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_employee_id: Option<String>,
    pub sender_display_name: String,
    pub kind: String,
    pub priority: MessagePriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// The full body; `None` only in a headers-only read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Headers-only reads: the body on one line, at most [`SNIPPET_MAX_CHARS`] characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// Headers-only reads: the full body's length in UTF-8 bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub meta: Meta,
    pub created_at: String,
    pub origin_sequence: u64,
    /// Replies in the thread this message roots (0 for a reply).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub reply_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_reply_at: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub attachment_count: u64,
    /// The message's files (`TASK-98.2`); kept in a headers-only read too.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentMeta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<ReactionView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<HandoffAcceptanceView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<AskAnswerRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawal: Option<AskWithdrawalRecord>,
}

impl fmt::Debug for ChatMessageView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatMessageView")
            .field("id", &self.id)
            .field("conversation_id", &self.conversation_id)
            .field("thread_id", &self.thread_id)
            .field("kind", &self.kind)
            .field("body_len", &self.body.as_ref().map_or(0, String::len))
            .field("body_bytes", &self.body_bytes)
            .field("meta_keys", &self.meta.len())
            .finish()
    }
}

/// `dm.open`: open or return an existing DM (2 members) or group DM (3..=8 members).
pub struct DmOpen;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DmOpenParams {
    /// Other participant member IDs or handles (the caller is always included automatically).
    pub participants: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DmOpenResult {
    pub conversation: ConversationSummary,
    pub created: bool,
}

impl Call for DmOpen {
    const METHOD: &'static str = "dm.open";
    type Params = DmOpenParams;
    type Result = DmOpenResult;
}

/// `chat.channels`: list channels visible to the caller (auto and manual).
pub struct ChatChannels;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatChannelsParams {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_archived: bool,
    /// Only channels where [`ConversationSummary::has_unread`] holds.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unread_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatChannelsResult {
    pub channels: Vec<ConversationSummary>,
}

impl Call for ChatChannels {
    const METHOD: &'static str = "chat.channels";
    type Params = ChatChannelsParams;
    type Result = ChatChannelsResult;
}

/// `chat.channel_create`: create a manual channel.
pub struct ChatChannelCreate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatChannelCreateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatChannelCreateResult {
    pub channel: ConversationSummary,
}

impl Call for ChatChannelCreate {
    const METHOD: &'static str = "chat.channel_create";
    type Params = ChatChannelCreateParams;
    type Result = ChatChannelCreateResult;
}

/// `chat.channel_join`: join a manual channel by ID or name (with or without `#`).
pub struct ChatChannelJoin;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatChannelMembershipParams {
    pub channel: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatChannelMembershipResult {
    pub channel: ConversationSummary,
    pub joined: bool,
}

impl Call for ChatChannelJoin {
    const METHOD: &'static str = "chat.channel_join";
    type Params = ChatChannelMembershipParams;
    type Result = ChatChannelMembershipResult;
}

/// `chat.channel_leave`: leave a manual channel by ID or name.
pub struct ChatChannelLeave;

impl Call for ChatChannelLeave {
    const METHOD: &'static str = "chat.channel_leave";
    type Params = ChatChannelMembershipParams;
    type Result = ChatChannelMembershipResult;
}

/// `chat.conversations`: list all conversations (`dm`, `group_dm`, `channel`) visible to caller.
pub struct ChatConversations;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatConversationsParams {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatConversationsResult {
    pub conversations: Vec<ConversationSummary>,
}

impl Call for ChatConversations {
    const METHOD: &'static str = "chat.conversations";
    type Params = ChatConversationsParams;
    type Result = ChatConversationsResult;
}

/// `chat.send`: send a message to a member (DM), conversation ID, or channel (`#name` or ID),
/// extracting `@handle` mentions (`WATCH-1/2/9`) and acknowledging at local commit (`ACK-1`).
pub struct ChatSend;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSendParams {
    /// Canonical lowercase UUIDv7 message ID.
    pub id: String,
    /// Recipient member (id/handle), channel (`#name` / name / id), or conversation ID.
    pub to: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<MessagePriority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub body: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub also_in_channel: bool,
    /// Required when `kind == "announcement"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Up to [`crate::attachments::MESSAGE_ATTACHMENTS_MAX`] of the caller's committed staged
    /// uploads, bound to the message (`TASK-98.2`). With files, `body` may be empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
}

fn default_kind() -> String {
    crate::msg::KIND_MSG.to_owned()
}

impl fmt::Debug for ChatSendParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatSendParams")
            .field("id", &self.id)
            .field("to", &self.to)
            .field("kind", &self.kind)
            .field("body_len", &self.body.len())
            .field("meta_keys", &self.meta.len())
            .field("attachments", &self.attachment_ids.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSendResult {
    pub id: String,
    pub conversation_id: String,
    pub thread_id: String,
    pub local_seq: u64,
    pub origin_seq: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<MentionedMember>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmatched_mentions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipient_presence: Vec<crate::directory::RecipientPresence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub woke_agents: Vec<String>,
}

impl Call for ChatSend {
    const METHOD: &'static str = "chat.send";
    type Params = ChatSendParams;
    type Result = ChatSendResult;
}

/// `chat.messages`: list full messages of one conversation or thread visible to the caller.
pub struct ChatMessages;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessagesParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Backward page cursor (`next_cursor`) of a plain read; never with a delta read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
    /// Delta read: the messages after the caller's read marker in `conversation` that the
    /// caller did not send, oldest first (`TASK-99` rung 2).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub since_read: bool,
    /// Delta read: the messages strictly after this message (canonical UUIDv7) in the
    /// conversation's `(created_at, local_seq, id)` order, oldest first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_id: Option<String>,
    /// Delta read paging: the page starts strictly after this message (the previous page's
    /// `next_after_id`), never before the `since_read` / `after_id` anchor, and `total` still
    /// counts every match after that anchor, so it holds steady from page to page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_after_id: Option<String>,
    /// Only thread roots and replies also posted to the channel; a thread's replies show in its
    /// root's `reply_count` and `latest_reply_at`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub top_level_only: bool,
    /// Omit `body` and `meta`; answer a one-line `snippet` and `body_bytes` instead.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub headers_only: bool,
    /// `has:image`: only messages with (`true`) or without (`false`) an image file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_image: Option<bool>,
    /// `has:file`: only messages with (`true`) or without (`false`) a document file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_file: Option<bool>,
}

impl ChatMessagesParams {
    /// Whether this is a delta read (`since_read` or `after_id`): oldest first, forward pages.
    pub fn is_delta(&self) -> bool {
        self.since_read || self.after_id.is_some()
    }

    /// The read's `has:image` / `has:file` filter.
    pub fn attachment_filter(&self) -> crate::attachments::AttachmentFilter {
        crate::attachments::AttachmentFilter::new(self.has_image, self.has_file)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessagesResult {
    pub conversation_id: String,
    pub messages: Vec<ChatMessageView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
    /// Delta reads with more to read: the `page_after_id` that reads the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_after_id: Option<String>,
    /// Delta reads: how many messages match after the `since_read` / `after_id` anchor,
    /// across every page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

impl Call for ChatMessages {
    const METHOD: &'static str = "chat.messages";
    type Params = ChatMessagesParams;
    type Result = ChatMessagesResult;
}

/// `chat.inbox`: enriched inbox listing with `delivered_because` and `--because` filter (`WATCH-1/2/9`).
pub struct ChatInbox;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatInboxParams {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_all: bool,
    /// Optional explicit lifecycle state filter (e.g., `snoozed` for the Snoozed inbox view).
    /// When set, returns only items in that state; when omitted, `include_all` controls whether
    /// non-`unread` items are included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<crate::msg::MessageLifecycleState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub because: Option<DeliveredBecause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// `has:image`: only items whose message has (`true`) or lacks (`false`) an image file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_image: Option<bool>,
    /// `has:file`: only items whose message has (`true`) or lacks (`false`) a document file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_file: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatInboxResult {
    pub items: Vec<ChatInboxItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
}

impl Call for ChatInbox {
    const METHOD: &'static str = "chat.inbox";
    type Params = ChatInboxParams;
    type Result = ChatInboxResult;
}

/// `chat.mark_read`: advance the caller's read marker in a conversation (`doc-90`, `TASK-99`).
/// The marker only moves forward, and inbox items (`message_recipient_states`) never change.
pub struct ChatMarkRead;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMarkReadParams {
    /// Conversation UUIDv7, `#channel`, or channel name.
    pub conversation_id: String,
    /// The message read up to; omitted, the conversation's latest message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_message_id: Option<String>,
    /// Decide and validate without changing anything.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMarkReadResult {
    pub conversation_id: String,
    /// When the marker was last moved (now when this call moved it).
    pub read_at: String,
    /// The marker after the call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_message_id: Option<String>,
    /// Whether the call moved the marker forward; `false` when it already covered the message.
    #[serde(default)]
    pub advanced: bool,
}

impl Call for ChatMarkRead {
    const METHOD: &'static str = "chat.mark_read";
    type Params = ChatMarkReadParams;
    type Result = ChatMarkReadResult;
}

/// `chat.mute`: update the caller's mute preference for an employee in a conversation (`doc-90`).
pub struct ChatMute;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMuteParams {
    pub conversation_id: String,
    pub muted_employee_id: String,
    pub mute_mode: MuteMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMuteResult {
    pub conversation_id: String,
    pub muted_employee_id: String,
    pub mute_mode: MuteMode,
}

impl Call for ChatMute {
    const METHOD: &'static str = "chat.mute";
    type Params = ChatMuteParams;
    type Result = ChatMuteResult;
}

/// `chat.react`: add or remove a `seen` or `looking` reaction on a message (`doc-90`).
pub struct ChatReact;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatReactParams {
    pub message_id: String,
    pub reaction: ReactionKind,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remove: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatReactResult {
    pub message_id: String,
    pub reaction: ReactionKind,
    pub present: bool,
}

impl Call for ChatReact {
    const METHOD: &'static str = "chat.react";
    type Params = ChatReactParams;
    type Result = ChatReactResult;
}

/// `msg.accept`: accept a handoff message addressed to the caller (`CHAT-18`, `doc-28` §3.3).
pub struct MsgAccept;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgAcceptParams {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgAcceptResult {
    pub id: String,
    pub accepted_by: String,
    pub accepted_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Call for MsgAccept {
    const METHOD: &'static str = "msg.accept";
    type Params = MsgAcceptParams;
    type Result = MsgAcceptResult;
}

/// Maximum server-side wait duration accepted by `chat.events_poll` (`TASK-140`).
pub const MAX_EVENTS_POLL_WAIT_MS: u64 = 25_000;

/// `chat.events_poll`: poll or event-wait for committed `org.changed` and live header-only
/// messaging, presence, and inbox-changed hints for the authenticated host channel (`/api/v1/events/stream`).
pub struct ChatEventsPoll;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEventsPollParams {
    /// Last applied organization sequence; `None` starts from the current head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_org_seq: Option<u64>,
    /// Last observed live message/inbox ring cursor; `None` starts from the current head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_live_seq: Option<u64>,
    /// Last observed presence ring cursor; `None` starts from the current head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_presence_seq: Option<u64>,
    /// Optional server-side condvar wait (milliseconds, clamped to [`MAX_EVENTS_POLL_WAIT_MS`])
    /// when no changes are immediately ready after the supplied cursors (`TASK-140`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEventsPollResult {
    pub org_head_seq: u64,
    pub live_head_seq: u64,
    pub presence_head_seq: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snapshot_required: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub live_lost: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub org_changes: Vec<OrgChangedData>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages_received: Vec<MessageReceivedData>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_states: Vec<MessageStateChangedData>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presence_changes: Vec<MemberPresenceChangedData>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inbox_changes: Vec<InboxChangedData>,
}

impl Call for ChatEventsPoll {
    const METHOD: &'static str = "chat.events_poll";
    type Params = ChatEventsPollParams;
    type Result = ChatEventsPollResult;
}

/// Helper to build a header-only `MessageReceivedData` with zero body exposure.
pub fn header_only_received(header: MessageHeader) -> MessageReceivedData {
    MessageReceivedData { header }
}

#[cfg(test)]
mod tests;
