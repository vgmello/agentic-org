//! One tool-less `channel.watch` pass and the validation of its matches.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};

use super::features::watcher_registry;
use super::inputs::{filter_watch_messages, sanitize_questions, trim_watch_inputs_to_budget};
use super::{
    map_copilot_error, CaptureLog, WatchError, WatchMatch, WatchMessageInput, WatchPassInput,
    WatchPassOutcome, WatchPassResult, WatchQuestionInput, MAX_WATCH_CONTEXT_MESSAGES,
    MAX_WATCH_NEW_MESSAGES_PER_PASS,
};
use crate::contract::DraftRequest;
use crate::limits::Limits;
use crate::runtime::CopilotRuntime;
use crate::service::{Copilot, CopilotError};

/// Runs one tool-less `channel.watch` pass (`WATCH-3`, `WATCH-4`).
///
/// - Secret-shaped messages are skipped without stopping the pass.
/// - Any returned ID outside supplied `new_messages` / `open_questions` fails the entire pass
///   (`WatchError::InvalidReference`) and delivers nothing.
/// - Matches where `message.author == question.asker` are dropped (`WATCH-3`).
/// - Measured token usage is returned even when the pass fails (`ADR-0024` d15).
pub fn run_watch_pass<R: CopilotRuntime>(
    runtime: R,
    limits: Limits,
    scope_key: &str,
    input: &WatchPassInput,
) -> WatchPassOutcome {
    let started = Instant::now();
    let questions = sanitize_questions(&input.open_questions);
    let (new_msgs, skipped_new) =
        filter_watch_messages(&input.new_messages, MAX_WATCH_NEW_MESSAGES_PER_PASS);
    let (earlier_msgs, _) =
        filter_watch_messages(&input.earlier_messages, MAX_WATCH_CONTEXT_MESSAGES);

    if questions.is_empty() || new_msgs.is_empty() {
        return WatchPassOutcome {
            result: Ok(WatchPassResult {
                matches: Vec::new(),
                skipped_secret_message_ids: skipped_new,
            }),
            usage: None,
            duration: started.elapsed(),
            model_called: false,
        };
    }

    let mut pass_limits = limits;
    // Allow generous prompt budget for up to 50 messages + 20 questions + 20 context messages.
    if pass_limits.max_input_bytes < 256 * 1024 {
        pass_limits.max_input_bytes = 256 * 1024;
    }
    let (questions, new_msgs, earlier_msgs) = trim_watch_inputs_to_budget(
        questions,
        new_msgs,
        earlier_msgs,
        pass_limits.max_input_bytes,
    );
    let capture = Arc::new(CaptureLog::default());
    let copilot = Copilot::new(watcher_registry(), runtime, pass_limits).with_log(capture.clone());
    let req = DraftRequest {
        feature: "channel.watch".to_owned(),
        input: json!({
            "open_questions": questions,
            "new_messages": new_msgs,
            "earlier_messages": earlier_msgs,
        }),
        context: Vec::new(),
    };
    let draft_res = copilot.draft(scope_key, &req);
    let captured = capture.last.lock().ok().and_then(|g| g.clone());
    let usage = captured.as_ref().and_then(|r| r.usage);
    let model_called = !matches!(
        draft_res,
        Err(CopilotError::Unavailable
            | CopilotError::Invalid(_)
            | CopilotError::Busy
            | CopilotError::Overloaded)
    );

    let draft = match draft_res {
        Ok(resp) => resp.draft,
        Err(err) => {
            return WatchPassOutcome {
                result: Err(map_copilot_error(err)),
                usage,
                duration: started.elapsed(),
                model_called,
            };
        }
    };
    let validated = validate_watch_matches(&draft, &questions, &new_msgs);
    WatchPassOutcome {
        result: validated.map(|matches| WatchPassResult {
            matches,
            skipped_secret_message_ids: skipped_new,
        }),
        usage,
        duration: started.elapsed(),
        model_called,
    }
}

fn validate_watch_matches(
    draft: &Value,
    questions: &[WatchQuestionInput],
    new_msgs: &[WatchMessageInput],
) -> Result<Vec<WatchMatch>, WatchError> {
    let q_askers: BTreeMap<&str, &str> = questions
        .iter()
        .map(|q| (q.id.as_str(), q.asker.as_str()))
        .collect();
    let m_authors: BTreeMap<&str, &str> = new_msgs
        .iter()
        .map(|m| (m.id.as_str(), m.author.as_str()))
        .collect();

    let raw_matches = draft
        .get("matches")
        .and_then(Value::as_array)
        .ok_or(WatchError::InvalidOutput)?;

    let mut dedup: BTreeMap<(String, String), bool> = BTreeMap::new();
    for entry in raw_matches {
        let message_id = entry
            .get("message_id")
            .and_then(Value::as_str)
            .ok_or(WatchError::InvalidOutput)?;
        let question_id = entry
            .get("question_id")
            .and_then(Value::as_str)
            .ok_or(WatchError::InvalidOutput)?;
        let confident = entry
            .get("confident")
            .and_then(Value::as_bool)
            .ok_or(WatchError::InvalidOutput)?;

        let Some(&msg_author) = m_authors.get(message_id) else {
            return Err(WatchError::InvalidReference);
        };
        let Some(&q_asker) = q_askers.get(question_id) else {
            return Err(WatchError::InvalidReference);
        };
        // ADR-0024 d9 / WATCH-3: drop any pair where the message was written by the asker.
        if msg_author == q_asker {
            continue;
        }
        let key = (message_id.to_owned(), question_id.to_owned());
        let prev = dedup.entry(key).or_insert(false);
        *prev = *prev || confident;
    }

    Ok(dedup
        .into_iter()
        .map(|((message_id, question_id), confident)| WatchMatch {
            message_id,
            question_id,
            confident,
        })
        .collect())
}
