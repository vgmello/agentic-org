//! Engine-independent domain decisions for B7 structured asks (`REQ-04`, `ADR-0020`, `TASK-46`).
//!
//! Validates 1..=8 typed questions (`choice`, `yes_no`, `free_text`), enforces bounded wait
//! durations (`ASK-1`, `ASK-2`, `ASK-10`), guarantees defaults never answer `required: true`
//! asks (`ADR-0020` decision 14), and persists answers and withdrawals in canonical durable
//! `participants/<employee-id>/answers/<ask-id>.json` files projected on both SQLite and Memory.

use std::collections::{BTreeMap, BTreeSet};

use org_proto::ask::{
    AskAnswerRecord, AskDefaultEntry, AskOption, AskQuestion, AskQuestionType, AskStatus,
    AskWithdrawalRecord, ChatAnswerParams, ChatAnswerResult, ChatAskParams, ChatAskResult,
    ChatWaitAskResult, MsgWithdrawParams, MsgWithdrawResult, QuestionAnswer, ASK_OPTIONS_MAX,
    ASK_QUESTIONS_MAX, DEFAULT_ASK_WAIT_MS, MAX_ASK_WAIT_MS, REQUIRED_ASK_WIND_DOWN_NOTICE,
};
use org_proto::chat::{ConversationKind, DeliveredBecause};
use org_proto::msg::{
    json_string_len, MessageHeader, MessageLifecycleState, MessagePriority, KIND_REPLY,
    KIND_REQUEST, MAX_BODY_BYTES, MAX_ID_BYTES, MAX_META_BYTES, MAX_NOTE_BYTES, MAX_SUBJECT_BYTES,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Ack, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::{Deserialize, Serialize};

use crate::organization::ports::OrganizationRead;
use crate::organization::records::{Employee, EmployeeKind};
use crate::time;

use super::features::{
    append_send_recipient_states, build_mention_candidates, check_hr_access,
    effective_auto_members, is_active_employee, put_or_create_file, refusal,
    require_conversation_access, resolve_employee, resolve_send_target, to_wire_header,
    upsert_participant_snapshot,
};
use super::mentions::extract_mentions;
use super::ports::{MessagingRead, MessagingWrite};
use super::records::{
    self, ConversationMembership, MessageRecord, ParticipantAnswerFile, RecipientStateRecord,
    SenderKind, SCHEMA_VERSION,
};

/// Non-required asks remain answerable for 24 hours after creation (`ADR-0020` rules).
const NON_REQUIRED_EXPIRE_MS: u64 = 86_400_000;

/// Durable `metadata["card"]` shape stored on `kind = "request"` messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskCard {
    pub questions: Vec<AskQuestion>,
    #[serde(default)]
    pub required: bool,
    pub wait_ms: u64,
    pub wait_until: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addressed_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocks: Option<String>,
    #[serde(default)]
    pub approval: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<AskOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_option: Option<String>,
}

/// Durable envelope inside `ParticipantAnswerFile.answer`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "record", rename_all = "snake_case")]
enum AskOutcomeEnvelope {
    Answer(AskAnswerRecord),
    Withdrawal(AskWithdrawalRecord),
}

/// Decodes a structured ask answer or withdrawal from [`ParticipantAnswerFile`].
pub fn decode_ask_answer_file(
    file: &ParticipantAnswerFile,
) -> (Option<AskAnswerRecord>, Option<AskWithdrawalRecord>) {
    let Some(val) = &file.answer else {
        return (None, None);
    };
    match serde_json::from_value::<AskOutcomeEnvelope>(val.clone()) {
        Ok(AskOutcomeEnvelope::Answer(ans)) => (Some(ans), None),
        Ok(AskOutcomeEnvelope::Withdrawal(wd)) => (None, Some(wd)),
        Err(_) => (None, None),
    }
}

/// Parses `metadata["card"]` from a `request` message when present.
pub fn parse_ask_card(msg: &MessageRecord) -> Option<AskCard> {
    let val = msg.metadata.get("card")?;
    serde_json::from_value::<AskCard>(val.clone()).ok()
}

/// Committed output of `msg.ask`, carrying wire headers for live event & waiter notifications.
#[derive(Debug)]
pub struct AskCommitted {
    pub result: ChatAskResult,
    pub inserted: bool,
    pub card: AskCard,
    pub message: MessageRecord,
    pub recipients: Vec<(Id, RecipientStateRecord)>,
}

