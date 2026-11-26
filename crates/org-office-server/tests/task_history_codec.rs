//! The office writes task history in the workspace's one `canonical-json/2` codec
//! (`doc-79`, `ADR-0006`): every event `org-office` encodes verifies, byte for byte, with
//! `org-work`'s `selected_history` (the event admission codec), and `org-work`'s producer
//! output verifies with the office's reader. The two crates share no dependency edge, so this
//! crate, which links both, holds the conformance check.

use org_office::tasks::records::{created_change, encode_event, verify_event, EventInput};
use org_store::Id;
use org_work::selected_codec;
use org_work::selected_history::{self, Admission, EventFields, ExpectedIdentity};
use serde_json::{json, Map};

fn id(n: u64) -> Id {
    Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
}

fn office_event(revision: u64, change: serde_json::Value) -> Vec<u8> {
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

fn expected(filename: &str) -> ExpectedIdentity<'_> {
    ExpectedIdentity {
        organization_id: "01900000-0000-7000-8000-000000000001",
        task_id: "01900000-0000-7000-8000-000000000002",
        filename,
    }
}

#[test]
fn every_event_the_office_writes_verifies_with_the_event_admission_codec() {
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
        let filename = format!("{revision}.json");
        let verified = selected_history::verify(&bytes, expected(&filename), Admission::New)
            .unwrap_or_else(|e| panic!("revision {revision}: {e}"));
        assert_eq!(verified.revision(), revision as i64);
        // The office's bytes are already the codec's canonical form.
        let reparsed = selected_codec::parse(&bytes).unwrap();
        assert_eq!(selected_codec::canonical(&reparsed).unwrap(), bytes);
    }
}

#[test]
fn an_event_the_admission_codec_produces_verifies_with_the_office_reader() {
    let change =
        selected_codec::parse(br#"{"type":"status_changed","from":"todo","to":"done"}"#).unwrap();
    let bytes = selected_history::encode(
        EventFields {
            organization_id: id(1).as_str().to_owned(),
            task_id: id(2).as_str().to_owned(),
            revision: 4,
            occurred_at: Some("2026-11-26T12:34:56.789Z".into()),
            actor_kind: "employee".into(),
            actor_id: Some(id(3).as_str().to_owned()),
            operation_id: Some("op".into()),
            change,
            source_metadata: Default::default(),
            extensions: Default::default(),
        },
        Admission::New,
    )
    .unwrap();
    let record = verify_event(&bytes, &id(1), &id(2), 4).expect("the office reads it");
    assert_eq!(record.revision, 4);
    assert_eq!(record.actor_id, Some(id(3)));
    // The office and the codec agree on the checksum.
    let value = selected_codec::parse(&bytes).unwrap();
    assert_eq!(
        selected_codec::payload_sha256(&value).unwrap(),
        record.payload_sha256
    );
    // A tampered event fails both readers.
    let tampered = String::from_utf8(bytes).unwrap().replace("done", "gone");
    assert!(verify_event(tampered.as_bytes(), &id(1), &id(2), 4).is_err());
    assert!(
        selected_history::verify(tampered.as_bytes(), expected("4.json"), Admission::New).is_err()
    );
}
