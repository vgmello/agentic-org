use serde_json::json;

use super::*;

const SECRETS: [&str; 5] = [
    "TOP_SECRET_TITLE",
    "TOP_SECRET_WHY",
    "TOP_SECRET_ALTERNATIVES",
    "top-secret-tag",
    "TOP_SECRET_AGENT_REASONING",
];

fn assert_redacted(rendered: &str) {
    for secret in SECRETS {
        assert!(!rendered.contains(secret), "{secret} leaked: {rendered}");
    }
}

fn wire() -> PrivateNoteWire {
    PrivateNoteWire {
        id: "0193a5c0-0000-7000-8000-000000000010".into(),
        task_id: Some("0193a5c0-0000-7000-8000-000000000011".into()),
        project_id: None,
        author_id: "0193a5c0-0000-7000-8000-000000000012".into(),
        kind: PrivateNoteKind::Decision,
        revision: 1,
        title: Some(SECRETS[0].into()),
        why: Some(SECRETS[1].into()),
        alternatives: Some(SECRETS[2].into()),
        tags: vec![SECRETS[3].into()],
        body_markdown: Some(SECRETS[4].into()),
        created_at: "2026-11-22T00:00:00.000Z".into(),
        updated_at: "2026-11-22T00:00:00.000Z".into(),
        editable: true,
    }
}

#[test]
fn debug_output_of_every_private_note_shape_redacts_private_text() {
    let note = wire();
    let dbg = format!("{note:?}");
    assert_redacted(&dbg);
    assert!(dbg.contains("<redacted>") && dbg.contains("<1 redacted>"));
    assert!(dbg.contains("Decision"), "kind stays visible: {dbg}");

    let list = TasksNotesListResult {
        task_id: None,
        notes: vec![note.clone()],
        total: 1,
        next_cursor: None,
    };
    assert_redacted(&format!("{list:?}"));
    assert_redacted(&format!("{:?}", TaskNoteResult { note }));

    let add = PrivateNoteAddParams {
        command_id: "0193a5c0-0000-7000-8000-000000000013".into(),
        note_id: "0193a5c0-0000-7000-8000-000000000010".into(),
        task: None,
        project_id: None,
        kind: PrivateNoteKind::Decision,
        title: Some(SECRETS[0].into()),
        why: Some(SECRETS[1].into()),
        alternatives: Some(SECRETS[2].into()),
        tags: vec![SECRETS[3].into()],
        body_markdown: SECRETS[4].into(),
    };
    assert_redacted(&format!("{add:?}"));

    let update = PrivateNoteUpdateParams {
        command_id: add.command_id.clone(),
        note_id: add.note_id.clone(),
        expected_revision: 1,
        kind: None,
        title: Some(SECRETS[0].into()),
        why: Some(SECRETS[1].into()),
        alternatives: Some(SECRETS[2].into()),
        tags: Some(vec![SECRETS[3].into()]),
        body_markdown: Some(SECRETS[4].into()),
    };
    assert_redacted(&format!("{update:?}"));

    let list_params = PrivateNoteListParams {
        tag: Some(SECRETS[3].into()),
        ..PrivateNoteListParams::default()
    };
    assert_redacted(&format!("{list_params:?}"));

    let legacy_add = TasksNotesAddParams {
        command_id: add.command_id.clone(),
        task: "T-A1".into(),
        note_id: add.note_id.clone(),
        body_markdown: SECRETS[4].into(),
    };
    assert_redacted(&format!("{legacy_add:?}"));
    let legacy_update = TasksNotesUpdateParams {
        command_id: add.command_id,
        note_id: add.note_id,
        expected_revision: 1,
        body_markdown: SECRETS[4].into(),
    };
    assert_redacted(&format!("{legacy_update:?}"));
}

#[test]
fn task_scoped_cli_requests_decode_as_the_canonical_params() {
    let legacy = TasksNotesAddParams {
        command_id: "c".into(),
        task: "T-A1".into(),
        note_id: "n".into(),
        body_markdown: "body".into(),
    };
    let decoded: PrivateNoteAddParams =
        serde_json::from_value(serde_json::to_value(&legacy).unwrap()).unwrap();
    assert_eq!(decoded, PrivateNoteAddParams::from(legacy));
    assert_eq!(decoded.kind, PrivateNoteKind::Note);
    assert!(decoded.tags.is_empty() && decoded.title.is_none());

    let legacy_list = TasksNotesListParams {
        task: "T-A1".into(),
        author_id: Some("a".into()),
    };
    let decoded: PrivateNoteListParams =
        serde_json::from_value(serde_json::to_value(&legacy_list).unwrap()).unwrap();
    assert_eq!(decoded, PrivateNoteListParams::from(legacy_list));

    let legacy_update = TasksNotesUpdateParams {
        command_id: "c".into(),
        note_id: "n".into(),
        expected_revision: 2,
        body_markdown: "edited".into(),
    };
    let decoded: PrivateNoteUpdateParams =
        serde_json::from_value(serde_json::to_value(&legacy_update).unwrap()).unwrap();
    assert_eq!(decoded, PrivateNoteUpdateParams::from(legacy_update));
    assert_eq!(decoded.body_markdown.as_deref(), Some("edited"));
    assert!(decoded.tags.is_none() && decoded.kind.is_none());
}

#[test]
fn task_scoped_answers_still_decode_and_pruned_fields_are_omitted() {
    // A `TASK-50` era answer (string task_id, no kind/tags/total) still decodes.
    let old: TasksNotesListResult = serde_json::from_value(json!({
        "task_id": "0193a5c0-0000-7000-8000-000000000011",
        "notes": [{
            "id": "0193a5c0-0000-7000-8000-000000000010",
            "task_id": "0193a5c0-0000-7000-8000-000000000011",
            "author_id": "0193a5c0-0000-7000-8000-000000000012",
            "revision": 1,
            "body_markdown": "x",
            "created_at": "2026-11-22T00:00:00.000Z",
            "updated_at": "2026-11-22T00:00:00.000Z",
            "editable": true
        }]
    }))
    .unwrap();
    assert_eq!(old.notes[0].kind, PrivateNoteKind::Note);
    assert_eq!(old.total, 0);

    let mut pruned = wire();
    pruned.task_id = None;
    pruned.body_markdown = None;
    pruned.tags.clear();
    let value = serde_json::to_value(&pruned).unwrap();
    assert!(value.get("body_markdown").is_none());
    assert!(value.get("tags").is_none());
    assert_eq!(value["task_id"], serde_json::Value::Null);
    assert_eq!(value["kind"], "decision");
}

#[test]
fn kinds_and_fields_round_trip_their_wire_spelling() {
    for kind in [
        PrivateNoteKind::Note,
        PrivateNoteKind::Plan,
        PrivateNoteKind::Decision,
    ] {
        assert_eq!(PrivateNoteKind::parse(kind.as_str()), Some(kind));
        assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
    }
    assert_eq!(PrivateNoteKind::parse("Decision"), None);
    for field in PrivateNoteField::ALL {
        assert_eq!(PrivateNoteField::parse(field.as_str()), Some(field));
        assert_eq!(serde_json::to_value(field).unwrap(), field.as_str());
    }
    assert_eq!(PrivateNoteField::parse("id"), None);
    let bad: Result<PrivateNoteListParams, _> = serde_json::from_value(json!({"fields": ["body"]}));
    assert!(bad.is_err(), "unknown fields are refused");
}
