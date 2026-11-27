//! Pure, engine-independent builder and sanitizer for bounded hybrid [`ConflictDetail`]
//! payloads (`TASK-90`).
//!
//! Enforces the workspace-wide conflict contract:
//! - `changed_sections` covers ALL intervening events (even when `events_since` is truncated).
//! - `events_since` is strictly capped at `<= CONFLICT_MAX_EVENTS` (20) AND serialized JSON
//!   `<= CONFLICT_MAX_EVENTS_BYTES` (8 KiB).
//! - Never inlines `body_markdown`, public comment bodies, private task notes, or unbounded
//!   batch arrays.

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

use crate::{
    ConflictDetail, ConflictEvent, ConflictSummary, CONFLICT_MAX_EVENTS, CONFLICT_MAX_EVENTS_BYTES,
};

const MAX_PREVIEW_BYTES: usize = 160;
const MAX_BATCH_STEPS_INLINE: usize = 10;

impl ConflictDetail {
    /// Builds a bounded, sanitized [`ConflictDetail`] from raw intervening events (ordered oldest
    /// to newest, each with `revision > expected_revision`).
    pub fn from_events(
        expected_revision: u64,
        current_revision: u64,
        raw_events: Vec<ConflictEvent>,
        current_summary: Option<ConflictSummary>,
    ) -> Self {
        let total_since = raw_events.len() as u64;
        let mut sections = BTreeSet::new();
        for event in &raw_events {
            collect_changed_sections(&event.change, &mut sections);
        }
        let changed_sections: Vec<String> = sections.into_iter().collect();
        let events_since = bound_events(raw_events);
        let omitted_events = total_since.saturating_sub(events_since.len() as u64);
        let events_truncated = omitted_events > 0;
        let reread_recommended = needs_reread(
            expected_revision,
            current_revision,
            total_since,
            events_truncated,
            &changed_sections,
        );
        Self {
            expected_revision,
            current_revision,
            changed_sections,
            events_since,
            events_truncated,
            omitted_events,
            reread_recommended,
            current_summary,
        }
    }
}

fn needs_reread(
    expected_revision: u64,
    current_revision: u64,
    total_since: u64,
    events_truncated: bool,
    changed_sections: &[String],
) -> bool {
    if events_truncated || expected_revision > current_revision || total_since == 0 {
        return true;
    }
    changed_sections.iter().any(|s| {
        matches!(
            s.as_str(),
            "body" | "comments" | "details" | "final_summary"
        )
    })
}

fn bound_events(raw_events: Vec<ConflictEvent>) -> Vec<ConflictEvent> {
    let start = raw_events.len().saturating_sub(CONFLICT_MAX_EVENTS);
    let mut kept: Vec<ConflictEvent> = raw_events
        .into_iter()
        .skip(start)
        .map(|mut ev| {
            ev.change = compact_change(&ev.change);
            ev
        })
        .collect();
    while !kept.is_empty() {
        let size = serde_json::to_vec(&kept).map_or(usize::MAX, |v| v.len());
        if size <= CONFLICT_MAX_EVENTS_BYTES {
            break;
        }
        kept.remove(0);
    }
    kept
}

fn compact_change(change: &Value) -> Value {
    let Some(obj) = change.as_object() else {
        return json!({});
    };
    let kind = obj.get("type").and_then(Value::as_str).unwrap_or("unknown");
    match kind {
        "created" => json!({"type": "created"}),
        "comment_added" => compact_keys(obj, "comment_added", &["comment_id", "attachment_ids"]),
        "final_summary_set" | "final_summary_updated" => {
            let chars = obj
                .get("final_summary")
                .and_then(Value::as_str)
                .map_or(0, |s| s.chars().count());
            json!({"type": kind, "chars": chars})
        }
        "status_changed" => compact_status(obj),
        "task_updated" => compact_keys(
            obj,
            "task_updated",
            &[
                "fields",
                "changes",
                "added_attachment_ids",
                "removed_attachment_ids",
                "updated_attachment_id",
            ],
        ),
        k if k.starts_with("checklist_") => compact_checklist(kind, obj),
        _ => Value::Object(obj.clone()),
    }
}

fn compact_keys(obj: &Map<String, Value>, kind: &str, keys: &[&str]) -> Value {
    let mut out = Map::new();
    out.insert("type".into(), json!(kind));
    for key in keys {
        copy_key(obj, &mut out, key);
    }
    Value::Object(out)
}

