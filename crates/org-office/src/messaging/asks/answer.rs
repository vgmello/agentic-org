//! Answering an ask: answer validation and summary, recipient states, and the reply message
//! that carries the answer.

use std::collections::{BTreeMap, BTreeSet};

use org_proto::ask::{
    AskAnswerRecord, AskQuestion, AskQuestionType, ChatAnswerParams, ChatAnswerResult,
    QuestionAnswer,
};
use org_proto::chat::DeliveredBecause;
use org_proto::msg::{
    json_string_len, MessageHeader, MessageLifecycleState, MessagePriority, KIND_REPLY,
    KIND_REQUEST, MAX_BODY_BYTES, MAX_META_BYTES,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::messaging::features::{
    append_send_recipient_states, effective_auto_members, is_active_employee, put_or_create_file,
    refusal, require_conversation_access, to_wire_header, upsert_participant_snapshot,
};
use crate::messaging::files::{self, Binding, MessageFiles, ANSWER_SCOPE, MESSAGE_SCOPE};
use crate::messaging::ports::{MessagingRead, MessagingWrite};
use crate::messaging::records::{
    self, MessageAttachmentRecord, MessageRecord, ParticipantAnswerFile, SenderKind, SCHEMA_VERSION,
};
use crate::organization::ports::OrganizationRead;
use crate::organization::records::{Employee, EmployeeKind};

use super::{decode_ask_answer_file, parse_ask_card, AnswerCommitted, AskCard, AskOutcomeEnvelope};

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

pub(crate) fn validate_answer_set(
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

pub(crate) fn render_answer_summary(card: &AskCard, answers: &[QuestionAnswer]) -> String {
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

/// The files of one answer: on the participant's answer file and on the reply message.
struct AnswerFiles {
    answer: Vec<MessageAttachmentRecord>,
    reply: Vec<MessageAttachmentRecord>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "answer command decision takes explicit transaction, identity, caller, params, files, and clock inputs"
)]
pub fn decide_chat_answer<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &ChatAnswerParams,
    files: &MessageFiles,
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
    org_proto::attachments::check_attachment_ids(&params.attachment_ids)
        .map_err(|error| refusal(error.code, error.message))?;
    if !files.is_empty() && Id::parse(&ask.id).is_err() {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "files go only on an answer to an ask with a canonical UUIDv7 id",
        ));
    }
    let binding = |scope: &'static str, key: &str| -> Result<_, Refusal> {
        files.bind(&Binding {
            organization_id: &here.organization_id,
            conversation: &conv,
            members: &active_members,
            scope,
            key,
            created_at: now_utc,
        })
    };
    let answer_files = AnswerFiles {
        answer: binding(ANSWER_SCOPE, &format!("{}:{}", ask.id, responder.id))?,
        reply: binding(MESSAGE_SCOPE, reply_id)?,
    };
    let answer_record = AskAnswerRecord {
        by: caller.as_str().to_owned(),
        at: now_utc.to_owned(),
        option_id: (validated.len() == 1 && validated[0].option_ids.len() == 1)
            .then(|| validated[0].option_ids[0].clone()),
        text: (validated.len() == 1)
            .then(|| validated[0].text.clone())
            .flatten(),
        answers: validated,
        attachments: Vec::new(),
    };
    persist_answer_and_reply(
        uow,
        here,
        &ask,
        &card,
        responder,
        &employees,
        answer_record,
        answer_files,
        reply_id,
        org_seq,
        now_utc,
    )
}

pub(crate) fn mark_ask_recipients_done(
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
            st.snoozed_until = None;
            st.snooze_after_task = false;
            st.snooze_note = None;
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

#[expect(
    clippy::too_many_arguments,
    reason = "answer persistence updates ask card, responder reply, and recipient states in one unit of work"
)]
fn persist_answer_and_reply<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    ask: &MessageRecord,
    card: &AskCard,
    responder: &Employee,
    employees: &[Employee],
    mut answer_record: AskAnswerRecord,
    answer_files: AnswerFiles,
    reply_id: &str,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, AnswerCommitted), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    // The answer file keeps its files beside the envelope, never inside it.
    let envelope = AskOutcomeEnvelope::Answer(answer_record.clone());
    answer_record.attachments = files::metas(&answer_files.answer, now_utc, Some(&responder.id));
    let answer_file = ParticipantAnswerFile {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: here.organization_id.clone(),
        conversation_id: ask.conversation_id.clone(),
        employee_id: responder.id.clone(),
        message_id: ask.id.clone(),
        accepted_by: Some(responder.id.clone()),
        accepted_at: Some(now_utc.to_owned()),
        reason: (ask.origin_office_id == here.office_id).then(|| "origin-close".to_owned()),
        answer: Some(serde_json::to_value(&envelope).expect("envelope serializes")),
        attachments: answer_files.answer,
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
            answer_files.reply,
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

#[expect(
    clippy::too_many_arguments,
    reason = "reply append helper takes explicit read view, ask context, responder, body, and timestamps"
)]
fn append_answer_reply_message(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    ask: &MessageRecord,
    card: &AskCard,
    responder: &Employee,
    asker_id: &Id,
    employees: &[Employee],
    answer_record: &AskAnswerRecord,
    attachments: Vec<MessageAttachmentRecord>,
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
        attachments,
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
