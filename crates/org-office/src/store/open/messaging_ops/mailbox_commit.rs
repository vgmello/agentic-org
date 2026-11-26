//! `org-mailbox` storage: committing a mailbox message and claiming it for reading.

use std::collections::BTreeMap;

use org_proto::chat::{ConversationKind, DeliveredBecause};
use org_proto::msg::{
    Message, MessageHeader, MessageLifecycleState, KIND_DELIVERY_FAILED, OFFICE_SENDER,
};
use org_proto::org::Position;
use org_proto::{ApiError, ErrorCode};
use org_store::{Ack, Change, Id, OfficeIdentity, Refusal, Store, StoreError};

use crate::messaging::{
    features::to_wire_header,
    ports::MessagingRead,
    records::{
        self, ConversationMembership, ConversationRecord, MessageRecord,
        ParticipantReceivedSnapshot, RecipientStateRecord, SenderKind, SCHEMA_VERSION,
    },
};
use crate::store::open::{OfficeEngine, StoreOp};

use super::{storage_refusal, unwrap_reply};

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

pub(super) fn stored_from_records(
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

pub(super) fn resolve_local_seq(
    read: &impl MessagingRead,
    msg: &MessageRecord,
) -> Result<u64, org_store::EngineError> {
    Ok(read
        .message_local_seq(&msg.id)?
        .unwrap_or(msg.origin_sequence))
}

pub(super) fn upsert_states_batch(
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

pub(super) fn upsert_state_change(
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
