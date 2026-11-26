//! Sending a chat message: input validation, idempotent replay, the new message record and
//! its commit.

use org_proto::chat::{ChatSendParams, ChatSendResult, ConversationKind};
use org_proto::msg::{
    json_string_len, MessagePriority, Meta, KIND_DELIVERY_FAILED, KIND_REQUEST, MAX_BODY_BYTES,
    MAX_ID_BYTES, MAX_META_BYTES, MAX_SUBJECT_BYTES,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::messaging::{
    mentions::{extract_mentions, MentionScan},
    ports::{MessagingRead, MessagingWrite},
    records::{
        self, ConversationMembership, ConversationRecord, MessageRecord, RecipientStateRecord,
        SenderKind, SCHEMA_VERSION,
    },
};
use crate::organization::ports::OrganizationRead;
use crate::organization::records::Employee;

use super::auto_channels::effective_auto_members;
use super::messages::to_wire_header;
use super::recipients::{
    append_send_recipient_states, build_mention_candidates, collect_recipient_reasons,
    resolve_send_target,
};
use super::{check_hr_access, is_active_employee, refusal};

/// Output of committing a `chat.send` message on `org-store`, including the exact headers
/// to emit as `message.received` live events (header-only!) and the recipient employee IDs
/// whose `msg.wait` listeners should be woken.
#[derive(Debug)]
pub struct ChatSendCommitted {
    pub result: ChatSendResult,
    pub inserted: bool,
    pub conversation_kind: ConversationKind,
    pub open_question_asker_id: Option<String>,
    pub message: MessageRecord,
    pub recipients: Vec<(Id, RecipientStateRecord)>,
}

impl ChatSendCommitted {
    /// Builds header-only wire summaries for every delivered recipient (never carries body).
    pub fn delivered_headers(&self) -> Vec<org_proto::msg::MessageHeader> {
        self.recipients
            .iter()
            .map(|(_, state)| to_wire_header(&self.message, Some(state), self.result.local_seq))
            .collect()
    }
}

fn validate_send_sizes(params: &ChatSendParams) -> Result<(), Refusal> {
    if let Some(sub) = &params.subject {
        if json_string_len(sub) > MAX_SUBJECT_BYTES {
            return Err(refusal(ErrorCode::TooLarge, "subject exceeds size limit"));
        }
    }
    if let Some(rep) = &params.reply_to {
        if json_string_len(rep) > MAX_ID_BYTES {
            return Err(refusal(ErrorCode::TooLarge, "reply_to exceeds size limit"));
        }
    }
    if json_string_len(&params.body) > MAX_BODY_BYTES {
        return Err(refusal(ErrorCode::TooLarge, "body exceeds 256 KiB limit"));
    }
    let meta_bytes = serde_json::to_vec(&params.meta)
        .map_err(|_| refusal(ErrorCode::InvalidRequest, "invalid meta"))?
        .len();
    if meta_bytes > MAX_META_BYTES {
        return Err(refusal(ErrorCode::TooLarge, "meta exceeds 64 KiB limit"));
    }
    Ok(())
}

fn validate_chat_send_inputs(params: &ChatSendParams) -> Result<MessagePriority, Refusal> {
    if json_string_len(&params.id) > MAX_ID_BYTES
        || (org_proto::uuid_v7_time_ms(&params.id).is_none()
            && org_proto::ulid_time_ms(&params.id).is_none())
    {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "message id must be a canonical lowercase UUIDv7",
        ));
    }
    if params.kind.is_empty()
        || json_string_len(&params.kind) > MAX_ID_BYTES
        || matches!(params.kind.as_str(), KIND_REQUEST | KIND_DELIVERY_FAILED)
    {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "invalid message kind for send",
        ));
    }
    if params.kind == "announcement" && params.reason.as_deref().is_none_or(|r| r.trim().is_empty())
    {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "announcement messages require --reason",
        ));
    }
    let priority = params.priority.unwrap_or_default();
    if !matches!(
        priority,
        MessagePriority::Normal | MessagePriority::High | MessagePriority::Urgent
    ) {
        return Err(refusal(ErrorCode::InvalidRequest, "invalid priority"));
    }
    validate_send_sizes(params)?;
    Ok(priority)
}

