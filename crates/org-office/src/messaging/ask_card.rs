//! The durable shapes of a structured ask (`ADR-0020`, `TASK-46`): the card a `request` message
//! carries in `metadata["card"]`, and the answer or withdrawal envelope a participant's
//! `answers/<ask-id>.json` file holds. The ask decisions (`super::asks`) and the message reads
//! and sends (`super::features`) both read them; neither owns them.

use org_proto::ask::{AskAnswerRecord, AskOption, AskQuestion, AskWithdrawalRecord};
use org_proto::msg::KIND_REQUEST;
use serde::{Deserialize, Serialize};

use super::ports::MessagingRead;
use super::records::{MessageRecord, ParticipantAnswerFile};

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
pub(crate) enum AskOutcomeEnvelope {
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
        Ok(AskOutcomeEnvelope::Answer(mut ans)) => {
            // The answer's files sit beside the envelope in the answer file (`TASK-98.2`).
            if ans.attachments.is_empty() {
                ans.attachments =
                    super::files::metas(&file.attachments, &ans.at, Some(&file.employee_id));
            }
            (Some(ans), None)
        }
        Ok(AskOutcomeEnvelope::Withdrawal(wd)) => (None, Some(wd)),
        Err(_) => (None, None),
    }
}

/// Parses `metadata["card"]` from a `request` message when present.
pub fn parse_ask_card(msg: &MessageRecord) -> Option<AskCard> {
    let val = msg.metadata.get("card")?;
    serde_json::from_value::<AskCard>(val.clone()).ok()
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
