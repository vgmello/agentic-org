//! One tool-less `channel.summarize` pass and the validation of its draft.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};

use super::features::watcher_registry;
use super::inputs::{clamp_text, trim_summarize_messages_to_budget};
use super::{
    map_copilot_error, CaptureLog, ChannelSummarizeInput, ChannelSummarizeOutcome,
    ChannelSummarizeResult, SummarizeMessageInput, WatchError, DERIVED_SUMMARY_NOTICE,
    MAX_SUMMARIZE_MESSAGES,
};
use crate::contract::DraftRequest;
use crate::limits::Limits;
use crate::runtime::CopilotRuntime;
use crate::secrets::text_has_secret;
use crate::service::{Copilot, CopilotError};

/// Runs one tool-less `channel.summarize` pass over caller-visible channel messages (`WATCH-10`).
pub fn run_channel_summarize<R: CopilotRuntime>(
    runtime: R,
    limits: Limits,
    caller_id: &str,
    input: &ChannelSummarizeInput,
) -> ChannelSummarizeOutcome {
    let started = Instant::now();
    if text_has_secret(&input.question) {
        return ChannelSummarizeOutcome {
            result: Err(WatchError::InputRejected),
            usage: None,
            duration: started.elapsed(),
            model_called: false,
        };
    }

    let mut kept = Vec::new();
    let mut skipped = Vec::new();
    for m in input.messages.iter().take(MAX_SUMMARIZE_MESSAGES) {
        if text_has_secret(&m.text) {
            skipped.push(m.id.clone());
        } else {
            kept.push(SummarizeMessageInput {
                id: m.id.clone(),
                author: m.author.clone(),
                text: clamp_text(&m.text, 3500),
                created_at: m.created_at.clone(),
            });
        }
    }

    if kept.is_empty() {
        return ChannelSummarizeOutcome {
            result: Ok(ChannelSummarizeResult {
                summary: "No eligible channel messages were available to answer this question."
                    .to_owned(),
                message_ids: Vec::new(),
                derived_notice: DERIVED_SUMMARY_NOTICE.to_owned(),
                skipped_secret_message_ids: skipped,
            }),
            usage: None,
            duration: started.elapsed(),
            model_called: false,
        };
    }

    let mut pass_limits = limits;
    if pass_limits.max_input_bytes < 512 * 1024 {
        pass_limits.max_input_bytes = 512 * 1024;
    }
    let kept = trim_summarize_messages_to_budget(kept, pass_limits.max_input_bytes);
    let capture = Arc::new(CaptureLog::default());
    let copilot = Copilot::new(watcher_registry(), runtime, pass_limits).with_log(capture.clone());
    let req = DraftRequest {
        feature: "channel.summarize".to_owned(),
        input: json!({
            "channel": clamp_text(&input.channel, 128),
            "question": clamp_text(&input.question, 2000),
            "messages": kept,
        }),
        context: Vec::new(),
    };
    let draft_res = copilot.draft(caller_id, &req);
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
            return ChannelSummarizeOutcome {
                result: Err(map_copilot_error(err)),
                usage,
                duration: started.elapsed(),
                model_called,
            };
        }
    };

    let allowed_ids: BTreeSet<&str> = kept.iter().map(|m| m.id.as_str()).collect();
    let res = validate_summary_draft(&draft, &allowed_ids, skipped);
    ChannelSummarizeOutcome {
        result: res,
        usage,
        duration: started.elapsed(),
        model_called,
    }
}

fn validate_summary_draft(
    draft: &Value,
    allowed_ids: &BTreeSet<&str>,
    skipped: Vec<String>,
) -> Result<ChannelSummarizeResult, WatchError> {
    let summary = draft
        .get("summary")
        .and_then(Value::as_str)
        .ok_or(WatchError::InvalidOutput)?
        .trim()
        .to_owned();
    if summary.is_empty() {
        return Err(WatchError::InvalidOutput);
    }
    let raw_ids = draft
        .get("message_ids")
        .and_then(Value::as_array)
        .ok_or(WatchError::InvalidOutput)?;
    let mut message_ids = Vec::new();
    let mut seen = BTreeSet::new();
    for v in raw_ids {
        let id = v.as_str().ok_or(WatchError::InvalidOutput)?;
        if !allowed_ids.contains(id) {
            return Err(WatchError::InvalidReference);
        }
        if seen.insert(id) {
            message_ids.push(id.to_owned());
        }
    }
    Ok(ChannelSummarizeResult {
        summary,
        message_ids,
        derived_notice: DERIVED_SUMMARY_NOTICE.to_owned(),
        skipped_secret_message_ids: skipped,
    })
}