#[expect(
    clippy::too_many_arguments,
    reason = "replay check compares idempotency record against full send context and caller identity"
)]
fn check_chat_send_replay(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    owner: &Id,
    sender: &Employee,
    employees: &[Employee],
    params: &ChatSendParams,
    priority: MessagePriority,
    now_utc: &str,
) -> Result<Option<(Change, ChatSendCommitted)>, Refusal> {
    let Some(existing) = read
        .message(&params.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read message"))?
    else {
        return Ok(None);
    };
    let (target_conv, _) =
        resolve_send_target(read, here, owner, sender, employees, &params.to, now_utc)?;
    let mut expected_meta = params.meta.clone();
    if let Some(reason) = &params.reason {
        expected_meta.insert("reason".into(), serde_json::Value::String(reason.clone()));
    }
    let mut stored_meta = existing.metadata.clone();
    stored_meta.remove("mentions");
    expected_meta.remove("mentions");
    if existing.sender_employee_id.as_ref() != Some(&sender.id)
        || existing.conversation_id != target_conv.id
        || existing.body_markdown != params.body
        || existing.kind != params.kind
        || existing.priority != priority
        || existing.subject != params.subject
        || existing.reply_to_message_id != params.reply_to
        || existing.also_in_channel != params.also_in_channel
        || stored_meta != expected_meta
    {
        return Err(refusal(
            ErrorCode::AlreadyExists,
            "message id already used for different content",
        ));
    }
    let active_members = effective_auto_members(read, &target_conv, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to resolve members"))?;
    let candidates = build_mention_candidates(employees, &active_members, owner);
    let scan = extract_mentions(&params.body, &candidates);
    let thread_id = existing
        .thread_root_id
        .clone()
        .unwrap_or_else(|| existing.id.clone());
    let local_seq = read
        .message_local_seq(&existing.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read local_seq"))?
        .unwrap_or(existing.origin_sequence);
    let conversation_kind = read
        .conversation(&existing.conversation_id)
        .ok()
        .flatten()
        .map_or(ConversationKind::Channel, |c| c.kind);
    Ok(Some((
        Change::new(Ack::Committed),
        ChatSendCommitted {
            result: ChatSendResult {
                id: existing.id.clone(),
                conversation_id: existing.conversation_id.as_str().to_owned(),
                thread_id,
                local_seq,
                origin_seq: existing.origin_sequence,
                mentions: scan.resolved,
                unmatched_mentions: scan.unmatched,
                recipient_presence: Vec::new(),
                woke_agents: Vec::new(),
            },
            inserted: false,
            conversation_kind,
            open_question_asker_id: None,
            message: existing,
            recipients: Vec::new(),
        },
    )))
}

fn resolve_reply_thread_root(
    read: &impl MessagingRead,
    conv_id: &Id,
    reply_to: Option<&str>,
) -> Result<Option<String>, Refusal> {
    let Some(reply_to) = reply_to else {
        return Ok(None);
    };
    let parent = read
        .message(reply_to)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read reply_to"))?
        .ok_or_else(|| refusal(ErrorCode::NotFound, "reply_to message not found"))?;
    if parent.conversation_id != *conv_id {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "cannot reply across different conversations",
        ));
    }
    if parent.kind == KIND_DELIVERY_FAILED {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "cannot reply to a delivery_failed notice",
        ));
    }
    Ok(Some(parent.thread_root_id.unwrap_or(parent.id)))
}

