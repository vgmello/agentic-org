//! `StoreOp` and `ReadOp` wrappers for the messaging family, `org-mailbox` storage primitives,
//! and `OrgFeed` committed organization change replay (`doc-90`, `B6`).

use std::collections::BTreeMap;

use org_proto::ask::{ChatAnswerParams, ChatAskParams, ChatWaitAskResult, MsgWithdrawParams};
use org_proto::chat::{
    ChatChannelCreateParams, ChatChannelCreateResult, ChatChannelMembershipParams,
    ChatChannelMembershipResult, ChatChannelsParams, ChatChannelsResult, ChatConversationsParams,
    ChatConversationsResult, ChatInboxParams, ChatInboxResult, ChatMarkReadParams,
    ChatMarkReadResult, ChatMessagesParams, ChatMessagesResult, ChatMuteParams, ChatMuteResult,
    ChatReactParams, ChatReactResult, ChatSendParams, ConversationKind, DeliveredBecause,
    DmOpenParams, DmOpenResult, MsgAcceptParams, MsgAcceptResult,
};
use org_proto::msg::{
    HeaderPage, Message, MessageHeader, MessageLifecycleState, Meta, NotifyReason,
    KIND_DELIVERY_FAILED, OFFICE_SENDER,
};
use org_proto::org::{OrgChange, OrgChangedData, Position};
use org_proto::{Actor as WireActor, ActorKind, ApiError, ErrorCode};
use org_store::{Ack, Change, Id, OfficeIdentity, Outcome, Refusal, Reply, Store, StoreError};

use super::{OfficeEngine, ReadOp, StoreOp};
use crate::messaging::{
    asks::{self, AnswerCommitted, AskCard, AskCommitted, WithdrawCommitted},
    features::{self, decode_refusal, to_wire_header, ChatSendCommitted},
    ports::MessagingRead,
    records::{
        self, ConversationMembership, ConversationRecord, MessageRecord,
        ParticipantReceivedSnapshot, RecipientStateRecord, SenderKind, SCHEMA_VERSION,
    },
};
use crate::tasks::ports::TasksRead;

fn storage_refusal(msg: &str) -> Refusal {
    Refusal::rejected(
        serde_json::to_vec(&ApiError::new(ErrorCode::StorageError, msg)).expect("serializes"),
    )
}

fn unwrap_reply<T>(
    outcome: Result<Reply, StoreError>,
    slot: std::sync::Arc<std::sync::Mutex<Option<T>>>,
) -> Result<Result<T, ApiError>, StoreError> {
    let reply = outcome?;
    match reply.outcome {
        Outcome::Completed => {
            let val = slot
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take()
                .ok_or_else(|| StoreError::invalid("missing committed result"))?;
            Ok(Ok(val))
        }
        Outcome::Rejected => Ok(Err(decode_refusal(&Refusal::Rejected(reply.bytes)))),
    }
}

pub struct DmOpenOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: DmOpenParams,
    pub now_utc: String,
}

