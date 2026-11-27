use serde_json::{json, Value};

use crate::{execute_with_office, execute_with_office_and_env};

fn sample_resume_payload() -> Value {
    json!({
        "identity": {
            "member_id": "0193a5c0-0000-7000-8000-000000000003",
            "handle": "ada",
            "name": "Ada",
            "kind": "agent",
            "office_id": "0193a5c0-0000-7000-8000-000000000001",
            "session_mode": "managed"
        },
        "unread_inbox": {
            "unread_count": 2,
            "urgent_count": 1,
            "items": [{
                "id": "0193a5c0-0000-7000-8000-000000000700",
                "conversation_id": "0193a5c0-0000-7000-8000-000000000600",
                "from_member_id": "0193a5c0-0000-7000-8000-000000000001",
                "from_display_name": "Owner",
                "kind": "msg",
                "priority": "urgent",
                "urgent": true,
                "subject": "Urgent review",
                "delivered_because": "addressed",
                "created_at": "2026-11-27T12:00:00.000Z"
            }],
            "truncated": true
        },
        "active_tasks": [{
            "id": "0193a5c0-0000-7000-8000-000000000100",
            "key": "T-A1",
            "title": "Build resume dashboard",
            "status": "in_progress",
            "priority": "high",
            "revision": 3,
            "project_id": "0193a5c0-0000-7000-8000-000000000200",
            "project_key": "P-1",
            "latest_note": {
                "note_id": "0193a5c0-0000-7000-8000-000000000400",
                "revision": 2,
                "kind": "note",
                "title": "Checkpoint",
                "excerpt": "Next: wire CLI tests",
                "tags": ["checkpoint"],
                "is_checkpoint": true,
                "updated_at": "2026-11-27T12:01:00.000Z"
            }
        }],
        "active_tasks_total": 1,
        "open_asks": [],
        "active_worktrees": []
    })
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

struct RecordingTransport {
    calls: Vec<(String, Value)>,
    reply: Value,
}

impl crate::OfficeTransport for RecordingTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, crate::Refusal> {
        self.calls.push((method.to_owned(), params));
        Ok(self.reply.clone())
    }
}

#[test]
fn resume_and_me_shorthands_dispatch_single_agent_resume_call_with_limits() {
    for shorthand in ["resume", "me"] {
        let mut transport = RecordingTransport {
            calls: Vec::new(),
            reply: sample_resume_payload(),
        };
        let out = execute_with_office(
            "agent-os",
            &args(&[
                shorthand,
                "--tasks-limit",
                "5",
                "--inbox-limit",
                "3",
                "--asks-limit",
                "4",
                "--worktrees-limit",
                "2",
                "--json",
            ]),
            &mut crate::TransportOffice(&mut transport),
        );
        assert_eq!(out.exit_code, 0, "{shorthand}: {}", out.stdout);
        assert_eq!(transport.calls.len(), 1, "must be a single RPC round-trip");
        assert_eq!(transport.calls[0].0, "agent.resume");
        assert_eq!(
            transport.calls[0].1,
            json!({
                "tasks_limit": 5,
                "inbox_limit": 3,
                "asks_limit": 4,
                "worktrees_limit": 2
            })
        );
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["ok"], true);
        assert_eq!(env["data"]["identity"]["handle"], "ada");
        assert_eq!(env["data"]["active_tasks"][0]["key"], "T-A1");
        assert_eq!(
            env["data"]["active_tasks"][0]["latest_note"]["is_checkpoint"],
            true
        );
    }
}

