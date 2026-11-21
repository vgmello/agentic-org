//! B7 Structured asks, 1-to-8 typed questions, answers, withdrawals, and short-hold wait
//! (`REQ-04`, `ADR-0020`, `TASK-46`).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::calls::Call;
use crate::msg::Meta;

/// Maximum questions in a single structured ask (`REQ-04`, `doc-90`).
pub const ASK_QUESTIONS_MAX: usize = 8;
/// Maximum options in a single `choice` question (`REQ-04`).
pub const ASK_OPTIONS_MAX: usize = 6;
/// Default bounded wait duration for an ask (`ADR-0020` decision 1: 10 minutes).
pub const DEFAULT_ASK_WAIT_MS: u64 = 600_000;
/// Maximum stated wait duration an asker may request (`ASK-1`: 60 minutes).
pub const MAX_ASK_WAIT_MS: u64 = 3_600_000;
/// Maximum single-command short hold (`ADR-0020` decision 15 & `ASK-10`: 90 seconds).
pub const MAX_SHORT_HOLD_MS: u64 = 90_000;
/// Notice returned when a required ask's bounded wait expires (`ADR-0020` decision 5).
pub const REQUIRED_ASK_WIND_DOWN_NOTICE: &str = "The required answer has not arrived; finish or stop any background work you started, then end your turn; you will be started again when the answer arrives.";

/// Question component type (`REQ-04`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AskQuestionType {
    Choice,
    YesNo,
    FreeText,
    #[serde(other)]
    Unknown,
}

/// One selectable option for a `choice` question (`REQ-04`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One question inside a structured ask (`REQ-04`, 1..=8 per ask).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskQuestion {
    pub id: String,
    pub question_type: AskQuestionType,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<AskOption>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_multiple: bool,
    #[serde(
        default,
        alias = "default_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_value: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

/// Answer to one question in a structured ask (`REQ-04`).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionAnswer {
    pub question_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub option_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl fmt::Debug for QuestionAnswer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuestionAnswer")
            .field("question_id", &self.question_id)
            .field("option_ids", &self.option_ids)
            .field("has_text", &self.text.is_some())
            .finish()
    }
}

/// Recorded answer to a structured ask (`doc-90` / `REQ-04`).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskAnswerRecord {
    pub by: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<QuestionAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl fmt::Debug for AskAnswerRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AskAnswerRecord")
            .field("by", &self.by)
            .field("at", &self.at)
            .field("answers_len", &self.answers.len())
            .field("option_id", &self.option_id)
            .field("has_text", &self.text.is_some())
            .finish()
    }
}

/// Recorded withdrawal of an open ask (`ADR-0020` decision 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskWithdrawalRecord {
    pub by: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Outcome of an ask or `msg wait --ask` call (`ADR-0020` decisions 9 & 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AskStatus {
    Answered,
    Waiting,
    TimedOut,
    RequiredPending,
    Withdrawn,
    #[serde(other)]
    Unknown,
}

/// Default value returned alongside `AskStatus::TimedOut` for a non-required ask (`ADR-0020` decision 14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskDefaultEntry {
    pub question_id: String,
    pub default_value: String,
}

/// `chat.ask`: send a validated 1..=8 question structured ask and hold for at most one short hold.
pub struct ChatAsk;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAskParams {
    pub id: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub questions: Vec<AskQuestion>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocks: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addressed_to: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub meta: Meta,
}

impl fmt::Debug for ChatAskParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatAskParams")
            .field("id", &self.id)
            .field("to", &self.to)
            .field("questions_len", &self.questions.len())
            .field("required", &self.required)
            .field("wait_ms", &self.wait_ms)
            .field("hold_ms", &self.hold_ms)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAskResult {
    pub status: AskStatus,
    pub ask_id: String,
    pub conversation_id: String,
    pub thread_id: String,
    pub local_seq: u64,
    pub origin_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<AskAnswerRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawal: Option<AskWithdrawalRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defaults: Vec<AskDefaultEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_notice: Option<String>,
}

impl Call for ChatAsk {
    const METHOD: &'static str = "chat.ask";
    type Params = ChatAskParams;
    type Result = ChatAskResult;
}

/// `chat.answer`: submit one complete answer set to an open structured ask.
pub struct ChatAnswer;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAnswerParams {
    pub ask_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<QuestionAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl fmt::Debug for ChatAnswerParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatAnswerParams")
            .field("ask_id", &self.ask_id)
            .field("answers_len", &self.answers.len())
            .field("option_id", &self.option_id)
            .field("has_text", &self.text.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAnswerResult {
    pub ask_id: String,
    pub conversation_id: String,
    pub answer: AskAnswerRecord,
}

impl Call for ChatAnswer {
    const METHOD: &'static str = "chat.answer";
    type Params = ChatAnswerParams;
    type Result = ChatAnswerResult;
}

/// `msg.withdraw`: withdraw an open structured ask.
pub struct MsgWithdraw;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgWithdrawParams {
    pub ask_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgWithdrawResult {
    pub ask_id: String,
    pub conversation_id: String,
    pub withdrawal: AskWithdrawalRecord,
}

impl Call for MsgWithdraw {
    const METHOD: &'static str = "msg.withdraw";
    type Params = MsgWithdrawParams;
    type Result = MsgWithdrawResult;
}

/// `chat.wait_ask`: wait for up to one short hold (<= 90s) for an answer, withdrawal, or timeout.
pub struct ChatWaitAsk;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatWaitAskParams {
    pub ask_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatWaitAskResult {
    pub status: AskStatus,
    pub ask_id: String,
    pub conversation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<AskAnswerRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawal: Option<AskWithdrawalRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defaults: Vec<AskDefaultEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_notice: Option<String>,
}

impl Call for ChatWaitAsk {
    const METHOD: &'static str = "chat.wait_ask";
    type Params = ChatWaitAskParams;
    type Result = ChatWaitAskResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_wire_shapes_round_trip_and_redact_text_in_debug() {
        let q = AskQuestion {
            id: "env".into(),
            question_type: AskQuestionType::Choice,
            prompt: "Which environment?".into(),
            options: vec![
                AskOption {
                    id: "dev".into(),
                    label: "Development".into(),
                    description: Some("Staging".into()),
                },
                AskOption {
                    id: "prod".into(),
                    label: "Production".into(),
                    description: None,
                },
            ],
            allow_multiple: false,
            default_value: Some("dev".into()),
            optional: false,
        };
        let params = ChatAskParams {
            id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a0".into(),
            to: "owner".into(),
            body: Some("SECRET-PROMPT-BODY".into()),
            subject: None,
            questions: vec![q],
            required: true,
            wait_ms: Some(120_000),
            hold_ms: Some(5_000),
            blocks: Some("T-42".into()),
            approval: false,
            addressed_to: None,
            meta: Meta::new(),
        };
        assert!(!format!("{params:?}").contains("SECRET-PROMPT-BODY"));
        let decoded: ChatAskParams =
            serde_json::from_value(serde_json::to_value(&params).unwrap()).unwrap();
        assert_eq!(decoded, params);

        let ans = ChatAnswerParams {
            ask_id: params.id.clone(),
            answers: vec![QuestionAnswer {
                question_id: "env".into(),
                option_ids: vec!["dev".into()],
                text: Some("SECRET-ANSWER-TEXT".into()),
            }],
            option_id: None,
            text: None,
        };
        assert!(!format!("{ans:?}").contains("SECRET-ANSWER-TEXT"));
    }
}
