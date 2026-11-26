use org_store::{Id, Sha256};
use serde_json::{json, Map, Value};

use super::*;
use crate::organization::records::yaml::canonical_json;

fn id(n: u64) -> Id {
    Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
}

fn input(revision: u64, change: Value) -> (Id, Id, Id, Value) {
    let _ = revision;
    (id(1), id(2), id(3), change)
}

fn encode(revision: u64, change: Value) -> Vec<u8> {
    let (org, task, actor, change) = input(revision, change);
    encode_event(EventInput {
        organization_id: &org,
        task_id: &task,
        revision,
        occurred_at: "2026-11-26T00:00:00.000Z",
        actor_id: &actor,
        operation_id: "01900000-0000-7000-8000-000000000009",
        change,
    })
    .unwrap()
    .1
}

fn created() -> Value {
    let mut facts = Map::new();
    facts.insert("title".into(), json!("A task"));
    created_change(facts, "Body with \u{1} control and Zoë")
}

/// Re-signs `event` the way a producer would, so only the mutation under test is wrong.
fn resign(mut event: Map<String, Value>) -> Vec<u8> {
    event.remove("payload_sha256");
    let sum = Sha256::of(canonical_json(&Value::Object(event.clone())).as_bytes()).to_hex();
    event.insert("payload_sha256".into(), json!(sum));
    canonical_json(&Value::Object(event)).into_bytes()
}

fn object(bytes: &[u8]) -> Map<String, Value> {
    match serde_json::from_slice(bytes).unwrap() {
        Value::Object(map) => map,
        _ => panic!("an object"),
    }
}

#[test]
fn an_event_is_canonical_json_2_with_a_full_object_checksum() {
    let bytes = encode(1, created());
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(!text.ends_with('\n'));
    assert!(text.starts_with("{\"actor_id\":"), "{text}");
    let event = object(&bytes);
    for (key, value) in [
        ("schema_version", json!(2)),
        ("task_kind", json!("task")),
        ("revision", json!(1)),
        ("resource_profile", json!("selected-task-resources/1")),
        ("source_metadata", json!({})),
        ("actor_kind", json!("employee")),
    ] {
        assert_eq!(event[key], value, "{key}");
    }
    assert_eq!(
        event["change"]["public_snapshot"]["encoding"],
        "utf-8-markdown"
    );
    let sum = event["payload_sha256"].as_str().unwrap();
    assert_eq!(sum.len(), 64);
    let mut unsigned = event.clone();
    unsigned.remove("payload_sha256");
    assert_eq!(
        Sha256::of(canonical_json(&Value::Object(unsigned)).as_bytes()).to_hex(),
        sum
    );
    let record = verify_event(&bytes, &id(1), &id(2), 1).unwrap();
    assert_eq!(record.revision, 1);
    assert_eq!(record.actor_id, Some(id(3)));
    assert_eq!(record.payload_sha256, sum);
    assert_eq!(
        serde_json::from_str::<Value>(&record.change_json).unwrap()["type"],
        "created"
    );
}

#[test]
fn an_event_is_refused_unless_every_check_passes() {
    let bytes = encode(2, json!({"type": "comment_added", "comment_id": "c"}));
    assert!(verify_event(&bytes, &id(1), &id(2), 2).is_ok());
    // Named by another path, organization or revision.
    assert!(verify_event(&bytes, &id(9), &id(2), 2).is_err());
    assert!(verify_event(&bytes, &id(1), &id(9), 2).is_err());
    assert!(verify_event(&bytes, &id(1), &id(2), 3).is_err());
    // Tampered without re-signing.
    let tampered = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("comment_added", "comment_xdded");
    let err = verify_event(tampered.as_bytes(), &id(1), &id(2), 2).unwrap_err();
    assert!(err.contains("payload_sha256"), "{err}");
    // Not in canonical form (pretty, or a trailing newline).
    let pretty = serde_json::to_vec_pretty(&object(&bytes)).unwrap();
    assert!(verify_event(&pretty, &id(1), &id(2), 2).is_err());
    let mut newline = bytes.clone();
    newline.push(b'\n');
    assert!(verify_event(&newline, &id(1), &id(2), 2).is_err());
    // Re-signed but wrong in substance.
    let base = object(&bytes);
    let with = |key: &str, value: Value| {
        let mut event = base.clone();
        event.insert(key.into(), value);
        resign(event)
    };
    for bad in [
        with("schema_version", json!(1)),
        with("resource_profile", json!("other/1")),
        with("task_kind", json!("bug")),
        with("actor_kind", json!("system")),
        with("actor_id", Value::Null),
        with("change", json!({"type": ""})),
        with("change", json!({"type": "created", "known_facts": {}})),
        with("source_metadata", json!([])),
        with("extension", json!(1)),
        with("payload_sha256", json!("ABC")),
    ] {
        if bad == bytes {
            continue;
        }
        assert!(verify_event(&bad, &id(1), &id(2), 2).is_err());
    }
    // `assignment_changed` keeps the schema.sql shape.
    let assignment = encode(
        3,
        json!({"type": "assignment_changed",
               "from": {"assigned_team_id": null, "assignee_id": null},
               "to": {"assigned_team_id": "t", "assignee_id": null}}),
    );
    assert!(verify_event(&assignment, &id(1), &id(2), 3).is_ok());
    let mut broken = object(&assignment);
    broken.insert(
        "change".into(),
        json!({"type": "assignment_changed", "from": {}}),
    );
    assert!(verify_event(&resign(broken), &id(1), &id(2), 3).is_err());
}

#[test]
fn a_created_event_is_only_revision_one_and_carries_its_snapshot() {
    let (org, task, actor, change) = input(2, created());
    let refused = encode_event(EventInput {
        organization_id: &org,
        task_id: &task,
        revision: 2,
        occurred_at: "2026-11-26T00:00:00.000Z",
        actor_id: &actor,
        operation_id: "op",
        change,
    });
    assert!(refused.is_err());
    let bytes = encode(1, created());
    let mut blob = object(&bytes);
    blob.insert(
        "change".into(),
        json!({"type": "created", "known_facts": {},
               "public_snapshot": {"encoding": "utf-8-markdown", "sha256": "a".repeat(64),
                                   "byte_length": 10,
                                   "resource_profile": "selected-task-resources/1"}}),
    );
    assert!(verify_event(&resign(blob), &id(1), &id(2), 1).is_ok());
}
