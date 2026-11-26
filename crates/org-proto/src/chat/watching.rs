//! The organization's watching model setting and `chat.channel_summarize`, including the
//! `since_read` catch-up digest (`ADR-0024` d13/d18, `WATCH-5`, `WATCH-10`, `TASK-99`).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::calls::Call;

/// `chat.watching_model_get`: read the organization's watching model profile setting
/// (`ADR-0024` d13, `WATCH-5`).
pub struct ChatWatchingModelGet;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatWatchingModelGetParams {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatWatchingModelResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_profile_name: Option<String>,
    pub enabled: bool,
    pub revision: u64,
}

impl Call for ChatWatchingModelGet {
    const METHOD: &'static str = "chat.watching_model_get";
    type Params = ChatWatchingModelGetParams;
    type Result = ChatWatchingModelResult;
}

/// `chat.watching_model_set`: owner-only toggle setting or clearing the organization's
/// watching model profile (`ADR-0024` d8/d13, `WATCH-5`).
pub struct ChatWatchingModelSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatWatchingModelSetParams {
    pub command_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_profile_id: Option<String>,
}

impl Call for ChatWatchingModelSet {
    const METHOD: &'static str = "chat.watching_model_set";
    type Params = ChatWatchingModelSetParams;
    type Result = ChatWatchingModelResult;
}

/// Most caller-visible messages one `chat.channel_summarize` call digests (`ADR-0024` d18).
pub const MAX_SUMMARIZE_WINDOW: u32 = 200;

/// `chat.channel_summarize`: answer a question about a channel using a tool-less run over
/// exactly the messages `msg list` would return to the caller (`ADR-0024` d18, `WATCH-10`), or
/// digest the caller's new messages since its read marker (`since_read`, `TASK-99`).
pub struct ChatChannelSummarize;

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatChannelSummarizeParams {
    pub channel: String,
    /// The question to answer. Required unless `since_read` is set, where an empty question
    /// asks for the default decisions, blockers, handoffs and open-questions catch-up.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub question: String,
    /// Most messages to digest (`1..=`[`MAX_SUMMARIZE_WINDOW`], default the maximum).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Digest only the caller's new messages after its channel read marker (the set
    /// `chat.messages` `since_read` answers), oldest first, at most `limit` per call. A pure
    /// read: the marker never moves (`TASK-99` rung 3).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub since_read: bool,
}

impl fmt::Debug for ChatChannelSummarizeParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatChannelSummarizeParams")
            .field("channel", &self.channel)
            .field("question_len", &self.question.len())
            .field("limit", &self.limit)
            .field("since_read", &self.since_read)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatChannelSummarizeResult {
    pub conversation_id: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_ids: Vec<String>,
    pub derived_notice: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_record_id: Option<String>,
    /// `since_read` only: how many new messages this digest covers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_message_count: Option<u64>,
    /// `since_read` only: new messages still unread after `through_message_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_unread: Option<u64>,
    /// `since_read` only: the last message the digest covers, or the unchanged read marker when
    /// nothing is new. `chat.mark_read` up to it to move on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_message_id: Option<String>,
}

impl fmt::Debug for ChatChannelSummarizeResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatChannelSummarizeResult")
            .field("conversation_id", &self.conversation_id)
            .field("summary_len", &self.summary.len())
            .field("message_ids_len", &self.message_ids.len())
            .field("new_message_count", &self.new_message_count)
            .field("remaining_unread", &self.remaining_unread)
            .finish()
    }
}

impl Call for ChatChannelSummarize {
    const METHOD: &'static str = "chat.channel_summarize";
    type Params = ChatChannelSummarizeParams;
    type Result = ChatChannelSummarizeResult;
}