fn compact_status(obj: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    for key in ["type", "from", "to", "resolution"] {
        copy_key(obj, &mut out, key);
    }
    for key in ["blocked_reason", "note"] {
        if let Some(text) = obj.get(key).and_then(Value::as_str) {
            out.insert(key.into(), json!(truncate_utf8(text, MAX_PREVIEW_BYTES)));
        }
    }
    if obj.get("final_summary").is_some_and(Value::is_string) {
        out.insert("final_summary_set".into(), json!(true));
    }
    if obj.get("done_anyway").is_some_and(Value::is_object) {
        out.insert("done_anyway".into(), json!(true));
    }
    Value::Object(out)
}

fn compact_checklist(kind: &str, obj: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    out.insert("type".into(), json!(kind));
    for key in [
        "step_id",
        "section",
        "index",
        "from_section",
        "from_index",
        "position",
        "done",
        "required",
        "fields",
        "count",
    ] {
        copy_key(obj, &mut out, key);
    }
    if let Some(text) = obj.get("text").and_then(Value::as_str) {
        out.insert("text".into(), json!(truncate_utf8(text, MAX_PREVIEW_BYTES)));
    }
    if let Some(steps) = obj.get("steps").and_then(Value::as_array) {
        let trimmed: Vec<Value> = steps
            .iter()
            .take(MAX_BATCH_STEPS_INLINE)
            .map(compact_step_entry)
            .collect();
        out.insert("steps".into(), Value::Array(trimmed));
        if steps.len() > MAX_BATCH_STEPS_INLINE {
            out.insert("steps_truncated".into(), json!(true));
        }
    }
    Value::Object(out)
}

fn compact_step_entry(entry: &Value) -> Value {
    let Some(obj) = entry.as_object() else {
        return entry.clone();
    };
    let mut out = Map::new();
    for key in ["step_id", "section", "index", "required", "done"] {
        copy_key(obj, &mut out, key);
    }
    if let Some(text) = obj.get("text").and_then(Value::as_str) {
        out.insert("text".into(), json!(truncate_utf8(text, MAX_PREVIEW_BYTES)));
    }
    Value::Object(out)
}

fn copy_key(src: &Map<String, Value>, dst: &mut Map<String, Value>, key: &str) {
    if let Some(val) = src.get(key) {
        if !val.is_null() {
            dst.insert(key.into(), val.clone());
        }
    }
}

/// UTF-8 safe byte-bounded truncation using `str::floor_char_boundary`.
fn truncate_utf8(text: &str, budget: usize) -> String {
    if text.len() <= budget {
        return text.to_owned();
    }
    const ELLIPSIS: &str = "…";
    if budget <= ELLIPSIS.len() {
        return text[..text.floor_char_boundary(budget)].to_owned();
    }
    let cut = text.floor_char_boundary(budget - ELLIPSIS.len());
    format!("{}{ELLIPSIS}", &text[..cut])
}

fn collect_changed_sections(change: &Value, sections: &mut BTreeSet<String>) {
    let Some(kind) = change.get("type").and_then(Value::as_str) else {
        return;
    };
    match kind {
        "task_updated" => {
            let Some(fields) = change.get("fields").and_then(Value::as_array) else {
                sections.insert("details".to_owned());
                return;
            };
            for field in fields.iter().filter_map(Value::as_str) {
                let mapped = if field == "body_markdown" {
                    "body"
                } else {
                    field
                };
                sections.insert(mapped.to_owned());
            }
        }
        "status_changed" => {
            sections.insert("status".to_owned());
        }
        "assignment_changed" => {
            sections.insert("assignment".to_owned());
        }
        "final_summary_set" | "final_summary_updated" => {
            sections.insert("final_summary".to_owned());
        }
        "comment_added" => {
            sections.insert("comments".to_owned());
        }
        "watcher_added" | "watcher_removed" => {
            sections.insert("watchers".to_owned());
        }
        "dependency_added" | "dependency_removed" => {
            sections.insert("dependencies".to_owned());
        }
        k if k.starts_with("checklist_") => {
            sections.insert("checklist".to_owned());
        }
        other => {
            sections.insert(other.to_owned());
        }
    }
}