impl AskCommitted {
    pub fn delivered_headers(&self) -> Vec<MessageHeader> {
        self.recipients
            .iter()
            .map(|(_, st)| to_wire_header(&self.message, Some(st), self.result.local_seq))
            .collect()
    }
}

/// Committed output of `chat.answer`.
#[derive(Debug)]
pub struct AnswerCommitted {
    pub result: ChatAnswerResult,
    pub asker_id: Option<String>,
    pub question_summary: String,
    pub answer_summary: String,
    pub reply_header: Option<MessageHeader>,
    pub affected_members: Vec<String>,
}

/// Committed output of `msg.withdraw`.
#[derive(Debug)]
pub struct WithdrawCommitted {
    pub result: MsgWithdrawResult,
    pub asker_id: Option<String>,
    pub affected_members: Vec<String>,
}

fn validate_choice_options(q: &AskQuestion) -> Result<Vec<AskOption>, Refusal> {
    if q.options.is_empty() || q.options.len() > ASK_OPTIONS_MAX {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            format!(
                "question `{}` choice requires 1..={ASK_OPTIONS_MAX} options",
                q.id
            ),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut cleaned = Vec::with_capacity(q.options.len());
    for opt in &q.options {
        let oid = opt.id.trim();
        let label = opt.label.trim();
        if oid.is_empty() || oid.len() > 64 || label.is_empty() || !seen.insert(oid.to_owned()) {
            return Err(refusal(
                ErrorCode::InvalidRequest,
                format!(
                    "question `{}` options need unique non-empty ids and labels",
                    q.id
                ),
            ));
        }
        cleaned.push(AskOption {
            id: oid.to_owned(),
            label: label.to_owned(),
            description: opt
                .description
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        });
    }
    Ok(cleaned)
}

fn validate_one_question(q: &AskQuestion) -> Result<AskQuestion, Refusal> {
    let qid = q.id.trim();
    let prompt = q.prompt.trim();
    if qid.is_empty() || qid.len() > 64 || prompt.is_empty() {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "each question needs a non-empty id and prompt",
        ));
    }
    let options = match q.question_type {
        AskQuestionType::Choice => validate_choice_options(q)?,
        AskQuestionType::YesNo | AskQuestionType::FreeText => {
            if !q.options.is_empty() || q.allow_multiple {
                return Err(refusal(
                    ErrorCode::InvalidRequest,
                    format!("question `{qid}` takes no options or allow_multiple"),
                ));
            }
            Vec::new()
        }
        _ => {
            return Err(refusal(
                ErrorCode::InvalidRequest,
                format!("question `{qid}` type must be choice, yes_no, or free_text"),
            ));
        }
    };
    let default_value = match q
        .default_value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(def) => {
            let valid = match q.question_type {
                AskQuestionType::Choice => options.iter().any(|o| o.id == def),
                AskQuestionType::YesNo => matches!(def, "yes" | "no"),
                _ => false,
            };
            if !valid {
                return Err(refusal(
                    ErrorCode::InvalidRequest,
                    format!("question `{qid}` has an invalid default_value"),
                ));
            }
            Some(def.to_owned())
        }
    };
    Ok(AskQuestion {
        id: qid.to_owned(),
        question_type: q.question_type,
        prompt: prompt.to_owned(),
        options,
        allow_multiple: q.question_type == AskQuestionType::Choice && q.allow_multiple,
        default_value,
        optional: q.optional,
    })
}

pub fn validate_ask_questions(questions: &[AskQuestion]) -> Result<Vec<AskQuestion>, Refusal> {
    if questions.is_empty() || questions.len() > ASK_QUESTIONS_MAX {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            format!("an ask requires 1..={ASK_QUESTIONS_MAX} questions"),
        ));
    }
    let mut ids = BTreeSet::new();
    let mut out = Vec::with_capacity(questions.len());
    for q in questions {
        let cleaned = validate_one_question(q)?;
        if !ids.insert(cleaned.id.clone()) {
            return Err(refusal(
                ErrorCode::InvalidRequest,
                format!("duplicate question id `{}`", cleaned.id),
            ));
        }
        out.push(cleaned);
    }
    Ok(out)
}

