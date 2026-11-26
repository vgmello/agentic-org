//! TASK-85.3 AX contract across the slices: truthful acknowledgements and a fail-closed
//! `--durable`, retry-stable IDs, the delivery fields and outcome status of messages and asks,
//! flag combinations refused instead of dropped, bounded pages on every list, the `task list`
//! filters, value-free yes-or-no flags, the quickstart and the office's error metadata.

use std::collections::BTreeMap;

use org_command::{Ack, Kind};
use serde_json::{json, Value};

use crate::runtime::transport::{derive_uuid_v7, OfficeTransport, Refusal, TransportOffice};

/// Answers each method with a fixed reply and records every call.
#[derive(Default)]
struct Office {
    replies: BTreeMap<&'static str, Value>,
    calls: Vec<(String, Value)>,
}

impl Office {
    fn with(replies: &[(&'static str, Value)]) -> TransportOffice<Self> {
        TransportOffice(Self {
            replies: replies.iter().cloned().collect(),
            calls: Vec::new(),
        })
    }
}

impl OfficeTransport for Office {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        Ok(self
            .replies
            .get(method)
            .cloned()
            .unwrap_or_else(|| panic!("no reply for `{method}`")))
    }
}

fn run(office: &mut TransportOffice<Office>, line: &[&str]) -> (u8, Value) {
    let args: Vec<String> = line.iter().map(|word| (*word).to_owned()).collect();
    let output = crate::execute_with_office("agent-os", &args, office);
    let envelope = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|e| panic!("{line:?}: {e}: {}", output.stdout));
    (output.exit_code, envelope)
}

fn sent<'a>(office: &'a TransportOffice<Office>, method: &str) -> &'a Value {
    &office
        .0
        .calls
        .iter()
        .find(|(m, _)| m == method)
        .unwrap_or_else(|| panic!("`{method}` was not called"))
        .1
}

fn chat_send_reply() -> Value {
    json!({
        "id": "0193a5c0-0000-7000-8000-000000000101",
        "conversation_id": "0193a5c0-0000-7000-8000-000000000100",
        "thread_id": "0193a5c0-0000-7000-8000-000000000101",
        "local_seq": 1,
        "origin_seq": 1,
        "recipient_presence": [{"member_id": "0193a5c0-0000-7000-8000-000000000004", "presence": "asleep"}],
        "woke_agents": ["0193a5c0-0000-7000-8000-000000000004"]
    })
}

fn ask_reply(status: &str) -> Value {
    json!({
        "status": status,
        "ask_id": "0193a5c0-0000-7000-8000-000000000105",
        "conversation_id": "0193a5c0-0000-7000-8000-000000000100",
        "thread_id": "0193a5c0-0000-7000-8000-000000000105",
        "local_seq": 5,
        "origin_seq": 5,
        "recipient_presence": [{"member_id": "0193a5c0-0000-7000-8000-000000000002", "presence": "online"}]
    })
}

#[test]
fn every_write_declares_its_ack_class_and_durable_is_refused_where_it_cannot_hold() {
    let table = crate::registry::table();
    let committed: Vec<String> = table
        .rows()
        .iter()
        .filter(|row| row.ack() == Some(Ack::Committed))
        .map(|row| row.name())
        .collect();
    // Only what the office does not journal before it answers (with the external lease verbs
    // of `TASK-137`, which declare their own commit acknowledgement).
    for name in &committed {
        assert!(
            ["msg ", "channel ", "status "]
                .iter()
                .any(|p| name.starts_with(p))
                || [
                    "repo fetch",
                    "repo push",
                    "worktree prepare",
                    "agent approve",
                    "agent revoke",
                ]
                .contains(&name.as_str()),
            "`{name}` is acknowledged at commit"
        );
    }
    for durable in [
        "task create",
        "team create",
        "process create",
        "repo register",
        "member set-hiring",
    ] {
        let (resource, verb) = durable.split_once(' ').unwrap();
        assert_eq!(
            table.find(resource, verb).unwrap().ack(),
            Some(Ack::Durable),
            "{durable}"
        );
    }
    // `--durable` on a committed write is refused before anything is sent.
    let mut office = Office::with(&[("chat.send", chat_send_reply())]);
    let (code, envelope) = run(
        &mut office,
        &["msg", "send", "--to", "owner", "--body", "hi", "--durable"],
    );
    assert_eq!(code, 1);
    assert_eq!(envelope["error"]["code"], "unsupported");
    assert!(office.0.calls.is_empty(), "nothing is sent");
    // Without it, the write reports `committed`, never `durable`.
    let (code, envelope) = run(
        &mut office,
        &["msg", "send", "--to", "owner", "--body", "hi"],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["meta"]["ack"], "committed");
    // An always-durable write reports `durable`, with or without the flag.
    let task = json!({"task": {
        "id": "0193a5c0-0000-7000-8000-000000000100", "office_code": "A", "number": 1, "key": "T-A1",
        "revision": 1, "team_id": "0193a5c0-0000-7000-8000-000000000010", "type": "task",
        "title": "T", "status": "todo", "body_markdown": "", "priority": "normal",
        "created_by_kind": "person", "updated_by_kind": "person", "checklist_total": 0,
        "checklist_done": 0, "comment_count": 0, "watcher_count": 0
    }});
    for extra in [&[][..], &["--durable"]] {
        let mut office = Office::with(&[("tasks.create", task.clone())]);
        let mut line = vec!["task", "create", "--title", "T"];
        line.extend_from_slice(extra);
        let (code, envelope) = run(&mut office, &line);
        assert_eq!(code, 0, "{envelope}");
        assert_eq!(envelope["meta"]["ack"], "durable", "{extra:?}");
    }
}

