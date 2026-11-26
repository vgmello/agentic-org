use org_store::Id;

use super::*;
use crate::tasks::records::{
    ChecklistStepRecord, OfficeTaskCounterRecord, TaskCommentRecord, TaskWatcherRecord,
};

fn id(n: u64) -> Id {
    Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
}

fn step(n: u64, position: u64, done: bool) -> ChecklistStepRecord {
    ChecklistStepRecord {
        id: id(n),
        position,
        text: format!("step {n}: \"quoted\""),
        required: n.is_multiple_of(2),
        done,
        linked_task_id: done.then(|| id(99)),
        checked_by_employee_id: done.then(|| id(4)),
        checked_at: done.then(|| "2026-11-26T00:00:00.000Z".to_owned()),
        revision: n,
    }
}

#[test]
fn a_comment_is_front_matter_and_its_markdown() {
    let comment = TaskCommentRecord {
        task_id: id(1),
        id: id(2),
        position: 3,
        author_id: id(4),
        created_at: "2026-11-26T00:00:00.000Z".into(),
        body_markdown: "A **comment**\n---\nwith a fence-like line".into(),
    };
    let bytes = encode_comment(&comment).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    for key in [
        "schema_version: 1",
        "task_id: ",
        "author_id: ",
        "created_at: ",
    ] {
        assert!(text.contains(key), "{key}");
    }
    assert_eq!(decode_comment(&bytes).unwrap(), comment);
    let bad = text.replace("schema_version: 1", "schema_version: 1\nedited: true");
    assert!(decode_comment(bad.as_bytes()).is_err());
}

#[test]
fn a_checklist_keeps_its_order_and_every_step_field() {
    // Given out of order: written in position order, read back with positions by index.
    let steps = vec![step(3, 1, true), step(2, 0, false), step(5, 2, false)];
    let bytes = encode_checklist(&steps).unwrap();
    let decoded = decode_checklist(&bytes).unwrap();
    let mut expected = steps.clone();
    expected.sort_by_key(|s| s.position);
    assert_eq!(decoded, expected);
    assert!(String::from_utf8(bytes)
        .unwrap()
        .starts_with("checklist: ["));
    assert!(decode_checklist(b"schema_version: 1\nchecklist: []\n")
        .unwrap()
        .is_empty());
    let twice = encode_checklist(&[step(2, 0, false), step(2, 1, false)]).unwrap();
    assert!(decode_checklist(&twice).is_err());
}

#[test]
fn a_checklist_past_its_bound_is_refused_when_written() {
    let steps: Vec<ChecklistStepRecord> = (0..200)
        .map(|n| ChecklistStepRecord {
            text: "x".repeat(400),
            ..step(n + 1, n, false)
        })
        .collect();
    assert!(encode_checklist(&steps).is_err());
}

#[test]
fn operations_hold_watchers_and_refuse_records_no_office_keeps_yet() {
    let watchers = vec![
        TaskWatcherRecord {
            id: id(8),
            watcher_kind: "employee".into(),
            watcher_id: Some(id(4)),
        },
        TaskWatcherRecord {
            id: id(7),
            watcher_kind: "owner".into(),
            watcher_id: None,
        },
    ];
    let bytes = encode_operations(&watchers).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("owner_questions: []"), "{text}");
    assert!(text.contains("waiting_spans: []"), "{text}");
    let mut sorted = watchers.clone();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(decode_operations(&bytes).unwrap(), sorted);
    for (from, to) in [
        ("owner_questions: []", "owner_questions: [{\"id\":\"q\"}]"),
        ("waiting_spans: []", "waiting_spans: [{\"id\":\"w\"}]"),
        ("\"watcher_kind\":\"owner\"", "\"watcher_kind\":\"robot\""),
    ] {
        let bad = text.replacen(from, to, 1);
        assert!(decode_operations(bad.as_bytes()).is_err(), "{bad}");
    }
}

#[test]
fn the_counter_names_its_office_and_its_reservation() {
    let counter = OfficeTaskCounterRecord {
        office_code: "B".into(),
        last_number: 5,
        reserved_through: 32,
    };
    let bytes = encode_counter(&id(3), &counter);
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        format!(
            "last_number: 5\noffice_code: \"B\"\noffice_id: \"{}\"\nreserved_through: 32\nschema_version: 1\n",
            id(3)
        )
    );
    assert_eq!(decode_counter(&bytes).unwrap(), (id(3), counter.clone()));
    let behind = encode_counter(
        &id(3),
        &OfficeTaskCounterRecord {
            reserved_through: 4,
            ..counter
        },
    );
    assert!(decode_counter(&behind).is_err());
}