#[expect(
    clippy::too_many_arguments,
    reason = "message record builder receives explicit identity, conversation, sender, params, priority, and metadata"
)]
fn build_new_message_record(
    here: &OfficeIdentity,
    conv_id: &Id,
    sender: &Employee,
    params: &ChatSendParams,
    priority: MessagePriority,
    meta: Meta,
    thread_root_id: Option<String>,
    origin_seq: u64,
    org_seq: u64,
    now_utc: &str,
) -> MessageRecord {
    let mut record = MessageRecord {
        schema_version: SCHEMA_VERSION,
        organization_id: here.organization_id.clone(),
        id: params.id.clone(),
        conversation_id: conv_id.clone(),
        sender_kind: SenderKind::Employee,
        sender_employee_id: Some(sender.id.clone()),
        sender_office_id: None,
        sender_display_name: sender.name.clone(),
        kind: params.kind.clone(),
        priority,
        subject: params.subject.clone(),
        body_markdown: params.body.clone(),
        metadata: meta,
        thread_root_id,
        reply_to_message_id: params.reply_to.clone(),
        also_in_channel: params.also_in_channel,
        created_at: now_utc.to_owned(),
        expires_at: None,
        ttl_ms: None,
        origin_office_id: here.office_id.clone(),
        origin_incarnation: here.journal_incarnation.as_str().to_owned(),
        origin_sequence: origin_seq,
        org_epoch: 1,
        org_sequence: org_seq,
        content_sha256: String::new(),
        attachments: Vec::new(),
        mailbox_content_hash: None,
    };
    record.content_sha256 = record.compute_content_sha256();
    record
}

#[expect(
    clippy::too_many_arguments,
    reason = "send commit builder coordinates conversation state, record, recipients, and change assembly"
)]
fn build_send_commit_change(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    owner: &Id,
    conv: &mut ConversationRecord,
    conv_persisted: bool,
    conv_changed: bool,
    sender: &Employee,
    employees: &[Employee],
    active_members: &[Id],
    params: &ChatSendParams,
    priority: MessagePriority,
    scan: MentionScan,
    thread_root_id: Option<String>,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, ChatSendCommitted), Refusal> {
    let mut meta = params.meta.clone();
    if !scan.resolved.is_empty() {
        let ids: Vec<serde_json::Value> = scan
            .resolved
            .iter()
            .map(|m| serde_json::Value::String(m.employee_id.clone()))
            .collect();
        meta.insert("mentions".into(), serde_json::Value::Array(ids));
    }
    if let Some(reason) = &params.reason {
        meta.insert("reason".into(), serde_json::Value::String(reason.clone()));
    }
    let origin_incarnation = here.journal_incarnation.as_str();
    let origin_seq = read
        .max_origin_sequence(&here.office_id, origin_incarnation)
        .map_err(|_| {
            refusal(
                ErrorCode::StorageError,
                "failed to allocate origin_sequence",
            )
        })?
        .saturating_add(1);
    let local_seq = read
        .max_local_seq()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to allocate local_seq"))?
        .saturating_add(1);
    let record = build_new_message_record(
        here,
        &conv.id,
        sender,
        params,
        priority,
        meta,
        thread_root_id.clone(),
        origin_seq,
        org_seq,
        now_utc,
    );
    let mut change = Change::new(Ack::Committed);
    let conv_path = records::conversation_path(&conv.id);
    const MAX_CHANNEL_RECORD_BYTES: usize = 360 * 1024;
    if !conv_persisted {
        let encoded = records::encode_json(conv);
        if encoded.len() > MAX_CHANNEL_RECORD_BYTES {
            return Err(refusal(
                ErrorCode::TooLarge,
                "channel record exceeds replication size limit",
            ));
        }
        change = change.create(conv_path, encoded);
    } else if conv.auto_scope.is_some() && conv_changed {
        conv.revision = conv.revision.saturating_add(1);
        let encoded = records::encode_json(conv);
        if encoded.len() > MAX_CHANNEL_RECORD_BYTES {
            return Err(refusal(
                ErrorCode::TooLarge,
                "channel record exceeds replication size limit",
            ));
        }
        change = change.put(conv_path, encoded);
    }
    change = change.create(
        records::message_path(&conv.id, &record.id),
        records::encode_json(&record),
    );
    let reasons = collect_recipient_reasons(
        read,
        conv,
        &sender.id,
        active_members,
        &scan.resolved,
        thread_root_id.as_deref(),
        &record.kind,
        &record.metadata,
    )?;
    check_hr_access(read, employees, owner, &sender.id, reasons.keys().cloned())?;
    let (change, recipients) = append_send_recipient_states(
        read, here, &conv.id, &record, employees, reasons, org_seq, now_utc, change,
    )?;
    let open_question_asker_id = thread_root_id
        .as_deref()
        .and_then(|root_id| read.message(root_id).ok().flatten())
        .filter(|root| {
            root.kind == KIND_REQUEST && crate::messaging::ask_card::is_open_ask_card(read, root)
        })
        .and_then(|root| root.sender_employee_id.map(|id| id.as_str().to_owned()));
    let thread_id = thread_root_id.unwrap_or_else(|| record.id.clone());
    Ok((
        change,
        ChatSendCommitted {
            result: ChatSendResult {
                id: record.id.clone(),
                conversation_id: conv.id.as_str().to_owned(),
                thread_id,
                local_seq,
                origin_seq,
                mentions: scan.resolved,
                unmatched_mentions: scan.unmatched,
                recipient_presence: Vec::new(),
                woke_agents: Vec::new(),
            },
            inserted: true,
            conversation_kind: conv.kind,
            open_question_asker_id,
            message: record,
            recipients,
        },
    ))
}

