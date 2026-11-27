//! Pure, engine-independent builder and sanitizer for bounded hybrid [`ConflictDetail`]
//! payloads (`TASK-90`).
//!
//! Enforces the workspace-wide conflict contract:
//! - `changed_sections` covers ALL intervening events (even when `events_since` is truncated).
//! - `events_since` is strictly capped at `<= CONFLICT_MAX_EVENTS` (20) AND the total serialized
//!   [`ConflictDetail`] stays `<= CONFLICT_MAX_EVENTS_BYTES` (8 KiB) whenever events are kept.
//! - Fail-closed allowlist sanitization: never inlines `body_markdown`, public comment bodies,
//!   private task notes, arbitrary unknown event payloads, or unbounded batch/file arrays.

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
        let start = raw_events.len().saturating_sub(CONFLICT_MAX_EVENTS);
        let kept: Vec<ConflictEvent> = raw_events
            .into_iter()
            .skip(start)
            .map(|mut ev| {
                ev.change = compact_change(&ev.change);
                ev
            })
            .collect();

        let mut detail = Self {
            expected_revision,
            current_revision,
            changed_sections,
            events_since: kept,
            events_truncated: false,
            omitted_events: 0,
            reread_recommended: false,
            current_summary,
        };
        fit_within_byte_budget(&mut detail, total_since);
        detail
    }
}

fn fit_within_byte_budget(detail: &mut ConflictDetail, total_since: u64) {
    loop {
        update_truncation_and_reread(detail, total_since);
        if detail.events_since.is_empty() {
            return;
        }
        let size = serde_json::to_vec(detail).map_or(usize::MAX, |v| v.len());
        if size <= CONFLICT_MAX_EVENTS_BYTES {
            return;
        }
        detail.events_since.remove(0);
    }
}

fn update_truncation_and_reread(detail: &mut ConflictDetail, total_since: u64) {
    detail.omitted_events = total_since.saturating_sub(detail.events_since.len() as u64);
    detail.events_truncated = detail.omitted_events > 0;
    detail.reread_recommended = needs_reread(detail, total_since);
}

fn needs_reread(detail: &ConflictDetail, total_since: u64) -> bool {
    if detail.events_truncated
        || detail.expected_revision > detail.current_revision
        || total_since == 0
    {
        return true;
    }
    if detail.changed_sections.iter().any(|s| {
        matches!(
            s.as_str(),
            "body" | "comments" | "details" | "final_summary"
        )
    }) {
        return true;
    }
    detail
        .events_since
        .iter()
        .any(|ev| event_is_lossy(&ev.change))
}

fn event_is_lossy(change: &Value) -> bool {
    let Some(obj) = change.as_object() else {
        return true;
    };
    if obj.get("steps_truncated") == Some(&Value::Bool(true))
        || obj.get("final_summary_set") == Some(&Value::Bool(true))
        || obj.get("changes_truncated") == Some(&Value::Bool(true))
        || obj.get("unknown") == Some(&Value::Bool(true))
    {
        return true;
    }
    matches!(
        obj.get("type").and_then(Value::as_str),
        Some("created") | None
    )
}

fn compact_change(change: &Value) -> Value {
    let Some(obj) = change.as_object() else {
        return json!({"type": "unknown", "unknown": true});
    };
    let kind = obj.get("type").and_then(Value::as_str).unwrap_or("unknown");
    match kind {
        "created" => json!({"type": "created"}),
        "comment_added" => compact_keys(obj, "comment_added", &["comment_id", "attachment_ids"]),
        "final_summary_set" | "final_summary_updated" => compact_final_summary(kind, obj),
        "status_changed" => compact_status(obj),
        "assignment_changed" => compact_assignment(obj),
        "watcher_added" | "watcher_removed" => compact_keys(obj, kind, &["employee_id"]),
        "dependency_added" | "dependency_removed" => {
            compact_keys(obj, kind, &["depends_on_task_id", "depends_on_key"])
        }
        "task_updated" => compact_task_updated(obj),
        k if k.starts_with("checklist_") => compact_checklist(kind, obj),
        _ => json!({"type": truncate_utf8(kind, MAX_PREVIEW_BYTES), "unknown": true}),
    }
}

