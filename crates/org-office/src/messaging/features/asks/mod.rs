//! Engine-independent domain decisions for B7 structured asks (`REQ-04`, `ADR-0020`, `TASK-46`).
//!
//! Validates 1..=8 typed questions (`choice`, `yes_no`, `free_text`), enforces bounded wait
//! durations (`ASK-1`, `ASK-2`, `ASK-10`), guarantees defaults never answer `required: true`
//! asks (`ADR-0020` decision 14), and persists answers and withdrawals in canonical durable
//! `participants/<employee-id>/answers/<ask-id>.json` files projected on both SQLite and Memory.

use org_proto::ask::{ChatAnswerResult, ChatAskResult, ChatWaitAskResult, MsgWithdrawResult};
use org_proto::msg::{MessageHeader, KIND_REQUEST};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use crate::time;

use crate::messaging::ask_card::AskOutcomeEnvelope;
pub use crate::messaging::ask_card::{
    decode_ask_answer_file, is_open_ask_card, parse_ask_card, AskCard,
};
use crate::messaging::ports::MessagingRead;
use crate::messaging::records::{MessageRecord, RecipientStateRecord};

use super::{decode_refusal, require_conversation_access, to_wire_header};

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
        .map_err(|r| decode_refusal(&r))?;
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
