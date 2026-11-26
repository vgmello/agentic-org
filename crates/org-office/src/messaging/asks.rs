//! Engine-independent domain decisions for B7 structured asks (`REQ-04`, `ADR-0020`, `TASK-46`).
//!
//! Validates 1..=8 typed questions (`choice`, `yes_no`, `free_text`), enforces bounded wait
//! durations (`ASK-1`, `ASK-2`, `ASK-10`), guarantees defaults never answer `required: true`
//! asks (`ADR-0020` decision 14), and persists answers and withdrawals in canonical durable
//! `participants/<employee-id>/answers/<ask-id>.json` files projected on both SQLite and Memory.

use org_proto::ask::{
    AskAnswerRecord, AskOption, AskQuestion, AskWithdrawalRecord, ChatAnswerResult, ChatAskResult,
    ChatWaitAskResult, MsgWithdrawResult,
};
use org_proto::msg::{MessageHeader, KIND_REQUEST};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;
use serde::{Deserialize, Serialize};

use crate::time;

use super::features::{require_conversation_access, to_wire_header};
use super::ports::MessagingRead;
use super::records::{MessageRecord, ParticipantAnswerFile, RecipientStateRecord};

mod answer;
mod ask;
mod card;
mod questions;
mod withdraw;

pub use answer::decide_chat_answer;
pub(crate) use answer::{mark_ask_recipients_done, render_answer_summary, validate_answer_set};
pub use ask::decide_msg_ask;
use card::compute_wait_outcome;
pub use questions::validate_ask_questions;
pub use withdraw::decide_msg_withdraw;

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