fn compact_final_summary(kind: &str, obj: &Map<String, Value>) -> Value {
    let chars = obj
        .get("final_summary")
        .and_then(Value::as_str)
        .map_or(0, |s| s.chars().count());
    json!({"type": kind, "chars": chars})
}

fn compact_assignment(obj: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    out.insert("type".into(), json!("assignment_changed"));
    for side in ["from", "to"] {
        if let Some(side_obj) = obj.get(side).and_then(Value::as_object) {
            let mut clean = Map::new();
            copy_bounded_scalar(side_obj, &mut clean, "assigned_team_id");
            copy_bounded_scalar(side_obj, &mut clean, "assignee_id");
            out.insert(side.into(), Value::Object(clean));
        }
    }
    Value::Object(out)
}

fn compact_task_updated(obj: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    out.insert("type".into(), json!("task_updated"));
    copy_string_array(obj, &mut out, "fields", 32);
    copy_string_array(
        obj,
        &mut out,
        "added_attachment_ids",
        MAX_BATCH_STEPS_INLINE,
    );
    copy_string_array(
        obj,
        &mut out,
        "removed_attachment_ids",
        MAX_BATCH_STEPS_INLINE,
    );
    copy_bounded_scalar(obj, &mut out, "updated_attachment_id");
    if let Some(changes) = obj.get("changes").and_then(Value::as_object) {
        let (clean, truncated) = compact_task_changes(changes);
        if !clean.is_empty() {
            out.insert("changes".into(), Value::Object(clean));
        }
        if truncated {
            out.insert("changes_truncated".into(), json!(true));
        }
    }
    Value::Object(out)
}

fn compact_task_changes(changes: &Map<String, Value>) -> (Map<String, Value>, bool) {
    let mut out = Map::new();
    let mut truncated = false;
    for (key, val) in changes {
        let Some(entry) = val.as_object() else {
            continue;
        };
        match key.as_str() {
            "body_markdown" => {
                let mut m = Map::new();
                copy_bounded_scalar(entry, &mut m, "mode");
                copy_bounded_scalar(entry, &mut m, "before_chars");
                copy_bounded_scalar(entry, &mut m, "after_chars");
                out.insert(key.clone(), Value::Object(m));
            }
            "details" => {
                let mut m = Map::new();
                copy_bounded_scalar(entry, &mut m, "updated");
                out.insert(key.clone(), Value::Object(m));
            }
            "labels" | "reference_files" | "modified_files" => {
                let (m, was_cut) = compact_set_diff(entry);
                truncated |= was_cut;
                out.insert(key.clone(), Value::Object(m));
            }
            "title" | "type" | "work_type" | "priority" | "effort" | "severity" | "rank_order" => {
                let mut m = Map::new();
                copy_bounded_scalar(entry, &mut m, "from");
                copy_bounded_scalar(entry, &mut m, "to");
                out.insert(key.clone(), Value::Object(m));
            }
            _ => {
                truncated = true;
            }
        }
    }
    (out, truncated)
}

fn compact_set_diff(entry: &Map<String, Value>) -> (Map<String, Value>, bool) {
    let mut out = Map::new();
    let mut truncated = false;
    for dir in ["added", "removed"] {
        if let Some(arr) = entry.get(dir).and_then(Value::as_array) {
            if arr.len() > MAX_BATCH_STEPS_INLINE {
                truncated = true;
            }
            let items: Vec<Value> = arr
                .iter()
                .take(MAX_BATCH_STEPS_INLINE)
                .filter_map(Value::as_str)
                .map(|s| json!(truncate_utf8(s, MAX_PREVIEW_BYTES)))
                .collect();
            out.insert(dir.into(), Value::Array(items));
        }
    }
    if truncated {
        out.insert("truncated".into(), json!(true));
    }
    (out, truncated)
}

