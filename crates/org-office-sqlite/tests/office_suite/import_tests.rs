//! SQLite-backed Git state import validation tests moved from
//! `crates/org-office/src/organization/import/tests.rs` (`TASK-255`).

use std::fs;
use std::path::Path;

use org_office::import::{validate_state_dir, ImportError};
use org_office::{Office, TestOffice};

#[test]
fn validate_state_dir_accepts_valid_org_and_names_invalid_or_unknown_files() {
    org_office_sqlite::install();
    let office = Office::for_test(TestOffice::default());
    let state_dir = office.durable_root().join("state");
    let imported = validate_state_dir(&state_dir, "https://example.test/acme.git", "org/state")
        .expect("valid created state must pass import validation");
    assert_eq!(imported.summary.name, office.info().org_name);
    assert_eq!(imported.summary.teams_count, 1);
    assert_eq!(imported.summary.people.len(), 1);
    assert_eq!(imported.summary.agents_count, 2);
    assert!(imported.summary.has_active_root_team);
    assert!(imported.summary.reserved_handles.iter().any(|h| h == "hr"));
    assert!(imported
        .summary
        .reserved_handles
        .iter()
        .any(|h| h == "tess"));
    assert!(!imported.local_only_not_restored.is_empty());

    let copy = tempfile::tempdir().unwrap();
    copy_tree(&state_dir, copy.path());
    fs::write(copy.path().join("rogue.txt"), b"not an org file").unwrap();
    let err =
        validate_state_dir(copy.path(), "https://example.test/acme.git", "org/state").unwrap_err();
    match err {
        ImportError::InvalidFile { path, .. } => assert_eq!(path, "state/rogue.txt"),
        other => panic!("expected InvalidFile naming rogue.txt, got {other:?}"),
    }

    fs::remove_file(copy.path().join("rogue.txt")).unwrap();
    fs::write(copy.path().join("organization.yaml"), b"not: [valid").unwrap();
    let err =
        validate_state_dir(copy.path(), "https://example.test/acme.git", "org/state").unwrap_err();
    match err {
        ImportError::InvalidFile { path, .. } => assert_eq!(path, "state/organization.yaml"),
        other => panic!("expected InvalidFile naming organization.yaml, got {other:?}"),
    }
}

