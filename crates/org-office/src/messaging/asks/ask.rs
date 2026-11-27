//! Asking: idempotent replay, the ask's recipients, and committing the ask record.

use std::collections::BTreeMap;

use org_proto::ask::{AskQuestion, AskStatus, ChatAskParams, ChatAskResult};
use org_proto::chat::{ConversationKind, DeliveredBecause};
use org_proto::msg::{MessagePriority, KIND_REQUEST, MAX_META_BYTES};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::messaging::features::{
    append_send_recipient_states, build_mention_candidates, check_hr_access,
    effective_auto_members, is_active_employee, refusal, resolve_send_target,
};
use crate::messaging::files::{Binding, MessageFiles, MESSAGE_SCOPE};
use crate::messaging::mentions::extract_mentions;
use crate::messaging::ports::{MessagingRead, MessagingWrite};
use crate::messaging::records::{
    self, ConversationMembership, MessageAttachmentRecord, MessageRecord, SenderKind,
    SCHEMA_VERSION,
};
use crate::organization::ports::OrganizationRead;
use crate::organization::records::Employee;
use crate::time;

use super::card::{
    build_ask_card, compute_wait_outcome, normalize_blocks, resolve_addressed_to,
    validate_ask_inputs,
};
use super::{parse_ask_card, AskCard, AskCommitted, NON_REQUIRED_EXPIRE_MS};

fn non_card_meta(meta: &BTreeMap<String, serde_json::Value>) -> BTreeMap<&str, &serde_json::Value> {
    meta.iter()
        .filter(|(k, _)| k.as_str() != "card")
        .map(|(k, v)| (k.as_str(), v))
        .collect()
}

struct AskReplayExpected<'a> {
    organization_id: &'a Id,
    files: &'a MessageFiles,
    sender_id: &'a Id,
    conv_id: &'a Id,
    params: &'a ChatAskParams,
    questions: &'a [AskQuestion],
    wait_ms: u64,
    body: &'a str,
    addressed: Option<&'a Id>,
}

fn ask_replay_matches(
    existing: &MessageRecord,
    existing_card: Option<&AskCard>,
    expected: &AskReplayExpected<'_>,
) -> bool {
    let addressed_str = expected.addressed.map(|id| id.as_str());
    existing.kind == KIND_REQUEST
        && existing.sender_employee_id.as_ref() == Some(expected.sender_id)
        && existing.conversation_id == *expected.conv_id
        && existing.subject == expected.params.subject
        && existing.body_markdown == expected.body
        && non_card_meta(&existing.metadata) == non_card_meta(&expected.params.meta)
        && expected.files.matches(
            &existing.attachments,
            expected.organization_id,
            MESSAGE_SCOPE,
            &existing.id,
            &existing.created_at,
        )
        && existing_card.is_some_and(|c| {
            c.questions == expected.questions
                && c.required == expected.params.required
                && c.approval == expected.params.approval
                && c.wait_ms == expected.wait_ms
                && c.blocks == normalize_blocks(expected.params.blocks.as_deref())
                && c.addressed_to.as_deref() == addressed_str
        })
}

