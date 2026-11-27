use org_command::Refusal;
use serde_json::{json, Value};

use crate::runtime::transport::{OfficeTransport, TransportOffice};

struct EchoTransport {
    seen: Vec<(String, Value)>,
}

impl OfficeTransport for EchoTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.seen.push((method.to_owned(), params));
        match method {
            "tasks.create"
            | "tasks.update"
            | "tasks.transition"
            | "tasks.assign"
            | "tasks.watch"
            | "tasks.unwatch"
            | "tasks.add_dependency"
            | "tasks.remove_dependency" => Ok(json!({
                "task": {
                    "id": "0193a5c0-0000-7000-8000-000000000100",
                    "office_code": "A",
                    "number": 1,
                    "key": "T-A1",
                    "revision": 2,
                    "team_id": "0193a5c0-0000-7000-8000-000000000010",
                    "type": "task",
                    "title": "Test",
                    "status": "in_progress",
                    "body_markdown": "",
                    "priority": "high",
                    "rank_order": 1,
                    "labels": ["backend", "security"],
                    "reference_files": ["docs/a.md"],
                    "modified_files": ["src/a.rs"],
                    "depends_on": [],
                    "dependents": [],
                    "created_by_kind": "employee",
                    "updated_by_kind": "employee",
                    "checklist_total": 0,
                    "checklist_done": 0,
                    "comment_count": 0,
                    "watcher_count": 0
                }
            })),
            "tasks.list" => Ok(json!({ "tasks": [], "total": 0 })),
            "tasks.labels" => Ok(json!({ "labels": [{ "label": "security", "task_count": 2 }] })),
            "tasks.notes.add" | "tasks.notes.get" | "tasks.notes.update" => Ok(json!({
                "note": {
                    "id": "0193a5c0-0000-7000-8000-000000000400",
                    "task_id": "0193a5c0-0000-7000-8000-000000000100",
                    "author_id": "0193a5c0-0000-7000-8000-000000000001",
                    "revision": 1,
                    "body_markdown": "Private note",
                    "created_at": "2026-11-22T00:00:00.000Z",
                    "updated_at": "2026-11-22T00:00:00.000Z",
                    "editable": true
                }
            })),
            "tasks.notes.list" => Ok(json!({
                "task_id": "0193a5c0-0000-7000-8000-000000000100",
                "notes": []
            })),
            "tasks.notes.remove" => Ok(json!({
                "note_id": "0193a5c0-0000-7000-8000-000000000400",
                "task_id": "0193a5c0-0000-7000-8000-000000000100",
                "revision": 2,
                "deleted_at": "2026-11-22T00:01:00.000Z"
            })),
            "tasks.checklist.add" | "tasks.checklist.check" => Ok(json!({
                "task": {
                    "id": "0193a5c0-0000-7000-8000-000000000100",
                    "office_code": "A",
                    "number": 1,
                    "key": "T-A1",
                    "revision": 3,
                    "team_id": "0193a5c0-0000-7000-8000-000000000010",
                    "type": "task",
                    "title": "Test",
                    "status": "todo",
                    "body_markdown": "",
                    "priority": "normal",
                    "labels": [],
                    "reference_files": [],
                    "modified_files": [],
                    "depends_on": [],
                    "dependents": [],
                    "created_by_kind": "employee",
                    "updated_by_kind": "employee",
                    "checklist_total": 1,
                    "checklist_done": 0,
                    "comment_count": 0,
                    "watcher_count": 0
                },
                "checklist": [{
                    "id": "0193a5c0-0000-7000-8000-000000000200",
                    "position": 0,
                    "text": "Step 1",
                    "required": true,
                    "done": false,
                    "revision": 1
                }]
            })),
            _ => Ok(json!({})),
        }
    }
}