#[test]
fn validate_state_dir_accepts_org_with_tasks_and_processes_and_rejects_private_files() {
    org_office_sqlite::install();
    use org_proto::processes::{
        DocumentKindWire, DocumentStatusWire, ProcessScopeWire, ProcessWriteParams,
    };
    use org_proto::tasks::{TasksCommentParams, TasksCreateParams};

    let office = Office::for_test(TestOffice::default());
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let root_team_id = root_team.clone();

    let task_id = "01900000-0000-7000-8000-000000009001".to_owned();
    let created = office
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: "01900000-0000-7000-8000-000000009002".into(),
                task_id: task_id.clone(),
                team_id: Some(root_team),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: None,
                title: "Imported task".into(),
                body_markdown: "Must validate cleanly on Git import".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(1),
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
                dod: None,
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap()
        .task;
    office
        .task_comment(
            &owner,
            TasksCommentParams {
                command_id: "01900000-0000-7000-8000-000000009003".into(),
                task: created.key.clone(),
                comment_id: "01900000-0000-7000-8000-000000009004".into(),
                body_markdown: "First comment".into(),
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap();
    office
        .process_write(
            &owner,
            ProcessWriteParams {
                command_id: "01900000-0000-7000-8000-000000009005".into(),
                document_id: Some("01900000-0000-7000-8000-000000009006".into()),
                kind: DocumentKindWire::Process,
                scope: ProcessScopeWire::Organization,
                key: Some("import-check".into()),
                title: "Import Check Process".into(),
                status: DocumentStatusWire::Current,
                owner_employee_id: Some(owner.clone()),
                overridable: true,
                body_markdown: "Verify every state family imports cleanly.".into(),
                expected_revision: None,
            },
        )
        .unwrap()
        .unwrap();

    office.sync_store().unwrap();
    let state_dir = office.durable_root().join("state");
    let imported = validate_state_dir(&state_dir, "https://example.test/acme.git", "org/state")
        .expect("org containing tasks, comments, and process docs must pass Git import validation");
    let task_folder = format!("state/teams/{root_team_id}/work/tasks/{task_id}/");
    for file in [
        "task.md",
        "comments/01900000-0000-7000-8000-000000009004.md",
    ] {
        assert!(
            imported
                .files
                .keys()
                .any(|p| p.as_str() == format!("{task_folder}{file}")),
            "expected {file} in validated import"
        );
    }
    for revision in [1, 2] {
        let event = format!("state/work-history/tasks/{task_id}/{revision}.json");
        assert!(
            imported.files.keys().any(|p| p.as_str() == event),
            "expected {event} in validated import"
        );
    }
    assert!(
        !imported
            .files
            .keys()
            .any(|p| p.as_str().starts_with("state/work/")),
        "nothing is written under the flat pre-v1 layout"
    );

    let tampered = tempfile::tempdir().unwrap();
    copy_tree(&state_dir, tampered.path());
    let event = tampered
        .path()
        .join(format!("work-history/tasks/{task_id}/2.json"));
    let text = fs::read_to_string(&event).unwrap();
    fs::write(&event, text.replace("comment_added", "comment_hidden")).unwrap();
    match validate_state_dir(
        tampered.path(),
        "https://example.test/acme.git",
        "org/state",
    ) {
        Err(ImportError::InvalidFile { path, reason }) => {
            assert_eq!(path, format!("state/work-history/tasks/{task_id}/2.json"));
            assert!(reason.contains("payload_sha256"), "{reason}");
        }
        other => panic!("expected the tampered event refused, got {other:?}"),
    }
    assert!(
        imported
            .files
            .keys()
            .any(|p| p.as_str().starts_with("state/processes/")),
        "expected process files in validated import"
    );

    let copy = tempfile::tempdir().unwrap();
    copy_tree(&state_dir, copy.path());
    fs::create_dir_all(copy.path().join("messaging/conversations")).unwrap();
    fs::write(
        copy.path().join("messaging/conversations/leaked.json"),
        b"{}",
    )
    .unwrap();
    assert!(
        matches!(
            validate_state_dir(copy.path(), "https://example.test/acme.git", "org/state"),
            Err(ImportError::InvalidFile { ref path, .. })
                if path == "state/messaging/conversations/leaked.json"
        ),
        "private messaging files smuggled into state/ must be rejected"
    );

    let dup_copy = tempfile::tempdir().unwrap();
    copy_tree(&state_dir, dup_copy.path());
    let proc_file = dup_copy
        .path()
        .join("processes/01900000-0000-7000-8000-000000009006.md");
    let proc_bytes = fs::read_to_string(&proc_file).unwrap();
    let std_bytes = proc_bytes.replace("kind: process", "kind: standard");
    fs::create_dir_all(dup_copy.path().join("standards")).unwrap();
    fs::write(
        dup_copy
            .path()
            .join("standards/01900000-0000-7000-8000-000000009006.md"),
        std_bytes,
    )
    .unwrap();
    match validate_state_dir(
        dup_copy.path(),
        "https://example.test/acme.git",
        "org/state",
    ) {
        Err(ImportError::InvalidFile { path, reason }) => {
            assert_eq!(
                path,
                "state/standards/01900000-0000-7000-8000-000000009006.md"
            );
            assert!(
                reason.contains("state/processes/01900000-0000-7000-8000-000000009006.md"),
                "duplicate structure doc error must name conflicting path: {reason}"
            );
        }
        other => panic!("expected duplicate structure doc refusal, got {other:?}"),
    }
}

fn copy_tree(src: &Path, dst: &Path) {
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}
