use std::fs;
use std::path::Path;

use super::git::sanitize_git_stderr;
use super::tree::parse_ls_tree_entries;
use super::*;
use crate::office::import::validate_state_dir;
use crate::organization::records::OfficeCode;
use crate::TestOffice;

#[test]
fn next_office_code_skips_used_and_ambiguous_letters() {
    let now = "2026-11-21T00:00:00.000Z".to_owned();
    let oid = Id::parse("01900000-0000-7000-8000-000000000001").unwrap();
    let mk = |code: &str| OfficeCode {
        code: code.into(),
        office_id: oid.clone(),
        issued_at: now.clone(),
        retired_at: None,
    };
    assert_eq!(next_office_code(&[mk("A")]).as_deref(), Some("B"));
    let all_single: Vec<OfficeCode> = b"ABCDEFGHJKLMNPQRSTUVWXYZ"
        .iter()
        .map(|&b| mk(&(b as char).to_string()))
        .collect();
    assert_eq!(next_office_code(&all_single).as_deref(), Some("AA"));
}

#[test]
fn check_git_source_rejects_ext_credentials_and_option_injection() {
    for bad in [
        ("ext::sh -c id", "org/state"),
        ("-u", "org/state"),
        ("file://relative/path", "org/state"),
        ("https://user:pass@example.test/org.git", "org/state"),
        ("https://example.test/org.git?token=secret", "org/state"),
        ("ssh://-oProxyCommand=id@example.test/org.git", "org/state"),
        ("ssh://-host/org.git", "org/state"),
        ("https://example.test/org.git", "-bad"),
        ("https://example.test/org.git", "org..state"),
        ("https://example.test/org.git", "org/@{0}"),
        ("https://example.test/org.git", "org/state.lock"),
    ] {
        assert!(
            check_git_source(bad.0, bad.1).is_err(),
            "expected rejection for {bad:?}"
        );
    }
    assert!(check_git_source("https://example.test/org.git", "org/state").is_ok());
    assert!(check_git_source("ssh://git@example.test/org.git", "org/state").is_ok());
    assert!(check_git_source("file:///tmp/repo.git", "org/state").is_ok());
}

#[test]
fn sanitize_git_stderr_prefers_fatal_and_redacts_staging_and_credentials() {
    let staging = Path::new("/tmp/org-import-12345");
    let stderr = "Cloning into '/tmp/org-import-12345/repo.git'...\nfatal: repository 'https://alice:topsecret@example.test/org.git' not found in /tmp/org-import-12345/repo.git\n";
    let msg = sanitize_git_stderr(stderr, staging, "clone failed");
    assert!(msg.starts_with("fatal:"), "{msg}");
    assert!(!msg.contains("/tmp/org-import-12345"), "{msg}");
    assert!(!msg.contains("topsecret"), "{msg}");
    assert!(msg.contains("https://example.test/org.git"), "{msg}");
}

#[test]
fn parse_ls_tree_rejects_symlinks_and_submodules_before_blob_read() {
    let oid = "0123456789abcdef0123456789abcdef01234567";
    let symlink_raw =
        format!("100644 blob {oid} 10\torganization.yaml\0120000 blob {oid} 8\tlink.yaml\0");
    match parse_ls_tree_entries(symlink_raw.as_bytes()).unwrap_err() {
        ImportError::InvalidFile { path, reason } => {
            assert_eq!(path, "state/link.yaml");
            assert!(reason.contains("symlink"), "{reason}");
        }
        other => panic!("expected symlink refusal, got {other:?}"),
    }

    let submodule_raw =
        format!("100644 blob {oid} 10\torganization.yaml\0160000 commit {oid} -\tsubmod\0");
    match parse_ls_tree_entries(submodule_raw.as_bytes()).unwrap_err() {
        ImportError::InvalidFile { path, reason } => {
            assert_eq!(path, "state/submod");
            assert!(reason.contains("submodule"), "{reason}");
        }
        other => panic!("expected submodule refusal, got {other:?}"),
    }
}

#[test]
fn validate_state_dir_accepts_valid_org_and_names_invalid_or_unknown_files() {
    let office = crate::Office::for_test(TestOffice::default());
    let state_dir = office.durable_root().join("state");
    let imported = validate_state_dir(&state_dir, "https://example.test/acme.git", "org/state")
        .expect("valid created state must pass import validation");
    assert_eq!(imported.summary.name, office.info().org_name);
    assert_eq!(imported.summary.teams_count, 1);
    assert_eq!(imported.summary.people.len(), 1);
    // The HR lead and the Chief of Staff (`TASK-80.1`).
    assert_eq!(imported.summary.agents_count, 2);
    assert!(imported.summary.has_active_root_team);
    assert!(imported.summary.reserved_handles.iter().any(|h| h == "hr"));
    assert!(imported
        .summary
        .reserved_handles
        .iter()
        .any(|h| h == "tess"));
    assert!(!imported.local_only_not_restored.is_empty());

    // Copy to a scratch tree and inject an unrecognized file -> refused naming the exact path.
    let copy = tempfile::tempdir().unwrap();
    copy_tree(&state_dir, copy.path());
    fs::write(copy.path().join("rogue.txt"), b"not an org file").unwrap();
    let err =
        validate_state_dir(copy.path(), "https://example.test/acme.git", "org/state").unwrap_err();
    match err {
        ImportError::InvalidFile { path, .. } => assert_eq!(path, "state/rogue.txt"),
        other => panic!("expected InvalidFile naming rogue.txt, got {other:?}"),
    }

    // Remove rogue file and corrupt organization.yaml -> refused naming state/organization.yaml.
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
    use org_proto::processes::{
        DocumentKindWire, DocumentStatusWire, ProcessScopeWire, ProcessWriteParams,
    };
    use org_proto::tasks::{TasksCommentParams, TasksCreateParams};

    let office = crate::Office::for_test(TestOffice::default());
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();

    // Create a task + comment under `state/work/tasks/...` and a process doc under `state/processes/...`.
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
    assert!(
        imported
            .files
            .keys()
            .any(|p| p.as_str().starts_with("state/work/tasks/")),
        "expected task files in validated import"
    );
    assert!(
        imported
            .files
            .keys()
            .any(|p| p.as_str().starts_with("state/processes/")),
        "expected process files in validated import"
    );

    // Private/local files smuggled inside `state/` MUST still fail closed as unrecognized!
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

    // Duplicate structure document ID across processes/ and standards/ MUST fail closed (`Finding #10`).
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
