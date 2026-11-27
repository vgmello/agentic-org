//! `ReadOp` wrappers for asks, the owner inbox, channels, conversations and messages.

use org_proto::ask::ChatWaitAskResult;
use org_proto::attachments::AttachmentMeta;
use org_proto::chat::{
    ChatChannelsParams, ChatChannelsResult, ChatConversationsParams, ChatConversationsResult,
    ChatInboxParams, ChatInboxResult, ChatMessagesParams, ChatMessagesResult,
};
use org_proto::msg::{MessageHeader, MessageLifecycleState};
use org_proto::ApiError;
use org_store::{Id, StoreError};

use crate::messaging::{
    asks::{self, AskCard},
    features::{self, to_wire_header, ChannelCatchup},
    files,
    records::{ConversationRecord, MessageRecord},
};
use crate::store::open::ReadOp;
use crate::tasks::ports::TasksRead;

use super::mailbox_commit::resolve_local_seq;

pub struct InspectAskStatusRead {
    pub owner: Id,
    pub caller: Id,
    pub ask_id: String,
    pub now_ms: u64,
}

impl ReadOp for InspectAskStatusRead {
    type Out = Result<ChatWaitAskResult, ApiError>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let owner = crate::organization::owner::effective_owner_id(snapshot, &self.owner)?;
        Ok(asks::inspect_ask_status(
            snapshot,
            &owner,
            &self.caller,
            &self.ask_id,
            self.now_ms,
        ))
    }
}

/// One unread owner inbox entry enriched with parsed [`AskCard`] metadata when present.
#[derive(Debug, Clone)]
pub struct OwnerInboxEntry {
    pub header: MessageHeader,
    pub body: String,
    pub card: Option<AskCard>,
    /// The message's files (`TASK-98.2`).
    pub attachments: Vec<AttachmentMeta>,
}

pub struct OpenOwnerInboxRead {
    pub owner: Id,
    pub limit: usize,
}

impl ReadOp for OpenOwnerInboxRead {
    type Out = Vec<OwnerInboxEntry>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let owner = crate::organization::owner::effective_owner_id(snapshot, &self.owner)?;
        let mut entries = Vec::new();
        for st in snapshot.recipient_states_for_employee(&owner)? {
            if st.state == MessageLifecycleState::Done {
                continue;
            }
            let Some(msg) = snapshot.message(&st.message_id)? else {
                continue;
            };
            let card = asks::parse_ask_card(&msg);
            // Snoozed messages (including snoozed asks) stay hidden from active Inbox and
            // excluded from active badge counts until `snoozed_until` or early un-snooze (`TASK-81`).
            let include = match &card {
                Some(_) => {
                    asks::is_open_ask_card(snapshot, &msg)
                        && matches!(
                            st.state,
                            MessageLifecycleState::Unread | MessageLifecycleState::Read
                        )
                }
                None => st.state == MessageLifecycleState::Unread,
            };
            if !include {
                continue;
            }
            let seq = resolve_local_seq(snapshot, &msg)?;
            let header = to_wire_header(&msg, Some(&st), seq);
            let attachments = files::metas(
                &msg.attachments,
                &msg.created_at,
                msg.sender_employee_id.as_ref(),
            );
            entries.push(OwnerInboxEntry {
                header,
                body: msg.body_markdown,
                card,
                attachments,
            });
        }
        entries.sort_by(|a, b| {
            b.header
                .local_seq
                .cmp(&a.header.local_seq)
                .then_with(|| b.header.id.cmp(&a.header.id))
        });
        entries.truncate(self.limit);
        Ok(entries)
    }
}

// ---- ReadOps ----

pub struct ListChannelsRead {
    pub viewer: Id,
    pub owner: Id,
    pub params: ChatChannelsParams,
    pub now_utc: String,
}

impl ReadOp for ListChannelsRead {
    type Out = ChatChannelsResult;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let owner = crate::organization::owner::effective_owner_id(snapshot, &self.owner)?;
        Ok(features::list_channels(
            snapshot,
            &self.viewer,
            &owner,
            &self.params,
            &self.now_utc,
        )?)
    }
}

