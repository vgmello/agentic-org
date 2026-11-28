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
    // Doc/artifact target_ref must also refuse leading `-` and `..` traversal (P1-2).
    for bad_ref in [
        "-flag",
        "--option",
        "-",
        " -padded",
        "../secret.md",
        "docs/../etc/passwd",
        "..",
    ] {
        assert!(
            super::validation::validate_ref_token(bad_ref, false, "target_ref").is_err(),
            "target_ref {bad_ref} must be rejected"
        );
    }
    for good_ref in [
        "docs/design-v1.md",
        "attachments/report-v1.pdf",
        "Design Doc v2",
    ] {
        assert!(
            super::validation::validate_ref_token(good_ref, false, "target_ref").is_ok(),
            "target_ref {good_ref} must be accepted"
        );
    }

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

#[test]
fn memory_projector_rejects_round_submitter_as_reviewer_and_approved_with_blocker() {
    use org_proto::reviews::{
        FindingAnchor, FindingSeverity, FindingStatus, ReviewSessionMode, ReviewStatus,
        ReviewTargetKind, RoutingMode,
    };
    use org_store::testkit::MemoryEngine;
    use org_store::{Engine, Projector};

    use super::files::ReviewFiles;
    use super::records::{
        compute_event_sha256, encode_review_doc, encode_versioned, ReviewDocument,
        ReviewFindingRecord, ReviewRecord, ReviewReviewerRecord, ReviewRoundRecord,
    };

    let oid = Id::parse("01944000-0000-7000-8000-000000000001").unwrap();
    let author = Id::parse("01944000-0000-7000-8000-000000000010").unwrap();
    let submitter = Id::parse("01944000-0000-7000-8000-000000000011").unwrap();
    let reviewer = Id::parse("01944000-0000-7000-8000-000000000012").unwrap();
    let rid = Id::parse(SAMPLE_ID).unwrap();
    let fid = Id::parse("01944000-0000-7000-8000-000000000050").unwrap();
    let ts = "2026-11-27T00:00:00.000Z";
    let proj = ReviewFiles::new(oid.clone());

    let make_ev = |rev: u64| ReviewEventRecord {
        payload_sha256: compute_event_sha256(
            &rid,
            rev,
            "ev",
            &author,
            None,
            &serde_json::json!({}),
            ts,
        ),
        review_id: rid.clone(),
        revision: rev,
        event_type: "ev".into(),
        actor_id: author.clone(),
        operation_id: None,
        payload_json: "{}".into(),
        created_at: ts.into(),
    };
    let base_review = ReviewRecord {
        id: rid.clone(),
        office_id: oid,
        office_code: "A".into(),
        number: 1,
        project_id: None,
        task_id: None,
        target_kind: ReviewTargetKind::Doc,
        repository_id: None,
        branch: None,
        target_ref: Some("docs/spec.md".into()),
        kind_slug: "spec".into(),
        category_slug: None,
        title: "Spec review".into(),
        body_markdown: String::new(),
        independent: true,
        status: ReviewStatus::Open,
        current_round: 2,
        session_mode: ReviewSessionMode::Fresh,
        guidance_refs: Vec::new(),
        criteria: Vec::new(),
        closed_reason: None,
        revision: 1,
        created_by: author.clone(),
        created_at: ts.into(),
        updated_at: ts.into(),
        closed_at: None,
    };
    let r1 = ReviewRoundRecord {
        review_id: rid.clone(),
        round_number: 1,
        base_oid: None,
        head_oid: None,
        session_mode: ReviewSessionMode::Fresh,
        submitted_by: author.clone(),
        submitted_at: ts.into(),
        note_markdown: String::new(),
    };
    let r2 = ReviewRoundRecord {
        submitted_by: submitter.clone(),
        round_number: 2,
        ..r1.clone()
    };

    // Case 1: round 2 submitter assigned as active reviewer on independent review fails validate().
    let mem = MemoryEngine::new();
    let mut tx = mem.begin().unwrap();
    <ReviewFiles as Projector<MemoryEngine>>::apply(
        &proj,
        &mut tx,
        &round_path(&rid, 1),
        Some(&encode_versioned(&r1)),
    )
    .unwrap();
    <ReviewFiles as Projector<MemoryEngine>>::apply(
        &proj,
        &mut tx,
        &round_path(&rid, 2),
        Some(&encode_versioned(&r2)),
    )
    .unwrap();
    <ReviewFiles as Projector<MemoryEngine>>::apply(
        &proj,
        &mut tx,
        &event_path(&rid, 1),
        Some(&encode_event(&make_ev(1))),
    )
    .unwrap();
    let doc_bad_indep = ReviewDocument {
        review: base_review.clone(),
        reviewers: vec![ReviewReviewerRecord::assigned(
            rid.clone(),
            2,
            submitter,
            RoutingMode::Explicit,
            ReviewSessionMode::Fresh,
            author.clone(),
            ts.into(),
        )],
        findings: Vec::new(),
        replies: Vec::new(),
    };
    <ReviewFiles as Projector<MemoryEngine>>::apply(
        &proj,
        &mut tx,
        &review_path(&rid),
        Some(&encode_review_doc(&doc_bad_indep)),
    )
    .unwrap();
    assert!(<ReviewFiles as Projector<MemoryEngine>>::validate(&proj, &tx).is_err());

    // Case 2: Approved review with open Blocker finding fails validate().
    let mem2 = MemoryEngine::new();
    let mut tx2 = mem2.begin().unwrap();
    <ReviewFiles as Projector<MemoryEngine>>::apply(
        &proj,
        &mut tx2,
        &round_path(&rid, 1),
        Some(&encode_versioned(&r1)),
    )
    .unwrap();
    <ReviewFiles as Projector<MemoryEngine>>::apply(
        &proj,
        &mut tx2,
        &round_path(&rid, 2),
        Some(&encode_versioned(&r2)),
    )
    .unwrap();
    <ReviewFiles as Projector<MemoryEngine>>::apply(
        &proj,
        &mut tx2,
        &event_path(&rid, 1),
        Some(&encode_event(&make_ev(1))),
    )
    .unwrap();
    let mut approved = base_review;
    approved.status = ReviewStatus::Approved;
    let doc_bad_blocker = ReviewDocument {
        review: approved,
        reviewers: vec![ReviewReviewerRecord::assigned(
            rid.clone(),
            2,
            reviewer.clone(),
            RoutingMode::Explicit,
            ReviewSessionMode::Fresh,
            author,
            ts.into(),
        )],
        findings: vec![ReviewFindingRecord {
            id: fid,
            review_id: rid.clone(),
            number: 1,
            round_number: 2,
            author_id: reviewer,
            severity: FindingSeverity::Blocker,
            status: FindingStatus::Open,
            anchor: FindingAnchor::general(),
            body_markdown: "Blocker still open".into(),
            attachment_ids: Vec::new(),
            resolved_by: None,
            resolved_at: None,
            resolved_in_round: None,
            revision: 1,
            created_at: ts.into(),
            updated_at: ts.into(),
        }],
        replies: Vec::new(),
    };
    <ReviewFiles as Projector<MemoryEngine>>::apply(
        &proj,
        &mut tx2,
        &review_path(&rid),
        Some(&encode_review_doc(&doc_bad_blocker)),
    )
    .unwrap();
    assert!(<ReviewFiles as Projector<MemoryEngine>>::validate(&proj, &tx2).is_err());
}
