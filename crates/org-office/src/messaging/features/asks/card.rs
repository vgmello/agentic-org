//! Building an ask card from validated inputs: addressing, defaults and the bounded wait
//! outcome.

use org_proto::ask::{
    AskDefaultEntry, AskQuestion, AskQuestionType, AskStatus, ChatAskParams, ChatWaitAskResult,
    DEFAULT_ASK_WAIT_MS, MAX_ASK_WAIT_MS, REQUIRED_ASK_WIND_DOWN_NOTICE,
};
use org_proto::chat::ConversationKind;
use org_proto::msg::{
    json_string_len, MAX_BODY_BYTES, MAX_ID_BYTES, MAX_META_BYTES, MAX_SUBJECT_BYTES,
};
use org_proto::ErrorCode;
use org_store::{Id, Refusal};

use crate::messaging::features::{is_addressable_employee, refusal, resolve_employee};
use crate::messaging::records::{MessageRecord, ParticipantAnswerFile};
use crate::organization::records::Employee;
use crate::time;

use super::questions::validate_ask_questions;
use super::{decode_ask_answer_file, AskCard};

pub(super) fn validate_ask_inputs(
    params: &ChatAskParams,
) -> Result<(Vec<AskQuestion>, u64, String), Refusal> {
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
    crate::messaging::files::check_message_files(&params.id, &body, &params.attachment_ids)?;
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

pub(super) fn build_ask_card(
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

pub(super) fn normalize_blocks(blocks: Option<&str>) -> Option<String> {
    blocks
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub(super) fn resolve_addressed_to(
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
        if !is_addressable_employee(emp) || !active_members.contains(&emp.id) {
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

pub(super) fn compute_wait_outcome(
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
