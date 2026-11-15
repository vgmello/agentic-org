use org_work::{
    check_task_history_coverage, TaskHistoryCoverage, TaskHistoryCoverageError as Error,
    TaskHistoryHeader, TaskHistoryOrigin,
};

const ORG: &str = "legacy-org_01";
const TASK: &str = "old-B-0042_α";
// Syntactically valid placeholders. These tests exercise coverage, not payload verification.
const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const MAX_REVISION: u64 = i64::MAX as u64;

fn header(revision: u64, change_type: &str) -> TaskHistoryHeader<'_> {
    TaskHistoryHeader {
        organization_id: ORG,
        task_id: TASK,
        revision,
        filename_revision: revision,
        change_type,
        payload_sha256: DIGEST,
    }
}

fn check(
    current_revision: u64,
    origin: TaskHistoryOrigin,
    headers: &[TaskHistoryHeader<'_>],
) -> Result<TaskHistoryCoverage, Error> {
    check_task_history_coverage(ORG, TASK, current_revision, origin, headers)
}

#[test]
fn unordered_creation_history_and_identical_retries_cover_unique_revisions_without_mutation() {
    let events = [
        header(1, "created"),
        header(2, "future_change.v9"),
        header(3, "assignment_changed"),
    ];
    for [a, b, c] in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let headers = [events[a], events[b], events[c], events[b], events[a]];
        let before = headers;
        assert_eq!(
            check(3, TaskHistoryOrigin::Creation, &headers),
            Ok(TaskHistoryCoverage::FromCreation {
                current_revision: 3,
                unique_revisions: 3
            })
        );
        assert_eq!(headers, before);
    }
}

#[test]
fn single_creation_header_covers_revision_one() {
    assert_eq!(
        check(1, TaskHistoryOrigin::Creation, &[header(1, "created")]),
        Ok(TaskHistoryCoverage::FromCreation {
            current_revision: 1,
            unique_revisions: 1
        })
    );
}

#[test]
fn explicit_migration_baseline_keeps_earlier_history_unavailable_after_later_changes() {
    let headers = [
        header(9, "future_change.v10"),
        header(7, "history_baseline"),
        header(8, "assigned"),
        header(7, "history_baseline"),
    ];
    assert_eq!(
        check(
            9,
            TaskHistoryOrigin::Migration {
                baseline_revision: 7
            },
            &headers
        ),
        Ok(TaskHistoryCoverage::EarlierHistoryUnavailable {
            baseline_revision: 7,
            current_revision: 9,
            unique_revisions: 3,
        })
    );
}

#[test]
fn baseline_at_one_still_reports_earlier_history_unavailable() {
    assert_eq!(
        check(
            1,
            TaskHistoryOrigin::Migration {
                baseline_revision: 1
            },
            &[header(1, "history_baseline")]
        ),
        Ok(TaskHistoryCoverage::EarlierHistoryUnavailable {
            baseline_revision: 1,
            current_revision: 1,
            unique_revisions: 1,
        })
    );
}

#[test]
fn origin_marker_must_match_the_explicit_origin() {
    for (origin, change_type) in [
        (TaskHistoryOrigin::Creation, "history_baseline"),
        (TaskHistoryOrigin::Creation, "future_change"),
        (
            TaskHistoryOrigin::Migration {
                baseline_revision: 1,
            },
            "created",
        ),
        (
            TaskHistoryOrigin::Migration {
                baseline_revision: 1,
            },
            "future_change",
        ),
    ] {
        assert_eq!(
            check(1, origin, &[header(1, change_type)]),
            Err(Error::OriginTagMismatch { revision: 1 })
        );
    }
}