fn validate_ask_inputs(params: &ChatAskParams) -> Result<(Vec<AskQuestion>, u64, String), Refusal> {
    if json_string_len(&params.id) > MAX_ID_BYTES
        || (org_proto::uuid_v7_time_ms(&params.id).is_none()
            && org_proto::ulid_time_ms(&params.id).is_none())
    {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "ask id must be a canonical lowercase UUIDv7",
        ));
    }
    let questions = validate_ask_questions(&params.questions)?;
    let wait_ms = params.wait_ms.unwrap_or(DEFAULT_ASK_WAIT_MS);
    if wait_ms == 0 || wait_ms > MAX_ASK_WAIT_MS {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            format!("wait_ms must be 1..={MAX_ASK_WAIT_MS}"),
        ));
    }
    let body = params
        .body
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| questions[0].prompt.clone());
    if json_string_len(&body) > MAX_BODY_BYTES {
        return Err(refusal(ErrorCode::TooLarge, "body exceeds 256 KiB limit"));
    }
    if params
        .subject
        .as_deref()
        .is_some_and(|s| json_string_len(s) > MAX_SUBJECT_BYTES)
    {
        return Err(refusal(ErrorCode::TooLarge, "subject exceeds size limit"));
    }
    let meta_bytes = serde_json::to_vec(&params.meta)
        .map_err(|_| refusal(ErrorCode::InvalidRequest, "invalid meta"))?
        .len();
    if meta_bytes > MAX_META_BYTES {
        return Err(refusal(ErrorCode::TooLarge, "meta exceeds 64 KiB limit"));
    }
    Ok((questions, wait_ms, body))
}

fn build_ask_card(
    questions: Vec<AskQuestion>,
    required: bool,
    wait_ms: u64,
    wait_until: String,
    addressed_to: Option<Id>,
    blocks: Option<String>,
    approval: bool,
) -> AskCard {
    let has_defaults = questions.iter().any(|q| q.default_value.is_some());
    // ADR-0020 decision 14: defaults NEVER stand in for a required ask.
    let default_at = (!required && has_defaults).then(|| wait_until.clone());
    let (legacy_opts, legacy_default) = if questions.len() == 1
        && questions[0].question_type == AskQuestionType::Choice
        && !questions[0].allow_multiple
    {
        (
            questions[0].options.clone(),
            questions[0].default_value.clone(),
        )
    } else {
        (Vec::new(), None)
    };
    AskCard {
        questions,
        required,
        wait_ms,
        wait_until,
        default_at,
        addressed_to: addressed_to.map(|id| id.as_str().to_owned()),
        blocks: normalize_blocks(blocks.as_deref()),
        approval,
        options: legacy_opts,
        default_option: legacy_default,
    }
}

fn normalize_blocks(blocks: Option<&str>) -> Option<String> {
    blocks
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn resolve_addressed_to(
    employees: &[Employee],
    owner: &Id,
    sender: &Employee,
    conv_kind: ConversationKind,
    active_members: &[Id],
    explicit: Option<&str>,
    mentioned_ids: &[Id],
) -> Result<Option<Id>, Refusal> {
    if let Some(raw) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        let emp =
            resolve_employee(employees, owner, raw).map_err(|e| refusal(e.code, e.message))?;
        if emp.id == sender.id {
            return Err(refusal(
                ErrorCode::InvalidRequest,
                "addressed_to cannot be the sender",
            ));
        }
        if !is_active_employee(emp) || !active_members.contains(&emp.id) {
            return Err(refusal(
                ErrorCode::UnknownRecipient,
                "addressed_to is not an active member of this conversation",
            ));
        }
        return Ok(Some(emp.id.clone()));
    }
    if conv_kind == ConversationKind::Dm {
        let other = active_members.iter().find(|id| **id != sender.id).cloned();
        return Ok(other);
    }
    let other_mentions: Vec<&Id> = mentioned_ids
        .iter()
        .filter(|id| **id != sender.id)
        .collect();
    if other_mentions.len() == 1 {
        return Ok(Some(other_mentions[0].clone()));
    }
    Ok(None)
}

fn defaults_for_card(card: &AskCard) -> Vec<AskDefaultEntry> {
    if card.required {
        return Vec::new();
    }
    card.questions
        .iter()
        .filter_map(|q| {
            q.default_value.as_ref().map(|def| AskDefaultEntry {
                question_id: q.id.clone(),
                default_value: def.clone(),
            })
        })
        .collect()
}

