//! The office writes task history in the workspace's one `canonical-json/2` codec
//! (`doc-79`, `ADR-0006`): sorted Unicode scalar keys, compact UTF-8 without a trailing newline,
//! C0 control escapes (`\u00xx`), literal non-ASCII scalars, and a full-object `payload_sha256`
//! computed over the unsigned canonical JSON.

use org_office::tasks::records::{created_change, encode_event, verify_event, EventInput};
use org_store::{Id, Sha256};
use serde_json::{json, Map, Value};

fn id(n: u64) -> Id {
    Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
}

fn office_event(revision: u64, change: Value) -> Vec<u8> {
    encode_event(EventInput {
        organization_id: &id(1),
        task_id: &id(2),
        revision,
        occurred_at: "2026-11-26T12:34:56.789Z",
        actor_id: &id(3),
        operation_id: "01900000-0000-7000-8000-000000000004",
        change,
    })
    .expect("an event")
    .1
}

#[test]
fn every_event_the_office_writes_matches_canonical_json_2_and_verifies() {
    let mut facts = Map::new();
    facts.insert("title".into(), json!("Ünïcode \"title\" \u{1}"));
    facts.insert("labels".into(), json!(["a", "b"]));
    let events = [
        (1, created_change(facts, "Body\r\nwith CRLF and 🙂")),
        (2, json!({"type": "comment_added", "comment_id": "c"})),
        (
            3,
            json!({"type": "assignment_changed",
                   "from": {"assigned_team_id": null, "assignee_id": null},
                   "to": {"assigned_team_id": "t", "assignee_id": "e"}}),
        ),
    ];
    for (revision, change) in events {
        let bytes = office_event(revision, change);
        let text = std::str::from_utf8(&bytes).expect("valid utf-8");
        assert!(
            !text.ends_with('\n'),
            "revision {revision}: trailing newline"
        );
        let verified = verify_event(&bytes, &id(1), &id(2), revision)
            .unwrap_or_else(|e| panic!("revision {revision}: {e}"));
        assert_eq!(verified.revision, revision);
        assert_eq!(verified.actor_id, Some(id(3)));
        // Exact SHA-256 matches removing only top-level `payload_sha256` from the canonical bytes.
        let sum_field = format!(",\"payload_sha256\":\"{}\"", verified.payload_sha256);
        let unsigned = text.replacen(&sum_field, "", 1);
        assert_ne!(
            unsigned, text,
            "revision {revision}: missing payload_sha256"
        );
        assert_eq!(
            Sha256::of(unsigned.as_bytes()).to_hex(),
            verified.payload_sha256
        );
    }
}

#[test]
fn canonical_json_2_golden_escaping_and_tamper_rejection() {
    let bytes = office_event(
        4,
        json!({"type": "status_changed", "from": "todo", "to": "done"}),
    );
    let expected_unsigned = concat!(
        "{\"actor_id\":\"01900000-0000-7000-8000-000000000003\",",
        "\"actor_kind\":\"employee\",",
        "\"change\":{\"from\":\"todo\",\"to\":\"done\",\"type\":\"status_changed\"},",
        "\"occurred_at\":\"2026-11-26T12:34:56.789Z\",",
        "\"operation_id\":\"01900000-0000-7000-8000-000000000004\",",
        "\"organization_id\":\"01900000-0000-7000-8000-000000000001\",",
        "\"resource_profile\":\"selected-task-resources/1\",",
        "\"revision\":4,",
        "\"schema_version\":2,",
        "\"source_metadata\":{},",
        "\"task_id\":\"01900000-0000-7000-8000-000000000002\",",
        "\"task_kind\":\"task\"}"
    );
    let expected_sum = Sha256::of(expected_unsigned.as_bytes()).to_hex();
    let record = verify_event(&bytes, &id(1), &id(2), 4).expect("the office reads it");
    assert_eq!(record.revision, 4);
    assert_eq!(record.actor_id, Some(id(3)));
    assert_eq!(record.payload_sha256, expected_sum);

    // Tampered payload, pretty formatting, or trailing newline are refused.
    let tampered = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("done", "gone");
    assert!(verify_event(tampered.as_bytes(), &id(1), &id(2), 4).is_err());
    let mut with_nl = bytes;
    with_nl.push(b'\n');
    assert!(verify_event(&with_nl, &id(1), &id(2), 4).is_err());
}