pub struct ListConversationsRead {
    pub viewer: Id,
    pub owner: Id,
    pub params: ChatConversationsParams,
    pub now_utc: String,
}

impl ReadOp for ListConversationsRead {
    type Out = ChatConversationsResult;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let owner = crate::organization::owner::effective_owner_id(snapshot, &self.owner)?;
        Ok(features::list_conversations(
            snapshot,
            &self.viewer,
            &owner,
            &self.params,
            &self.now_utc,
        )?)
    }
}

pub struct ChatInboxRead {
    pub owner: Id,
    pub caller: Id,
    pub params: ChatInboxParams,
}

impl ReadOp for ChatInboxRead {
    type Out = Result<ChatInboxResult, ApiError>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let owner = crate::organization::owner::effective_owner_id(snapshot, &self.owner)?;
        Ok(features::list_chat_inbox(
            snapshot,
            &owner,
            &self.caller,
            &self.params,
        ))
    }
}

pub struct ChatMessagesRead {
    pub owner: Id,
    pub caller: Id,
    pub params: ChatMessagesParams,
    pub now_utc: String,
}

impl ReadOp for ChatMessagesRead {
    type Out = Result<ChatMessagesResult, ApiError>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let owner = crate::organization::owner::effective_owner_id(snapshot, &self.owner)?;
        Ok(features::list_chat_messages(
            snapshot,
            &owner,
            &self.caller,
            &self.params,
            &self.now_utc,
        ))
    }
}

/// Reads caller-visible messages in a channel (same visibility as `msg list`, capped at 200)
/// for `channel.summarize` (`WATCH-10`).
pub struct ChannelMessagesForCallerRead {
    pub owner: Id,
    pub caller: Id,
    pub channel_ref: String,
    pub limit: usize,
    pub now_utc: String,
}

impl ReadOp for ChannelMessagesForCallerRead {
    type Out = Result<(ConversationRecord, Vec<MessageRecord>), ApiError>;

    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let owner = crate::organization::owner::effective_owner_id(snapshot, &self.owner)?;
        Ok(features::channel_messages_for_caller(
            snapshot,
            &owner,
            &self.caller,
            &self.channel_ref,
            self.limit,
            &self.now_utc,
        ))
    }
}

/// Reads the caller's new channel messages after its read marker for a `since_read` digest
/// (`TASK-99` rung 3), oldest first, at most `limit`.
pub struct ChannelCatchupRead {
    pub owner: Id,
    pub caller: Id,
    pub channel_ref: String,
    pub limit: usize,
    pub now_utc: String,
}

impl ReadOp for ChannelCatchupRead {
    type Out = Result<ChannelCatchup, ApiError>;

    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let owner = crate::organization::owner::effective_owner_id(snapshot, &self.owner)?;
        Ok(features::channel_catchup_for_caller(
            snapshot,
            &owner,
            &self.caller,
            &self.channel_ref,
            self.limit,
            &self.now_utc,
        ))
    }
}

/// Reads all messages in a conversation for the office-local channel watcher (`WATCH-3/7`).
pub struct ChannelMessagesRawRead {
    pub conversation_id: Id,
}

impl ReadOp for ChannelMessagesRawRead {
    type Out = Vec<MessageRecord>;

    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(snapshot.messages_in_conversation(&self.conversation_id)?)
    }
}

/// Reads one conversation (persisted or virtual auto-channel) by ID (`WATCH-3/6`).
pub struct ConversationByIdRead {
    pub conversation_id: Id,
    pub now_utc: String,
}

impl ReadOp for ConversationByIdRead {
    type Out = Option<ConversationRecord>;

    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        if let Some(conv) = snapshot.conversation(&self.conversation_id)? {
            return Ok(Some(conv));
        }
        let autos = features::virtual_auto_channels(snapshot, &self.now_utc)?;
        Ok(autos.into_iter().find(|c| c.id == self.conversation_id))
    }
}
