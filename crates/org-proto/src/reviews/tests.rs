use serde_json::{json, Value};

use super::*;
use crate::attachments::ATTACHMENT_METHODS;

const OID_A: &str = "0123456789abcdef0123456789abcdef01234567";
const OID_B: &str = "89abcdef0123456789abcdef0123456789abcdef";
const SNIPPET: &str = "050276e596256d403e5eccd121ee403e170ec5b6e5e4a7baf482d3a2cbfc43b3";
const CMD: &str = "0193a5c0-0000-7000-8000-000000000001";

/// Decodes `value` as `T` and encodes it back unchanged.
fn round_trip<T: serde::de::DeserializeOwned + serde::Serialize>(value: &Value) -> T {
    let decoded: T = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(&serde_json::to_value(&decoded).unwrap(), value);
    decoded
}

/// Both halves of a call decode and re-encode unchanged.
fn call_round_trip<C: Call>(params: &Value, result: &Value) {
    round_trip::<C::Params>(params);
    round_trip::<C::Result>(result);
}

fn review_json() -> Value {
    json!({
        "id": "0193a5c0-0000-7000-8000-0000000000aa",
        "key": "RV-A12",
        "office_code": "A",
        "number": 12,
        "organization_id": "org-1",
        "project_id": "proj-1",
        "task_id": "task-1",
        "target_kind": "code",
        "repository_id": "repo-1",
        "branch": "task/T-A1-login",
        "kind_slug": "code-review",
        "category_slug": "security",
        "title": "Review the login fix",
        "body_markdown": "Please check the session handling.",
        "independent": true,
        "status": "changes_requested",
        "current_round": 2,
        "pin_stale": false,
        "guidance_refs": [".agents/rules/rust-security-and-modularity.md"],
        "criteria": ["No secrets in logs"],
        "open_findings": 1,
        "revision": 7,
        "created_by": "emp-author",
        "created_at": "2026-11-27T10:00:00.000Z",
        "updated_at": "2026-11-27T11:00:00.000Z"
    })
}

fn finding_json() -> Value {
    json!({
        "id": "0193a5c0-0000-7000-8000-0000000000f1",
        "review_id": "0193a5c0-0000-7000-8000-0000000000aa",
        "number": 1,
        "round_number": 1,
        "severity": "blocker",
        "status": "resolved",
        "anchor": {
            "kind": "file_line",
            "path": "src/session.rs",
            "start_line": 40,
            "end_line": 44,
            "anchor_snippet_sha256": SNIPPET
        },
        "anchor_stale": true,
        "body_markdown": "The token is logged.",
        "author_id": "emp-reviewer",
        "attachment_ids": ["att-1"],
        "resolved_by": "emp-author",
        "resolved_at": "2026-11-27T10:30:00.000Z",
        "resolved_in_round": 2,
        "replies": [{
            "id": "0193a5c0-0000-7000-8000-0000000000b1",
            "finding_id": "0193a5c0-0000-7000-8000-0000000000f1",
            "author_id": "emp-author",
            "body_markdown": "Redacted in round 2.",
            "set_status": "resolved",
            "created_at": "2026-11-27T10:30:00.000Z"
        }],
        "revision": 3,
        "created_at": "2026-11-27T10:05:00.000Z",
        "updated_at": "2026-11-27T10:30:00.000Z"
    })
}

fn round_json() -> Value {
    json!({
        "round_number": 2,
        "base_oid": OID_A,
        "head_oid": OID_B,
        "session_mode": "fresh",
        "submitted_by": "emp-author",
        "submitted_at": "2026-11-27T10:20:00.000Z",
        "note_markdown": "Fixed the logging.",
        "verdicts": [{
            "reviewer_id": "emp-reviewer",
            "round_number": 2,
            "verdict": "approve",
            "summary_markdown": "Looks good.",
            "finding_ids": ["0193a5c0-0000-7000-8000-0000000000f2"],
            "attachment_ids": ["att-2"],
            "submitted_at": "2026-11-27T10:40:00.000Z"
        }]
    })
}