fn compute_wait_outcome(
    msg: &MessageRecord,
    card: &AskCard,
    answer_file: Option<&ParticipantAnswerFile>,
    now_ms: u64,
) -> ChatWaitAskResult {
    let (answer, withdrawal) = answer_file
        .map(decode_ask_answer_file)
        .unwrap_or((None, None));
    if let Some(ans) = answer {
        return ChatWaitAskResult {
            status: AskStatus::Answered,
            ask_id: msg.id.clone(),
            conversation_id: msg.conversation_id.as_str().to_owned(),
            wait_until: Some(card.wait_until.clone()),
            remaining_ms: Some(0),
            answer: Some(ans),
            withdrawal: None,
            defaults: Vec::new(),
            required_notice: None,
        };
    }
    if let Some(wd) = withdrawal {
        return ChatWaitAskResult {
            status: AskStatus::Withdrawn,
            ask_id: msg.id.clone(),
            conversation_id: msg.conversation_id.as_str().to_owned(),
            wait_until: Some(card.wait_until.clone()),
            remaining_ms: Some(0),
            answer: None,
            withdrawal: Some(wd),
            defaults: Vec::new(),
            required_notice: None,
        };
    }
    let wait_until_ms = time::parse_rfc3339_ms(&card.wait_until).unwrap_or(now_ms);
    if now_ms < wait_until_ms {
        return ChatWaitAskResult {
            status: AskStatus::Waiting,
            ask_id: msg.id.clone(),
            conversation_id: msg.conversation_id.as_str().to_owned(),
            wait_until: Some(card.wait_until.clone()),
            remaining_ms: Some(wait_until_ms.saturating_sub(now_ms)),
            answer: None,
            withdrawal: None,
            defaults: Vec::new(),
            required_notice: None,
        };
    }
    if card.required {
        ChatWaitAskResult {
            status: AskStatus::RequiredPending,
            ask_id: msg.id.clone(),
            conversation_id: msg.conversation_id.as_str().to_owned(),
            wait_until: Some(card.wait_until.clone()),
            remaining_ms: Some(0),
            answer: None,
            withdrawal: None,
            defaults: Vec::new(),
            required_notice: Some(REQUIRED_ASK_WIND_DOWN_NOTICE.to_owned()),
        }
    } else {
        ChatWaitAskResult {
            status: AskStatus::TimedOut,
            ask_id: msg.id.clone(),
            conversation_id: msg.conversation_id.as_str().to_owned(),
            wait_until: Some(card.wait_until.clone()),
            remaining_ms: Some(0),
            answer: None,
            withdrawal: None,
            defaults: defaults_for_card(card),
            required_notice: None,
        }
    }
}

fn non_card_meta(meta: &BTreeMap<String, serde_json::Value>) -> BTreeMap<&str, &serde_json::Value> {
    meta.iter()
        .filter(|(k, _)| k.as_str() != "card")
        .map(|(k, v)| (k.as_str(), v))
        .collect()
}

struct AskReplayExpected<'a> {
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

#[allow(clippy::too_many_arguments)]
pub fn decide_msg_ask<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &ChatAskParams,
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
        org_seq,
        now_ms,
        &now_utc,
    )
}

