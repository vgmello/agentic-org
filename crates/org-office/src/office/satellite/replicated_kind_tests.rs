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