#[test]
fn cli_15_task_and_checklist_commands_dispatch_with_task_77_fields() {
    let mut office = TransportOffice(EchoTransport { seen: Vec::new() });
    let args: Vec<String> = [
        "task",
        "create",
        "--title",
        "Auth",
        "--team-id",
        "0193a5c0-0000-7000-8000-000000000010",
        "--order",
        "1",
        "--labels",
        "backend",
        "--labels",
        "security",
        "--reference-files",
        "docs/a.md",
        "--modified-files",
        "src/a.rs",
        "--depends-on",
        "T-A0",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let out = crate::execute_with_office("agent-os", &args, &mut office);
    assert_eq!(out.exit_code, 0, "stdout: {}", out.stdout);
    assert_eq!(office.0.seen[0].0, "tasks.create");
    assert_eq!(office.0.seen[0].1["rank_order"], 1);
    assert_eq!(office.0.seen[0].1["labels"], json!(["backend", "security"]));
    assert_eq!(office.0.seen[0].1["reference_files"], json!(["docs/a.md"]));
    assert_eq!(office.0.seen[0].1["modified_files"], json!(["src/a.rs"]));
    assert_eq!(office.0.seen[0].1["depends_on"], json!(["T-A0"]));

    let list_args: Vec<String> = [
        "task",
        "list",
        "--labels",
        "backend",
        "--labels",
        "security",
        "--label-match",
        "all",
        "--sort",
        "order",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let out2 = crate::execute_with_office("agent-os", &list_args, &mut office);
    assert_eq!(out2.exit_code, 0, "stdout: {}", out2.stdout);
    assert_eq!(office.0.seen[1].0, "tasks.list");

    let chk_args: Vec<String> = [
        "checklist",
        "add",
        "--task",
        "T-A1",
        "--text",
        "Step 1",
        "--required",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let out3 = crate::execute_with_office("agent-os", &chk_args, &mut office);
    assert_eq!(out3.exit_code, 0, "stdout: {}", out3.stdout);
    assert_eq!(office.0.seen[2].0, "tasks.checklist.add");

    // Idempotency retry derives deterministic command_id and task_id even for non-UUID keys.
    let retry_args: Vec<String> = [
        "task",
        "create",
        "--idempotency-key",
        "retry-key-42",
        "--title",
        "Retryable",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let r1 = crate::execute_with_office("agent-os", &retry_args, &mut office);
    let r2 = crate::execute_with_office("agent-os", &retry_args, &mut office);
    assert_eq!(r1.exit_code, 0);
    assert_eq!(r2.exit_code, 0);
    assert_eq!(
        office.0.seen[3].1["command_id"],
        office.0.seen[4].1["command_id"]
    );
    assert_eq!(office.0.seen[3].1["task_id"], office.0.seen[4].1["task_id"]);
    assert!(org_proto::is_uuid_v7(
        office.0.seen[3].1["command_id"].as_str().unwrap()
    ));

    // --dry-run must refuse before contacting transport.
    let before = office.0.seen.len();
    let dry_args: Vec<String> = ["task", "create", "--dry-run", "--title", "Should not run"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let dry_out = crate::execute_with_office("agent-os", &dry_args, &mut office);
    assert_ne!(dry_out.exit_code, 0);
    assert_eq!(office.0.seen.len(), before);

    // checklist check --undone sends done=false.
    let uncheck_args: Vec<String> = [
        "checklist",
        "check",
        "--task",
        "T-A1",
        "--step-id",
        "0193a5c0-0000-7000-8000-000000000999",
        "--undone",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let uncheck_out = crate::execute_with_office("agent-os", &uncheck_args, &mut office);
    assert_eq!(uncheck_out.exit_code, 0, "stdout: {}", uncheck_out.stdout);
    assert_eq!(office.0.seen.last().unwrap().1["done"], json!(false));

    // `task note add|list|update|remove` alias rewrites to `task-note` (TASK-50 AC #3).
    let note_add: Vec<String> = [
        "task",
        "note",
        "add",
        "--idempotency-key",
        "note-key-1",
        "--task",
        "T-A1",
        "--text",
        "My private plan",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let n1 = crate::execute_with_office("agent-os", &note_add, &mut office);
    let n2 = crate::execute_with_office("agent-os", &note_add, &mut office);
    assert_eq!(n1.exit_code, 0, "stdout: {}", n1.stdout);
    assert_eq!(n2.exit_code, 0, "stdout: {}", n2.stdout);
    let len = office.0.seen.len();
    assert_eq!(office.0.seen[len - 2].0, "tasks.notes.add");
    assert_eq!(
        office.0.seen[len - 2].1["note_id"],
        office.0.seen[len - 1].1["note_id"]
    );

    let note_list: Vec<String> = ["task", "note", "list", "--task", "T-A1"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        crate::execute_with_office("agent-os", &note_list, &mut office).exit_code,
        0
    );
}

#[test]
fn task_create_update_comment_inspect_attachments_and_zero_overhead() {
    use crate::runtime::file_guard::{with_test_roots, FileRoots, RootEntry};

    const STAGED_ID: &str = "0193a5c0-0000-7000-8000-000000000901";
    const EXISTING_ID: &str = "0193a5c0-0000-7000-8000-000000000902";
    const REMOVE_ID: &str = "0193a5c0-0000-7000-8000-000000000903";

    struct TaskAttachMock {
        seen: Vec<(String, Value)>,
    }

    fn task_json() -> Value {
        json!({
            "id": "0193a5c0-0000-7000-8000-000000000100",
            "office_code": "A",
            "number": 1,
            "key": "T-A1",
            "revision": 2,
            "team_id": "0193a5c0-0000-7000-8000-000000000010",
            "type": "task",
            "title": "Test",
            "status": "todo",
            "body_markdown": "",
            "priority": "normal",
            "labels": [],
            "reference_files": [],
            "modified_files": [],
            "depends_on": [],
            "dependents": [],
            "created_by_kind": "employee",
            "updated_by_kind": "employee",
            "checklist_total": 0,
            "checklist_done": 0,
            "comment_count": 1,
            "watcher_count": 0
        })
    }

    fn meta_json(id: &str) -> Value {
        json!({
            "id": id,
            "filename": "spec.md",
            "title": "Spec",
            "artifact_status": "final",
            "mime_type": "text/markdown",
            "byte_count": 5,
            "uploaded_by_employee_id": "0193a5c0-0000-7000-8000-000000000020",
            "created_at": "2026-11-27T00:00:00.000Z"
        })
    }

    impl OfficeTransport for TaskAttachMock {
        fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
            self.seen.push((method.to_owned(), params));
            match method {
                "attachment.upload_begin" => Ok(json!({
                    "upload": {
                        "upload_id": STAGED_ID,
                        "filename": "spec.md",
                        "mime_type": "text/markdown",
                        "byte_count": 5,
                        "received_bytes": 0,
                        "committed": false,
                        "expires_at": "2026-11-27T01:00:00.000Z"
                    },
                    "chunk_size_bytes": 524288
                })),
                "attachment.upload_chunk" => {
                    Ok(json!({ "upload_id": STAGED_ID, "received_bytes": 5, "complete": true }))
                }
                "attachment.upload_commit" => Ok(json!({
                    "upload": {
                        "upload_id": STAGED_ID,
                        "filename": "spec.md",
                        "mime_type": "text/markdown",
                        "byte_count": 5,
                        "received_bytes": 5,
                        "committed": true,
                        "expires_at": "2026-11-27T01:00:00.000Z",
                        "content_sha256": "0".repeat(64)
                    }
                })),
                "tasks.create" | "tasks.update" => Ok(json!({ "task": task_json() })),
                "tasks.comment" => Ok(json!({
                    "task": task_json(),
                    "comment": {
                        "id": "0193a5c0-0000-7000-8000-000000000300",
                        "position": 0,
                        "author_id": "0193a5c0-0000-7000-8000-000000000020",
                        "created_at": "2026-11-27T00:00:00.000Z",
                        "body_markdown": "",
                        "attachments": [meta_json(STAGED_ID)]
                    }
                })),
                "tasks.inspect" => Ok(json!({
                    "task": task_json(),
                    "checklist": [],
                    "comments": [{
                        "id": "0193a5c0-0000-7000-8000-000000000300",
                        "position": 0,
                        "author_id": "0193a5c0-0000-7000-8000-000000000020",
                        "created_at": "2026-11-27T00:00:00.000Z",
                        "body_markdown": "",
                        "attachments": [meta_json(STAGED_ID)]
                    }],
                    "attachments": [meta_json(EXISTING_ID)],
                    "depends_on_tasks": [],
                    "dependent_tasks": [],
                    "watchers": [],
                    "history": []
                })),
                other => panic!("unexpected RPC: {other}"),
            }
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("spec.md"), b"hello").unwrap();
    let roots = FileRoots {
        cwd: tmp.path().to_path_buf(),
        workspace_roots: vec![RootEntry::from_path(tmp.path().to_path_buf()).unwrap()],
        ..FileRoots::default()
    };

    with_test_roots(roots, || {
        // 1. Plain `task create` without `--attach`: only 1 RPC (`tasks.create`).
        let mut t = TransportOffice(TaskAttachMock { seen: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "task".into(),
                "create".into(),
                "--title".into(),
                "Plain".into(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(t.0.seen.len(), 1);
        assert_eq!(t.0.seen[0].0, "tasks.create");

        // 2. `task create --attach --attachment-id`.
        let mut t = TransportOffice(TaskAttachMock { seen: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "task".into(),
                "create".into(),
                "--title".into(),
                "With files".into(),
                "--attach".into(),
                "spec.md".into(),
                "--attachment-id".into(),
                EXISTING_ID.into(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        let create = t.0.seen.last().unwrap();
        assert_eq!(create.0, "tasks.create");
        assert_eq!(create.1["attachment_ids"], json!([STAGED_ID, EXISTING_ID]));

        // 3. `task update --if-match 1 --attach --add-attachment --remove-attachment`.
        let mut t = TransportOffice(TaskAttachMock { seen: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "task".into(),
                "update".into(),
                "--task".into(),
                "T-A1".into(),
                "--if-match".into(),
                "1".into(),
                "--attach".into(),
                "spec.md".into(),
                "--add-attachment".into(),
                EXISTING_ID.into(),
                "--remove-attachment".into(),
                REMOVE_ID.into(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        let update = t.0.seen.last().unwrap();
        assert_eq!(update.0, "tasks.update");
        assert_eq!(update.1["add_attachments"], json!([STAGED_ID, EXISTING_ID]));
        assert_eq!(update.1["remove_attachments"], json!([REMOVE_ID]));

        // 4. `task comment --attach` (body omitted!) and empty comment refusal.
        let mut t = TransportOffice(TaskAttachMock { seen: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "task".into(),
                "comment".into(),
                "--task".into(),
                "T-A1".into(),
                "--attach".into(),
                "spec.md".into(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        let comment = t.0.seen.last().unwrap();
        assert_eq!(comment.0, "tasks.comment");
        assert_eq!(comment.1["attachment_ids"], json!([STAGED_ID]));

        let mut t = TransportOffice(TaskAttachMock { seen: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "task".into(),
                "comment".into(),
                "--task".into(),
                "T-A1".into(),
            ],
            &mut t,
        );
        assert_ne!(out.exit_code, 0, "{}", out.stdout);
        assert!(t.0.seen.is_empty());

        // 5. `task inspect` surfaces task and comment attachments.
        let mut t = TransportOffice(TaskAttachMock { seen: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "task".into(),
                "inspect".into(),
                "--task".into(),
                "T-A1".into(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["data"]["attachments"][0]["id"], EXISTING_ID);
        assert_eq!(
            env["data"]["comments"][0]["attachments"][0]["id"],
            STAGED_ID
        );
    });
}

#[test]
fn task_attach_aborts_staged_uploads_on_downstream_rpc_error() {
    use org_command::Refusal;
    use org_proto::ErrorCode;

    use crate::runtime::file_guard::{with_test_roots, FileRoots, RootEntry};
    use crate::runtime::transport::{OfficeTransport, TransportOffice};

    const STAGED_ID: &str = "0193a5c0-0000-7000-8000-000000000881";
    const EXISTING_ID: &str = "0193a5c0-0000-7000-8000-000000000882";

    struct FailingTaskMock {
        fail_method: &'static str,
        seen: Vec<(String, Value)>,
    }

    impl OfficeTransport for FailingTaskMock {
        fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
            self.seen.push((method.to_owned(), params));
            match method {
                "attachment.upload_begin" => Ok(json!({
                    "upload": {
                        "upload_id": STAGED_ID,
                        "filename": "spec.md",
                        "mime_type": "text/markdown",
                        "byte_count": 5,
                        "received_bytes": 0,
                        "committed": false,
                        "expires_at": "2026-11-27T01:00:00.000Z"
                    },
                    "chunk_size_bytes": 65536
                })),
                "attachment.upload_chunk" => Ok(json!({
                    "upload_id": STAGED_ID,
                    "received_bytes": 5,
                    "complete": true
                })),
                "attachment.upload_commit" => Ok(json!({
                    "upload": {
                        "upload_id": STAGED_ID,
                        "filename": "spec.md",
                        "mime_type": "text/markdown",
                        "byte_count": 5,
                        "received_bytes": 5,
                        "committed": true,
                        "expires_at": "2026-11-27T01:00:00.000Z",
                        "content_sha256": "0".repeat(64)
                    }
                })),
                "attachment.upload_abort" => Ok(json!({"aborted": true})),
                m if m == self.fail_method => Err(Refusal::new(
                    ErrorCode::Conflict,
                    format!("{m} failed"),
                    "retry",
                )),
                other => panic!("unexpected RPC: {other}"),
            }
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("spec.md"), b"hello").unwrap();
    let roots = FileRoots {
        cwd: tmp.path().to_path_buf(),
        workspace_roots: vec![RootEntry::from_path(tmp.path().to_path_buf()).unwrap()],
        ..FileRoots::default()
    };

    with_test_roots(roots, || {
        let cases: &[(&'static str, Vec<&str>)] = &[
            (
                "tasks.create",
                vec![
                    "task",
                    "create",
                    "--title",
                    "Fail",
                    "--attach",
                    "spec.md",
                    "--attachment-id",
                    EXISTING_ID,
                ],
            ),
            (
                "tasks.update",
                vec![
                    "task",
                    "update",
                    "--task",
                    "T-A1",
                    "--if-match",
                    "1",
                    "--attach",
                    "spec.md",
                    "--add-attachment",
                    EXISTING_ID,
                ],
            ),
            (
                "tasks.comment",
                vec![
                    "task",
                    "comment",
                    "--task",
                    "T-A1",
                    "--attach",
                    "spec.md",
                    "--attachment-id",
                    EXISTING_ID,
                ],
            ),
        ];
        for (fail_method, args) in cases {
            let mut t = TransportOffice(FailingTaskMock {
                fail_method,
                seen: vec![],
            });
            let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
            let out = crate::execute_with_office("agent-os", &owned, &mut t);
            assert_ne!(out.exit_code, 0, "{fail_method}: {}", out.stdout);
            let aborted: Vec<&str> =
                t.0.seen
                    .iter()
                    .filter(|(m, _)| m == "attachment.upload_abort")
                    .filter_map(|(_, p)| p["upload_id"].as_str())
                    .collect();
            assert_eq!(
                aborted,
                vec![STAGED_ID],
                "{fail_method} must abort only freshly staged upload, calls: {:?}",
                t.0.seen
            );
        }
    });
}

#[test]
fn task_90_surgical_checklist_fields_pruning_and_task_update_cli() {
    use org_proto::{ApiError, ConflictDetail, ConflictEvent, ConflictSummary};

    use crate::runtime::binding::map_client_error;

    struct AxMock {
        seen: Vec<(String, Value)>,
        fail_update_conflict: bool,
    }

    fn sample_task(rev: u64) -> Value {
        json!({
            "id": "0193a5c0-0000-7000-8000-000000000100",
            "office_code": "A",
            "number": 1,
            "key": "T-A1",
            "revision": rev,
            "team_id": "0193a5c0-0000-7000-8000-000000000010",
            "type": "task",
            "work_type": "run",
            "title": "AX Task",
            "status": "in_progress",
            "priority": "high",
            "effort": 5,
            "severity": "major",
            "labels": [],
            "reference_files": [],
            "modified_files": [],
            "depends_on": [],
            "dependents": [],
            "created_by_kind": "employee",
            "updated_by_kind": "employee",
            "checklist_total": 3,
            "checklist_done": 1,
            "comment_count": 0,
            "watcher_count": 0
        })
    }

    fn sample_steps() -> Value {
        json!([
            {
                "id": "0193a5c0-0000-7000-8000-000000000201",
                "section": "acceptance",
                "position": 0,
                "text": "AC 1",
                "required": true,
                "done": true,
                "revision": 2
            },
            {
                "id": "0193a5c0-0000-7000-8000-000000000202",
                "section": "acceptance",
                "position": 1,
                "text": "AC 2",
                "required": true,
                "done": false,
                "revision": 1
            },
            {
                "id": "0193a5c0-0000-7000-8000-000000000203",
                "section": "work",
                "position": 0,
                "text": "Work 1",
                "required": false,
                "done": false,
                "revision": 1
            }
        ])
    }

    impl OfficeTransport for AxMock {
        fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
            self.seen.push((method.to_owned(), params.clone()));
            match method {
                "tasks.inspect" => {
                    // Simulate pruned server response (omitting body_markdown, comments, etc.).
                    Ok(json!({
                        "task": sample_task(4),
                        "checklist": sample_steps()
                    }))
                }
                "tasks.history" => Ok(json!({
                    "task_id": "0193a5c0-0000-7000-8000-000000000100",
                    "events": [{
                        "task_id": "0193a5c0-0000-7000-8000-000000000100",
                        "revision": 2,
                        "occurred_at": "2026-11-27T10:00:00.000Z",
                        "actor_kind": "employee",
                        "change": {"type": "task_updated", "fields": ["title"]},
                        "payload_sha256": "0".repeat(64)
                    }]
                })),
                "tasks.checklist.add"
                | "tasks.checklist.check"
                | "tasks.checklist.edit"
                | "tasks.checklist.move" => Ok(json!({
                    "task": sample_task(5),
                    "checklist": sample_steps(),
                    "mutated_step_ids": ["0193a5c0-0000-7000-8000-000000000202"]
                })),
                "tasks.checklist.remove" => Ok(json!({
                    "task": sample_task(6),
                    "checklist": [],
                    "mutated_step_ids": ["0193a5c0-0000-7000-8000-000000000202"]
                })),
                "tasks.update" if self.fail_update_conflict => {
                    let detail = ConflictDetail {
                        expected_revision: 2,
                        current_revision: 4,
                        changed_sections: vec!["title".into()],
                        events_since: vec![ConflictEvent {
                            revision: 3,
                            occurred_at: None,
                            actor_kind: "employee".into(),
                            actor_id: None,
                            change: json!({"type": "task_updated", "fields": ["title"]}),
                        }],
                        events_truncated: false,
                        omitted_events: 0,
                        reread_recommended: false,
                        current_summary: Some(ConflictSummary {
                            key: Some("T-A1".into()),
                            title: "AX Task".into(),
                            status: "in_progress".into(),
                            priority: "high".into(),
                            assignee_id: None,
                            assigned_team_id: None,
                            checklist_total: 3,
                            checklist_done: 1,
                            ac_total: 2,
                            ac_done: 1,
                            dod_total: 0,
                            dod_done: 0,
                            work_steps_total: 1,
                            work_steps_done: 0,
                            comment_count: 0,
                            watcher_count: 0,
                        }),
                    };
                    Err(map_client_error(org_client::ClientError::Api(
                        ApiError::conflict("stale revision", detail),
                    )))
                }
                "tasks.update" => Ok(json!({ "task": sample_task(5) })),
                other => panic!("unexpected RPC: {other}"),
            }
        }
    }

    let run = |office: &mut TransportOffice<AxMock>, args: &[&str]| {
        let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        crate::execute_with_office("agent-os", &owned, office)
    };

    let mut office = TransportOffice(AxMock {
        seen: vec![],
        fail_update_conflict: false,
    });

    // 1. Server-side `--fields` propagation and `shape::conform` with pruned optional sections.
    let out = run(
        &mut office,
        &[
            "task",
            "inspect",
            "--task",
            "T-A1",
            "--fields",
            "task.key,task.title,task.status,task.work_type,task.effort,task.severity,task.revision,checklist",
        ],
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let env: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(
        office.0.seen.last().unwrap().1["fields"],
        json!(["task", "checklist"])
    );
    assert_eq!(env["data"]["task"]["work_type"], "run");
    assert_eq!(env["data"]["task"]["effort"], 5);
    assert_eq!(env["data"]["task"]["severity"], "major");
    assert!(env["data"].get("body_markdown").is_none());
    assert!(env["data"].get("history").is_none());
    assert_eq!(env["data"]["checklist"].as_array().unwrap().len(), 3);

    // 2. `checklist list --section acceptance --unchecked` keeps true 1-based ordinal (index=2) + full progress.
    let out = run(
        &mut office,
        &[
            "checklist",
            "list",
            "--task",
            "T-A1",
            "--section",
            "acceptance",
            "--unchecked",
        ],
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(
        office.0.seen.last().unwrap().1["fields"],
        json!(["checklist"])
    );
    let env: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(env["data"]["progress"]["total"], 3);
    assert_eq!(env["data"]["progress"]["done"], 1);
    assert_eq!(env["data"]["progress"]["acceptance"]["total"], 2);
    assert_eq!(env["data"]["progress"]["acceptance"]["done"], 1);
    let steps = env["data"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["index"], 2);
    assert_eq!(steps[0]["step_id"], "0193a5c0-0000-7000-8000-000000000202");

    // 3. `checklist add` repeatable `--text` batch + `--section`, default `required` omitted.
    let out = run(
        &mut office,
        &[
            "checklist",
            "add",
            "--task",
            "T-A1",
            "--section",
            "acceptance",
            "--text",
            "First",
            "--text",
            "Second",
        ],
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let last_params = &office.0.seen.last().unwrap().1;
    assert_eq!(last_params["steps"].as_array().unwrap().len(), 2);
    assert!(last_params["steps"][0].get("required").is_none());

    // 4. `checklist check` batch 1-based `--index` requires `--if-match`; refuses missing `--if-match` or `--index 0`.
    let before = office.0.seen.len();
    for bad in [
        vec![
            "checklist",
            "check",
            "--task",
            "T-A1",
            "--section",
            "acceptance",
            "--index",
            "1",
        ],
        vec![
            "checklist",
            "check",
            "--task",
            "T-A1",
            "--section",
            "acceptance",
            "--index",
            "0",
            "--if-match",
            "4",
        ],
        vec![
            "checklist",
            "check",
            "--task",
            "T-A1",
            "--section",
            "acceptance",
            "--index",
            "1",
            "--index",
            "1",
            "--if-match",
            "4",
        ],
        vec![
            "checklist",
            "edit",
            "--task",
            "T-A1",
            "--section",
            "acceptance",
            "--index",
            "1",
            "--if-match",
            "4",
            "--required",
            "--optional",
        ],
        vec![
            "task",
            "update",
            "--task",
            "T-A1",
            "--if-match",
            "4",
            "--body",
            "a",
            "--append-body",
            "b",
        ],
    ] {
        let out = run(&mut office, &bad);
        assert_ne!(out.exit_code, 0, "{bad:?}: {}", out.stdout);
    }
    assert_eq!(
        office.0.seen.len(),
        before,
        "fail-closed validation must not call transport"
    );

    let out = run(
        &mut office,
        &[
            "checklist",
            "check",
            "--task",
            "T-A1",
            "--section",
            "acceptance",
            "--index",
            "1",
            "--index",
            "2",
            "--if-match",
            "4",
        ],
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let p = &office.0.seen.last().unwrap().1;
    assert_eq!(p["section"], "acceptance");
    assert_eq!(p["indices"], json!([1, 2]));
    assert_eq!(p["expected_revision"], 4);

    // 5. `checklist edit --optional`, `checklist move --to-section`, `checklist remove` receipts.
    let out = run(
        &mut office,
        &[
            "checklist",
            "edit",
            "--task",
            "T-A1",
            "--section",
            "acceptance",
            "--index",
            "2",
            "--if-match",
            "4",
            "--optional",
        ],
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(office.0.seen.last().unwrap().1["required"], false);
    let env: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(env["data"]["steps"].as_array().unwrap().len(), 1);

    let out = run(
        &mut office,
        &[
            "checklist",
            "move",
            "--task",
            "T-A1",
            "--section",
            "acceptance",
            "--index",
            "2",
            "--to-section",
            "dod",
            "--position",
            "0",
            "--if-match",
            "5",
        ],
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(
        office.0.seen.last().unwrap().1["from_section"],
        "acceptance"
    );
    assert_eq!(office.0.seen.last().unwrap().1["section"], "dod");
    assert_eq!(office.0.seen.last().unwrap().1["expected_revision"], 5);

    let out = run(
        &mut office,
        &[
            "checklist",
            "remove",
            "--task",
            "T-A1",
            "--section",
            "acceptance",
            "--index",
            "2",
            "--if-match",
            "5",
        ],
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let env: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(
        env["data"]["removed_step_id"],
        "0193a5c0-0000-7000-8000-000000000202"
    );

    // 6. `task update` `--work-type`, `--clear-effort`, `--append-body`, and `task history` `change`.
    let out = run(
        &mut office,
        &[
            "task",
            "update",
            "--task",
            "T-A1",
            "--if-match",
            "4",
            "--work-type",
            "run",
            "--clear-effort",
            "--append-body",
            "## Appended",
        ],
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let p = &office.0.seen.last().unwrap().1;
    assert_eq!(p["work_type"], "run");
    assert_eq!(p["effort"], Value::Null);
    assert_eq!(p["append_body"], "## Appended");

    let out = run(&mut office, &["task", "history", "--task", "T-A1"]);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let env: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(env["data"][0]["change"]["type"], "task_updated");

    // 7. Stale conflict populates BOTH `error.retry_with.if-match` and `error.conflict`.
    office.0.fail_update_conflict = true;
    let out = run(
        &mut office,
        &[
            "task",
            "update",
            "--task",
            "T-A1",
            "--if-match",
            "2",
            "--title",
            "Stale",
        ],
    );
    assert_ne!(out.exit_code, 0);
    let env: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(env["error"]["code"], "conflict");
    assert_eq!(env["error"]["retry_with"]["if-match"], 4);
    assert_eq!(env["error"]["conflict"]["current_revision"], 4);
    assert_eq!(env["error"]["conflict"]["events_since"][0]["revision"], 3);
}
