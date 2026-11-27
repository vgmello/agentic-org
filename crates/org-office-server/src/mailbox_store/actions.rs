//! The inline actions of an owner Inbox item made from a message (`TASK-98.2`): Reply to a
//! message, Reply to a one-question free-text ask, and Approve or Deny an approval. Reply and
//! Deny take text and up to ten of the owner's committed staged uploads (`attachment_ids`).

use org_office::messaging::asks::AskCard;
use org_proto::ask::{AskQuestionType, ChatAnswer};
use org_proto::attachments::MESSAGE_ATTACHMENTS_MAX;
use org_proto::inbox::{Action, ActionInput};
use org_proto::msg::{MsgReply, MAX_BODY_BYTES};
use org_proto::Call;
use serde_json::{Map, Value};

/// The params field an inline action takes its files in.
const ATTACHMENTS_FIELD: &str = "attachment_ids";
/// Characters an inline text holds: even fully escaped, they fit the 256 KiB body limit.
const INLINE_TEXT_MAX_CHARS: u32 = (MAX_BODY_BYTES / 6) as u32;
/// Option IDs an approval's single choice question says yes or no with.
const APPROVE_IDS: [&str; 4] = ["approve", "approved", "accept", "yes"];
const DENY_IDS: [&str; 6] = ["deny", "denied", "reject", "rejected", "decline", "no"];

fn text_with_files(field: &str, required: bool) -> ActionInput {
    ActionInput {
        field: field.into(),
        required,
        max_chars: INLINE_TEXT_MAX_CHARS,
        attachments_field: Some(ATTACHMENTS_FIELD.into()),
        max_attachments: MESSAGE_ATTACHMENTS_MAX as u32,
    }
}

fn call(
    label: &str,
    op: &str,
    params: Map<String, Value>,
    fresh: &[&str],
    input: Option<ActionInput>,
) -> Action {
    Action::Call {
        label: label.into(),
        op: op.into(),
        params,
        fresh: fresh.iter().map(|f| (*f).to_owned()).collect(),
        input,
    }
}

/// Reply to a message, in its thread: text, files, or both.
pub(super) fn reply(message_id: &str) -> Action {
    let mut params = Map::new();
    params.insert("to_id".into(), Value::String(message_id.to_owned()));
    call(
        "Reply",
        MsgReply::METHOD,
        params,
        &["id"],
        Some(text_with_files("body", false)),
    )
}

fn answer(ask_id: &str, option_id: Option<&str>) -> Map<String, Value> {
    let mut params = Map::new();
    params.insert("ask_id".into(), Value::String(ask_id.to_owned()));
    if let Some(option) = option_id {
        params.insert("option_id".into(), Value::String(option.to_owned()));
    }
    params
}

/// The inline answers of an open ask: Approve and Deny for an approval, Reply for a single
/// free-text question; none for anything else (it opens).
pub(super) fn ask(ask_id: &str, card: &AskCard) -> Vec<Action> {
    let [question] = card.questions.as_slice() else {
        return Vec::new();
    };
    if !card.approval {
        if question.question_type != AskQuestionType::FreeText {
            return Vec::new();
        }
        return vec![call(
            "Reply",
            ChatAnswer::METHOD,
            answer(ask_id, None),
            &[],
            Some(text_with_files("text", true)),
        )];
    }
    let (approve, deny, takes_text) = match question.question_type {
        AskQuestionType::YesNo => (Some("yes"), Some("no"), false),
        AskQuestionType::Choice => {
            let find = |ids: &[&str]| {
                question
                    .options
                    .iter()
                    .find(|o| ids.contains(&o.id.as_str()))
                    .map(|o| o.id.as_str())
            };
            (find(&APPROVE_IDS), find(&DENY_IDS), true)
        }
        _ => (None, None, false),
    };
    let mut actions = Vec::new();
    if let Some(option) = approve {
        actions.push(call(
            "Approve",
            ChatAnswer::METHOD,
            answer(ask_id, Some(option)),
            &[],
            None,
        ));
    }
    if let Some(option) = deny {
        // A yes/no answer keeps no text, so only a choice's Deny says why, with files.
        let input = takes_text.then(|| text_with_files("text", false));
        actions.push(call(
            "Deny",
            ChatAnswer::METHOD,
            answer(ask_id, Some(option)),
            &[],
            input,
        ));
    }
    actions
}