#[test]
fn missing_origin_and_headers_before_the_origin_are_not_inferred_or_ignored() {
    let cases = [
        (
            TaskHistoryOrigin::Creation,
            vec![header(7, "history_baseline")],
            1,
            7,
        ),
        (
            TaskHistoryOrigin::Migration {
                baseline_revision: 7,
            },
            vec![header(8, "status_changed")],
            7,
            8,
        ),
        (
            TaskHistoryOrigin::Migration {
                baseline_revision: 7,
            },
            vec![header(6, "created"), header(7, "history_baseline")],
            7,
            6,
        ),
    ];
    for (origin, headers, expected, actual) in cases {
        assert_eq!(
            check(8, origin, &headers),
            Err(Error::OriginRevisionMismatch { expected, actual })
        );
    }
}

#[test]
fn a_snapshot_revision_and_origin_alone_are_not_history() {
    for (current_revision, origin) in [
        (1, TaskHistoryOrigin::Creation),
        (
            5,
            TaskHistoryOrigin::Migration {
                baseline_revision: 5,
            },
        ),
    ] {
        assert_eq!(
            check(current_revision, origin, &[]),
            Err(Error::EmptyHistory)
        );
    }
}

#[test]
fn missing_middle_revisions_fail_for_creation_and_migration() {
    for (origin, headers, previous_revision, next_revision) in [
        (
            TaskHistoryOrigin::Creation,
            [header(3, "future_change"), header(1, "created")],
            1,
            3,
        ),
        (
            TaskHistoryOrigin::Migration {
                baseline_revision: 5,
            },
            [header(7, "future_change"), header(5, "history_baseline")],
            5,
            7,
        ),
    ] {
        assert_eq!(
            check(next_revision, origin, &headers),
            Err(Error::RevisionGap {
                previous_revision,
                next_revision
            })
        );
    }
}

#[test]
fn truncated_and_extra_history_disagree_with_the_snapshot_or_tombstone_revision() {
    for (origin, headers, expected, actual) in [
        (
            TaskHistoryOrigin::Creation,
            vec![header(1, "created")],
            2,
            1,
        ),
        (
            TaskHistoryOrigin::Creation,
            vec![header(1, "created"), header(2, "future_change")],
            1,
            2,
        ),
        (
            TaskHistoryOrigin::Migration {
                baseline_revision: 5,
            },
            vec![header(5, "history_baseline")],
            6,
            5,
        ),
        (
            TaskHistoryOrigin::Migration {
                baseline_revision: 5,
            },
            vec![header(5, "history_baseline"), header(6, "future_change")],
            5,
            6,
        ),
    ] {
        assert_eq!(
            check(expected, origin, &headers),
            Err(Error::FinalRevisionMismatch { expected, actual })
        );
    }
}

#[test]
fn a_later_baseline_never_resets_the_declared_origin() {
    for (origin, headers, revision) in [
        (
            TaskHistoryOrigin::Creation,
            [header(1, "created"), header(2, "history_baseline")],
            2,
        ),
        (
            TaskHistoryOrigin::Migration {
                baseline_revision: 5,
            },
            [header(5, "history_baseline"), header(6, "history_baseline")],
            6,
        ),
    ] {
        assert_eq!(
            check(revision, origin, &headers),
            Err(Error::UnexpectedBaseline { revision })
        );
    }
    assert_eq!(
        check(
            3,
            TaskHistoryOrigin::Creation,
            &[header(1, "created"), header(3, "history_baseline")]
        ),
        Err(Error::RevisionGap {
            previous_revision: 1,
            next_revision: 3
        })
    );
    assert_eq!(
        check(
            8,
            TaskHistoryOrigin::Migration {
                baseline_revision: 5
            },
            &[header(8, "history_baseline"), header(5, "history_baseline")]
        ),
        Err(Error::RevisionGap {
            previous_revision: 5,
            next_revision: 8
        })
    );
}

