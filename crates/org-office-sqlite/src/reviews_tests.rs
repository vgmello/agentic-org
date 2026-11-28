//! Direct SQLite adapter tests for universal review triggers and `put_findings` PK point lookup.

use org_office::reviews::ports::{ReviewsRead, ReviewsWrite};
use org_office::reviews::records::{
    ReviewDocument, ReviewFindingRecord, ReviewRecord, ReviewReviewerRecord, ReviewRoundRecord,
};
use org_proto::reviews::{
    FindingAnchor, FindingSeverity, FindingStatus, ReviewSessionMode, ReviewStatus,
    ReviewTargetKind, RoutingMode,
};
use org_store::{testkit::engine_suite::cursor, Engine, Id};

use super::SqliteEngine;

const ORG: &str = "01900000-0000-7000-8000-0000000000a2";
const OFFICE: &str = "01900000-0000-7000-8000-0000000000b1";
const TEAM: &str = "01900000-0000-7000-8000-0000000000c1";
const AUTHOR: &str = "01900000-0000-7000-8000-0000000000d1";
const SUBMITTER: &str = "01900000-0000-7000-8000-0000000000d2";
const REVIEWER: &str = "01900000-0000-7000-8000-0000000000d3";
const REVIEW_1: &str = "01900000-0000-7000-8000-0000000000e1";
const REVIEW_2: &str = "01900000-0000-7000-8000-0000000000e2";
const FINDING_1: &str = "01900000-0000-7000-8000-0000000000f1";
const TS: &str = "2026-11-27T00:00:00.000Z";

fn id(s: &str) -> Id {
    Id::parse(s).expect("valid id")
}

fn open_seeded() -> (SqliteEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canon");
    let engine = SqliteEngine::open(&root.join("office.db"))
        .expect("open sqlite")
        .for_organization(&id(ORG));
    let tx = engine.begin().expect("begin");
    let sql = format!(
        "INSERT INTO organizations (id, slug, schema_version, revision, name, organization_repository_id, state_branch)
           VALUES ('{ORG}', 'org', 1, 1, 'Org', 'repo', 'org/state');
         INSERT INTO repositories (organization_id, id, schema_version, revision, kind, remote_url)
           VALUES ('{ORG}', 'repo', 1, 1, 'remote', 'https://example.test/o.git');
         INSERT INTO offices (organization_id, id, slug, schema_version, revision, name, role)
           VALUES ('{ORG}', '{OFFICE}', 'hq', 1, 1, 'HQ', 'hq');
         INSERT INTO teams (organization_id, id, number, schema_version, revision, name, status)
           VALUES ('{ORG}', '{TEAM}', 1, 1, 1, 'Eng', 'active');
         INSERT INTO employees (organization_id, id, handle, schema_version, revision, kind, name, status, home_team_id)
           VALUES ('{ORG}', '{AUTHOR}', 'author', 1, 1, 'agent', 'Author', 'active', '{TEAM}'),
                  ('{ORG}', '{SUBMITTER}', 'submitter', 1, 1, 'agent', 'Submitter', 'active', '{TEAM}'),
                  ('{ORG}', '{REVIEWER}', 'reviewer', 1, 1, 'agent', 'Reviewer', 'active', '{TEAM}');"
    );
    tx.db().execute_batch(&sql).expect("seed org/employees");
    engine.commit(tx, &cursor(1)).expect("commit seed");
    (engine, dir)
}

fn sample_review(rid: &str, number: u64, independent: bool) -> ReviewRecord {
    ReviewRecord {
        id: id(rid),
        office_id: id(OFFICE),
        office_code: "A".into(),
        number,
        project_id: None,
        task_id: None,
        target_kind: ReviewTargetKind::Doc,
        repository_id: None,
        branch: None,
        target_ref: Some("docs/spec.md".into()),
        kind_slug: "spec".into(),
        category_slug: None,
        title: format!("Review {number}"),
        body_markdown: String::new(),
        independent,
        status: ReviewStatus::Open,
        current_round: 1,
        session_mode: ReviewSessionMode::Fresh,
        guidance_refs: Vec::new(),
        criteria: Vec::new(),
        closed_reason: None,
        revision: 1,
        created_by: id(AUTHOR),
        created_at: TS.into(),
        updated_at: TS.into(),
        closed_at: None,
    }
}

fn sample_round(rid: &str, round_number: u32, submitted_by: &str) -> ReviewRoundRecord {
    ReviewRoundRecord {
        review_id: id(rid),
        round_number,
        base_oid: None,
        head_oid: None,
        session_mode: ReviewSessionMode::Fresh,
        submitted_by: id(submitted_by),
        submitted_at: TS.into(),
        note_markdown: String::new(),
    }
}

fn sample_reviewer(
    rid: &str,
    round_number: u32,
    reviewer_id: &str,
    removed: bool,
) -> ReviewReviewerRecord {
    let mut row = ReviewReviewerRecord::assigned(
        id(rid),
        round_number,
        id(reviewer_id),
        RoutingMode::Explicit,
        ReviewSessionMode::Fresh,
        id(AUTHOR),
        TS.into(),
    );
    row.removed = removed;
    row
}

