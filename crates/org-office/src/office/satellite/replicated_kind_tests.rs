//! Replicated journal kind checks.

use super::filters::is_replicated_op_kind;

#[test]
fn replicated_op_kinds_include_handoff_accept_without_ring_fallback() {
    for kind in [
        "organization.apply_replicated_files",
        "replication.apply_message",
        "replication.apply_conversation",
        "replication.apply_origin_close",
        "replication.apply_handoff_accept",
    ] {
        assert!(
            is_replicated_op_kind(kind),
            "missing replicated kind {kind}"
        );
    }
    assert!(!is_replicated_op_kind("messaging.accept_handoff"));
    assert!(!is_replicated_op_kind("messaging.send"));
}

#[test]
fn shared_file_records_apply_after_the_projects_and_members_they_name() {
    use super::filters::{classify_replicated_snapshot_path, SnapshotPathClass};
    let project = "0195a000-0000-7000-8000-0000000000a1";
    let file = "0195a000-0000-7000-8000-0000000000a2";
    for path in [
        format!("state/files/{file}.yaml"),
        format!("state/projects/{project}/files/{file}.yaml"),
    ] {
        assert_eq!(
            classify_replicated_snapshot_path(&path),
            SnapshotPathClass::DependentExecutionDocsAndBudgets,
            "{path}"
        );
    }
    assert_eq!(
        classify_replicated_snapshot_path(&format!("state/projects/{project}/project.yaml")),
        SnapshotPathClass::ModelProfilesAndProjects
    );
}

#[test]
fn project_documentation_applies_after_the_projects_and_employees_it_names() {
    use super::filters::{classify_replicated_snapshot_path, SnapshotPathClass};
    let project = "0195a000-0000-7000-8000-0000000000a1";
    for rel in [
        "docs/index.yaml",
        "docs/overview.md",
        "docs/guides/setup.md",
        "docs/media/architecture.png",
        "docs/media/architecture.png.yaml",
        "features/_counter.yaml",
        "features/0195a000-0000-7000-8000-0000000000b1.md",
        "releases/_counter.yaml",
        "releases/0195a000-0000-7000-8000-0000000000b2.md",
        "releases/0195a000-0000-7000-8000-0000000000b2/changelog.md",
        "releases/0195a000-0000-7000-8000-0000000000b2/milestones/_counter.yaml",
        "releases/0195a000-0000-7000-8000-0000000000b2/milestones/0195a000-0000-7000-8000-0000000000b3.md",
        "deployments/0195a000-0000-7000-8000-0000000000b4.md",
        "deployments/0195a000-0000-7000-8000-0000000000b4/changelog.md",
        "flags/0195a000-0000-7000-8000-0000000000b5.md",
        "links/feature/0195a000-0000-7000-8000-0000000000b1/0195a000-0000-7000-8000-0000000000b6.yaml",
    ] {
        let path = format!("state/projects/{project}/{rel}");
        assert_eq!(
            classify_replicated_snapshot_path(&path),
            SnapshotPathClass::DependentExecutionDocsAndBudgets,
            "{path}"
        );
    }
}

#[test]
fn review_files_apply_as_work_sidecars_and_are_immutable_during_reconciliation() {
    use super::filters::{classify_replicated_snapshot_path, SnapshotPathClass};
    use super::is_immutable_state_path;
    let rid = "0195a000-0000-7000-8000-000000000099";
    for path in [
        format!("state/reviews/{rid}/review.json"),
        format!("state/reviews/{rid}/rounds/000001.json"),
        format!("state/reviews/{rid}/events/0000000001.json"),
    ] {
        assert_eq!(
            classify_replicated_snapshot_path(&path),
            SnapshotPathClass::WorkSidecar,
            "{path}"
        );
        assert!(is_immutable_state_path(&path), "{path} must be immutable");
    }
}