#[test]
fn malformed_retry_claims_are_checked_before_deduplication() {
    let original = header(1, "created");
    for (invalid, expected) in [
        (
            TaskHistoryHeader {
                organization_id: "another-org",
                ..original
            },
            Error::OrganizationMismatch { header_index: 1 },
        ),
        (
            TaskHistoryHeader {
                task_id: "another-task",
                ..original
            },
            Error::TaskMismatch { header_index: 1 },
        ),
        (
            TaskHistoryHeader {
                filename_revision: 2,
                ..original
            },
            Error::FilenameRevisionMismatch { header_index: 1 },
        ),
        (
            TaskHistoryHeader {
                filename_revision: 0,
                ..original
            },
            Error::InvalidFilenameRevision {
                header_index: 1,
                revision: 0,
            },
        ),
        (
            TaskHistoryHeader {
                change_type: "",
                ..original
            },
            Error::EmptyChangeType { header_index: 1 },
        ),
        (
            TaskHistoryHeader {
                payload_sha256: "bad-digest",
                ..original
            },
            Error::InvalidChecksum { header_index: 1 },
        ),
    ] {
        let headers = [original, invalid];
        let before = headers;
        assert_eq!(
            check(1, TaskHistoryOrigin::Creation, &headers),
            Err(expected)
        );
        assert_eq!(headers, before);
    }
}

#[test]
fn different_checksums_or_header_facts_conflict_in_either_order() {
    let original = header(1, "created");
    for conflicting in [
        TaskHistoryHeader {
            payload_sha256: OTHER_DIGEST,
            ..original
        },
        // The digest matches but this claim does not describe the same change tag.
        TaskHistoryHeader {
            change_type: "future_change",
            ..original
        },
    ] {
        for headers in [[original, conflicting], [conflicting, original]] {
            assert_eq!(
                check(1, TaskHistoryOrigin::Creation, &headers),
                Err(Error::ConflictingRevision { revision: 1 })
            );
        }
    }
}

#[test]
fn all_header_checks_precede_even_an_earlier_conflicting_retry() {
    let original = header(1, "created");
    let headers = [
        original,
        TaskHistoryHeader {
            payload_sha256: OTHER_DIGEST,
            ..original
        },
        TaskHistoryHeader {
            filename_revision: 2,
            ..original
        },
    ];
    assert_eq!(
        check(1, TaskHistoryOrigin::Creation, &headers),
        Err(Error::FilenameRevisionMismatch { header_index: 2 })
    );
}