fn sample_finding(fid: &str, rid: &str) -> ReviewFindingRecord {
    ReviewFindingRecord {
        id: id(fid),
        review_id: id(rid),
        number: 1,
        round_number: 1,
        author_id: id(REVIEWER),
        severity: FindingSeverity::Blocker,
        status: FindingStatus::Open,
        anchor: FindingAnchor::general(),
        body_markdown: "Fix issue".into(),
        attachment_ids: Vec::new(),
        resolved_by: None,
        resolved_at: None,
        resolved_in_round: None,
        revision: 1,
        created_at: TS.into(),
        updated_at: TS.into(),
    }
}

#[test]
fn sqlite_put_findings_short_circuits_empty_and_rejects_cross_review_collision() {
    let (engine, _dir) = open_seeded();
    let mut tx = engine.begin().expect("begin");
    tx.put_review_round(&sample_round(REVIEW_1, 1, AUTHOR))
        .expect("round 1");
    tx.put_review_document(&ReviewDocument {
        review: sample_review(REVIEW_1, 1, true),
        reviewers: vec![sample_reviewer(REVIEW_1, 1, REVIEWER, false)],
        findings: vec![sample_finding(FINDING_1, REVIEW_1)],
        replies: Vec::new(),
    })
    .expect("review 1 with finding");
    // Empty findings short-circuits cleanly on review 2.
    tx.put_review_round(&sample_round(REVIEW_2, 1, AUTHOR))
        .expect("round 2");
    tx.put_review_document(&ReviewDocument {
        review: sample_review(REVIEW_2, 2, false),
        reviewers: vec![],
        findings: vec![],
        replies: Vec::new(),
    })
    .expect("review 2 empty findings");
    // Reusing FINDING_1 on REVIEW_2 must fail via PK point lookup.
    let err = tx
        .put_review_document(&ReviewDocument {
            review: sample_review(REVIEW_2, 2, false),
            reviewers: vec![],
            findings: vec![sample_finding(FINDING_1, REVIEW_2)],
            replies: Vec::new(),
        })
        .unwrap_err();
    assert!(
        err.to_string().contains("belongs to another review"),
        "unexpected error: {err}"
    );
    // Updating FINDING_1 on its owning REVIEW_1 still succeeds.
    let mut updated = sample_finding(FINDING_1, REVIEW_1);
    updated.revision = 2;
    tx.put_review_document(&ReviewDocument {
        review: sample_review(REVIEW_1, 1, true),
        reviewers: vec![sample_reviewer(REVIEW_1, 1, REVIEWER, false)],
        findings: vec![updated],
        replies: Vec::new(),
    })
    .expect("owning update succeeds");
    assert_eq!(
        tx.review_finding_by_id(&id(FINDING_1))
            .expect("lookup")
            .expect("exists")
            .revision,
        2
    );
}

#[test]
fn sqlite_independent_triggers_enforce_round_submitted_by_parity() {
    let (engine, _dir) = open_seeded();
    // Case 1: round inserted first (submitted_by = SUBMITTER), then active reviewer INSERT must fail.
    {
        let mut tx = engine.begin().expect("begin");
        tx.put_review_round(&sample_round(REVIEW_1, 1, AUTHOR))
            .expect("r1");
        tx.put_review_round(&sample_round(REVIEW_1, 2, SUBMITTER))
            .expect("r2");
        let mut r = sample_review(REVIEW_1, 1, true);
        r.current_round = 2;
        let err = tx
            .put_review_document(&ReviewDocument {
                review: r.clone(),
                reviewers: vec![sample_reviewer(REVIEW_1, 2, SUBMITTER, false)],
                findings: Vec::new(),
                replies: Vec::new(),
            })
            .unwrap_err();
        assert!(
            err.to_string().contains("round submitter"),
            "insert trigger should block round submitter: {err}"
        );
        // Removed reviewer row is allowed, but un-removing via UPDATE must fail.
        tx.put_review_document(&ReviewDocument {
            review: r.clone(),
            reviewers: vec![sample_reviewer(REVIEW_1, 2, SUBMITTER, true)],
            findings: Vec::new(),
            replies: Vec::new(),
        })
        .expect("removed row allowed");
        let err = tx
            .put_review_document(&ReviewDocument {
                review: r,
                reviewers: vec![sample_reviewer(REVIEW_1, 2, SUBMITTER, false)],
                findings: Vec::new(),
                replies: Vec::new(),
            })
            .unwrap_err();
        assert!(
            err.to_string().contains("round submitter"),
            "update trigger should block un-removing round submitter: {err}"
        );
    }
    // Case 2: active reviewer inserted first, then round inserted with submitted_by == reviewer_id must fail.
    {
        let mut tx = engine.begin().expect("begin");
        let mut r = sample_review(REVIEW_2, 2, true);
        r.current_round = 2;
        tx.put_review_document(&ReviewDocument {
            review: r,
            reviewers: vec![sample_reviewer(REVIEW_2, 2, SUBMITTER, false)],
            findings: Vec::new(),
            replies: Vec::new(),
        })
        .expect("deferred reviewer insert before round");
        let err = tx
            .put_review_round(&sample_round(REVIEW_2, 2, SUBMITTER))
            .unwrap_err();
        assert!(
            err.to_string().contains("round submitter"),
            "round insert trigger should reject submitter == active reviewer: {err}"
        );
    }
}