#[test]
fn method_names_are_pinned_unique_and_in_the_review_family() {
    assert_eq!(
        REVIEW_METHODS,
        [
            "review.request",
            "review.list",
            "review.inspect",
            "review.diff",
            "review.route",
            "review.finding",
            "review.verdict",
            "review.resubmit",
            "review.close",
        ]
    );
    let mut seen = std::collections::BTreeSet::new();
    for method in REVIEW_METHODS {
        assert!(method.starts_with("review."), "{method}");
        assert!(seen.insert(method), "{method} twice");
        assert!(!ATTACHMENT_METHODS.contains(&method));
    }
}

macro_rules! assert_wire_names {
    ($kind:ty, [$(($value:expr, $name:literal)),+ $(,)?]) => {{
        let expected: Vec<($kind, &str)> = vec![$(($value, $name)),+];
        assert_eq!(<$kind>::ALL.len(), expected.len(), "{} coverage", stringify!($kind));
        for (value, name) in expected {
            assert_eq!(value.as_str(), name);
            assert_eq!(serde_json::to_value(value).unwrap(), json!(name));
            assert_eq!(serde_json::from_value::<$kind>(json!(name)).unwrap(), value);
            assert_eq!(<$kind>::parse(name), Some(value));
            assert_eq!(<$kind>::parse(&name.to_uppercase()), None);
        }
        assert!(serde_json::from_value::<$kind>(json!("unknown_value")).is_err());
    }};
}

#[test]
fn every_enum_has_pinned_snake_case_wire_names() {
    assert_wire_names!(
        ReviewStatus,
        [
            (ReviewStatus::Open, "open"),
            (ReviewStatus::ChangesRequested, "changes_requested"),
            (ReviewStatus::Approved, "approved"),
            (ReviewStatus::Closed, "closed"),
        ]
    );
    assert_wire_names!(
        ReviewTargetKind,
        [
            (ReviewTargetKind::Code, "code"),
            (ReviewTargetKind::Doc, "doc"),
            (ReviewTargetKind::Task, "task"),
            (ReviewTargetKind::Artifact, "artifact"),
        ]
    );
    assert_wire_names!(
        ReviewSessionMode,
        [
            (ReviewSessionMode::Warm, "warm"),
            (ReviewSessionMode::Fresh, "fresh"),
        ]
    );
    assert_wire_names!(
        ReviewVerdict,
        [
            (ReviewVerdict::Approve, "approve"),
            (ReviewVerdict::ChangesRequested, "changes_requested"),
            (ReviewVerdict::Comment, "comment"),
            (ReviewVerdict::Incomplete, "incomplete"),
        ]
    );
    assert_wire_names!(
        FindingSeverity,
        [
            (FindingSeverity::Blocker, "blocker"),
            (FindingSeverity::Major, "major"),
            (FindingSeverity::Minor, "minor"),
            (FindingSeverity::Nit, "nit"),
            (FindingSeverity::Question, "question"),
        ]
    );
    assert_wire_names!(
        FindingStatus,
        [
            (FindingStatus::Open, "open"),
            (FindingStatus::Resolved, "resolved"),
            (FindingStatus::WontFix, "wont_fix"),
        ]
    );
    assert_wire_names!(
        AnchorKind,
        [
            (AnchorKind::General, "general"),
            (AnchorKind::FileLine, "file_line"),
            (AnchorKind::DocSection, "doc_section"),
            (AnchorKind::ChecklistItem, "checklist_item"),
        ]
    );
    assert_wire_names!(
        RoutingMode,
        [
            (RoutingMode::Explicit, "explicit"),
            (RoutingMode::Pool, "pool"),
            (RoutingMode::SingleAgent, "single_agent"),
        ]
    );
    assert_wire_names!(
        ReviewDiffFileStatus,
        [
            (ReviewDiffFileStatus::Added, "added"),
            (ReviewDiffFileStatus::Modified, "modified"),
            (ReviewDiffFileStatus::Deleted, "deleted"),
            (ReviewDiffFileStatus::TypeChanged, "type_changed"),
        ]
    );
    assert_eq!(ReviewSessionMode::default(), ReviewSessionMode::Warm);
    assert_eq!(AnchorKind::default(), AnchorKind::General);
}