fn compact_keys(obj: &Map<String, Value>, kind: &str, keys: &[&str]) -> Value {
    let mut out = Map::new();
    out.insert("type".into(), json!(kind));
    for key in keys {
        if *key == "attachment_ids" {
            copy_string_array(obj, &mut out, key, MAX_BATCH_STEPS_INLINE);
        } else {
            copy_bounded_scalar(obj, &mut out, key);
        }
    }
    Value::Object(out)
}

fn compact_status(obj: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    for key in ["type", "from", "to", "resolution"] {
        copy_bounded_scalar(obj, &mut out, key);
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
        "count",
    ] {
        copy_bounded_scalar(obj, &mut out, key);
    }
    copy_string_array(obj, &mut out, "fields", 16);
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
        return json!({});
    };
    let mut out = Map::new();
    for key in ["step_id", "section", "index", "required", "done"] {
        copy_bounded_scalar(obj, &mut out, key);
    }
    if let Some(text) = obj.get("text").and_then(Value::as_str) {
        out.insert("text".into(), json!(truncate_utf8(text, MAX_PREVIEW_BYTES)));
    }
    Value::Object(out)
}

fn copy_bounded_scalar(src: &Map<String, Value>, dst: &mut Map<String, Value>, key: &str) {
    let Some(val) = src.get(key) else {
        return;
    };
    match val {
        Value::Null => {}
        Value::Bool(_) | Value::Number(_) => {
            dst.insert(key.into(), val.clone());
        }
        Value::String(s) => {
            dst.insert(key.into(), json!(truncate_utf8(s, MAX_PREVIEW_BYTES)));
        }
        Value::Array(_) | Value::Object(_) => {}
    }
}