pub fn decide_chat_send<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &ChatSendParams,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, ChatSendCommitted), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let priority = validate_chat_send_inputs(params)?;
    let employees = uow
        .view()
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    let sender = employees
        .iter()
        .find(|e| e.id == *caller && is_active_employee(e))
        .ok_or_else(|| refusal(ErrorCode::NotBound, "sender is not an active employee"))?;

    if let Some(replayed) = check_chat_send_replay(
        uow.view(),
        here,
        owner,
        sender,
        &employees,
        params,
        priority,
        now_utc,
    )? {
        return Ok(replayed);
    }

    let (mut conv, conv_persisted) = resolve_send_target(
        uow.view(),
        here,
        owner,
        sender,
        &employees,
        &params.to,
        now_utc,
    )?;
    if conv.archived {
        return Err(refusal(ErrorCode::Conflict, "conversation is archived"));
    }
    let active_members = effective_auto_members(uow.view(), &conv, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to resolve members"))?;
    if !active_members.contains(&sender.id) && sender.id != *owner {
        return Err(refusal(
            ErrorCode::NotAuthorized,
            "sender is not an active member of this conversation",
        ));
    }
    if conv.kind != ConversationKind::Channel {
        let others = active_members
            .iter()
            .filter(|id| *id != &sender.id)
            .cloned();
        check_hr_access(uow.view(), &employees, owner, caller, others)?;
    }

    let mut conv_changed = false;
    if conv.auto_scope.is_none() {
        for eid in &active_members {
            if !conv.memberships.iter().any(|m| m.employee_id == *eid) {
                conv.memberships.push(ConversationMembership {
                    employee_id: eid.clone(),
                    joined_at: now_utc.to_owned(),
                    left_at: None,
                });
                conv_changed = true;
            }
        }
    }

    let thread_root_id =
        resolve_reply_thread_root(uow.view(), &conv.id, params.reply_to.as_deref())?;
    let candidates = build_mention_candidates(&employees, &active_members, owner);
    let scan = extract_mentions(&params.body, &candidates);
    let mentioned_ids: Vec<Id> = scan
        .resolved
        .iter()
        .filter_map(|m| Id::parse(&m.employee_id).ok())
        .collect();
    check_hr_access(uow.view(), &employees, owner, caller, mentioned_ids)?;

    build_send_commit_change(
        uow.view(),
        here,
        owner,
        &mut conv,
        conv_persisted,
        conv_changed,
        sender,
        &employees,
        &active_members,
        params,
        priority,
        scan,
        thread_root_id,
        org_seq,
        now_utc,
    )
}