#[test]
fn checksum_syntax_requires_exactly_64_lowercase_ascii_hexadecimal_characters() {
    for invalid in [
        String::new(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        format!("{}g", "0".repeat(63)),
        format!("{}\n", "0".repeat(63)),
        "é".repeat(32),
    ] {
        let headers = [
            header(1, "created"),
            TaskHistoryHeader {
                payload_sha256: &invalid,
                ..header(1, "created")
            },
        ];
        assert_eq!(
            check(1, TaskHistoryOrigin::Creation, &headers),
            Err(Error::InvalidChecksum { header_index: 1 }),
            "{invalid:?}"
        );
    }
}

#[test]
fn target_and_baseline_revisions_must_fit_positive_signed_sqlite_integers() {
    for revision in [0, MAX_REVISION + 1, u64::MAX] {
        assert_eq!(
            check(revision, TaskHistoryOrigin::Creation, &[]),
            Err(Error::InvalidCurrentRevision { revision })
        );
        assert_eq!(
            check(
                MAX_REVISION,
                TaskHistoryOrigin::Migration {
                    baseline_revision: revision
                },
                &[]
            ),
            Err(Error::InvalidBaselineRevision { revision })
        );
    }
    assert_eq!(
        check(
            3,
            TaskHistoryOrigin::Migration {
                baseline_revision: 4
            },
            &[]
        ),
        Err(Error::BaselineAfterCurrent {
            baseline_revision: 4,
            current_revision: 3
        })
    );
}

#[test]
fn event_and_filename_revisions_are_checked_at_integer_boundaries() {
    for revision in [0, MAX_REVISION + 1, u64::MAX] {
        assert_eq!(
            check(
                1,
                TaskHistoryOrigin::Creation,
                &[header(revision, "created")]
            ),
            Err(Error::InvalidEventRevision {
                header_index: 0,
                revision
            })
        );
        assert_eq!(
            check(
                1,
                TaskHistoryOrigin::Creation,
                &[TaskHistoryHeader {
                    filename_revision: revision,
                    ..header(1, "created")
                }]
            ),
            Err(Error::InvalidFilenameRevision {
                header_index: 0,
                revision
            })
        );
    }
}

#[test]
fn a_baseline_at_the_sqlite_maximum_needs_no_successor_arithmetic() {
    assert_eq!(
        check(
            MAX_REVISION,
            TaskHistoryOrigin::Migration {
                baseline_revision: MAX_REVISION
            },
            &[header(MAX_REVISION, "history_baseline")]
        ),
        Ok(TaskHistoryCoverage::EarlierHistoryUnavailable {
            baseline_revision: MAX_REVISION,
            current_revision: MAX_REVISION,
            unique_revisions: 1,
        })
    );
}

#[test]
fn consecutive_revisions_can_end_exactly_at_the_sqlite_maximum() {
    let headers = [
        header(MAX_REVISION, "future_change"),
        header(MAX_REVISION - 1, "history_baseline"),
        header(MAX_REVISION, "future_change"),
    ];
    assert_eq!(
        check(
            MAX_REVISION,
            TaskHistoryOrigin::Migration {
                baseline_revision: MAX_REVISION - 1
            },
            &headers
        ),
        Ok(TaskHistoryCoverage::EarlierHistoryUnavailable {
            baseline_revision: MAX_REVISION - 1,
            current_revision: MAX_REVISION,
            unique_revisions: 2,
        })
    );
}

#[test]
fn enormous_gaps_are_rejected_by_walking_supplied_headers_only() {
    assert_eq!(
        check(
            MAX_REVISION,
            TaskHistoryOrigin::Creation,
            &[header(MAX_REVISION, "future_change"), header(1, "created")]
        ),
        Err(Error::RevisionGap {
            previous_revision: 1,
            next_revision: MAX_REVISION
        })
    );
    assert_eq!(
        check(
            MAX_REVISION,
            TaskHistoryOrigin::Migration {
                baseline_revision: MAX_REVISION - 2
            },
            &[
                header(MAX_REVISION - 2, "history_baseline"),
                header(MAX_REVISION, "future_change")
            ]
        ),
        Err(Error::RevisionGap {
            previous_revision: MAX_REVISION - 2,
            next_revision: MAX_REVISION
        })
    );
}

#[test]
fn opaque_legacy_ids_are_compared_exactly_without_uuid_or_case_normalization() {
    let headers = [header(1, "created")];
    assert!(
        check_task_history_coverage(ORG, TASK, 1, TaskHistoryOrigin::Creation, &headers).is_ok()
    );
    assert_eq!(
        check_task_history_coverage(
            &ORG.to_uppercase(),
            TASK,
            1,
            TaskHistoryOrigin::Creation,
            &headers
        ),
        Err(Error::OrganizationMismatch { header_index: 0 })
    );
    assert_eq!(
        check_task_history_coverage(
            ORG,
            &TASK.to_uppercase(),
            1,
            TaskHistoryOrigin::Creation,
            &headers
        ),
        Err(Error::TaskMismatch { header_index: 0 })
    );
}

#[test]
fn coverage_only_checks_digest_syntax_and_does_not_establish_payload_integrity() {
    let digest = "0".repeat(64);
    let headers = [TaskHistoryHeader {
        payload_sha256: &digest,
        ..header(1, "created")
    }];
    assert_eq!(
        check(1, TaskHistoryOrigin::Creation, &headers),
        Ok(TaskHistoryCoverage::FromCreation {
            current_revision: 1,
            unique_revisions: 1
        })
    );
}