fn copy_string_array(
    src: &Map<String, Value>,
    dst: &mut Map<String, Value>,
    key: &str,
    limit: usize,
) {
    let Some(arr) = src.get(key).and_then(Value::as_array) else {
        return;
    };
    let clean: Vec<Value> = arr
        .iter()
        .take(limit)
        .filter_map(Value::as_str)
        .map(|s| json!(truncate_utf8(s, MAX_PREVIEW_BYTES)))
        .collect();
    dst.insert(key.into(), Value::Array(clean));
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
            if change.get("final_summary").is_some_and(Value::is_string) {
                sections.insert("final_summary".to_owned());
            }
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
            sections.insert(truncate_utf8(other, MAX_PREVIEW_BYTES));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(rev: u64, change: Value) -> ConflictEvent {
        ConflictEvent {
            revision: rev,
            occurred_at: Some("2026-11-27T10:00:00Z".into()),
            actor_kind: "employee".into(),
            actor_id: Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into()),
            change,
        }
    }

    #[test]
    fn truncate_utf8_respects_multibyte_boundaries_and_small_budgets() {
        // 4-byte emoji repeated 50 times (200 bytes > 160-byte cap).
        let emojis = "🚀".repeat(50);
        let cut = truncate_utf8(&emojis, MAX_PREVIEW_BYTES);
        assert!(cut.len() <= MAX_PREVIEW_BYTES);
        assert!(cut.ends_with('…'));

        // 3-byte CJK spanning exact boundary bytes 157..160.
        let cjk = format!("a{}", "漢".repeat(60));
        let cut_cjk = truncate_utf8(&cjk, MAX_PREVIEW_BYTES);
        assert!(cut_cjk.len() <= MAX_PREVIEW_BYTES);
        assert!(cut_cjk.ends_with('…'));

        // Tiny budgets (<= ELLIPSIS.len() = 3) never panic or exceed budget.
        assert_eq!(truncate_utf8("🚀abc", 0), "");
        assert_eq!(truncate_utf8("🚀abc", 2), "");
        assert_eq!(truncate_utf8("🚀abc", 3), "");
        assert_eq!(truncate_utf8("ab🚀", 3), "ab");
    }

    #[test]
    fn status_changed_with_final_summary_marks_section_and_reread() {
        let detail = ConflictDetail::from_events(
            1,
            2,
            vec![event(
                2,
                json!({
                    "type": "status_changed",
                    "from": "in_progress",
                    "to": "done",
                    "final_summary": "PRIVATE_OR_LONG_CLOSING_SUMMARY_TEXT",
                }),
            )],
            None,
        );
        assert!(detail.changed_sections.iter().any(|s| s == "status"));
        assert!(detail.changed_sections.iter().any(|s| s == "final_summary"));
        assert!(detail.reread_recommended);
        assert_eq!(detail.events_since[0].change["final_summary_set"], true);
        let raw = serde_json::to_string(&detail).unwrap();
        assert!(!raw.contains("PRIVATE_OR_LONG_CLOSING_SUMMARY_TEXT"));
    }

    #[test]
    fn checklist_steps_truncated_and_created_event_require_reread() {
        let steps: Vec<Value> = (1..=15)
            .map(|i| json!({"step_id": format!("s-{i}"), "section": "work", "index": i, "text": format!("step {i}")}))
            .collect();
        let detail = ConflictDetail::from_events(
            1,
            2,
            vec![event(
                2,
                json!({"type": "checklist_step_added", "steps": steps}),
            )],
            None,
        );
        assert_eq!(detail.events_since[0].change["steps_truncated"], true);
        assert_eq!(
            detail.events_since[0].change["steps"]
                .as_array()
                .unwrap()
                .len(),
            MAX_BATCH_STEPS_INLINE
        );
        assert!(detail.reread_recommended);

        let created =
            ConflictDetail::from_events(0, 1, vec![event(1, json!({"type": "created"}))], None);
        assert!(created.reread_recommended);

        let reversed = ConflictDetail::from_events(
            5,
            3,
            vec![event(
                4,
                json!({"type": "watcher_added", "employee_id": "e1"}),
            )],
            None,
        );
        assert!(reversed.reread_recommended);
    }

    #[test]
    fn unknown_event_fails_closed_and_large_task_updated_stays_under_total_8kib_cap() {
        let unknown = ConflictDetail::from_events(
            1,
            2,
            vec![event(
                2,
                json!({"type": "future_custom_event", "secret_body": "NEVER_LEAK_ME"}),
            )],
            None,
        );
        assert!(unknown.reread_recommended);
        assert_eq!(unknown.events_since[0].change["unknown"], true);
        let raw = serde_json::to_string(&unknown).unwrap();
        assert!(!raw.contains("NEVER_LEAK_ME"));

        // 256 reference_files in a single task_updated event + large ConflictSummary stays <= 8 KiB total.
        let files: Vec<String> = (0..256)
            .map(|i| format!("crates/org-office/src/very/deep/nested/module_{i:03}/file.rs"))
            .collect();
        let big_summary = ConflictSummary {
            key: Some("T-A99".into()),
            title: "🚀".repeat(50),
            status: "in_progress".into(),
            priority: "urgent".into(),
            assignee_id: Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into()),
            assigned_team_id: Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a2".into()),
            checklist_total: 10,
            checklist_done: 5,
            ac_total: 4,
            ac_done: 2,
            dod_total: 3,
            dod_done: 1,
            work_steps_total: 3,
            work_steps_done: 2,
            comment_count: 12,
            watcher_count: 4,
        };
        let events: Vec<ConflictEvent> = (2..=25u64)
            .map(|rev| {
                event(
                    rev,
                    json!({
                        "type": "task_updated",
                        "fields": ["reference_files"],
                        "changes": {
                            "reference_files": { "added": files, "removed": [] },
                            "rogue_unlisted": { "raw": "MUST_BE_DROPPED" },
                        }
                    }),
                )
            })
            .collect();
        let detail = ConflictDetail::from_events(1, 25, events, Some(big_summary));
        let total_bytes = serde_json::to_vec(&detail).unwrap().len();
        assert!(
            total_bytes <= CONFLICT_MAX_EVENTS_BYTES,
            "total ConflictDetail bytes {total_bytes} > {CONFLICT_MAX_EVENTS_BYTES}"
        );
        assert!(detail.reread_recommended);
        let serialized = serde_json::to_string(&detail).unwrap();
        assert!(!serialized.contains("MUST_BE_DROPPED"));
        assert!(!detail.events_since.is_empty());
        assert_eq!(detail.events_since[0].change["changes_truncated"], true);
    }
}