#[test]
fn request_list_and_inspect_round_trip() {
    let review = review_json();
    call_round_trip::<ReviewRequest>(
        &json!({
            "command_id": CMD,
            "review_id": "0193a5c0-0000-7000-8000-0000000000aa",
            "target_kind": "code",
            "title": "Review the login fix",
            "body_markdown": "Please check the session handling.",
            "kind_slug": "code-review",
            "category_slug": "security",
            "project_id": "proj-1",
            "task_id": "task-1",
            "repository_id": "repo-1",
            "branch": "task/T-A1-login",
            "base_oid": OID_A,
            "expected_head_oid": OID_B,
            "independent": true,
            "routing": {"mode": "explicit", "reviewer_ids": ["emp-reviewer"]},
            "session_mode": "fresh",
            "guidance_refs": ["doc-35"],
            "criteria": ["No secrets in logs"]
        }),
        &json!({"review": review, "round": round_json()}),
    );
    call_round_trip::<ReviewList>(
        &json!({
            "project_id": "proj-1",
            "status": "open",
            "target_kind": "doc",
            "reviewer_id": "emp-reviewer",
            "limit": 50,
            "offset": 100
        }),
        &json!({"reviews": [review], "total": 151}),
    );
    let inspect: ReviewInspectResult = round_trip(&json!({
        "review": review,
        "rounds": [round_json()],
        "reviewers": [{
            "reviewer_id": "emp-reviewer",
            "routing": "pool",
            "team_id": "team-security",
            "session_mode": "fresh",
            "assigned_by": "emp-author",
            "assigned_at": "2026-11-27T10:00:00.000Z",
            "last_verdict": "changes_requested",
            "last_verdict_round": 1,
            "removed": true
        }],
        "findings": [finding_json()]
    }));
    assert_eq!(inspect.review.key, "RV-A12");
    assert_eq!(inspect.findings[0].anchor.last_line(), Some(44));
    assert_eq!(inspect.findings[0].anchor.check(), Ok(()));
    round_trip::<ReviewInspectParams>(&json!({"review": "RV-A12"}));
}

#[test]
fn diff_route_and_close_round_trip() {
    call_round_trip::<ReviewDiff>(
        &json!({
            "review": "RV-A12",
            "round": 2,
            "since_round": 1,
            "path": "src",
            "stat_only": true,
            "context_lines": 5
        }),
        &json!({
            "review_id": "0193a5c0-0000-7000-8000-0000000000aa",
            "round_number": 2,
            "since_round": 1,
            "base_oid": OID_A,
            "head_oid": OID_B,
            "files": [
                {"path": "src/session.rs", "status": "modified", "added": 3, "deleted": 1},
                {"path": "assets/logo.png", "status": "added", "added": 0, "deleted": 0, "binary": true}
            ],
            "total_files": 1200,
            "files_truncated": true,
            "unified_diff": "diff --git a/src/session.rs b/src/session.rs\n",
            "truncated": true
        }),
    );
    call_round_trip::<ReviewRoute>(
        &json!({
            "command_id": CMD,
            "review": "RV-A12",
            "expected_revision": 7,
            "routing": {"mode": "pool", "team_id": "team-security"},
            "session_mode": "warm"
        }),
        &json!({"review": review_json(), "reviewers": []}),
    );
    call_round_trip::<ReviewClose>(
        &json!({
            "command_id": CMD,
            "review": "RV-A12",
            "expected_revision": 8,
            "reason": "merged"
        }),
        &json!({"review": review_json()}),
    );
}

