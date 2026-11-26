use super::*;
use crate::dispatch::TransportOffice;

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