#[test]
fn resume_supports_fields_pruning_formats_and_agent_os_format_env() {
    let mut transport = RecordingTransport {
        calls: Vec::new(),
        reply: sample_resume_payload(),
    };
    let out = execute_with_office(
        "agent-os",
        &args(&[
            "me",
            "--fields",
            "identity.handle,unread_inbox.unread_count",
            "--json",
        ]),
        &mut crate::TransportOffice(&mut transport),
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let env: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(
        env["data"],
        json!({
            "identity": { "handle": "ada" },
            "unread_inbox": { "unread_count": 2 }
        })
    );

    // AGENT_OS_FORMAT=compact honored when no --json / --format passed.
    let mut compact_transport = RecordingTransport {
        calls: Vec::new(),
        reply: sample_resume_payload(),
    };
    let compact_out = execute_with_office_and_env(
        "agent-os",
        &args(&["resume", "--fields", "identity.handle,active_tasks_total"]),
        &mut crate::TransportOffice(&mut compact_transport),
        Some("compact"),
    );
    assert_eq!(compact_out.exit_code, 0, "{}", compact_out.stdout);
    assert!(
        !compact_out.stdout.trim_start().starts_with('{'),
        "compact format should not emit a JSON envelope: {}",
        compact_out.stdout
    );
    assert!(compact_out.stdout.contains("ada"));

    // AGENT_OS_FORMAT=yaml honored when no --json / --format passed.
    let mut yaml_transport = RecordingTransport {
        calls: Vec::new(),
        reply: sample_resume_payload(),
    };
    let yaml_out = execute_with_office_and_env(
        "agent-os",
        &args(&["me", "--fields", "identity.handle"]),
        &mut crate::TransportOffice(&mut yaml_transport),
        Some("yaml"),
    );
    assert_eq!(yaml_out.exit_code, 0, "{}", yaml_out.stdout);
    assert!(
        yaml_out.stdout.contains("handle: ada"),
        "{}",
        yaml_out.stdout
    );
}

#[test]
fn task_note_checkpoint_flag_forwards_to_add_and_list_rpcs() {
    let mut add_transport = RecordingTransport {
        calls: Vec::new(),
        reply: json!({
            "note": {
                "id": "0193a5c0-0000-7000-8000-000000000400",
                "task_id": "0193a5c0-0000-7000-8000-000000000100",
                "author_id": "0193a5c0-0000-7000-8000-000000000003",
                "revision": 1,
                "kind": "note",
                "title": "Paused before review",
                "tags": ["checkpoint"],
                "body_markdown": "Paused before review",
                "created_at": "2026-11-27T12:00:00.000Z",
                "updated_at": "2026-11-27T12:00:00.000Z",
                "editable": true
            }
        }),
    };
    let add_out = execute_with_office(
        "agent-os",
        &args(&[
            "task-note",
            "add",
            "--task",
            "T-A1",
            "--checkpoint",
            "--text",
            "Paused before review",
            "--json",
        ]),
        &mut crate::TransportOffice(&mut add_transport),
    );
    assert_eq!(add_out.exit_code, 0, "{}", add_out.stdout);
    assert_eq!(add_transport.calls[0].0, "tasks.notes.add");
    assert_eq!(add_transport.calls[0].1["checkpoint"], true);

    let mut list_transport = RecordingTransport {
        calls: Vec::new(),
        reply: json!({ "task_id": "0193a5c0-0000-7000-8000-000000000100", "notes": [], "total": 0 }),
    };
    let list_out = execute_with_office(
        "agent-os",
        &args(&[
            "task-note",
            "list",
            "--task",
            "T-A1",
            "--checkpoint",
            "--json",
        ]),
        &mut crate::TransportOffice(&mut list_transport),
    );
    assert_eq!(list_out.exit_code, 0, "{}", list_out.stdout);
    assert_eq!(list_transport.calls[0].0, "tasks.notes.list");
    assert_eq!(list_transport.calls[0].1["tag"], "checkpoint");
}

#[test]
fn resume_and_me_help_and_schema_work_without_calling_office() {
    let mut transport = RecordingTransport {
        calls: Vec::new(),
        reply: Value::Null,
    };
    for flag in ["--help", "--schema"] {
        for word in ["resume", "me"] {
            let out = execute_with_office(
                "agent-os",
                &args(&[word, flag]),
                &mut crate::TransportOffice(&mut transport),
            );
            assert_eq!(out.exit_code, 0, "{word} {flag}: {}", out.stdout);
            assert!(out.stdout.contains("session resume") || out.stdout.contains("active_tasks"));
        }
    }
    assert!(transport.calls.is_empty());
}

#[test]
fn resume_and_me_reject_unknown_fields_before_calling_office() {
    let mut transport = RecordingTransport {
        calls: Vec::new(),
        reply: sample_resume_payload(),
    };
    for word in ["resume", "me"] {
        let out = execute_with_office(
            "agent-os",
            &args(&[word, "--fields", "identity.handle,no_such_field", "--json"]),
            &mut crate::TransportOffice(&mut transport),
        );
        assert_ne!(
            out.exit_code, 0,
            "{word} must reject unknown --fields: {}",
            out.stdout
        );
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["ok"], false, "{word}: {env}");
        assert_eq!(env["error"]["code"], "invalid_request", "{word}: {env}");
    }
    assert!(
        transport.calls.is_empty(),
        "unknown --fields must fail locally before calling the office"
    );
}