#[test]
fn finding_ops_are_tagged_by_action() {
    let add = json!({
        "command_id": CMD,
        "review": "RV-A12",
        "op": {
            "action": "add",
            "finding_id": "0193a5c0-0000-7000-8000-0000000000f3",
            "severity": "question",
            "anchor": {"kind": "checklist_item", "checklist_item_id": "step-2"},
            "body_markdown": "Is step 2 really done?",
            "attachment_ids": ["att-3"]
        }
    });
    let params: ReviewFindingParams = round_trip(&add);
    let ReviewFindingOp::Add(draft) = &params.op else {
        panic!("expected add: {:?}", params.op);
    };
    assert_eq!(draft.anchor.kind, AnchorKind::ChecklistItem);
    let reply = json!({
        "command_id": CMD,
        "review": "RV-A12",
        "op": {
            "action": "reply",
            "finding_id": "0193a5c0-0000-7000-8000-0000000000f3",
            "reply_id": "0193a5c0-0000-7000-8000-0000000000b2",
            "set_status": "wont_fix"
        }
    });
    let params: ReviewFindingParams = round_trip(&reply);
    assert!(matches!(
        params.op,
        ReviewFindingOp::Reply { set_status: Some(FindingStatus::WontFix), ref body_markdown, .. }
            if body_markdown.is_empty()
    ));
    round_trip::<ReviewFindingResult>(&json!({"review": review_json(), "finding": finding_json()}));
    let unknown = json!({"command_id": CMD, "review": "RV-A12", "op": {"action": "delete", "finding_id": "x"}});
    assert!(serde_json::from_value::<ReviewFindingParams>(unknown).is_err());
}

#[test]
fn a_verdict_carries_its_round_pin_and_findings_batch() {
    let params = json!({
        "command_id": CMD,
        "review": "RV-A12",
        "round_number": 2,
        "head_oid": OID_B,
        "verdict": "changes_requested",
        "summary_markdown": "Two problems.",
        "findings": [
            {
                "finding_id": "0193a5c0-0000-7000-8000-0000000000f4",
                "severity": "major",
                "anchor": {"kind": "file_line", "path": "src/a.rs", "start_line": 3},
                "body_markdown": "Unchecked unwrap."
            },
            {
                "finding_id": "0193a5c0-0000-7000-8000-0000000000f5",
                "severity": "nit",
                "anchor": {"kind": "general"},
                "body_markdown": "Typo in the title."
            }
        ],
        "attachment_ids": ["att-4"]
    });
    call_round_trip::<ReviewVerdictCall>(
        &params,
        &json!({
            "review": review_json(),
            "verdict": round_json()["verdicts"][0].clone(),
            "findings": [finding_json()]
        }),
    );
    let decoded: ReviewVerdictParams = serde_json::from_value(params).unwrap();
    assert_eq!(decoded.findings.len(), 2);
    assert!(decoded.findings.iter().all(|f| f.anchor.check().is_ok()));
    // A draft without an anchor is a general finding.
    let bare: ReviewFindingDraft = serde_json::from_value(json!({
        "finding_id": "f", "severity": "minor", "body_markdown": "b"
    }))
    .unwrap();
    assert_eq!(bare.anchor, FindingAnchor::general());
}

#[test]
fn resubmit_resolves_fresh_and_session_mode() {
    let params: ReviewResubmitParams = round_trip(&json!({
        "command_id": CMD,
        "review": "RV-A12",
        "expected_revision": 7,
        "expected_head_oid": OID_B,
        "fresh": true,
        "note_markdown": "Addressed every finding."
    }));
    assert_eq!(
        params.requested_session_mode(),
        Ok(Some(ReviewSessionMode::Fresh))
    );
    let with = |fresh, session_mode| ReviewResubmitParams {
        fresh,
        session_mode,
        ..params.clone()
    };
    assert_eq!(with(false, None).requested_session_mode(), Ok(None));
    assert_eq!(
        with(false, Some(ReviewSessionMode::Warm)).requested_session_mode(),
        Ok(Some(ReviewSessionMode::Warm))
    );
    assert_eq!(
        with(true, Some(ReviewSessionMode::Fresh)).requested_session_mode(),
        Ok(Some(ReviewSessionMode::Fresh))
    );
    assert!(with(true, Some(ReviewSessionMode::Warm))
        .requested_session_mode()
        .is_err());
    round_trip::<ReviewRoundResult>(&json!({"review": review_json(), "round": round_json()}));
}