#[allow(clippy::too_many_arguments)]
fn commit_ask_record(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    conv: &mut super::records::ConversationRecord,
    conv_persisted: bool,
    conv_changed: bool,
    sender: &Employee,
    employees: &[Employee],
    params: &ChatAskParams,
    body: String,
    card: AskCard,
    reasons: BTreeMap<Id, DeliveredBecause>,
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
        attachments: Vec::new(),
        mailbox_content_hash: None,
    };
    record.content_sha256 = record.compute_content_sha256();

    let mut change = Change::new(Ack::Committed);
    let conv_path = records::conversation_path(&conv.id);
    if !conv_persisted {
        change = change.create(conv_path, records::encode_json(conv));
    } else if conv.auto_scope.is_some() && conv_changed {
        conv.revision = conv.revision.saturating_add(1);
        change = change.put(conv_path, records::encode_json(conv));
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

fn normalize_submitted_answers(
    card: &AskCard,
    params: &ChatAnswerParams,
) -> Result<Vec<QuestionAnswer>, Refusal> {
    if !params.answers.is_empty() {
        return Ok(params.answers.clone());
    }
    if card.questions.len() == 1 {
        let q = &card.questions[0];
        let option_ids = params.option_id.iter().cloned().collect::<Vec<_>>();
        let text = params
            .text
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if !option_ids.is_empty() || text.is_some() {
            return Ok(vec![QuestionAnswer {
                question_id: q.id.clone(),
                option_ids,
                text,
            }]);
        }
    }
    Err(refusal(
        ErrorCode::InvalidRequest,
        "at least one question answer is required",
    ))
}

fn validate_one_answer(q: &AskQuestion, ans: &QuestionAnswer) -> Result<QuestionAnswer, Refusal> {
    match q.question_type {
        AskQuestionType::Choice => {
            if ans.option_ids.is_empty() || (!q.allow_multiple && ans.option_ids.len() > 1) {
                return Err(refusal(
                    ErrorCode::InvalidRequest,
                    format!("question `{}` requires valid option selection", q.id),
                ));
            }
            let mut dedup = BTreeSet::new();
            let mut clean_ids = Vec::new();
            for oid in &ans.option_ids {
                if !q.options.iter().any(|o| o.id == *oid) {
                    return Err(refusal(
                        ErrorCode::InvalidRequest,
                        format!("unknown option `{oid}` for question `{}`", q.id),
                    ));
                }
                if dedup.insert(oid.clone()) {
                    clean_ids.push(oid.clone());
                }
            }
            let extra_text = ans
                .text
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            if extra_text
                .as_deref()
                .is_some_and(|t| json_string_len(t) > MAX_BODY_BYTES)
            {
                return Err(refusal(
                    ErrorCode::TooLarge,
                    "answer text exceeds size limit",
                ));
            }
            Ok(QuestionAnswer {
                question_id: q.id.clone(),
                option_ids: clean_ids,
                text: extra_text,
            })
        }
        AskQuestionType::YesNo => {
            if ans.option_ids.len() != 1 || !matches!(ans.option_ids[0].as_str(), "yes" | "no") {
                return Err(refusal(
                    ErrorCode::InvalidRequest,
                    format!("question `{}` requires `yes` or `no`", q.id),
                ));
            }
            Ok(QuestionAnswer {
                question_id: q.id.clone(),
                option_ids: ans.option_ids.clone(),
                text: None,
            })
        }
        AskQuestionType::FreeText => {
            let trimmed = ans.text.as_deref().map(str::trim).unwrap_or("");
            if trimmed.is_empty() || !ans.option_ids.is_empty() {
                return Err(refusal(
                    ErrorCode::InvalidRequest,
                    format!("question `{}` requires non-empty text", q.id),
                ));
            }
            if json_string_len(trimmed) > MAX_BODY_BYTES {
                return Err(refusal(
                    ErrorCode::TooLarge,
                    "answer text exceeds size limit",
                ));
            }
            Ok(QuestionAnswer {
                question_id: q.id.clone(),
                option_ids: Vec::new(),
                text: Some(trimmed.to_owned()),
            })
        }
        _ => Err(refusal(ErrorCode::InvalidRequest, "unsupported question")),
    }
}

fn validate_answer_set(
    card: &AskCard,
    submitted: Vec<QuestionAnswer>,
) -> Result<Vec<QuestionAnswer>, Refusal> {
    let by_id: BTreeMap<&str, &AskQuestion> =
        card.questions.iter().map(|q| (q.id.as_str(), q)).collect();
    let mut seen = BTreeSet::new();
    let mut validated = BTreeMap::new();
    for ans in submitted {
        let Some(&q) = by_id.get(ans.question_id.as_str()) else {
            return Err(refusal(
                ErrorCode::InvalidRequest,
                format!("unknown question_id `{}`", ans.question_id),
            ));
        };
        if !seen.insert(q.id.clone()) {
            return Err(refusal(
                ErrorCode::InvalidRequest,
                format!("duplicate answer for question `{}`", q.id),
            ));
        }
        validated.insert(q.id.clone(), validate_one_answer(q, &ans)?);
    }
    let mut ordered = Vec::new();
    for q in &card.questions {
        if let Some(a) = validated.remove(&q.id) {
            ordered.push(a);
        } else if !q.optional {
            return Err(refusal(
                ErrorCode::InvalidRequest,
                format!("question `{}` requires an answer", q.id),
            ));
        }
    }
    let answers_bytes = serde_json::to_vec(&ordered)
        .map_err(|_| refusal(ErrorCode::InvalidRequest, "answers cannot be serialized"))?;
    if answers_bytes.len() > MAX_META_BYTES {
        return Err(refusal(
            ErrorCode::TooLarge,
            "aggregate answer payload exceeds size limit",
        ));
    }
    Ok(ordered)
}

fn render_answer_summary(card: &AskCard, answers: &[QuestionAnswer]) -> String {
    let mut lines = Vec::with_capacity(answers.len());
    for ans in answers {
        let q = card.questions.iter().find(|q| q.id == ans.question_id);
        let value = if !ans.option_ids.is_empty() {
            let labels: Vec<String> = ans
                .option_ids
                .iter()
                .map(|oid| {
                    q.and_then(|q| q.options.iter().find(|o| o.id == *oid))
                        .map(|o| o.label.clone())
                        .unwrap_or_else(|| oid.clone())
                })
                .collect();
            match &ans.text {
                Some(t) => format!("{}: {t}", labels.join(", ")),
                None => labels.join(", "),
            }
        } else {
            ans.text.clone().unwrap_or_default()
        };
        if card.questions.len() == 1 {
            lines.push(value);
        } else {
            lines.push(format!("{}: {value}", ans.question_id));
        }
    }
    lines.join("\n")
}

#[allow(clippy::too_many_arguments)]
pub fn decide_chat_answer<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &ChatAnswerParams,
    reply_id: &str,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, AnswerCommitted), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let ask = uow
        .view()
        .message(&params.ask_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read ask"))?
        .filter(|m| m.kind == KIND_REQUEST)
        .ok_or_else(|| refusal(ErrorCode::NotFound, "ask not found"))?;
    let (conv, _) =
        require_conversation_access(uow.view(), owner, caller, &ask.conversation_id, now_utc)?;
    if conv.archived {
        return Err(refusal(ErrorCode::Conflict, "conversation is archived"));
    }
    let active_members = effective_auto_members(uow.view(), &conv, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to resolve members"))?;
    if !active_members.contains(caller) && caller != owner {
        return Err(refusal(
            ErrorCode::NotAuthorized,
            "caller is not an active member of this conversation",
        ));
    }
    let card = parse_ask_card(&ask)
        .ok_or_else(|| refusal(ErrorCode::InvalidRequest, "message is not a structured ask"))?;
    let employees = uow
        .view()
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    let responder = employees
        .iter()
        .find(|e| e.id == *caller && is_active_employee(e))
        .ok_or_else(|| refusal(ErrorCode::NotBound, "caller is not an active employee"))?;
    if card.approval && responder.kind != EmployeeKind::Person {
        return Err(refusal(
            ErrorCode::NotAuthorized,
            "only a person may answer an approval",
        ));
    }
    if ask.sender_employee_id.as_ref() == Some(caller) {
        return Err(refusal(
            ErrorCode::NotAuthorized,
            "the asker cannot answer their own ask",
        ));
    }
    if let Some(target) = &card.addressed_to {
        if target != caller.as_str() {
            return Err(refusal(
                ErrorCode::NotAuthorized,
                "only the addressed recipient may answer this ask",
            ));
        }
    }
    if let Some(existing) = uow
        .view()
        .answer_for_message(&ask.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read existing answer"))?
    {
        let (a, w) = decode_ask_answer_file(&existing);
        if a.is_some() || w.is_some() {
            return Err(refusal(ErrorCode::Conflict, "ask is already closed"));
        }
    }
    let raw_answers = normalize_submitted_answers(&card, params)?;
    let validated = validate_answer_set(&card, raw_answers)?;
    let answer_record = AskAnswerRecord {
        by: caller.as_str().to_owned(),
        at: now_utc.to_owned(),
        option_id: (validated.len() == 1 && validated[0].option_ids.len() == 1)
            .then(|| validated[0].option_ids[0].clone()),
        text: (validated.len() == 1)
            .then(|| validated[0].text.clone())
            .flatten(),
        answers: validated,
    };
    persist_answer_and_reply(
        uow,
        here,
        &ask,
        &card,
        responder,
        &employees,
        answer_record,
        reply_id,
        org_seq,
        now_utc,
    )
}

fn mark_ask_recipients_done(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    ask: &MessageRecord,
    org_seq: u64,
    now_utc: &str,
    mut change: Change,
) -> Result<(Change, Vec<String>), Refusal> {
    let mut affected = Vec::new();
    let all_states = read
        .all_recipient_states()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read recipient states"))?;
    for mut st in all_states {
        if st.message_id == ask.id && st.state != MessageLifecycleState::Done {
            st.state = MessageLifecycleState::Done;
            st.updated_at = now_utc.to_owned();
            st.org_sequence = org_seq;
            let emp_id = st.employee_id.clone();
            affected.push(emp_id.as_str().to_owned());
            change = upsert_participant_snapshot(
                read,
                &here.organization_id,
                &ask.conversation_id,
                &emp_id,
                st,
                change,
            )?;
        }
    }
    Ok((change, affected))
}

#[allow(clippy::too_many_arguments)]
fn persist_answer_and_reply<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    ask: &MessageRecord,
    card: &AskCard,
    responder: &Employee,
    employees: &[Employee],
    answer_record: AskAnswerRecord,
    reply_id: &str,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, AnswerCommitted), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let envelope = AskOutcomeEnvelope::Answer(answer_record.clone());
    let answer_file = ParticipantAnswerFile {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: here.organization_id.clone(),
        conversation_id: ask.conversation_id.clone(),
        employee_id: responder.id.clone(),
        message_id: ask.id.clone(),
        accepted_by: None,
        accepted_at: None,
        reason: None,
        answer: Some(serde_json::to_value(&envelope).expect("envelope serializes")),
    };
    let ans_path = records::participant_answer_path(&ask.conversation_id, &responder.id, &ask.id);
    let change = put_or_create_file(
        uow,
        Change::new(Ack::Committed),
        ans_path,
        records::encode_json(&answer_file),
    )?;
    let (mut change, mut affected) =
        mark_ask_recipients_done(uow.view(), here, ask, org_seq, now_utc, change)?;
    if !affected.iter().any(|id| id == responder.id.as_str()) {
        affected.push(responder.id.as_str().to_owned());
    }

    let mut reply_header = None;
    if let Some(asker_id) = ask
        .sender_employee_id
        .as_ref()
        .filter(|id| **id != responder.id)
    {
        let (next_change, hdr) = append_answer_reply_message(
            uow.view(),
            here,
            ask,
            card,
            responder,
            asker_id,
            employees,
            &answer_record,
            reply_id,
            org_seq,
            now_utc,
            change,
        )?;
        change = next_change;
        reply_header = Some(hdr);
        affected.push(asker_id.as_str().to_owned());
    }
    let question_summary = card
        .questions
        .iter()
        .map(|q| q.prompt.as_str())
        .collect::<Vec<_>>()
        .join(" / ");
    let answer_summary = render_answer_summary(card, &answer_record.answers);
    Ok((
        change,
        AnswerCommitted {
            result: ChatAnswerResult {
                ask_id: ask.id.clone(),
                conversation_id: ask.conversation_id.as_str().to_owned(),
                answer: answer_record,
            },
            asker_id: ask
                .sender_employee_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            question_summary,
            answer_summary,
            reply_header,
            affected_members: affected,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn append_answer_reply_message(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    ask: &MessageRecord,
    card: &AskCard,
    responder: &Employee,
    asker_id: &Id,
    employees: &[Employee],
    answer_record: &AskAnswerRecord,
    reply_id: &str,
    org_seq: u64,
    now_utc: &str,
    mut change: Change,
) -> Result<(Change, MessageHeader), Refusal> {
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
    let mut meta = BTreeMap::new();
    meta.insert(
        "answers".into(),
        serde_json::to_value(&answer_record.answers).expect("answers serialize"),
    );
    let mut reply = MessageRecord {
        schema_version: SCHEMA_VERSION,
        organization_id: here.organization_id.clone(),
        id: reply_id.to_owned(),
        conversation_id: ask.conversation_id.clone(),
        sender_kind: SenderKind::Employee,
        sender_employee_id: Some(responder.id.clone()),
        sender_office_id: None,
        sender_display_name: responder.name.clone(),
        kind: KIND_REPLY.to_owned(),
        priority: MessagePriority::Normal,
        subject: ask.subject.clone(),
        body_markdown: render_answer_summary(card, &answer_record.answers),
        metadata: meta,
        thread_root_id: Some(ask.thread_root_id.clone().unwrap_or_else(|| ask.id.clone())),
        reply_to_message_id: Some(ask.id.clone()),
        also_in_channel: false,
        created_at: now_utc.to_owned(),
        expires_at: None,
        ttl_ms: None,
        origin_office_id: here.office_id.clone(),
        origin_incarnation: origin_incarnation.to_owned(),
        origin_sequence: origin_seq,
        org_epoch: 1,
        org_sequence: org_seq,
        content_sha256: String::new(),
        attachments: Vec::new(),
        mailbox_content_hash: None,
    };
    reply.content_sha256 = reply.compute_content_sha256();
    change = change.create(
        records::message_path(&ask.conversation_id, &reply.id),
        records::encode_json(&reply),
    );
    let mut reasons = BTreeMap::new();
    reasons.insert(asker_id.clone(), DeliveredBecause::Addressed);
    let (change, states) = append_send_recipient_states(
        read,
        here,
        &ask.conversation_id,
        &reply,
        employees,
        reasons,
        org_seq,
        now_utc,
        change,
    )?;
    let hdr = to_wire_header(&reply, states.first().map(|(_, s)| s), local_seq);
    Ok((change, hdr))
}

#[allow(clippy::too_many_arguments)]
pub fn decide_msg_withdraw<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &MsgWithdrawParams,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, WithdrawCommitted), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    if params
        .reason
        .as_deref()
        .is_some_and(|r| json_string_len(r) > MAX_NOTE_BYTES)
    {
        return Err(refusal(ErrorCode::TooLarge, "withdrawal reason too long"));
    }
    let employees = uow
        .view()
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    if !employees
        .iter()
        .any(|e| e.id == *caller && is_active_employee(e))
    {
        return Err(refusal(
            ErrorCode::NotBound,
            "caller is not an active employee",
        ));
    }
    let ask = uow
        .view()
        .message(&params.ask_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read ask"))?
        .filter(|m| m.kind == KIND_REQUEST)
        .ok_or_else(|| refusal(ErrorCode::NotFound, "ask not found"))?;
    if parse_ask_card(&ask).is_none() {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "message is not a structured ask",
        ));
    }
    let is_asker = ask.sender_employee_id.as_ref() == Some(caller);
    if !is_asker && caller != owner {
        return Err(refusal(
            ErrorCode::NotAuthorized,
            "only the asker or organization owner may withdraw this ask",
        ));
    }
    if let Some(existing) = uow
        .view()
        .answer_for_message(&ask.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read existing answer"))?
    {
        let (a, w) = decode_ask_answer_file(&existing);
        if a.is_some() || w.is_some() {
            return Err(refusal(ErrorCode::Conflict, "ask is already closed"));
        }
    }
    let withdrawal = AskWithdrawalRecord {
        by: caller.as_str().to_owned(),
        at: now_utc.to_owned(),
        reason: params
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    };
    let envelope = AskOutcomeEnvelope::Withdrawal(withdrawal.clone());
    let file = ParticipantAnswerFile {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: here.organization_id.clone(),
        conversation_id: ask.conversation_id.clone(),
        employee_id: caller.clone(),
        message_id: ask.id.clone(),
        accepted_by: None,
        accepted_at: None,
        reason: withdrawal.reason.clone(),
        answer: Some(serde_json::to_value(&envelope).expect("envelope serializes")),
    };
    let path = records::participant_answer_path(&ask.conversation_id, caller, &ask.id);
    let change = put_or_create_file(
        uow,
        Change::new(Ack::Committed),
        path,
        records::encode_json(&file),
    )?;
    let (change, affected) =
        mark_ask_recipients_done(uow.view(), here, &ask, org_seq, now_utc, change)?;
    Ok((
        change,
        WithdrawCommitted {
            result: MsgWithdrawResult {
                ask_id: ask.id,
                conversation_id: ask.conversation_id.as_str().to_owned(),
                withdrawal,
            },
            asker_id: ask
                .sender_employee_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            affected_members: affected,
        },
    ))
}

/// Evaluates the current status of `ask_id` at `now_ms` without mutating state.
pub fn inspect_ask_status(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    ask_id: &str,
    now_ms: u64,
) -> Result<ChatWaitAskResult, ApiError> {
    let now_utc = time::format_rfc3339_ms(now_ms);
    let ask = read
        .message(ask_id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read ask"))?
        .filter(|m| m.kind == KIND_REQUEST)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "ask not found"))?;
    require_conversation_access(read, owner, caller, &ask.conversation_id, &now_utc)
        .map_err(|r| super::features::decode_refusal(&r))?;
    let card = parse_ask_card(&ask).ok_or_else(|| {
        ApiError::new(ErrorCode::InvalidRequest, "message is not a structured ask")
    })?;
    let answer_file = read
        .answer_for_message(&ask.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read answer"))?;
    Ok(compute_wait_outcome(
        &ask,
        &card,
        answer_file.as_ref(),
        now_ms,
    ))
}

/// Checks whether a message has an open (unanswered, unwithdrawn) structured ask card.
pub fn is_open_ask_card(read: &impl MessagingRead, msg: &MessageRecord) -> bool {
    if msg.kind != KIND_REQUEST || parse_ask_card(msg).is_none() {
        return false;
    }
    match read.answer_for_message(&msg.id) {
        Ok(Some(file)) => {
            let (a, w) = decode_ask_answer_file(&file);
            a.is_none() && w.is_none()
        }
        Ok(None) => true,
        Err(_) => false,
    }
}
