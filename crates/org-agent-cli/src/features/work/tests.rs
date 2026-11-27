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