#[test]
fn optional_fields_are_omitted_and_unknown_fields_are_ignored() {
    let params: ReviewRequestParams = serde_json::from_value(json!({
        "command_id": CMD,
        "review_id": "r",
        "target_kind": "task",
        "title": "Check the task",
        "kind_slug": "task-review",
        "future_field": {"nested": true}
    }))
    .unwrap();
    assert!(!params.independent);
    assert_eq!(params.routing, None);
    let wire = serde_json::to_value(&params).unwrap();
    for absent in [
        "body_markdown",
        "routing",
        "independent",
        "base_oid",
        "criteria",
        "future_field",
    ] {
        assert!(wire.get(absent).is_none(), "{absent}");
    }
    let list = serde_json::to_value(ReviewListParams::default()).unwrap();
    assert_eq!(list, json!({}));
    let diff: ReviewDiffParams = serde_json::from_value(json!({"review": "RV-A1"})).unwrap();
    assert_eq!(
        serde_json::to_value(diff).unwrap(),
        json!({"review": "RV-A1"})
    );
}

#[test]
fn anchors_are_checked_for_their_kind() {
    let good = [
        FindingAnchor::general(),
        FindingAnchor::file_lines("src/lib.rs", 1, 1),
        FindingAnchor::file_lines("src/lib.rs", 10, 10 + REVIEW_ANCHOR_MAX_LINES - 1),
        FindingAnchor {
            end_line: None,
            anchor_snippet_sha256: Some(SNIPPET.into()),
            ..FindingAnchor::file_lines("a.rs", 5, 5)
        },
        FindingAnchor {
            kind: AnchorKind::DocSection,
            path: Some("docs/design.md".into()),
            doc_section: Some("Security".into()),
            ..FindingAnchor::default()
        },
        FindingAnchor {
            kind: AnchorKind::ChecklistItem,
            checklist_item_id: Some("step-1".into()),
            ..FindingAnchor::default()
        },
    ];
    for anchor in &good {
        assert_eq!(anchor.check(), Ok(()), "{anchor:?}");
    }
    let file = FindingAnchor::file_lines("src/lib.rs", 3, 4);
    let bad = [
        FindingAnchor {
            path: Some("x".into()),
            ..FindingAnchor::general()
        },
        FindingAnchor {
            start_line: Some(1),
            ..FindingAnchor::general()
        },
        FindingAnchor {
            path: None,
            ..file.clone()
        },
        FindingAnchor {
            start_line: None,
            ..file.clone()
        },
        FindingAnchor {
            start_line: Some(0),
            ..file.clone()
        },
        FindingAnchor {
            end_line: Some(2),
            ..file.clone()
        },
        FindingAnchor::file_lines("a.rs", 1, 1 + REVIEW_ANCHOR_MAX_LINES),
        FindingAnchor::file_lines("../etc/passwd", 1, 1),
        FindingAnchor::file_lines("/etc/passwd", 1, 1),
        FindingAnchor::file_lines("-p", 1, 1),
        FindingAnchor::file_lines("a//b", 1, 1),
        FindingAnchor::file_lines("a\nb", 1, 1),
        FindingAnchor::file_lines("x".repeat(REVIEW_PATH_MAX_LEN + 1), 1, 1),
        FindingAnchor {
            anchor_snippet_sha256: Some(SNIPPET.to_uppercase()),
            ..file.clone()
        },
        FindingAnchor {
            anchor_snippet_sha256: Some("abc".into()),
            ..file.clone()
        },
        FindingAnchor {
            doc_section: Some("S".into()),
            ..file.clone()
        },
        FindingAnchor {
            kind: AnchorKind::DocSection,
            ..FindingAnchor::default()
        },
        FindingAnchor {
            kind: AnchorKind::DocSection,
            doc_section: Some(String::new()),
            ..FindingAnchor::default()
        },
        FindingAnchor {
            kind: AnchorKind::DocSection,
            doc_section: Some("S".into()),
            start_line: Some(1),
            ..FindingAnchor::default()
        },
        FindingAnchor {
            kind: AnchorKind::ChecklistItem,
            ..FindingAnchor::default()
        },
        FindingAnchor {
            kind: AnchorKind::ChecklistItem,
            checklist_item_id: Some("s".into()),
            path: Some("a.rs".into()),
            ..FindingAnchor::default()
        },
    ];
    for anchor in &bad {
        assert!(anchor.check().is_err(), "{anchor:?}");
    }
}