fn check_ask_replay(
    read: &impl MessagingRead,
    expected: &AskReplayExpected<'_>,
    now_ms: u64,
) -> Result<Option<(Change, AskCommitted)>, Refusal> {
    let Some(existing) = read
        .message(&expected.params.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read message"))?
    else {
        return Ok(None);
    };
    let existing_card = parse_ask_card(&existing);
    if !ask_replay_matches(&existing, existing_card.as_ref(), expected) {
        return Err(refusal(
            ErrorCode::AlreadyExists,
            "ask id already exists with different content",
        ));
    }
    let card = existing_card.expect("checked above");
    let answer_file = read
        .answer_for_message(&existing.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read answer"))?;
    let outcome = compute_wait_outcome(&existing, &card, answer_file.as_ref(), now_ms);
    let local_seq = read
        .message_local_seq(&existing.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read local_seq"))?
        .unwrap_or(existing.origin_sequence);
    let thread_id = existing
        .thread_root_id
        .clone()
        .unwrap_or_else(|| existing.id.clone());
    Ok(Some((
        Change::new(Ack::Committed),
        AskCommitted {
            result: ChatAskResult {
                status: outcome.status,
                ask_id: existing.id.clone(),
                conversation_id: existing.conversation_id.as_str().to_owned(),
                thread_id,
                local_seq,
                origin_seq: existing.origin_sequence,
                wait_until: outcome.wait_until,
                remaining_ms: outcome.remaining_ms,
                answer: outcome.answer,
                withdrawal: outcome.withdrawal,
                defaults: outcome.defaults,
                required_notice: outcome.required_notice,
                recipient_presence: Vec::new(),
                woke_agents: Vec::new(),
                watched: None,
                watch_not_started_reason: None,
            },
            inserted: false,
            card,
            message: existing,
            recipients: Vec::new(),
        },
    )))
}

fn collect_ask_recipient_reasons(
    conv_kind: ConversationKind,
    sender_id: &Id,
    active_members: &[Id],
    addressed: Option<&Id>,
    mentioned_ids: &[Id],
) -> BTreeMap<Id, DeliveredBecause> {
    let mut reasons = BTreeMap::new();
    if let Some(target) = addressed {
        if target != sender_id {
            reasons.insert(target.clone(), DeliveredBecause::Addressed);
        }
    } else if matches!(conv_kind, ConversationKind::Dm | ConversationKind::GroupDm) {
        for eid in active_members {
            if eid != sender_id {
                reasons.insert(eid.clone(), DeliveredBecause::Addressed);
            }
        }
    }
    for eid in mentioned_ids {
        if eid != sender_id && active_members.contains(eid) {
            reasons
                .entry(eid.clone())
                .or_insert(DeliveredBecause::Mention);
        }
    }
    reasons
}

#[expect(
    clippy::too_many_arguments,
    reason = "ask decision takes explicit transaction, identity, caller, params, files, and clock inputs"
)]
pub fn decide_msg_ask<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &ChatAskParams,
    files: &MessageFiles,
    org_seq: u64,
    now_ms: u64,
) -> Result<(Change, AskCommitted), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let (questions, wait_ms, body) = validate_ask_inputs(params)?;
    let now_utc = time::format_rfc3339_ms(now_ms);
    let employees = uow
        .view()
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    let sender = employees
        .iter()
        .find(|e| e.id == *caller && is_active_employee(e))
        .ok_or_else(|| refusal(ErrorCode::NotBound, "sender is not an active employee"))?;

    let (mut conv, conv_persisted) = resolve_send_target(
        uow.view(),
        here,
        owner,
        sender,
        &employees,
        &params.to,
        &now_utc,
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

    let candidates = build_mention_candidates(&employees, &active_members, owner);
    let scan = extract_mentions(&body, &candidates);
    let mentioned_ids: Vec<Id> = scan
        .resolved
        .iter()
        .filter_map(|m| Id::parse(&m.employee_id).ok())
        .collect();
    let addressed = resolve_addressed_to(
        &employees,
        owner,
        sender,
        conv.kind,
        &active_members,
        params.addressed_to.as_deref(),
        &mentioned_ids,
    )?;

    let expected = AskReplayExpected {
        organization_id: &here.organization_id,
        files,
        sender_id: &sender.id,
        conv_id: &conv.id,
        params,
        questions: &questions,
        wait_ms,
        body: &body,
        addressed: addressed.as_ref(),
    };
    if let Some(replayed) = check_ask_replay(uow.view(), &expected, now_ms)? {
        return Ok(replayed);
    }

    let mut conv_changed = false;
    if conv.auto_scope.is_none() {
        for eid in &active_members {
            if !conv.memberships.iter().any(|m| m.employee_id == *eid) {
                conv.memberships.push(ConversationMembership {
                    employee_id: eid.clone(),
                    joined_at: now_utc.clone(),
                    left_at: None,
                });
                conv_changed = true;
            }
        }
    }

    let reasons = collect_ask_recipient_reasons(
        conv.kind,
        &sender.id,
        &active_members,
        addressed.as_ref(),
        &mentioned_ids,
    );
    let mut hr_targets: Vec<Id> = reasons.keys().cloned().chain(addressed.clone()).collect();
    if conv.kind != ConversationKind::Channel {
        hr_targets.extend(
            active_members
                .iter()
                .filter(|id| **id != sender.id)
                .cloned(),
        );
    }
    check_hr_access(uow.view(), &employees, owner, &sender.id, hr_targets)?;
    let attachments = files.bind(&Binding {
        organization_id: &here.organization_id,
        conversation: &conv,
        members: &active_members,
        scope: MESSAGE_SCOPE,
        key: &params.id,
        created_at: &now_utc,
    })?;

    let wait_until = time::format_rfc3339_ms(now_ms.saturating_add(wait_ms));
    let card = build_ask_card(
        questions,
        params.required,
        wait_ms,
        wait_until.clone(),
        addressed,
        params.blocks.clone(),
        params.approval,
    );
    commit_ask_record(
        uow.view(),
        here,
        &mut conv,
        conv_persisted,
        conv_changed,
        sender,
        &employees,
        params,
        body,
        card,
        reasons,
        attachments,
        org_seq,
        now_ms,
        &now_utc,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "ask commit helper threads read view, conversation state, recipients, card, and clock explicitly"
)]
fn commit_ask_record(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    conv: &mut crate::messaging::records::ConversationRecord,
    conv_persisted: bool,
    conv_changed: bool,
    sender: &Employee,
    employees: &[Employee],
    params: &ChatAskParams,
    body: String,
    card: AskCard,
    reasons: BTreeMap<Id, DeliveredBecause>,
    attachments: Vec<MessageAttachmentRecord>,
    org_seq: u64,
    now_ms: u64,
    now_utc: &str,
) -> Result<(Change, AskCommitted), Refusal> {
    let mut meta = params.meta.clone();
    meta.insert(
        "card".into(),
        serde_json::to_value(&card).expect("AskCard serializes"),
    );
    let meta_bytes = serde_json::to_vec(&meta).map_err(|_| {
        refusal(
            ErrorCode::InvalidRequest,
            "ask metadata cannot be serialized",
        )
    })?;
    if meta_bytes.len() > MAX_META_BYTES {
        return Err(refusal(
            ErrorCode::TooLarge,
            "ask card metadata exceeds size limit",
        ));
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
    let expires_at = (!card.required)
        .then(|| time::format_rfc3339_ms(now_ms.saturating_add(NON_REQUIRED_EXPIRE_MS)));
    let ttl_ms = (!card.required).then_some(NON_REQUIRED_EXPIRE_MS);

    let mut record = MessageRecord {
        schema_version: SCHEMA_VERSION,
        organization_id: here.organization_id.clone(),
        id: params.id.clone(),
        conversation_id: conv.id.clone(),
        sender_kind: SenderKind::Employee,
        sender_employee_id: Some(sender.id.clone()),
        sender_office_id: None,
        sender_display_name: sender.name.clone(),
        kind: KIND_REQUEST.to_owned(),
        priority: MessagePriority::Urgent,
        subject: params.subject.clone(),
        body_markdown: body,
        metadata: meta,
        thread_root_id: None,
        reply_to_message_id: None,
        also_in_channel: false,
        created_at: now_utc.to_owned(),
        expires_at,
        ttl_ms,
        origin_office_id: here.office_id.clone(),
        origin_incarnation: origin_incarnation.to_owned(),
        origin_sequence: origin_seq,
        org_epoch: 1,
        org_sequence: org_seq,
        content_sha256: String::new(),
        attachments,
        mailbox_content_hash: None,
    };
    record.content_sha256 = record.compute_content_sha256();

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
    let (change, recipients) = append_send_recipient_states(
        read, here, &conv.id, &record, employees, reasons, org_seq, now_utc, change,
    )?;
    let wait_until = card.wait_until.clone();
    Ok((
        change,
        AskCommitted {
            result: ChatAskResult {
                status: AskStatus::Waiting,
                ask_id: record.id.clone(),
                conversation_id: conv.id.as_str().to_owned(),
                thread_id: record.id.clone(),
                local_seq,
                origin_seq,
                wait_until: Some(wait_until),
                remaining_ms: Some(card.wait_ms),
                answer: None,
                withdrawal: None,
                defaults: Vec::new(),
                required_notice: None,
                recipient_presence: Vec::new(),
                woke_agents: Vec::new(),
                watched: None,
                watch_not_started_reason: None,
            },
            inserted: true,
            card,
            message: record,
            recipients,
        },
    ))
}
