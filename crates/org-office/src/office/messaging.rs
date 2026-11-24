//! Messaging, presence/status lines, `OrgFeed` replay, and mailbox store methods on [`Office`].

use org_proto::ask::{ChatAnswerParams, ChatAskParams, ChatWaitAskResult, MsgWithdrawParams};
use org_proto::chat::{
    ChatChannelCreateParams, ChatChannelCreateResult, ChatChannelMembershipParams,
    ChatChannelMembershipResult, ChatChannelsParams, ChatChannelsResult, ChatConversationsParams,
    ChatConversationsResult, ChatInboxParams, ChatInboxResult, ChatMarkReadParams,
    ChatMarkReadResult, ChatMessagesParams, ChatMessagesResult, ChatMuteParams, ChatMuteResult,
    ChatReactParams, ChatReactResult, ChatSendParams, DmOpenParams, DmOpenResult, MsgAcceptParams,
    MsgAcceptResult,
};
use org_proto::directory::{
    MemberPresence, MemberPresenceChangedData, StatusSetParams, StatusSetResult,
};
use org_proto::msg::{HeaderPage, MessageHeader};
use org_proto::org::OrgChangedData;
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use crate::{
    error::OfficeError,
    messaging::{
        asks::{AnswerCommitted, AskCommitted, WithdrawCommitted},
        features::ChatSendCommitted,
    },
    office::{storage, Office},
    store::open::{
        messaging_ops::{
            LocalTransitionInput, MailboxCommitInput, MailboxCommitOut, MailboxStored,
            NotifyCandidateOut, OwnerInboxEntry, PeekUnreadOut,
        },
        ChannelCreateOp, ChannelMembershipOp, ChannelMessagesForCallerRead, ChannelMessagesRawRead,
        ChatAnswerOp, ChatInboxRead, ChatMarkReadOp, ChatMessagesRead, ChatMuteOp, ChatReactOp,
        ChatSendOp, ClaimMailboxReadOp, CommitMailboxMessageOp, ConversationByIdRead,
        DeliverAnswerCandidatesOp, DmOpenOp, FindReplyForAskRead, GetMailboxMessageRead,
        InspectAskStatusRead, ListActiveSnoozeDeadlinesRead, ListChannelsRead,
        ListConversationsRead, ListMailboxHeadersRead, MailboxCheckNotifyOp, MailboxDeferOp,
        MailboxUnsnoozeOp, MailboxUpdateStateOp, MsgAcceptOp, MsgAskOp, MsgWithdrawOp,
        OpenOwnerInboxRead, OrgChangesAfterRead, PeekUnreadRead,
    },
    time,
};

fn parse_member_id(raw: &str) -> Result<Id, ApiError> {
    Id::parse(raw).map_err(|_| ApiError::new(ErrorCode::NotBound, "invalid bound member id"))
}

impl Office {
    /// Sets or clears `member_id`'s self-reported status line (`status.set`).
    pub fn set_status(
        &self,
        member_id: &str,
        params: &StatusSetParams,
    ) -> Result<(StatusSetResult, MemberPresenceChangedData), ApiError> {
        let now_ms = self.monotonic_now_ms();
        self.runtime.set_status(member_id, params, now_ms)
    }

    /// Updates a member's runtime presence (used on session activity/connect/disconnect).
    pub fn set_presence(
        &self,
        member_id: &str,
        presence: MemberPresence,
    ) -> MemberPresenceChangedData {
        let now_ms = self.monotonic_now_ms();
        self.runtime.set_presence(member_id, presence, now_ms)
    }

    /// Reads committed organization changes after `after_seq` for `OrgFeed` replay (`subscribe`
    /// and `/api/v1/events/stream` resume).
    pub fn org_changes_after(
        &self,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<OrgChangedData>, OfficeError> {
        let (rows, _) = self
            .store
            .read(OrgChangesAfterRead {
                owner: self.owner.clone(),
                after_seq,
                limit,
            })
            .map_err(storage)?;
        Ok(rows)
    }

    /// Canonical journal incarnation of this open office store.
    pub fn journal_incarnation(&self) -> &str {
        self.identity.journal_incarnation.as_str()
    }

    pub fn dm_open(
        &self,
        caller: &str,
        params: DmOpenParams,
    ) -> Result<Result<DmOpenResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        self.store
            .run(DmOpenOp {
                here: self.identity.clone(),
                owner: self.owner.clone(),
                caller: caller_id,
                params,
                now_utc,
            })
            .map_err(storage)
    }