#[test]
fn a_retried_key_that_is_not_a_uuid_makes_the_same_message_and_command_ids() {
    let mut office = Office::with(&[("chat.send", chat_send_reply())]);
    for _ in 0..2 {
        let (code, _) = run(
            &mut office,
            &[
                "msg",
                "send",
                "--to",
                "owner",
                "--body",
                "hi",
                "--idempotency-key",
                "retry-1",
            ],
        );
        assert_eq!(code, 0);
    }
    let ids: Vec<&Value> = office.0.calls.iter().map(|(_, p)| &p["id"]).collect();
    assert_eq!(ids[0], ids[1]);
    assert_eq!(ids[0], &json!(derive_uuid_v7("msg-message", "retry-1")));
    // The hiring and organization writes derive their command IDs the same way.
    let mut office = Office::with(&[(
        "member.set_may_hire",
        json!({"id": "0193a5c0-0000-7000-8000-000000000051", "revision": 2, "generation": 3}),
    )]);
    let (code, envelope) = run(
        &mut office,
        &[
            "member",
            "set-hiring",
            "--member",
            "0193a5c0-0000-7000-8000-000000000051",
            "--allow",
            "--if-match",
            "1",
            "--idempotency-key",
            "retry-2",
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let params = sent(&office, "member.set_may_hire");
    assert_eq!(
        params["command_id"],
        json!(derive_uuid_v7("hiring-command", "retry-2"))
    );
    assert_eq!(params["may_hire"], true);
    assert_eq!(envelope["meta"]["operation_id"], "retry-2");
}

#[test]
fn set_hiring_takes_allow_or_revoke_and_never_both_or_neither() {
    let reply =
        json!({"id": "0193a5c0-0000-7000-8000-000000000051", "revision": 2, "generation": 3});
    let member = "0193a5c0-0000-7000-8000-000000000051";
    let mut office = Office::with(&[("member.set_may_hire", reply)]);
    let (code, _) = run(
        &mut office,
        &[
            "member",
            "set-hiring",
            "--member",
            member,
            "--revoke",
            "--if-match",
            "1",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(sent(&office, "member.set_may_hire")["may_hire"], false);
    for flags in [&["--allow", "--revoke"][..], &[]] {
        let mut office = Office::with(&[]);
        let mut line = vec![
            "member",
            "set-hiring",
            "--member",
            member,
            "--if-match",
            "1",
        ];
        line.extend_from_slice(flags);
        let (code, envelope) = run(&mut office, &line);
        assert_eq!(code, 1, "{flags:?}");
        assert_eq!(envelope["error"]["code"], "invalid_request");
        assert!(!envelope["error"]["hint"].as_str().unwrap().contains("true"));
        assert!(office.0.calls.is_empty());
    }
    // The old value-taking spelling is not a flag any more.
    let mut office = Office::with(&[]);
    let (code, _) = run(
        &mut office,
        &[
            "member",
            "set-hiring",
            "--member",
            member,
            "--may-hire",
            "--if-match",
            "1",
        ],
    );
    assert_eq!(code, 2);
}

#[test]
fn process_writes_are_overridable_unless_non_overridable_is_given() {
    let reply = json!({"document_id": "0193a5c0-0000-7000-8000-000000000091", "revision": 1, "document": {
        "id": "0193a5c0-0000-7000-8000-000000000091", "revision": 1, "kind": "standard", "title": "T",
        "status": "current", "level": "organization", "overridable": true, "summary": "S",
        "body_markdown": "B", "path": "p", "created_by_kind": "person", "updated_by_kind": "person"
    }});
    for (flag, overridable) in [(None, true), (Some("--non-overridable"), false)] {
        let mut office = Office::with(&[("process.write", reply.clone())]);
        let mut line = vec![
            "process", "create", "--kind", "standard", "--title", "T", "--body", "B",
        ];
        line.extend(flag);
        let (code, envelope) = run(&mut office, &line);
        assert_eq!(code, 0, "{envelope}");
        assert_eq!(sent(&office, "process.write")["overridable"], overridable);
    }
}

#[test]
fn msg_send_and_ask_report_presence_woken_agents_and_a_matching_meta_status() {
    let mut office = Office::with(&[("chat.send", chat_send_reply())]);
    let (_, envelope) = run(
        &mut office,
        &["msg", "send", "--to", "owner", "--body", "hi"],
    );
    assert_eq!(
        envelope["data"]["recipient_presence"][0]["presence"],
        "asleep"
    );
    assert_eq!(
        envelope["data"]["woke_agents"][0],
        "0193a5c0-0000-7000-8000-000000000004"
    );
    for (status, meta) in [
        ("answered", "completed"),
        ("waiting", "waiting"),
        ("timed_out", "timed_out"),
        ("required_pending", "suspended"),
        ("withdrawn", "withdrawn"),
    ] {
        let mut office = Office::with(&[("chat.ask", ask_reply(status))]);
        let (code, envelope) = run(
            &mut office,
            &[
                "msg",
                "ask",
                "--to",
                "owner",
                "--prompt",
                "Ship?",
                "--question-type",
                "yes_no",
            ],
        );
        assert_eq!(code, 0, "{envelope}");
        assert_eq!(envelope["data"]["status"], status);
        assert_eq!(envelope["meta"]["status"], meta, "{status}");
        assert_eq!(
            envelope["data"]["recipient_presence"][0]["presence"],
            "online"
        );
        assert_eq!(envelope["data"]["woke_agents"], json!([]));
    }
}

#[test]
fn msg_ask_builds_one_question_from_scalar_flags_and_never_mixes_the_forms() {
    let mut office = Office::with(&[("chat.ask", ask_reply("waiting"))]);
    let (code, envelope) = run(
        &mut office,
        &[
            "msg",
            "ask",
            "--to",
            "owner",
            "--prompt",
            "Where?",
            "--option",
            "dev=Development",
            "--option",
            "prod",
            "--default-value",
            "dev",
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(
        sent(&office, "chat.ask")["questions"],
        json!([{"id": "q1", "question_type": "choice", "prompt": "Where?",
                "options": [{"id": "dev", "label": "Development"}, {"id": "prod", "label": "prod"}],
                "default_value": "dev"}])
    );
    let questions = r#"{"questions":[{"id":"q1","question_type":"yes_no","prompt":"Ship?"}]}"#;
    for line in [
        &[
            "msg", "ask", "--to", "owner", "--prompt", "Ship?", "--json", questions,
        ][..],
        &["msg", "ask", "--to", "owner"],
        &["msg", "ask", "--to", "owner", "--option", "a"],
        &[
            "msg",
            "ask",
            "--to",
            "owner",
            "--prompt",
            "Ship?",
            "--question-type",
            "yes_no",
            "--option",
            "a",
        ],
        &[
            "msg",
            "ask",
            "--to",
            "owner",
            "--prompt",
            "Ship?",
            "--question-type",
            "choice",
        ],
        &[
            "msg", "ask", "--to", "owner", "--prompt", "Ship?", "--option", "=x",
        ],
    ] {
        let mut office = Office::with(&[]);
        let (code, envelope) = run(&mut office, line);
        assert_eq!(code, 1, "{line:?}: {envelope}");
        assert_eq!(envelope["error"]["code"], "invalid_request", "{line:?}");
        assert!(office.0.calls.is_empty(), "{line:?}");
    }
}

#[test]
fn reply_and_list_refuse_flag_combinations_they_would_drop() {
    let id = "0193a5c0-0000-7000-8000-000000000101";
    for line in [
        &["msg", "reply", "--id", id, "--answer", "[]", "--body", "x"][..],
        &[
            "msg",
            "reply",
            "--id",
            id,
            "--answer",
            "[]",
            "--priority",
            "high",
        ],
        &["msg", "reply", "--id", id],
        &["msg", "list", "--conversation", id, "--channel", "#general"],
        &[
            "msg",
            "list",
            "--channel",
            "#general",
            "--participant",
            "ada",
        ],
        &["msg", "list", "--channel", "#general", "--unread-only"],
        &["msg", "list", "--thread", id, "--unread-only"],
    ] {
        let mut office = Office::with(&[]);
        let (code, envelope) = run(&mut office, line);
        assert_eq!(code, 1, "{line:?}: {envelope}");
        assert_eq!(envelope["error"]["code"], "invalid_request", "{line:?}");
        assert!(office.0.calls.is_empty(), "{line:?}");
    }
    let mut office = Office::with(&[("msg.list", json!({"headers": []}))]);
    let (code, envelope) = run(
        &mut office,
        &["msg", "list", "--participant", "ada", "--unread-only"],
    );
    assert_eq!(code, 0, "{envelope}");
}

/// The seven lists that answered everything at once, each with the reply that holds two items
/// (`task list`, the eighth, pages on the office and is tested on its own).
fn list_cases() -> Vec<(Vec<&'static str>, &'static str, Value)> {
    let card = |n: u8| json!({"id": format!("0193a5c0-0000-7000-8000-00000000000{n}"), "name": "A", "kind": "agent", "presence": "busy", "teams": []});
    let channel = |n: u8| json!({"id": format!("0193a5c0-0000-7000-8000-00000000010{n}"), "kind": "channel", "name": "general", "archived": false, "revision": 1, "created_at": "2026-11-26T00:00:00.000Z", "members": [], "is_member": true});
    let event = |n: u64| json!({"task_id": "0193a5c0-0000-7000-8000-000000000100", "revision": n, "actor_kind": "person", "change": {}, "payload_sha256": "00"});
    let label = |name: &str| json!({"label": name, "task_count": 1});
    let note = |n: u8| json!({"id": format!("0193a5c0-0000-7000-8000-00000000040{n}"), "task_id": "0193a5c0-0000-7000-8000-000000000100", "author_id": "0193a5c0-0000-7000-8000-000000000001", "revision": 1, "body_markdown": "x", "created_at": "2026-11-26T00:00:00.000Z", "updated_at": "2026-11-26T00:00:00.000Z", "editable": true});
    let skill =
        |key: &str| json!({"key": key, "title": "T", "description": "D", "source": "built_in"});
    let process = |n: u8| {
        let id = format!("0193a5c0-0000-7000-8000-00000000050{n}");
        json!({"id": id, "key": null, "kind": "process", "applies": true, "mandatory": false,
            "winner_kind": null, "replaces": null, "problem": null, "chain": [],
            "document": {"id": id, "key": null, "revision": 1, "kind": "process", "title": "P",
                "status": "current", "owner_employee_id": null, "owner_name": null,
                "level": "organization", "team_id": null, "project_id": null, "scope_name": null,
                "overridable": true, "summary": "S", "body_markdown": "B", "path": "p.md",
                "created_at": null, "created_by_kind": "person", "created_by_id": null,
                "updated_at": null, "updated_by_kind": "person", "updated_by_id": null}})
    };
    vec![
        (vec!["who"], "who", json!({"members": [card(1), card(2)]})),
        (
            vec!["channel", "list"],
            "chat.channels",
            json!({"channels": [channel(1), channel(2)]}),
        ),
        (
            vec!["task", "history", "--task", "T-A1"],
            "tasks.history",
            json!({"task_id": "0193a5c0-0000-7000-8000-000000000100", "events": [event(1), event(2)]}),
        ),
        (
            vec!["task", "labels"],
            "tasks.labels",
            json!({"labels": [label("a"), label("b")]}),
        ),
        (
            vec!["task", "note", "list", "--task", "T-A1"],
            "tasks.notes.list",
            json!({"task_id": "0193a5c0-0000-7000-8000-000000000100", "notes": [note(1), note(2)]}),
        ),
        (
            vec!["skill", "list"],
            "skill.list",
            json!({"skills": [skill("a"), skill("b")]}),
        ),
        (
            vec!["process", "list"],
            "process.effective",
            json!({"scope_chain": ["Organization"], "entries": [process(1), process(2)],
                "problems": [], "computed_at": "2026-11-26T00:00:00Z"}),
        ),
    ]
}

#[test]
fn every_list_answers_bounded_pages_with_a_cursor_and_a_total() {
    let table = crate::registry::table();
    for row in table.rows().iter().filter(|row| row.kind == Kind::List) {
        assert!(
            row.input.iter().all(|f| f.name != "max_results"),
            "{}",
            row.name()
        );
    }
    for (line, method, reply) in list_cases() {
        let mut office = Office::with(&[(method, reply)]);
        let mut first = line.clone();
        first.extend(["--limit", "1"]);
        let (code, envelope) = run(&mut office, &first);
        assert_eq!(code, 0, "{line:?}: {envelope}");
        assert_eq!(
            envelope["data"].as_array().map(Vec::len),
            Some(1),
            "{line:?}"
        );
        assert_eq!(envelope["meta"]["next_cursor"], "1", "{line:?}");
        assert_eq!(envelope["meta"]["total"], 2, "{line:?}");
        let mut second = line.clone();
        second.extend(["--limit", "1", "--cursor", "1"]);
        let (code, envelope) = run(&mut office, &second);
        assert_eq!(code, 0, "{line:?}: {envelope}");
        assert_eq!(
            envelope["data"].as_array().map(Vec::len),
            Some(1),
            "{line:?}"
        );
        assert!(envelope["meta"].get("next_cursor").is_none(), "{line:?}");
        let mut over = line.clone();
        over.extend(["--limit", "201"]);
        let (code, _) = run(&mut office, &over);
        assert_eq!(code, 2, "{line:?}: --limit is at most 200");
    }
}

/// One `tasks.list` task as the office answers it.
fn task_view(n: u64) -> Value {
    json!({
        "id": format!("0193a5c0-0000-7000-8000-{n:012}"),
        "office_code": "A",
        "number": n,
        "key": format!("T-A{n}"),
        "revision": 1,
        "team_id": "0193a5c0-0000-7000-8000-000000000001",
        "type": "task",
        "title": format!("Task {n}"),
        "status": "todo",
        "body_markdown": "",
        "priority": "normal",
        "created_by_kind": "person",
        "updated_by_kind": "person",
        "checklist_total": 0,
        "checklist_done": 0,
        "comment_count": 0,
        "watcher_count": 0
    })
}

#[test]
fn task_list_pages_on_the_office_and_sends_every_filter() {
    let reply = json!({"tasks": [], "total": 45});
    let mut office = Office::with(&[("tasks.list", reply.clone())]);
    let (code, envelope) = run(
        &mut office,
        &[
            "task",
            "list",
            "--exclude-status",
            "done",
            "--exclude-status",
            "cancelled",
            "--unassigned",
            "--modified-file",
            "src/auth",
            "--limit",
            "20",
            "--cursor",
            "20",
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let params = sent(&office, "tasks.list");
    assert_eq!(params["exclude_status"], json!(["done", "cancelled"]));
    assert_eq!(params["unassigned"], true);
    assert_eq!(params["modified_file"], "src/auth");
    assert_eq!(
        (params["offset"].clone(), params["limit"].clone()),
        (json!(20), json!(20))
    );
    assert_eq!(envelope["meta"]["total"], 45);
    // An empty page short of the total never points on: its cursor would be its own.
    assert_eq!(envelope["meta"].get("next_cursor"), None, "{envelope}");
    // A page with tasks points past the tasks it returned.
    let tasks: Vec<Value> = (0..3).map(task_view).collect();
    let mut office = Office::with(&[("tasks.list", json!({"tasks": tasks, "total": 45}))]);
    let (code, envelope) = run(&mut office, &["task", "list", "--cursor", "20"]);
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"].as_array().map(Vec::len), Some(3));
    assert_eq!(envelope["meta"]["next_cursor"], "23", "{envelope}");
    // The last page does not.
    let tasks: Vec<Value> = (0..2).map(task_view).collect();
    let mut office = Office::with(&[("tasks.list", json!({"tasks": tasks, "total": 45}))]);
    let (code, envelope) = run(&mut office, &["task", "list", "--cursor", "43"]);
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["meta"].get("next_cursor"), None, "{envelope}");
    // `--count`: no task, only the total.
    let mut office = Office::with(&[("tasks.list", reply)]);
    let (code, envelope) = run(
        &mut office,
        &["task", "list", "--count", "--status", "todo"],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(sent(&office, "tasks.list")["limit"], 0);
    assert_eq!(
        (envelope["data"].clone(), envelope["meta"]["total"].clone()),
        (json!([]), json!(45))
    );
    for line in [
        &[
            "task",
            "list",
            "--status",
            "done",
            "--exclude-status",
            "done",
        ][..],
        &[
            "task",
            "list",
            "--unassigned",
            "--assignee-id",
            "0193a5c0-0000-7000-8000-000000000003",
        ],
        &["task", "list", "--count", "--cursor", "20"],
    ] {
        let mut office = Office::with(&[]);
        let (code, envelope) = run(&mut office, line);
        assert_eq!(
            (code, envelope["error"]["code"].clone()),
            (1, json!("invalid_request")),
            "{line:?}"
        );
        assert!(office.0.calls.is_empty(), "{line:?}");
    }
    let mut office = Office::with(&[]);
    assert_eq!(
        run(&mut office, &["task", "list", "--max-results", "500"]).0,
        2
    );
}

#[test]
fn an_unreadable_task_answer_is_unavailable_not_a_storage_error() {
    let mut office = Office::with(&[("tasks.list", json!({"tasks": "nope"}))]);
    let (code, envelope) = run(&mut office, &["task", "list"]);
    assert_eq!(code, 1);
    assert_eq!(envelope["error"]["code"], "unavailable");
}

#[test]
fn the_office_error_metadata_and_its_hints_reach_the_envelope() {
    let mut limited = org_proto::ApiError::rate_limited(2500);
    limited.message = "too many attempts".into();
    let refusal = crate::runtime::binding::api_refusal(limited);
    let details = refusal.details.as_deref().expect("details");
    assert_eq!(details.retry_after_ms, Some(2500));
    assert!(refusal.hint.contains("2500 ms"), "{}", refusal.hint);
    let refusal =
        crate::runtime::binding::api_refusal(org_proto::ApiError::rule("login", "format"));
    let details = refusal.details.as_deref().expect("details");
    assert_eq!(
        (details.field.as_deref(), details.rule.as_deref()),
        (Some("login"), Some("format"))
    );
    assert!(refusal.hint.contains("`login`") && refusal.hint.contains("`format`"));
}

#[test]
fn agent_os_help_opens_with_the_quickstart_of_its_binding_state() {
    use crate::runtime::binding::Binding;
    let help = vec!["--help".to_owned()];
    let unbound = crate::execute(&help, Binding::default());
    let lines: Vec<&str> = unbound.stdout.lines().take(5).collect();
    assert!(
        lines[1].contains("agent-os login --as @<handle>"),
        "{lines:?}"
    );
    assert!(lines[2].contains("agent-os boot --offline"), "{lines:?}");
    assert_eq!(lines[4], "", "four lines, then the help");
    let bound = crate::execute(
        &help,
        Binding {
            session_token: Some("token".into()),
            ..Binding::default()
        },
    );
    let lines: Vec<&str> = bound.stdout.lines().take(5).collect();
    assert!(
        lines[1].contains("agent-os boot --format yaml"),
        "{lines:?}"
    );
    assert!(lines[2].contains("context compaction"), "{lines:?}");
    assert!(bound.stdout.contains("\nUsage: agent-os <resource> <verb>"));
    // Only the top-level help has it.
    let resource = crate::execute(&["task".into(), "--help".into()], Binding::default());
    assert!(resource.stdout.starts_with("Usage: "));
}

#[test]
fn shorthands_read_the_same_in_help_and_schema() {
    use crate::runtime::binding::Binding;
    for (short, long) in [
        ("boot --schema", "session boot --schema"),
        ("who --schema", "directory list --schema"),
        ("instructions overview --help", "session boot --help"),
        ("task note list --schema", "task-note list --schema"),
    ] {
        let words = |line: &str| -> Vec<String> { line.split(' ').map(str::to_owned).collect() };
        let a = crate::execute(&words(short), Binding::default());
        let b = crate::execute(&words(long), Binding::default());
        assert_eq!(a, b, "{short}");
        assert_eq!(a.exit_code, 0, "{short}: {}", a.stdout);
    }
}
