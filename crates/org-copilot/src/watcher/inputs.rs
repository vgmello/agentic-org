//! What a pass sends the model: secret-shaped text filtered out, text clamped, and
//! dense inputs trimmed to the byte budget.

use serde_json::json;

use super::{
    SummarizeMessageInput, WatchMessageInput, WatchQuestionInput, MAX_WATCH_QUESTIONS_PER_PASS,
};
use crate::secrets::text_has_secret;

pub(super) fn clamp_text(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// Keeps dense `channel.watch` inputs within `max_input_bytes` (Low #15) by progressively
/// trimming earlier context and then long per-item text budgets so batches never fail `TooLarge`.
pub(super) fn trim_watch_inputs_to_budget(
    mut questions: Vec<WatchQuestionInput>,
    mut new_msgs: Vec<WatchMessageInput>,
    mut earlier_msgs: Vec<WatchMessageInput>,
    max_input_bytes: usize,
) -> (
    Vec<WatchQuestionInput>,
    Vec<WatchMessageInput>,
    Vec<WatchMessageInput>,
) {
    let safe_budget = max_input_bytes.saturating_mul(3) / 4;
    let payload_len =
        |q: &[WatchQuestionInput], n: &[WatchMessageInput], e: &[WatchMessageInput]| {
            crate::prompt::data_json(
                &json!({ "open_questions": q, "new_messages": n, "earlier_messages": e }),
                &[],
            )
            .len()
        };
    while !earlier_msgs.is_empty()
        && payload_len(&questions, &new_msgs, &earlier_msgs) > safe_budget
    {
        earlier_msgs.remove(0);
    }
    for cap in [1200usize, 500, 200] {
        if payload_len(&questions, &new_msgs, &earlier_msgs) <= safe_budget {
            break;
        }
        for m in &mut new_msgs {
            m.text = clamp_text(&m.text, cap);
        }
        for q in &mut questions {
            q.text = clamp_text(&q.text, cap);
            for a in &mut q.additions {
                *a = clamp_text(a, cap / 2);
            }
        }
    }
    (questions, new_msgs, earlier_msgs)
}

/// Keeps dense `channel.summarize` inputs within `max_input_bytes` (Low #15) by bounding per-message
/// length and dropping oldest messages only if needed.
pub(super) fn trim_summarize_messages_to_budget(
    mut msgs: Vec<SummarizeMessageInput>,
    max_input_bytes: usize,
) -> Vec<SummarizeMessageInput> {
    let safe_budget = max_input_bytes.saturating_mul(3) / 4;
    let payload_len = |m: &[SummarizeMessageInput]| {
        crate::prompt::data_json(&json!({ "messages": m }), &[]).len()
    };
    for cap in [1500usize, 700, 300] {
        if payload_len(&msgs) <= safe_budget {
            break;
        }
        for m in &mut msgs {
            m.text = clamp_text(&m.text, cap);
        }
    }
    while msgs.len() > 1 && payload_len(&msgs) > safe_budget {
        msgs.remove(0);
    }
    msgs
}

pub(super) fn sanitize_questions(questions: &[WatchQuestionInput]) -> Vec<WatchQuestionInput> {
    questions
        .iter()
        .take(MAX_WATCH_QUESTIONS_PER_PASS)
        .filter(|q| !text_has_secret(&q.text))
        .map(|q| WatchQuestionInput {
            id: q.id.clone(),
            asker: q.asker.clone(),
            text: clamp_text(&q.text, 3500),
            additions: q
                .additions
                .iter()
                .filter(|a| !text_has_secret(a))
                .take(20)
                .map(|a| clamp_text(a, 1500))
                .collect(),
        })
        .collect()
}

pub(super) fn filter_watch_messages(
    messages: &[WatchMessageInput],
    cap: usize,
) -> (Vec<WatchMessageInput>, Vec<String>) {
    let mut kept = Vec::new();
    let mut skipped = Vec::new();
    for m in messages.iter().take(cap) {
        if text_has_secret(&m.text) {
            skipped.push(m.id.clone());
        } else {
            kept.push(WatchMessageInput {
                id: m.id.clone(),
                author: m.author.clone(),
                text: clamp_text(&m.text, 3500),
            });
        }
    }
    (kept, skipped)
}
