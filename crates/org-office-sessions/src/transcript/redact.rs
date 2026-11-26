//! Transcript-shaped redaction (`REQ-05`, `ENROLL-3`): walks session summaries and
//! transcripts and sends every user-visible field through the crate's single redaction
//! choke-point, [`crate::redact`], before any of it leaves the office process.

use org_proto::sessions::{
    SessionSummaryView, SessionTranscriptResult, SubagentTranscriptView, TranscriptEntry,
};

pub use crate::redact::{redact_json, redact_text};

/// Redacts all user-visible fields of a [`SessionSummaryView`].
pub fn redact_summary(summary: &mut SessionSummaryView, active_secrets: &[&str]) {
    if let Some(ref preview) = summary.preview {
        summary.preview = Some(redact_text(preview, active_secrets));
    }
    if let Some(ref started) = summary.started_at {
        summary.started_at = Some(redact_text(started, active_secrets));
    }
}

/// Redacts an entire [`SessionTranscriptResult`] including nested sub-agents and file summaries.
pub fn redact_transcript(result: &mut SessionTranscriptResult, active_secrets: &[&str]) {
    redact_opt_str(&mut result.usage.model, active_secrets);
    for file in &mut result.files {
        file.path = redact_text(&file.path, active_secrets);
    }
    for entry in &mut result.entries {
        redact_entry(entry, active_secrets);
    }
}

fn redact_entry(entry: &mut TranscriptEntry, active_secrets: &[&str]) {
    redact_opt_str(&mut entry.timestamp, active_secrets);
    redact_opt_str(&mut entry.role, active_secrets);
    redact_opt_str(&mut entry.text, active_secrets);
    redact_opt_str(&mut entry.tool_name, active_secrets);
    redact_opt_str(&mut entry.tool_use_id, active_secrets);
    redact_opt_str(&mut entry.tool_output, active_secrets);
    redact_opt_str(&mut entry.file_path, active_secrets);
    redact_opt_str(&mut entry.diff_preview, active_secrets);
    redact_opt_str(&mut entry.raw_type, active_secrets);
    if let Some(ref mut usage) = entry.usage {
        redact_opt_str(&mut usage.model, active_secrets);
    }
    if let Some(ref input) = entry.tool_input {
        entry.tool_input = Some(redact_json(input, active_secrets));
    }
    if let Some(ref mut sub) = entry.subagent {
        redact_subagent(sub, active_secrets);
    }
}

fn redact_opt_str(slot: &mut Option<String>, active_secrets: &[&str]) {
    if let Some(ref val) = *slot {
        *slot = Some(redact_text(val, active_secrets));
    }
}

fn redact_subagent(sub: &mut SubagentTranscriptView, active_secrets: &[&str]) {
    redact_opt_str(&mut sub.label, active_secrets);
    redact_opt_str(&mut sub.parent_tool_use_id, active_secrets);
    redact_opt_str(&mut sub.usage.model, active_secrets);
    for file in &mut sub.files {
        file.path = redact_text(&file.path, active_secrets);
    }
    for entry in &mut sub.entries {
        redact_entry(entry, active_secrets);
    }
}
