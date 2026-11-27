//! Unit tests for universal reviews key parsing, durable path parsing, and event digest integrity.

use org_store::{DurablePath, Id};

use super::files::{event_path, parse_path, review_path, round_path, ReviewFileKind};
use super::records::{decode_event, encode_event, parse_review_key, ReviewEventRecord};

const SAMPLE_ID: &str = "01944000-0000-7000-8000-000000000042";

#[test]
fn key_path_and_event_hash_parsing_unit_checks() {
    assert_eq!(parse_review_key("RV-A1"), Some(("A".into(), 1)));
    assert_eq!(parse_review_key("rv-ab42"), Some(("AB".into(), 42)));
    assert_eq!(parse_review_key("RV-A01"), None);
    assert_eq!(parse_review_key("RV-I1"), None);
    assert_eq!(parse_review_key("RV-O1"), None);
    assert_eq!(parse_review_key("T-A1"), None);
    // Multibyte UTF-8 strings straddling byte offset 3 must never panic (P0-1).
    for bad in ["🚀1", "éé", "🔥a", "日本語", "RV-🚀1"] {
        assert_eq!(parse_review_key(bad), None, "multibyte {bad}");
    }
    // Git ref metacharacters & leading dash refused for branches (P2-4).
    for bad_branch in ["-main", "feat@{0}", "a~1", "a^2", "a:b", "a\\b", "a..b"] {
        assert!(
            super::validation::validate_ref_token(bad_branch, true, "branch").is_err(),
            "branch {bad_branch} must be rejected"
        );
    }
    assert!(super::validation::validate_ref_token("task/T-A42", true, "branch").is_ok());

    let rid = Id::parse(SAMPLE_ID).unwrap();
    assert_eq!(
        parse_path(&review_path(&rid)),
        Some(ReviewFileKind::Review(rid.clone()))
    );
    assert_eq!(
        parse_path(&round_path(&rid, 7)),
        Some(ReviewFileKind::Round(rid.clone(), 7))
    );
    assert_eq!(
        parse_path(&event_path(&rid, 12)),
        Some(ReviewFileKind::Event(rid.clone(), 12))
    );
    assert_eq!(
        parse_path(&DurablePath::parse(&format!("state/reviews/{rid}/rounds/1.json")).unwrap()),
        None
    );
    assert_eq!(
        parse_path(
            &DurablePath::parse(&format!("state/reviews/{rid}/events/000001.json")).unwrap()
        ),
        None
    );

    let ev = ReviewEventRecord {
        review_id: rid.clone(),
        revision: 1,
        event_type: "review_requested".into(),
        actor_id: rid,
        operation_id: None,
        payload_json: "{\"k\":1}".into(),
        payload_sha256: "0".repeat(64),
        created_at: "2026-11-27T00:00:00.000Z".into(),
    };
    let bytes = encode_event(&ev);
    assert!(decode_event(&bytes).is_err());
}