impl StoreOp for DmOpenOp {
    type Out = Result<Result<DmOpenResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_dm_open(
                uow,
                &self.here,
                &self.owner,
                &self.caller,
                &self.params,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChannelCreateOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatChannelCreateParams,
    pub now_utc: String,
}

impl StoreOp for ChannelCreateOp {
    type Out = Result<Result<ChatChannelCreateResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_channel_create(
                uow,
                &self.here,
                &self.owner,
                &self.caller,
                &self.params,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChannelMembershipOp {
    pub owner: Id,
    pub caller: Id,
    pub params: ChatChannelMembershipParams,
    pub join: bool,
    pub now_utc: String,
}

impl StoreOp for ChannelMembershipOp {
    type Out = Result<Result<ChatChannelMembershipResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_channel_membership(
                uow,
                &self.owner,
                &self.caller,
                &self.params,
                self.join,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChatSendOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatSendParams,
    pub org_seq: u64,
    pub now_utc: String,
}

impl StoreOp for ChatSendOp {
    type Out = Result<Result<ChatSendCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_chat_send(
                uow,
                &self.here,
                &self.owner,
                &self.caller,
                &self.params,
                self.org_seq,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChatMarkReadOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatMarkReadParams,
    pub now_utc: String,
}

impl StoreOp for ChatMarkReadOp {
    type Out = Result<Result<ChatMarkReadResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_mark_read(
                uow,
                &self.here,
                &self.owner,
                &self.caller,
                &self.params,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChatMuteOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatMuteParams,
    pub now_utc: String,
}

impl StoreOp for ChatMuteOp {
    type Out = Result<Result<ChatMuteResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_mute(
                uow,
                &self.here,
                &self.owner,
                &self.caller,
                &self.params,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChatReactOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatReactParams,
    pub now_utc: String,
}

impl StoreOp for ChatReactOp {
    type Out = Result<Result<ChatReactResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_react(
                uow,
                &self.here,
                &self.owner,
                &self.caller,
                &self.params,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct MsgAcceptOp {
    pub here: OfficeIdentity,
    pub caller: Id,
    pub params: MsgAcceptParams,
    pub org_seq: u64,
    pub now_utc: String,
}

impl StoreOp for MsgAcceptOp {
    type Out = Result<Result<MsgAcceptResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_accept_handoff(
                uow,
                &self.here,
                &self.caller,
                &self.params,
                self.org_seq,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct MsgAskOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatAskParams,
    pub org_seq: u64,
    pub now_ms: u64,
}

impl StoreOp for MsgAskOp {
    type Out = Result<Result<AskCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = asks::decide_msg_ask(
                uow,
                &self.here,
                &self.owner,
                &self.caller,
                &self.params,
                self.org_seq,
                self.now_ms,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChatAnswerOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatAnswerParams,
    pub reply_id: String,
    pub org_seq: u64,
    pub now_utc: String,
}

impl StoreOp for ChatAnswerOp {
    type Out = Result<Result<AnswerCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = asks::decide_chat_answer(
                uow,
                &self.here,
                &self.owner,
                &self.caller,
                &self.params,
                &self.reply_id,
                self.org_seq,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct MsgWithdrawOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: MsgWithdrawParams,
    pub org_seq: u64,
    pub now_utc: String,
    pub removed_offices: std::collections::BTreeSet<String>,
}

impl StoreOp for MsgWithdrawOp {
    type Out = Result<Result<WithdrawCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = asks::decide_msg_withdraw(
                uow,
                &self.here,
                &self.owner,
                &self.caller,
                &self.params,
                self.org_seq,
                &self.now_utc,
                &self.removed_offices,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct InspectAskStatusRead {
    pub owner: Id,
    pub caller: Id,
    pub ask_id: String,
    pub now_ms: u64,
}

impl ReadOp for InspectAskStatusRead {
    type Out = Result<ChatWaitAskResult, ApiError>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(asks::inspect_ask_status(
            snapshot,
            &self.owner,
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
}

pub struct OpenOwnerInboxRead {
    pub owner: Id,
    pub limit: usize,
}

impl ReadOp for OpenOwnerInboxRead {
    type Out = Vec<OwnerInboxEntry>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let mut entries = Vec::new();
        for st in snapshot.recipient_states_for_employee(&self.owner)? {
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
            entries.push(OwnerInboxEntry {
                header,
                body: msg.body_markdown,
                card,
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
        Ok(features::list_channels(
            snapshot,
            &self.viewer,
            &self.owner,
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
        Ok(features::list_conversations(
            snapshot,
            &self.viewer,
            &self.owner,
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
        Ok(features::list_chat_inbox(
            snapshot,
            &self.owner,
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
        Ok(features::list_chat_messages(
            snapshot,
            &self.owner,
            &self.caller,
            &self.params,
            &self.now_utc,
        ))
    }
}

// ---- Mailbox-compatible StoreOps & ReadOps (for org_mailbox::MessageStore + TriageStore) ----

/// Engine-neutral payload mirroring `org_mailbox::NewStoredMessage` without adding a dependency
/// from `org-office` onto `org-mailbox`.
#[derive(Debug, Clone)]
pub struct MailboxCommitInput {
    pub message: Message,
    pub origin_office_id: String,
    pub origin_incarnation: String,
    pub origin_seq: Option<u64>,
    pub org_position: Position,
    pub content_hash: String,
    pub initial_state: MessageLifecycleState,
}

#[derive(Debug, Clone)]
pub struct MailboxStored {
    pub message: Message,
    pub origin_office_id: String,
    pub origin_incarnation: String,
    pub origin_seq: u64,
    pub org_position: Position,
    pub content_hash: String,
}

#[derive(Debug, Clone)]
pub struct MailboxCommitOut {
    pub inserted: bool,
    pub stored: MailboxStored,
}

fn stored_from_records(
    msg: &MessageRecord,
    state: Option<&RecipientStateRecord>,
    local_seq: u64,
) -> MailboxStored {
    let header = to_wire_header(msg, state, local_seq);
    MailboxStored {
        message: Message {
            header,
            body: msg.body_markdown.clone(),
            meta: msg.metadata.clone(),
        },
        origin_office_id: msg.origin_office_id.as_str().to_owned(),
        origin_incarnation: msg.origin_incarnation.clone(),
        origin_seq: msg.origin_sequence,
        org_position: Position {
            epoch: msg.org_epoch,
            seq: msg.org_sequence,
        },
        content_hash: msg
            .mailbox_content_hash
            .clone()
            .unwrap_or_else(|| msg.content_sha256.clone()),
    }
}

fn resolve_local_seq(
    read: &impl MessagingRead,
    msg: &MessageRecord,
) -> Result<u64, org_store::EngineError> {
    Ok(read
        .message_local_seq(&msg.id)?
        .unwrap_or(msg.origin_sequence))
}

fn upsert_states_batch(
    read: &impl MessagingRead,
    org_id: &Id,
    conv_id: &Id,
    emp_id: &Id,
    updates: Vec<RecipientStateRecord>,
    change: Change,
) -> Result<Change, Refusal> {
    let existing = read
        .recipient_states_for_employee(emp_id)
        .map_err(|_| storage_refusal("failed to read recipient states"))?;
    let mut map: BTreeMap<String, RecipientStateRecord> = existing
        .into_iter()
        .filter(|s| s.conversation_id == *conv_id)
        .map(|s| (s.message_id.clone(), s))
        .collect();
    for state in updates {
        map.insert(state.message_id.clone(), state);
    }
    let snap = ParticipantReceivedSnapshot {
        schema_version: SCHEMA_VERSION,
        revision: map.len() as u64,
        organization_id: org_id.clone(),
        conversation_id: conv_id.clone(),
        employee_id: emp_id.clone(),
        states: map,
    };
    let path = records::participant_received_snapshot_path(conv_id, emp_id);
    let existed = read
        .source_hash(&path)
        .map_err(|_| storage_refusal("failed to check snapshot path"))?
        .is_some();
    let bytes = records::encode_json(&snap);
    Ok(if existed {
        change.put(path, bytes)
    } else {
        change.create(path, bytes)
    })
}

fn upsert_state_change(
    read: &impl MessagingRead,
    org_id: &Id,
    conv_id: &Id,
    emp_id: &Id,
    state: RecipientStateRecord,
    change: Change,
) -> Result<Change, Refusal> {
    upsert_states_batch(read, org_id, conv_id, emp_id, vec![state], change)
}

fn check_mailbox_replay(
    read: &impl MessagingRead,
    input: &MailboxCommitInput,
) -> Result<Option<MailboxCommitOut>, Refusal> {
    let hdr = &input.message.header;
    let Some(existing) = read
        .message(&hdr.id)
        .map_err(|_| storage_refusal("read message"))?
    else {
        return Ok(None);
    };
    let existing_from = existing
        .sender_employee_id
        .as_ref()
        .map(|i| i.as_str())
        .unwrap_or(OFFICE_SENDER);
    let existing_hash = existing
        .mailbox_content_hash
        .as_deref()
        .unwrap_or(&existing.content_sha256);
    if existing_from != hdr.from_member_id || existing_hash != input.content_hash {
        return Err(Refusal::rejected(
            serde_json::to_vec(&ApiError::new(
                ErrorCode::AlreadyExists,
                "message id already exists with different content",
            ))
            .unwrap(),
        ));
    }
    let to_id = Id::parse(&hdr.to_member_id).ok();
    let st = to_id
        .as_ref()
        .and_then(|eid| read.recipient_state(&existing.id, eid).ok().flatten());
    let seq = resolve_local_seq(read, &existing).map_err(|_| storage_refusal("local_seq"))?;
    Ok(Some(MailboxCommitOut {
        inserted: false,
        stored: stored_from_records(&existing, st.as_ref(), seq),
    }))
}

fn ensure_mailbox_conversation(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    conv_id: &Id,
    to_id: &Id,
    sender_emp: Option<&Id>,
    created_at: &str,
    mut change: Change,
) -> Result<Change, Refusal> {
    let existing_conv = read
        .conversation(conv_id)
        .map_err(|_| storage_refusal("read conv"))?;
    if existing_conv.is_some() {
        return Ok(change);
    }
    let mut memberships = vec![ConversationMembership {
        employee_id: to_id.clone(),
        joined_at: created_at.to_owned(),
        left_at: None,
    }];
    if let Some(from_id) = sender_emp {
        if from_id != to_id {
            memberships.push(ConversationMembership {
                employee_id: from_id.clone(),
                joined_at: created_at.to_owned(),
                left_at: None,
            });
        }
    }
    let conv = ConversationRecord {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: here.organization_id.clone(),
        id: conv_id.clone(),
        kind: ConversationKind::Dm,
        name: None,
        auto_scope: None,
        team_id: None,
        project_id: None,
        archived: false,
        created_at: created_at.to_owned(),
        memberships,
        pins: Vec::new(),
    };
    change = change.create(
        records::conversation_path(conv_id),
        records::encode_json(&conv),
    );
    Ok(change)
}

fn ensure_single_delivery_failed(
    read: &impl MessagingRead,
    hdr: &MessageHeader,
) -> Result<(), Refusal> {
    if hdr.kind != KIND_DELIVERY_FAILED {
        return Ok(());
    }
    let Some(reply_to) = &hdr.reply_to else {
        return Ok(());
    };
    let all_msgs = read
        .all_messages()
        .map_err(|_| storage_refusal("list messages"))?;
    if all_msgs.iter().any(|m| {
        m.kind == KIND_DELIVERY_FAILED
            && m.reply_to_message_id.as_deref() == Some(reply_to.as_str())
    }) {
        return Err(Refusal::rejected(
            serde_json::to_vec(&ApiError::new(
                ErrorCode::AlreadyExists,
                "delivery_failed already exists for ask",
            ))
            .unwrap(),
        ));
    }
    Ok(())
}

fn commit_new_mailbox_message(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    input: &MailboxCommitInput,
) -> Result<(Change, MailboxCommitOut), Refusal> {
    let hdr = &input.message.header;
    ensure_single_delivery_failed(read, hdr)?;
    let to_id =
        Id::parse(&hdr.to_member_id).map_err(|_| storage_refusal("to_member_id must be UUIDv7"))?;
    let (sender_kind, sender_emp, sender_off) = if hdr.from_member_id == OFFICE_SENDER {
        (SenderKind::Office, None, Some(here.office_id.clone()))
    } else {
        let from_id = Id::parse(&hdr.from_member_id)
            .map_err(|_| storage_refusal("from_member_id must be UUIDv7"))?;
        (SenderKind::Employee, Some(from_id), None)
    };
    let conv_id = match &sender_emp {
        Some(from_id) => records::dm_conversation_id(&here.organization_id, from_id, &to_id),
        None => records::derive_conversation_id(
            "office_notice",
            &format!("{}:{}", here.organization_id, to_id),
        ),
    };
    let mut change = ensure_mailbox_conversation(
        read,
        here,
        &conv_id,
        &to_id,
        sender_emp.as_ref(),
        &hdr.created_at,
        Change::new(Ack::Committed),
    )?;
    let origin_office =
        Id::parse(&input.origin_office_id).unwrap_or_else(|_| here.office_id.clone());
    let origin_seq = match input.origin_seq {
        Some(s) if s > 0 => s,
        _ => read
            .max_origin_sequence(&origin_office, &input.origin_incarnation)
            .map_err(|_| storage_refusal("origin_seq"))?
            .saturating_add(1),
    };
    let local_seq = read
        .max_local_seq()
        .map_err(|_| storage_refusal("local_seq"))?
        .saturating_add(1);
    let mut msg_rec = build_mailbox_message_record(
        here,
        input,
        &conv_id,
        sender_kind,
        sender_emp,
        sender_off,
        origin_office.clone(),
        origin_seq,
    );
    msg_rec.content_sha256 = msg_rec.compute_content_sha256();
    change = change.create(
        records::message_path(&conv_id, &msg_rec.id),
        records::encode_json(&msg_rec),
    );
    let state_seq = read
        .max_state_sequence(&origin_office, &input.origin_incarnation)
        .map_err(|_| storage_refusal("state_seq"))?
        .saturating_add(1);
    let st = build_mailbox_state_record(here, input, &conv_id, &to_id, origin_office, state_seq);
    change = upsert_state_change(
        read,
        &here.organization_id,
        &conv_id,
        &to_id,
        st.clone(),
        change,
    )?;
    let out = MailboxCommitOut {
        inserted: true,
        stored: stored_from_records(&msg_rec, Some(&st), local_seq),
    };
    Ok((change, out))
}

#[expect(
    clippy::too_many_arguments,
    reason = "mailbox message record assembly takes explicit identity, input, conversation, sender, and metadata fields"
)]
fn build_mailbox_message_record(
    here: &OfficeIdentity,
    input: &MailboxCommitInput,
    conv_id: &Id,
    sender_kind: SenderKind,
    sender_emp: Option<Id>,
    sender_off: Option<Id>,
    origin_office: Id,
    origin_seq: u64,
) -> MessageRecord {
    let hdr = &input.message.header;
    let thread_root = if hdr.thread_id == hdr.id {
        None
    } else {
        Some(hdr.thread_id.clone())
    };
    MessageRecord {
        schema_version: SCHEMA_VERSION,
        organization_id: here.organization_id.clone(),
        id: hdr.id.clone(),
        conversation_id: conv_id.clone(),
        sender_kind,
        sender_employee_id: sender_emp,
        sender_office_id: sender_off,
        sender_display_name: hdr.from_display_name.clone(),
        kind: hdr.kind.clone(),
        priority: hdr.priority,
        subject: hdr.subject.clone(),
        body_markdown: input.message.body.clone(),
        metadata: input.message.meta.clone(),
        thread_root_id: thread_root,
        reply_to_message_id: hdr.reply_to.clone(),
        also_in_channel: false,
        created_at: hdr.created_at.clone(),
        expires_at: hdr.expires_at.clone(),
        ttl_ms: hdr.ttl_ms,
        origin_office_id: origin_office,
        origin_incarnation: input.origin_incarnation.clone(),
        origin_sequence: origin_seq,
        org_epoch: input.org_position.epoch.max(1),
        org_sequence: input.org_position.seq,
        content_sha256: String::new(),
        attachments: Vec::new(),
        mailbox_content_hash: Some(input.content_hash.clone()),
    }
}

fn build_mailbox_state_record(
    here: &OfficeIdentity,
    input: &MailboxCommitInput,
    conv_id: &Id,
    to_id: &Id,
    origin_office: Id,
    state_seq: u64,
) -> RecipientStateRecord {
    let hdr = &input.message.header;
    RecipientStateRecord {
        organization_id: here.organization_id.clone(),
        message_id: hdr.id.clone(),
        conversation_id: conv_id.clone(),
        employee_id: to_id.clone(),
        recipient_display_name: Some(hdr.to_display_name.clone()),
        state: input.initial_state,
        triage_generation: hdr.triage_gen,
        snoozed_until: hdr.snoozed_until.clone(),
        snooze_after_task: hdr.snooze_after_task,
        snooze_note: hdr.snooze_note.clone(),
        triaged_at: None,
        read_at: hdr.read_at.clone(),
        updated_at: hdr.created_at.clone(),
        reason: None,
        delivered_because: DeliveredBecause::Addressed,
        related_message_ids: Vec::new(),
        expired_waiting_person: hdr.expired_waiting_person,
        owner_office_id: origin_office,
        origin_incarnation: input.origin_incarnation.clone(),
        origin_sequence: state_seq,
        org_epoch: input.org_position.epoch.max(1),
        org_sequence: input.org_position.seq,
    }
}

pub struct CommitMailboxMessageOp {
    pub here: OfficeIdentity,
    pub input: MailboxCommitInput,
}

impl StoreOp for CommitMailboxMessageOp {
    type Out = Result<Result<MailboxCommitOut, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let here = self.here;
        let input = self.input;
        let reply = store.submit(None, false, move |uow| {
            if let Some(replayed) = check_mailbox_replay(uow.view(), &input)? {
                *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(replayed);
                return Ok(Change::new(Ack::Committed));
            }
            let (change, committed) = commit_new_mailbox_message(uow.view(), &here, &input)?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(committed);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ClaimMailboxReadOp {
    pub here: OfficeIdentity,
    pub to_member_id: Id,
    pub id: Option<String>,
    pub now_utc: String,
}

impl StoreOp for ClaimMailboxReadOp {
    type Out = Result<Result<Option<MailboxStored>, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let target_state = if let Some(mid) = &self.id {
                uow.view()
                    .recipient_state(mid, &self.to_member_id)
                    .map_err(|_| storage_refusal("read state"))?
            } else {
                let mut states = uow
                    .view()
                    .recipient_states_for_employee(&self.to_member_id)
                    .map_err(|_| storage_refusal("list states"))?;
                states.retain(|s| s.state == MessageLifecycleState::Unread);
                states.sort_by(|a, b| {
                    a.origin_sequence
                        .cmp(&b.origin_sequence)
                        .then_with(|| a.message_id.cmp(&b.message_id))
                });
                states.into_iter().next()
            };
            let Some(mut st) = target_state else {
                *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(None);
                return Ok(Change::new(Ack::Committed));
            };
            let Some(msg) = uow
                .view()
                .message(&st.message_id)
                .map_err(|_| storage_refusal("read message"))?
            else {
                *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(None);
                return Ok(Change::new(Ack::Committed));
            };
            let mut change = Change::new(Ack::Committed);
            if matches!(
                st.state,
                MessageLifecycleState::Unread | MessageLifecycleState::Snoozed
            ) {
                st.state = MessageLifecycleState::Read;
                st.read_at = Some(self.now_utc.clone());
                st.snoozed_until = None;
                st.snooze_after_task = false;
                st.snooze_note = None;
                st.updated_at = self.now_utc.clone();
                change = upsert_state_change(
                    uow.view(),
                    &self.here.organization_id,
                    &st.conversation_id,
                    &self.to_member_id,
                    st.clone(),
                    change,
                )?;
            }
            let seq =
                resolve_local_seq(uow.view(), &msg).map_err(|_| storage_refusal("local_seq"))?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) =
                Some(Some(stored_from_records(&msg, Some(&st), seq)));
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

#[derive(Debug, Clone)]
pub struct LocalTransitionInput {
    pub id: String,
    pub to_member_id: Id,
    pub state: MessageLifecycleState,
    pub snoozed_until: Option<String>,
    pub snooze_after_task: bool,
    pub reason: Option<String>,
    pub now_utc: String,
    pub owner_office_id: Id,
    pub origin_incarnation: String,
    pub org_position: Position,
    pub expired_waiting_person: bool,
}

pub struct MailboxUpdateStateOp {
    pub here: OfficeIdentity,
    pub input: LocalTransitionInput,
}

fn apply_local_transition(
    st: &mut RecipientStateRecord,
    inp: &LocalTransitionInput,
) -> Result<bool, Refusal> {
    let invalid = || {
        Refusal::rejected(
            serde_json::to_vec(&ApiError::new(
                ErrorCode::Conflict,
                "invalid state transition",
            ))
            .unwrap(),
        )
    };
    let mut mutated = true;
    match (st.state, inp.state) {
        (s, t)
            if s == t
                && matches!(
                    s,
                    MessageLifecycleState::Read
                        | MessageLifecycleState::Done
                        | MessageLifecycleState::Expired
                ) =>
        {
            mutated = false;
        }
        (
            MessageLifecycleState::Unread
            | MessageLifecycleState::Read
            | MessageLifecycleState::Snoozed,
            MessageLifecycleState::Snoozed,
        ) => {
            if inp.snoozed_until.is_some() == inp.snooze_after_task {
                return Err(invalid());
            }
            st.state = MessageLifecycleState::Snoozed;
            st.triage_generation = st.triage_generation.saturating_add(1);
            st.snoozed_until = inp.snoozed_until.clone();
            st.snooze_after_task = inp.snooze_after_task;
            st.snooze_note = inp.reason.clone();
            st.triaged_at = Some(inp.now_utc.clone());
            st.read_at = None;
        }
        (
            MessageLifecycleState::Unread
            | MessageLifecycleState::Read
            | MessageLifecycleState::Snoozed,
            MessageLifecycleState::Done,
        ) => {
            st.state = MessageLifecycleState::Done;
            st.snoozed_until = None;
            st.snooze_after_task = false;
            st.snooze_note = None;
            st.reason = inp.reason.clone();
        }
        (
            MessageLifecycleState::Unread | MessageLifecycleState::Snoozed,
            MessageLifecycleState::Read,
        ) => {
            st.state = MessageLifecycleState::Read;
            st.snoozed_until = None;
            st.snooze_after_task = false;
            st.snooze_note = None;
            st.read_at = Some(inp.now_utc.clone());
        }
        (MessageLifecycleState::Read, MessageLifecycleState::Unread) => {
            if inp.expired_waiting_person {
                return Err(invalid());
            }
            st.state = MessageLifecycleState::Unread;
            st.read_at = None;
        }
        (MessageLifecycleState::Unread, MessageLifecycleState::Unread) => {
            if inp.expired_waiting_person {
                st.expired_waiting_person = true;
            } else {
                mutated = false;
            }
        }
        (
            MessageLifecycleState::Unread | MessageLifecycleState::Snoozed,
            MessageLifecycleState::Expired,
        ) => {
            st.state = MessageLifecycleState::Expired;
            st.snoozed_until = None;
            st.snooze_after_task = false;
            st.snooze_note = None;
        }
        _ => return Err(invalid()),
    }
    Ok(mutated)
}

fn check_snooze_expiry_guard(
    msg: &MessageRecord,
    inp: &LocalTransitionInput,
) -> Result<(), Refusal> {
    if inp.state != MessageLifecycleState::Snoozed {
        return Ok(());
    }
    let Some(exp) = msg.expires_at.as_deref() else {
        return Ok(());
    };
    let (already_expired, snooze_past_expiry) = match (
        crate::time::parse_rfc3339_ms(exp),
        crate::time::parse_rfc3339_ms(&inp.now_utc),
    ) {
        (Some(e), Some(n)) => {
            let past = inp
                .snoozed_until
                .as_deref()
                .is_some_and(|at| crate::time::parse_rfc3339_ms(at).is_none_or(|a| a >= e));
            (e <= n, past)
        }
        // Fail closed on corrupted expires_at / now_utc.
        _ => (true, true),
    };
    if already_expired || snooze_past_expiry {
        return Err(Refusal::rejected(
            serde_json::to_vec(&ApiError::new(
                ErrorCode::InvalidRequest,
                "snoozed_until must be earlier than the ask expires_at deadline",
            ))
            .unwrap(),
        ));
    }
    Ok(())
}

fn stamp_local_sync(st: &mut RecipientStateRecord, inp: &LocalTransitionInput) {
    st.updated_at = inp.now_utc.clone();
    st.owner_office_id = inp.owner_office_id.clone();
    st.origin_incarnation = inp.origin_incarnation.clone();
    st.org_epoch = inp.org_position.epoch.max(1);
    st.org_sequence = inp.org_position.seq;
}

fn not_found_refusal() -> Refusal {
    Refusal::rejected(
        serde_json::to_vec(&ApiError::new(ErrorCode::NotFound, "message not found")).unwrap(),
    )
}

fn run_local_state_op<E: OfficeEngine>(
    store: &Store<E>,
    here: OfficeIdentity,
    input: LocalTransitionInput,
    enforce_expiry_guard: bool,
) -> Result<Result<MessageHeader, ApiError>, StoreError> {
    let out = std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot = std::sync::Arc::clone(&out);
    let reply = store.submit(None, false, move |uow| {
        let inp = &input;
        let mut st = uow
            .view()
            .recipient_state(&inp.id, &inp.to_member_id)
            .map_err(|_| storage_refusal("read state"))?
            .ok_or_else(not_found_refusal)?;
        let msg = uow
            .view()
            .message(&inp.id)
            .map_err(|_| storage_refusal("read message"))?
            .ok_or_else(not_found_refusal)?;
        if enforce_expiry_guard {
            check_snooze_expiry_guard(&msg, inp)?;
        }
        let mutated = apply_local_transition(&mut st, inp)?;
        let mut change = Change::new(Ack::Committed);
        if mutated {
            stamp_local_sync(&mut st, inp);
            change = upsert_state_change(
                uow.view(),
                &here.organization_id,
                &st.conversation_id,
                &inp.to_member_id,
                st.clone(),
                change,
            )?;
        }
        let seq = resolve_local_seq(uow.view(), &msg).map_err(|_| storage_refusal("local_seq"))?;
        *slot.lock().unwrap_or_else(|p| p.into_inner()) =
            Some(to_wire_header(&msg, Some(&st), seq));
        Ok(change)
    });
    unwrap_reply(reply, out)
}

impl StoreOp for MailboxUpdateStateOp {
    type Out = Result<Result<MessageHeader, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        run_local_state_op(store, self.here, self.input, false)
    }
}

pub struct MailboxDeferOp {
    pub here: OfficeIdentity,
    pub input: LocalTransitionInput,
}

impl StoreOp for MailboxDeferOp {
    type Out = Result<Result<MessageHeader, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        run_local_state_op(store, self.here, self.input, true)
    }
}

pub struct MailboxUnsnoozeOp {
    pub here: OfficeIdentity,
    pub input: LocalTransitionInput,
}

fn apply_unsnooze_record(
    st: &mut RecipientStateRecord,
    inp: &LocalTransitionInput,
) -> Result<(), Refusal> {
    if st.state != MessageLifecycleState::Snoozed || inp.expired_waiting_person {
        return Err(Refusal::rejected(
            serde_json::to_vec(&ApiError::new(
                ErrorCode::Conflict,
                "message is not snoozed",
            ))
            .unwrap(),
        ));
    }
    st.state = MessageLifecycleState::Unread;
    st.triage_generation = st.triage_generation.saturating_add(1);
    st.snoozed_until = None;
    st.snooze_after_task = false;
    st.snooze_note = None;
    st.triaged_at = Some(inp.now_utc.clone());
    st.read_at = None;
    stamp_local_sync(st, inp);
    Ok(())
}

impl StoreOp for MailboxUnsnoozeOp {
    type Out = Result<Result<MessageHeader, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let inp = &self.input;
            let mut st = uow
                .view()
                .recipient_state(&inp.id, &inp.to_member_id)
                .map_err(|_| storage_refusal("read state"))?
                .ok_or_else(not_found_refusal)?;
            let msg = uow
                .view()
                .message(&inp.id)
                .map_err(|_| storage_refusal("read message"))?
                .ok_or_else(not_found_refusal)?;
            apply_unsnooze_record(&mut st, inp)?;
            let change = upsert_state_change(
                uow.view(),
                &self.here.organization_id,
                &st.conversation_id,
                &inp.to_member_id,
                st.clone(),
                Change::new(Ack::Committed),
            )?;
            let seq =
                resolve_local_seq(uow.view(), &msg).map_err(|_| storage_refusal("local_seq"))?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) =
                Some(to_wire_header(&msg, Some(&st), seq));
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

#[derive(Debug, Clone)]
pub struct NotifyCandidateOut {
    pub header: MessageHeader,
    pub reason: NotifyReason,
}

pub struct MailboxCheckNotifyOp {
    pub here: OfficeIdentity,
    pub to_member_id: Id,
    pub after_task: bool,
    pub urgent_only: bool,
    pub sweep_before_utc: String,
    pub now_utc: String,
}

fn evaluate_notify_state(
    st: &mut RecipientStateRecord,
    msg: &MessageRecord,
    after_task: bool,
    sweep_before_utc: &str,
    now_utc: &str,
) -> (Option<NotifyReason>, bool) {
    let urgent = org_proto::msg::is_urgent(&msg.kind, msg.priority);
    if st.state == MessageLifecycleState::Snoozed {
        // Per TriageStore::check_notify contract, an unreadable snoozed_until releases as due.
        let time_due = st.snoozed_until.as_deref().is_some_and(|at| {
            match (
                crate::time::parse_rfc3339_ms(at),
                crate::time::parse_rfc3339_ms(now_utc),
            ) {
                (Some(a), Some(n)) => a <= n,
                _ => true,
            }
        });
        let task_due = after_task && st.snooze_after_task;
        if time_due || task_due {
            st.state = MessageLifecycleState::Unread;
            st.triage_generation = st.triage_generation.saturating_add(1);
            st.snoozed_until = None;
            st.snooze_after_task = false;
            st.snooze_note = None;
            st.updated_at = now_utc.to_owned();
            let reason = if time_due {
                NotifyReason::Due
            } else {
                NotifyReason::AfterTask
            };
            return (Some(reason), true);
        }
        return (None, false);
    }
    if st.state == MessageLifecycleState::Unread {
        if after_task || urgent {
            return (Some(NotifyReason::New), false);
        }
        if st.triaged_at.is_none() && msg.created_at.as_str() < sweep_before_utc {
            return (Some(NotifyReason::Sweep), false);
        }
    }
    (None, false)
}

impl StoreOp for MailboxCheckNotifyOp {
    type Out = Result<Result<Vec<NotifyCandidateOut>, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let states = uow
                .view()
                .recipient_states_for_employee(&self.to_member_id)
                .map_err(|_| storage_refusal("list states"))?;
            let mut change = Change::new(Ack::Committed);
            let mut candidates = Vec::new();
            let mut woken_by_conv: BTreeMap<Id, Vec<RecipientStateRecord>> = BTreeMap::new();
            for mut st in states {
                let Some(msg) = uow
                    .view()
                    .message(&st.message_id)
                    .map_err(|_| storage_refusal("read msg"))?
                else {
                    continue;
                };
                let (reason, woke) = evaluate_notify_state(
                    &mut st,
                    &msg,
                    self.after_task,
                    &self.sweep_before_utc,
                    &self.now_utc,
                );
                if woke {
                    woken_by_conv
                        .entry(st.conversation_id.clone())
                        .or_default()
                        .push(st.clone());
                }
                let urgent = org_proto::msg::is_urgent(&msg.kind, msg.priority);
                if let Some(r) = reason.filter(|_| !self.urgent_only || urgent) {
                    let seq = resolve_local_seq(uow.view(), &msg)
                        .map_err(|_| storage_refusal("local_seq"))?;
                    candidates.push(NotifyCandidateOut {
                        header: to_wire_header(&msg, Some(&st), seq),
                        reason: r,
                    });
                }
            }
            for (conv_id, woken) in woken_by_conv {
                change = upsert_states_batch(
                    uow.view(),
                    &self.here.organization_id,
                    &conv_id,
                    &self.to_member_id,
                    woken,
                    change,
                )?;
            }
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(candidates);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct GetMailboxMessageRead(pub String);

impl ReadOp for GetMailboxMessageRead {
    type Out = Option<MailboxStored>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let Some(msg) = snapshot.message(&self.0)? else {
            return Ok(None);
        };
        let states = snapshot.all_recipient_states()?;
        let st = states.into_iter().find(|s| s.message_id == msg.id);
        let seq = resolve_local_seq(snapshot, &msg)?;
        Ok(Some(stored_from_records(&msg, st.as_ref(), seq)))
    }
}

#[derive(Debug, Clone)]
pub struct PeekUnreadOut {
    pub message: Option<MailboxStored>,
    pub expired_ids: Vec<String>,
}

pub struct PeekUnreadRead {
    pub to_member_id: Id,
    pub from_member_id: Option<String>,
    pub urgent_only: bool,
    pub now_utc: String,
}

impl ReadOp for PeekUnreadRead {
    type Out = PeekUnreadOut;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let mut states = snapshot.recipient_states_for_employee(&self.to_member_id)?;
        states.retain(|s| s.state == MessageLifecycleState::Unread);
        states.sort_by(|a, b| {
            a.origin_sequence
                .cmp(&b.origin_sequence)
                .then_with(|| a.message_id.cmp(&b.message_id))
        });
        let now_ms = crate::time::parse_rfc3339_ms(&self.now_utc);
        let mut expired_ids = Vec::new();
        let mut chosen = None;
        for st in states {
            let Some(msg) = snapshot.message(&st.message_id)? else {
                continue;
            };
            let from = msg
                .sender_employee_id
                .as_ref()
                .map(|i| i.as_str())
                .unwrap_or(OFFICE_SENDER);
            if let Some(filter) = &self.from_member_id {
                if from != filter {
                    continue;
                }
            }
            if self.urgent_only && !org_proto::msg::is_urgent(&msg.kind, msg.priority) {
                continue;
            }
            let is_expired = if st.expired_waiting_person {
                false
            } else if let Some(exp) = &msg.expires_at {
                let skew = (msg.ttl_ms.unwrap_or(0) / 4).min(2_000);
                match (crate::time::parse_rfc3339_ms(exp), now_ms) {
                    (Some(exp_ms), Some(now)) => now >= exp_ms.saturating_add(skew),
                    _ => true,
                }
            } else {
                false
            };
            if is_expired {
                if expired_ids.len() < 100 {
                    expired_ids.push(msg.id);
                }
                continue;
            }
            if chosen.is_none() {
                let seq = resolve_local_seq(snapshot, &msg)?;
                chosen = Some(stored_from_records(&msg, Some(&st), seq));
            }
        }
        Ok(PeekUnreadOut {
            message: chosen,
            expired_ids,
        })
    }
}

pub struct ListMailboxHeadersRead {
    pub scope_inbox: Option<Id>,
    pub scope_participant: Option<Id>,
    pub from_member_id: Option<String>,
    pub thread_id: Option<String>,
    pub unread_only: bool,
    pub include_snoozed: bool,
    pub cursor: Option<u64>,
    pub limit: u32,
}

impl ListMailboxHeadersRead {
    fn matches_common_filters(&self, hdr: &MessageHeader) -> bool {
        if self.unread_only && hdr.state != MessageLifecycleState::Unread {
            return false;
        }
        if !self.include_snoozed && hdr.state == MessageLifecycleState::Snoozed {
            return false;
        }
        if self
            .thread_id
            .as_deref()
            .is_some_and(|t| hdr.thread_id != t)
        {
            return false;
        }
        if self.cursor.is_some_and(|c| hdr.local_seq >= c) {
            return false;
        }
        true
    }

    fn collect_inbox<S: MessagingRead>(
        &self,
        snapshot: &S,
        inbox_emp: &Id,
    ) -> Result<Vec<MessageHeader>, StoreError> {
        let mut headers = Vec::new();
        for st in snapshot.recipient_states_for_employee(inbox_emp)? {
            let Some(msg) = snapshot.message(&st.message_id)? else {
                continue;
            };
            let seq = resolve_local_seq(snapshot, &msg)?;
            let hdr = to_wire_header(&msg, Some(&st), seq);
            if !self.matches_common_filters(&hdr) {
                continue;
            }
            if self
                .from_member_id
                .as_deref()
                .is_some_and(|f| hdr.from_member_id != f)
            {
                continue;
            }
            headers.push(hdr);
        }
        Ok(headers)
    }

    fn collect_all_or_participant<S: MessagingRead>(
        &self,
        snapshot: &S,
    ) -> Result<Vec<MessageHeader>, StoreError> {
        let mut headers = Vec::new();
        let states = snapshot.all_recipient_states()?;
        for msg in snapshot.all_messages()? {
            let st = states.iter().find(|s| s.message_id == msg.id);
            let seq = resolve_local_seq(snapshot, &msg)?;
            let hdr = to_wire_header(&msg, st, seq);
            if !self.matches_common_filters(&hdr) {
                continue;
            }
            if let Some(part) = &self.scope_participant {
                let p = part.as_str();
                if hdr.from_member_id != p && hdr.to_member_id != p {
                    continue;
                }
            }
            headers.push(hdr);
        }
        Ok(headers)
    }
}

impl ReadOp for ListMailboxHeadersRead {
    type Out = HeaderPage;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let mut headers = match &self.scope_inbox {
            Some(inbox_emp) => self.collect_inbox(snapshot, inbox_emp)?,
            None => self.collect_all_or_participant(snapshot)?,
        };
        headers.sort_by(|a, b| b.local_seq.cmp(&a.local_seq).then_with(|| b.id.cmp(&a.id)));
        let more = headers.len() > self.limit as usize;
        headers.truncate(self.limit as usize);
        let next_cursor = if more {
            headers.last().map(|h| h.local_seq)
        } else {
            None
        };
        Ok(HeaderPage {
            headers,
            next_cursor,
        })
    }
}

pub struct FindReplyForAskRead {
    pub ask_id: String,
    pub responder_id: String,
}

impl ReadOp for FindReplyForAskRead {
    type Out = Option<MailboxStored>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let Some(ask) = snapshot.message(&self.ask_id)? else {
            return Ok(None);
        };
        let asker = ask
            .sender_employee_id
            .as_ref()
            .map(|i| i.as_str())
            .unwrap_or(OFFICE_SENDER);
        let states = snapshot.all_recipient_states()?;
        for msg in snapshot.all_messages()? {
            if msg.reply_to_message_id.as_deref() != Some(self.ask_id.as_str()) {
                continue;
            }
            let from = msg
                .sender_employee_id
                .as_ref()
                .map(|i| i.as_str())
                .unwrap_or(OFFICE_SENDER);
            let st = states.iter().find(|s| s.message_id == msg.id);
            let to = st.map(|s| s.employee_id.as_str()).unwrap_or("");
            let is_match = to == asker
                && (from == self.responder_id
                    || (from == OFFICE_SENDER && msg.kind == KIND_DELIVERY_FAILED));
            if is_match {
                let seq = resolve_local_seq(snapshot, &msg)?;
                return Ok(Some(stored_from_records(&msg, st, seq)));
            }
        }
        Ok(None)
    }
}

pub struct ListActiveSnoozeDeadlinesRead;

impl ReadOp for ListActiveSnoozeDeadlinesRead {
    type Out = Vec<(String, String, String)>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let mut out = Vec::new();
        for st in snapshot.all_recipient_states()? {
            if st.state == MessageLifecycleState::Snoozed {
                if let Some(at) = st.snoozed_until {
                    out.push((st.message_id, st.employee_id.as_str().to_owned(), at));
                }
            }
        }
        Ok(out)
    }
}

/// Reads committed organization receipts with `seq > after_seq` for `OrgFeed` replay (`subscribe`
/// and `/api/v1/events/stream` restart/resume semantics).
pub struct OrgChangesAfterRead {
    pub owner: Id,
    pub after_seq: u64,
    pub limit: usize,
}

impl ReadOp for OrgChangesAfterRead {
    type Out = Vec<OrgChangedData>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let org_snap = crate::organization::snapshot::project(snapshot, &self.owner)?;
        let teams: BTreeMap<&str, &org_proto::Team> =
            org_snap.teams.iter().map(|t| (t.id.as_str(), t)).collect();
        let members: BTreeMap<&str, &org_proto::Member> = org_snap
            .members
            .iter()
            .map(|m| (m.id.as_str(), m))
            .collect();
        let projects: BTreeMap<&str, &org_proto::Project> = org_snap
            .projects
            .iter()
            .map(|p| (p.id.as_str(), p))
            .collect();
        let receipts = snapshot.list_receipts_by_prefix("organization.")?;
        let mut out = Vec::new();
        for receipt in receipts {
            if receipt.outcome != Outcome::Completed || receipt.kind == "organization.create" {
                continue;
            }
            let val: Meta = serde_json::from_slice(&receipt.reply).unwrap_or_default();
            let seq = val
                .get("generation")
                .and_then(|v| v.as_u64())
                .unwrap_or(receipt.ordinal);
            if seq <= self.after_seq {
                continue;
            }
            let pos = Position { epoch: 1, seq };
            let actor = match receipt.actor {
                org_store::Actor::Employee(id) => Some(WireActor {
                    kind: ActorKind::Human,
                    member: id.as_str().to_owned(),
                }),
                org_store::Actor::System => None,
            };
            let changes =
                map_receipt_to_org_changes(&receipt.kind, &val, &teams, &members, &projects);
            out.push(OrgChangedData {
                position: pos,
                command_id: receipt.id.as_str().to_owned(),
                changes,
                actor,
            });
        }
        out.sort_by_key(|c| c.position.seq);
        out.truncate(self.limit);
        Ok(out)
    }
}

fn map_receipt_to_org_changes(
    kind: &str,
    reply: &Meta,
    teams: &BTreeMap<&str, &org_proto::Team>,
    members: &BTreeMap<&str, &org_proto::Member>,
    projects: &BTreeMap<&str, &org_proto::Project>,
) -> Vec<OrgChange> {
    let entity_id = reply
        .get("id")
        .or_else(|| reply.get("employee_id"))
        .or_else(|| reply.get("project_id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if entity_id.is_empty() {
        return Vec::new();
    }
    match kind {
        "organization.create_team"
        | "organization.update_team"
        | "organization.move_team"
        | "organization.set_team_max_members" => teams
            .get(entity_id)
            .map(|t| vec![OrgChange::TeamUpserted { team: (*t).clone() }])
            .unwrap_or_else(|| {
                vec![OrgChange::TeamDeleted {
                    team_id: entity_id.to_owned(),
                }]
            }),
        "organization.archive_team" => vec![OrgChange::TeamDeleted {
            team_id: entity_id.to_owned(),
        }],
        "organization.add_person"
        | "organization.update_person"
        | "organization.move_person"
        | "organization.set_may_hire"
        | "organization.hire_agent"
        | "organization.confirm_hire"
        | "organization.update_agent"
        | "organization.move_agent"
        | "organization.set_agent_status"
        | "organization.upsert_agent_execution" => members
            .get(entity_id)
            .map(|m| {
                vec![OrgChange::MemberUpserted {
                    member: (*m).clone(),
                }]
            })
            .unwrap_or_else(|| {
                vec![OrgChange::MemberRemoved {
                    member_id: entity_id.to_owned(),
                }]
            }),
        "organization.deactivate_person" | "organization.offboard_agent" => {
            vec![OrgChange::MemberRemoved {
                member_id: entity_id.to_owned(),
            }]
        }
        "organization.create_project"
        | "organization.add_project_team"
        | "organization.remove_project_team" => projects
            .get(entity_id)
            .map(|p| {
                vec![OrgChange::ProjectUpserted {
                    project: (*p).clone(),
                }]
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Atomically delivers validated `answer_candidate` inbox entries (`ADR-0024` d11, `WATCH-3/7`).
pub struct DeliverAnswerCandidatesOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub conversation_id: Id,
    pub deliveries: Vec<features::AnswerCandidateDelivery>,
    pub org_seq: u64,
    pub now_utc: String,
}

impl StoreOp for DeliverAnswerCandidatesOp {
    type Out = Result<Result<Vec<(String, Id)>, ApiError>, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, delivered) = features::deliver_answer_candidates(
                uow,
                &self.here,
                &self.owner,
                &self.conversation_id,
                &self.deliveries,
                self.org_seq,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(delivered);
            Ok(change)
        });
        unwrap_reply(reply, out)
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
        Ok(features::channel_messages_for_caller(
            snapshot,
            &self.owner,
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
