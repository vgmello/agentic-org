//! Persisted session state goes through the crate's single redaction choke-point
//! (`TASK-190`): `history.jsonl` and the pending-wake and suspended-ask records.

use crate::redact::testing::{assert_no_corpus_secret, secret_corpus};
use crate::wake::{
    PendingAskAnswer, PendingWakeItem, PendingWakeQueueRecord, SuspendedAskEntry,
    SuspendedStateRecord, WakeKind,
};

use super::*;

#[test]
fn append_history_redacts_every_secret_shape_and_keeps_valid_json_lines() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path(), "office-redact");
    let corpus = secret_corpus();
    let entry = serde_json::json!({
        "type": "turn_trigger",
        "body": corpus.text,
        "nested": { "headers": { "Authorization": "Bearer nested-0042" }, "list": [corpus.text] },
        "access_token": { "whole": "object-0042" },
    });
    store
        .append_history("emp-redact", &entry, &["active-only-0042"])
        .unwrap();
    store
        .append_history(
            "emp-redact",
            &serde_json::json!({ "text": "active-only-0042 then\n-----BEGIN PRIVATE KEY-----\nunterminated" }),
            &["active-only-0042"],
        )
        .unwrap();
    let history = fs::read_to_string(store.history_path("emp-redact").unwrap()).unwrap();
    assert_no_corpus_secret(&corpus, &history, "append_history");
    for leaked in [
        "nested-0042",
        "object-0042",
        "active-only-0042",
        "unterminated",
    ] {
        assert!(
            !history.contains(leaked),
            "append_history leaked {leaked}: {history}"
        );
    }
    assert_eq!(history.lines().count(), 2);
    for line in history.lines() {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("a redacted history line is valid JSON");
        assert!(parsed.is_object());
    }
}

#[test]
fn wake_store_redacts_pending_wakes_and_suspended_asks() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path(), "office-redact");
    let corpus = secret_corpus();
    let mut queue = PendingWakeQueueRecord::empty("emp-redact");
    queue.push_dedup(PendingWakeItem {
        item_id: "wake-1".into(),
        kind: WakeKind::Recovery,
        sender_id: "owner".into(),
        sender_name: "Owner".into(),
        sender_handle: None,
        sender_relation: None,
        sender_is_person: true,
        conversation_id: None,
        source_id: "src-1".into(),
        summary: corpus.text.clone(),
        attachment_ids: Vec::new(),
        enqueued_at_ms: 1,
        session_affinity: None,
        model_override: None,
        effort_override: None,
    });
    store.write_pending_wakes_redacted(&queue, &[]).unwrap();
    let raw = fs::read_to_string(store.pending_wakes_path("emp-redact").unwrap()).unwrap();
    assert_no_corpus_secret(&corpus, &raw, "wake_store pending wakes");

    let record = SuspendedStateRecord {
        version: 1,
        employee_id: "emp-redact".into(),
        session_id: "sess-1".into(),
        runtime_session_id: None,
        suspended_since: crate::credentials::rfc3339_now(),
        asks: vec![SuspendedAskEntry {
            ask_id: "ask-1".into(),
            conversation_id: "conv-1".into(),
            question_summary: corpus.text.clone(),
            waiting_for_member_id: None,
            waiting_for_name: None,
            asked_by_subagent: false,
            opened_at_ms: 1,
        }],
        reaped_background_tasks: Vec::new(),
        pending_resume_answers: vec![PendingAskAnswer {
            ask_id: "ask-1".into(),
            question_summary: corpus.text.clone(),
            answer_summary: corpus.text.clone(),
            answered_by: "owner".into(),
            answered_at: crate::credentials::rfc3339_now(),
            asked_by_subagent: false,
            attachment_ids: Vec::new(),
        }],
        hold_reason: None,
        held_since_ms: None,
        updated_at: crate::credentials::rfc3339_now(),
    };
    store.write_suspended_state_redacted(&record, &[]).unwrap();
    let raw = fs::read_to_string(store.suspended_state_path("emp-redact").unwrap()).unwrap();
    assert_no_corpus_secret(&corpus, &raw, "wake_store suspended asks");
}