    pub fn list_channels(
        &self,
        caller: &str,
        params: ChatChannelsParams,
    ) -> Result<Result<ChatChannelsResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        let (res, _) = self
            .store
            .read(ListChannelsRead {
                viewer: caller_id,
                owner: self.owner.clone(),
                params,
                now_utc,
            })
            .map_err(storage)?;
        Ok(Ok(res))
    }

    pub fn create_channel(
        &self,
        caller: &str,
        params: ChatChannelCreateParams,
    ) -> Result<Result<ChatChannelCreateResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        self.store
            .run(ChannelCreateOp {
                here: self.identity.clone(),
                owner: self.owner.clone(),
                caller: caller_id,
                params,
                now_utc,
            })
            .map_err(storage)
    }

    pub fn set_channel_membership(
        &self,
        caller: &str,
        params: ChatChannelMembershipParams,
        join: bool,
    ) -> Result<Result<ChatChannelMembershipResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        self.store
            .run(ChannelMembershipOp {
                owner: self.owner.clone(),
                caller: caller_id,
                params,
                join,
                now_utc,
            })
            .map_err(storage)
    }

    pub fn list_conversations(
        &self,
        caller: &str,
        params: ChatConversationsParams,
    ) -> Result<Result<ChatConversationsResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        let (res, _) = self
            .store
            .read(ListConversationsRead {
                viewer: caller_id,
                owner: self.owner.clone(),
                params,
                now_utc,
            })
            .map_err(storage)?;
        Ok(Ok(res))
    }

    pub fn chat_send(
        &self,
        caller: &str,
        params: ChatSendParams,
    ) -> Result<Result<ChatSendCommitted, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let org_seq = self.org_snapshot()?.position.seq;
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        self.store
            .run(ChatSendOp {
                here: self.identity.clone(),
                owner: self.owner.clone(),
                caller: caller_id,
                params,
                org_seq,
                now_utc,
            })
            .map_err(storage)
    }

    pub fn chat_inbox(
        &self,
        caller: &str,
        params: ChatInboxParams,
    ) -> Result<Result<ChatInboxResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let (res, _) = self
            .store
            .read(ChatInboxRead {
                owner: self.owner.clone(),
                caller: caller_id,
                params,
            })
            .map_err(storage)?;
        Ok(res)
    }

    pub fn chat_messages(
        &self,
        caller: &str,
        params: ChatMessagesParams,
    ) -> Result<Result<ChatMessagesResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        let (res, _) = self
            .store
            .read(ChatMessagesRead {
                owner: self.owner.clone(),
                caller: caller_id,
                params,
                now_utc,
            })
            .map_err(storage)?;
        Ok(res)
    }

    pub fn chat_mark_read(
        &self,
        caller: &str,
        params: ChatMarkReadParams,
    ) -> Result<Result<ChatMarkReadResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        self.store
            .run(ChatMarkReadOp {
                here: self.identity.clone(),
                owner: self.owner.clone(),
                caller: caller_id,
                params,
                now_utc,
            })
            .map_err(storage)
    }

    pub fn chat_mute(
        &self,
        caller: &str,
        params: ChatMuteParams,
    ) -> Result<Result<ChatMuteResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        self.store
            .run(ChatMuteOp {
                here: self.identity.clone(),
                owner: self.owner.clone(),
                caller: caller_id,
                params,
                now_utc,
            })
            .map_err(storage)
    }

    pub fn chat_react(
        &self,
        caller: &str,
        params: ChatReactParams,
    ) -> Result<Result<ChatReactResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        self.store
            .run(ChatReactOp {
                here: self.identity.clone(),
                owner: self.owner.clone(),
                caller: caller_id,
                params,
                now_utc,
            })
            .map_err(storage)
    }

    pub fn msg_accept(
        &self,
        caller: &str,
        params: MsgAcceptParams,
    ) -> Result<Result<MsgAcceptResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let org_seq = self.org_snapshot()?.position.seq;
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        self.store
            .run(MsgAcceptOp {
                here: self.identity.clone(),
                caller: caller_id,
                params,
                org_seq,
                now_utc,
            })
            .map_err(storage)
    }

    pub fn msg_ask(
        &self,
        caller: &str,
        params: ChatAskParams,
    ) -> Result<Result<AskCommitted, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let org_seq = self.org_snapshot()?.position.seq;
        let now_ms = self.monotonic_now_ms();
        self.store
            .run(MsgAskOp {
                here: self.identity.clone(),
                owner: self.owner.clone(),
                caller: caller_id,
                params,
                org_seq,
                now_ms,
            })
            .map_err(storage)
    }

    pub fn chat_answer(
        &self,
        caller: &str,
        params: ChatAnswerParams,
    ) -> Result<Result<AnswerCommitted, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let org_seq = self.org_snapshot()?.position.seq;
        let now_ms = self.monotonic_now_ms();
        let now_utc = time::format_rfc3339_ms(now_ms);
        let reply_id = crate::messaging::records::derive_conversation_id(
            "ask_reply",
            &format!(
                "{}:{}:{caller_id}:{now_ms}",
                self.identity.organization_id, params.ask_id
            ),
        )
        .as_str()
        .to_owned();
        self.store
            .run(ChatAnswerOp {
                here: self.identity.clone(),
                owner: self.owner.clone(),
                caller: caller_id,
                params,
                reply_id,
                org_seq,
                now_utc,
            })
            .map_err(storage)
    }

    pub fn msg_withdraw(
        &self,
        caller: &str,
        params: MsgWithdrawParams,
    ) -> Result<Result<WithdrawCommitted, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let org_seq = self.org_snapshot()?.position.seq;
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        self.store
            .run(MsgWithdrawOp {
                here: self.identity.clone(),
                owner: self.owner.clone(),
                caller: caller_id,
                params,
                org_seq,
                now_utc,
            })
            .map_err(storage)
    }

    pub fn inspect_ask_status(
        &self,
        caller: &str,
        ask_id: &str,
    ) -> Result<Result<ChatWaitAskResult, ApiError>, OfficeError> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_ms = self.monotonic_now_ms();
        let (res, _) = self
            .store
            .read(InspectAskStatusRead {
                owner: self.owner.clone(),
                caller: caller_id,
                ask_id: ask_id.to_owned(),
                now_ms,
            })
            .map_err(storage)?;
        Ok(res)
    }

    pub fn open_owner_inbox_entries(
        &self,
        limit: usize,
    ) -> Result<Vec<OwnerInboxEntry>, OfficeError> {
        let (rows, _) = self
            .store
            .read(OpenOwnerInboxRead {
                owner: self.owner.clone(),
                limit,
            })
            .map_err(storage)?;
        Ok(rows)
    }

    /// Atomically delivers validated `answer_candidate` inbox entries (`WATCH-1`, `WATCH-3`, `WATCH-7`).
    pub fn deliver_answer_candidates(
        &self,
        conversation_id: &str,
        deliveries: Vec<crate::messaging::features::AnswerCandidateDelivery>,
    ) -> Result<Result<Vec<(String, Id)>, ApiError>, OfficeError> {
        let Ok(conv_id) = Id::parse(conversation_id) else {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "invalid conversation id",
            )));
        };
        let org_seq = self.org_snapshot()?.position.seq;
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        self.store
            .run(DeliverAnswerCandidatesOp {
                here: self.identity.clone(),
                owner: self.owner.clone(),
                conversation_id: conv_id,
                deliveries,
                org_seq,
                now_utc,
            })
            .map_err(storage)
    }

    /// Returns caller-visible messages in a channel (same visibility as `msg list`, capped at 200)
    /// for `channel.summarize` (`WATCH-10`).
    pub fn channel_messages_for_caller(
        &self,
        caller: &str,
        channel_ref: &str,
        limit: usize,
    ) -> Result<
        Result<
            (
                crate::messaging::records::ConversationRecord,
                Vec<crate::messaging::records::MessageRecord>,
            ),
            ApiError,
        >,
        OfficeError,
    > {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        let (res, _) = self
            .store
            .read(ChannelMessagesForCallerRead {
                owner: self.owner.clone(),
                caller: caller_id,
                channel_ref: channel_ref.to_owned(),
                limit,
                now_utc,
            })
            .map_err(storage)?;
        Ok(res)
    }

    /// Reads all messages in a conversation for the office-local channel watcher (`WATCH-3/7`).
    pub fn messages_in_channel(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<crate::messaging::records::MessageRecord>, OfficeError> {
        let Ok(conv_id) = Id::parse(conversation_id) else {
            return Ok(Vec::new());
        };
        let (msgs, _) = self
            .store
            .read(ChannelMessagesRawRead {
                conversation_id: conv_id,
            })
            .map_err(storage)?;
        Ok(msgs)
    }

    /// Reads one conversation (persisted or virtual auto-channel) by UUIDv7 ID.
    pub fn conversation_by_id(
        &self,
        conversation_id: &str,
    ) -> Result<Option<crate::messaging::records::ConversationRecord>, OfficeError> {
        let Ok(conv_id) = Id::parse(conversation_id) else {
            return Ok(None);
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        let (conv, _) = self
            .store
            .read(ConversationByIdRead {
                conversation_id: conv_id,
                now_utc,
            })
            .map_err(storage)?;
        Ok(conv)
    }

    // ---- Low-level org-mailbox storage bridge methods ----

    pub fn mailbox_commit(
        &self,
        input: MailboxCommitInput,
    ) -> Result<Result<MailboxCommitOut, ApiError>, OfficeError> {
        self.store
            .run(CommitMailboxMessageOp {
                here: self.identity.clone(),
                input,
            })
            .map_err(storage)
    }

    pub fn mailbox_claim_read(
        &self,
        to_member_id: &str,
        id: Option<&str>,
        now_utc: &str,
    ) -> Result<Result<Option<MailboxStored>, ApiError>, OfficeError> {
        let Ok(emp) = Id::parse(to_member_id) else {
            return Ok(Ok(None));
        };
        self.store
            .run(ClaimMailboxReadOp {
                here: self.identity.clone(),
                to_member_id: emp,
                id: id.map(str::to_owned),
                now_utc: now_utc.to_owned(),
            })
            .map_err(storage)
    }

    pub fn mailbox_peek_unread(
        &self,
        to_member_id: &str,
        from_member_id: Option<&str>,
        urgent_only: bool,
        now_utc: &str,
    ) -> Result<PeekUnreadOut, OfficeError> {
        let Ok(emp) = Id::parse(to_member_id) else {
            return Ok(PeekUnreadOut {
                message: None,
                expired_ids: Vec::new(),
            });
        };
        let (out, _) = self
            .store
            .read(PeekUnreadRead {
                to_member_id: emp,
                from_member_id: from_member_id.map(str::to_owned),
                urgent_only,
                now_utc: now_utc.to_owned(),
            })
            .map_err(storage)?;
        Ok(out)
    }

    pub fn mailbox_get(&self, id: &str) -> Result<Option<MailboxStored>, OfficeError> {
        let (out, _) = self
            .store
            .read(GetMailboxMessageRead(id.to_owned()))
            .map_err(storage)?;
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn mailbox_list_headers(
        &self,
        scope_inbox: Option<&str>,
        scope_participant: Option<&str>,
        from_member_id: Option<&str>,
        thread_id: Option<&str>,
        unread_only: bool,
        include_snoozed: bool,
        cursor: Option<u64>,
        limit: u32,
    ) -> Result<HeaderPage, OfficeError> {
        let (page, _) = self
            .store
            .read(ListMailboxHeadersRead {
                scope_inbox: scope_inbox.and_then(|s| Id::parse(s).ok()),
                scope_participant: scope_participant.and_then(|s| Id::parse(s).ok()),
                from_member_id: from_member_id.map(str::to_owned),
                thread_id: thread_id.map(str::to_owned),
                unread_only,
                include_snoozed,
                cursor,
                limit,
            })
            .map_err(storage)?;
        Ok(page)
    }

    pub fn mailbox_find_reply_for_ask(
        &self,
        ask_id: &str,
        responder_id: &str,
    ) -> Result<Option<MailboxStored>, OfficeError> {
        let (found, _) = self
            .store
            .read(FindReplyForAskRead {
                ask_id: ask_id.to_owned(),
                responder_id: responder_id.to_owned(),
            })
            .map_err(storage)?;
        Ok(found)
    }

    pub fn mailbox_update_state_local(
        &self,
        input: LocalTransitionInput,
    ) -> Result<Result<MessageHeader, ApiError>, OfficeError> {
        self.store
            .run(MailboxUpdateStateOp {
                here: self.identity.clone(),
                input,
            })
            .map_err(storage)
    }

    pub fn mailbox_defer_local(
        &self,
        input: LocalTransitionInput,
    ) -> Result<Result<MessageHeader, ApiError>, OfficeError> {
        self.store
            .run(MailboxDeferOp {
                here: self.identity.clone(),
                input,
            })
            .map_err(storage)
    }

    pub fn mailbox_unsnooze_local(
        &self,
        input: LocalTransitionInput,
    ) -> Result<Result<MessageHeader, ApiError>, OfficeError> {
        self.store
            .run(MailboxUnsnoozeOp {
                here: self.identity.clone(),
                input,
            })
            .map_err(storage)
    }

    pub fn mailbox_check_notify(
        &self,
        to_member_id: &str,
        after_task: bool,
        urgent_only: bool,
        sweep_before_utc: &str,
        now_utc: &str,
    ) -> Result<Result<Vec<NotifyCandidateOut>, ApiError>, OfficeError> {
        let Ok(emp) = Id::parse(to_member_id) else {
            return Ok(Ok(Vec::new()));
        };
        self.store
            .run(MailboxCheckNotifyOp {
                here: self.identity.clone(),
                to_member_id: emp,
                after_task,
                urgent_only,
                sweep_before_utc: sweep_before_utc.to_owned(),
                now_utc: now_utc.to_owned(),
            })
            .map_err(storage)
    }

    pub fn mailbox_active_snooze_deadlines(
        &self,
    ) -> Result<Vec<(String, String, String)>, OfficeError> {
        let (rows, _) = self
            .store
            .read(ListActiveSnoozeDeadlinesRead)
            .map_err(storage)?;
        Ok(rows)
    }
}