#[test]
fn routing_is_checked_for_its_mode() {
    let spec = |mode, reviewers: &[&str], team: Option<&str>| ReviewRoutingSpec {
        mode,
        reviewer_ids: reviewers.iter().map(|id| (*id).to_owned()).collect(),
        team_id: team.map(str::to_owned),
    };
    let sixteen: Vec<String> = (0..REVIEW_MAX_REVIEWERS)
        .map(|n| format!("emp-{n}"))
        .collect();
    let sixteen: Vec<&str> = sixteen.iter().map(String::as_str).collect();
    let seventeen: Vec<String> = (0..=REVIEW_MAX_REVIEWERS)
        .map(|n| format!("emp-{n}"))
        .collect();
    let seventeen: Vec<&str> = seventeen.iter().map(String::as_str).collect();
    for good in [
        spec(RoutingMode::Explicit, &["a"], None),
        spec(RoutingMode::Explicit, &sixteen, None),
        spec(RoutingMode::Pool, &[], Some("team-1")),
        spec(RoutingMode::SingleAgent, &[], None),
        spec(RoutingMode::SingleAgent, &["agent-1"], None),
    ] {
        assert_eq!(good.check(), Ok(()), "{good:?}");
    }
    for bad in [
        spec(RoutingMode::Explicit, &[], None),
        spec(RoutingMode::Explicit, &seventeen, None),
        spec(RoutingMode::Explicit, &["a", "a"], None),
        spec(RoutingMode::Explicit, &[""], None),
        spec(RoutingMode::Explicit, &["a"], Some("team-1")),
        spec(RoutingMode::Pool, &[], None),
        spec(RoutingMode::Pool, &[], Some("")),
        spec(RoutingMode::Pool, &["a"], Some("team-1")),
        spec(RoutingMode::SingleAgent, &["a", "b"], None),
        spec(RoutingMode::SingleAgent, &[], Some("team-1")),
    ] {
        assert!(bad.check().is_err(), "{bad:?}");
    }
}

#[test]
fn diff_bounds_match_the_office_helpers_and_fit_a_frame() {
    assert_eq!(REVIEW_DIFF_MAX_BYTES, 256 * 1024);
    assert_eq!(REVIEW_DIFF_MAX_CONTEXT_LINES, 20);
    assert_eq!(REVIEW_DIFF_MAX_FILES, 1000);
    assert_eq!(REVIEW_ANCHOR_MAX_LINES, 500);
    assert_eq!(REVIEW_KEY_PREFIX, "RV-");
    // Ordinary diff text (quotes, backslashes, tabs) at most doubles when JSON-escaped.
    const { assert!(2 * REVIEW_DIFF_MAX_BYTES + 64 * 1024 < crate::MAX_FRAME_BYTES) };
    let worst_ordinary = "\"\\\t".repeat(REVIEW_DIFF_MAX_BYTES / 3);
    let encoded = serde_json::to_vec(&worst_ordinary).unwrap();
    assert!(encoded.len() < crate::MAX_FRAME_BYTES);
}
