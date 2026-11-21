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
use crate::calls::Call;
use crate::directory::MemberPresenceChangedData;
use crate::inbox::InboxChangedData;
use crate::msg::{
    MessageHeader, MessageLifecycleState, MessagePriority, MessageReceivedData,
    MessageStateChangedData, Meta,
};
use crate::org::OrgChangedData;

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
    #[serde(default)]
    pub unread_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_at: Option<String>,
    #[serde(default)]
    pub is_member: bool,
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
    pub local_seq: u64,
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
    pub body: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub meta: Meta,
    pub created_at: String,
    pub origin_sequence: u64,
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
            .field("body_len", &self.body.len())
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessagesResult {
    pub conversation_id: String,
    pub messages: Vec<ChatMessageView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
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

/// `chat.mark_read`: update the caller's read marker in a conversation (`doc-90`).
pub struct ChatMarkRead;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMarkReadParams {
    pub conversation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMarkReadResult {
    pub conversation_id: String,
    pub read_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_message_id: Option<String>,
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

/// `chat.events_poll`: poll committed `org.changed` and live header-only messaging,
/// presence, and inbox-changed hints for the authenticated host channel (`/api/v1/events/stream`).
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
mod tests {
    use super::*;

    #[test]
    fn chat_wire_shapes_round_trip_and_redact_body_in_debug() {
        let send = ChatSendParams {
            id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a0".into(),
            to: "#general".into(),
            kind: "msg".into(),
            priority: None,
            subject: Some("Standup".into()),
            body: "super-secret-body-text".into(),
            meta: Meta::new(),
            reply_to: None,
            also_in_channel: false,
            reason: None,
        };
        let dbg = format!("{send:?}");
        assert!(!dbg.contains("super-secret-body-text"), "{dbg}");
        let encoded = serde_json::to_value(&send).unwrap();
        let decoded: ChatSendParams = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, send);

        let item = ChatInboxItem {
            id: send.id.clone(),
            conversation_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into(),
            thread_id: send.id.clone(),
            reply_to: None,
            from_member_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a2".into(),
            from_display_name: "Ada".into(),
            kind: "msg".into(),
            priority: MessagePriority::Normal,
            urgent: false,
            subject: None,
            created_at: "2026-11-21T12:00:00.000Z".into(),
            state: MessageLifecycleState::Unread,
            triage_gen: 0,
            snoozed_until: None,
            snooze_after_task: false,
            snooze_note: None,
            read_at: None,
            delivered_because: DeliveredBecause::Mention,
            related_message_ids: vec![],
            local_seq: 1,
        };
        let encoded = serde_json::to_value(&item).unwrap();
        assert_eq!(encoded["delivered_because"], "mention");
        assert_eq!(
            serde_json::from_value::<ChatInboxItem>(encoded).unwrap(),
            item
        );
    }
}
