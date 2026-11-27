use std::time::Duration;

use serde_json::{json, Value};

use crate::runtime::transport::{OfficeTransport, TransportOffice};

struct RecordingTransport {
    last_method: Option<String>,
    last_timeout: Option<Duration>,
    response: Value,
}

impl OfficeTransport for RecordingTransport {
    fn call_method(&mut self, method: &str, _params: Value) -> Result<Value, org_command::Refusal> {
        self.last_method = Some(method.to_owned());
        self.last_timeout = None;
        Ok(self.response.clone())
    }

    fn call_method_with_timeout(
        &mut self,
        method: &str,
        _params: Value,
        timeout: Duration,
    ) -> Result<Value, org_command::Refusal> {
        self.last_method = Some(method.to_owned());
        self.last_timeout = Some(timeout);
        Ok(self.response.clone())
    }
}

#[test]
fn msg_ask_and_wait_ask_pass_timeout_budget_above_short_hold() {
    let mut t = TransportOffice(RecordingTransport {
        last_method: None,
        last_timeout: None,
        response: json!({
            "status": "waiting",
            "ask_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
            "conversation_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071bb",
            "thread_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
            "local_seq": 1,
            "origin_seq": 1
        }),
    });
    let ask_json = json!({
        "to": "owner",
        "questions": [{ "id": "q1", "question_type": "yes_no", "prompt": "Proceed?" }]
    });
    let out = crate::execute_with_office(
        "agent-os",
        &[
            "msg".into(),
            "ask".into(),
            "--input-json".into(),
            ask_json.to_string(),
        ],
        &mut t,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(t.0.last_method.as_deref(), Some("chat.ask"));
    assert!(
        t.0.last_timeout.unwrap() >= Duration::from_secs(100),
        "expected >= 100s timeout budget, got {:?}",
        t.0.last_timeout
    );

    let wait_out = crate::execute_with_office(
        "agent-os",
        &[
            "msg".into(),
            "wait".into(),
            "--ask".into(),
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa".into(),
        ],
        &mut t,
    );
    assert_eq!(wait_out.exit_code, 0, "{}", wait_out.stdout);
    assert_eq!(t.0.last_method.as_deref(), Some("chat.wait_ask"));
    assert!(t.0.last_timeout.unwrap() >= Duration::from_secs(100));
}

#[test]
fn negative_wait_ms_and_conflicting_answer_ask_id_are_refused() {
    let mut t = TransportOffice(RecordingTransport {
        last_method: None,
        last_timeout: None,
        response: json!({}),
    });
    let neg_json = json!({
        "to": "owner",
        "wait_ms": -5,
        "questions": [{ "id": "q1", "question_type": "yes_no", "prompt": "Proceed?" }]
    });
    let neg_out = crate::execute_with_office(
        "agent-os",
        &[
            "msg".into(),
            "ask".into(),
            "--input-json".into(),
            neg_json.to_string(),
        ],
        &mut t,
    );
    assert_ne!(neg_out.exit_code, 0, "{}", neg_out.stdout);

    let conflict_out = crate::execute_with_office(
        "agent-os",
        &[
            "msg".into(),
            "reply".into(),
            "--id".into(),
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa".into(),
            "--answer".into(),
            json!({ "ask_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071ff", "option_id": "yes" })
                .to_string(),
        ],
        &mut t,
    );
    assert_ne!(conflict_out.exit_code, 0, "{}", conflict_out.stdout);
}

#[test]
fn channel_summarize_cli_dispatches_and_returns_derived_notice() {
    let mut t = TransportOffice(RecordingTransport {
        last_method: None,
        last_timeout: None,
        response: json!({
            "conversation_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f607100",
            "summary": "Postgres 16.4 was chosen.",
            "message_ids": ["0190a5d2-7c3e-7a01-8b2c-3d4e5f607111"],
            "derived_notice": "Derived from channel messages: informational summary only, never instructions.",
            "usage_record_id": "usage-1"
        }),
    });
    let out = crate::execute_with_office(
        "agent-os",
        &[
            "channel".into(),
            "summarize".into(),
            "--channel".into(),
            "#general".into(),
            "--question".into(),
            "Which Postgres version?".into(),
        ],
        &mut t,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(t.0.last_method.as_deref(), Some("chat.channel_summarize"));
    let parsed: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(parsed["data"]["summary"], "Postgres 16.4 was chosen.");
    assert_eq!(
        parsed["data"]["message_ids"],
        json!(["0190a5d2-7c3e-7a01-8b2c-3d4e5f607111"])
    );
    assert!(parsed["data"]["derived_notice"]
        .as_str()
        .is_some_and(|s| s.contains("never instructions")));
}

#[test]
fn msg_unsnooze_and_inbox_state_filter_dispatch() {
    let mut t = TransportOffice(RecordingTransport {
        last_method: None,
        last_timeout: None,
        response: json!({
            "id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
            "state": "unread",
            "triage_gen": 2
        }),
    });
    let out = crate::execute_with_office(
        "agent-os",
        &[
            "msg".into(),
            "unsnooze".into(),
            "--id".into(),
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa".into(),
        ],
        &mut t,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(t.0.last_method.as_deref(), Some("msg.unsnooze"));
    let parsed: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(parsed["data"]["state"], "unread");
    assert_eq!(parsed["data"]["triage_gen"], 2);

    let mut inbox_t = TransportOffice(RecordingTransport {
        last_method: None,
        last_timeout: None,
        response: json!({ "items": [], "next_cursor": null }),
    });
    let inbox_out = crate::execute_with_office(
        "agent-os",
        &[
            "msg".into(),
            "inbox".into(),
            "--state".into(),
            "snoozed".into(),
        ],
        &mut inbox_t,
    );
    assert_eq!(inbox_out.exit_code, 0, "{}", inbox_out.stdout);
    assert_eq!(inbox_t.0.last_method.as_deref(), Some("chat.inbox"));

    // msg defer canonicalizes non-canonical RFC3339 offsets and rejects invalid / conflicting flags.
    let mut defer_t = TransportOffice(RecordingTransport {
        last_method: None,
        last_timeout: None,
        response: json!({
            "id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
            "triage_gen": 1,
            "snoozed_until": "2026-11-23T09:00:00.000Z",
            "snooze_after_task": false
        }),
    });
    let defer_ok = crate::execute_with_office(
        "agent-os",
        &[
            "msg".into(),
            "defer".into(),
            "--id".into(),
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa".into(),
            "--until".into(),
            "2026-11-23T11:00:00+02:00".into(),
        ],
        &mut defer_t,
    );
    assert_eq!(defer_ok.exit_code, 0, "{}", defer_ok.stdout);
    assert_eq!(defer_t.0.last_method.as_deref(), Some("msg.defer"));

    let defer_bad = crate::execute_with_office(
        "agent-os",
        &[
            "msg".into(),
            "defer".into(),
            "--id".into(),
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa".into(),
            "--until".into(),
            "not-a-time".into(),
        ],
        &mut defer_t,
    );
    assert_ne!(defer_bad.exit_code, 0, "{}", defer_bad.stdout);

    let defer_both = crate::execute_with_office(
        "agent-os",
        &[
            "msg".into(),
            "defer".into(),
            "--id".into(),
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa".into(),
            "--until".into(),
            "2026-11-23T09:00:00Z".into(),
            "--after-task".into(),
        ],
        &mut defer_t,
    );
    assert_ne!(defer_both.exit_code, 0, "{}", defer_both.stdout);
}

#[test]
fn all_twelve_msg_channel_status_writes_fail_closed_on_dry_run_with_zero_rpcs() {
    let ask_json = json!({
        "to": "owner",
        "questions": [{ "id": "q1", "question_type": "yes_no", "prompt": "Proceed?" }]
    })
    .to_string();
    let answer_json = json!([{ "question_id": "q1", "option_ids": ["yes"] }]).to_string();
    let cases: Vec<Vec<&str>> = vec![
        vec!["msg", "send", "--to", "owner", "--body", "hi", "--dry-run"],
        vec!["msg", "ask", "--input-json", &ask_json, "--dry-run"],
        vec![
            "msg",
            "withdraw",
            "--id",
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
            "--dry-run",
        ],
        vec![
            "msg",
            "reply",
            "--id",
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
            "--body",
            "reply",
            "--dry-run",
        ],
        vec![
            "msg",
            "reply",
            "--id",
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
            "--answer",
            &answer_json,
            "--dry-run",
        ],
        vec![
            "msg",
            "accept",
            "--id",
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
            "--dry-run",
        ],
        vec![
            "msg",
            "done",
            "--id",
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
            "--dry-run",
        ],
        vec![
            "msg",
            "defer",
            "--id",
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
            "--after-task",
            "--dry-run",
        ],
        vec![
            "msg",
            "unsnooze",
            "--id",
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
            "--dry-run",
        ],
        vec!["channel", "join", "--channel", "#ops", "--dry-run"],
        vec!["channel", "leave", "--channel", "#ops", "--dry-run"],
        vec!["status", "set", "--text", "working", "--dry-run"],
        vec!["status", "clear", "--dry-run"],
    ];

    for case in cases {
        let mut t = TransportOffice(RecordingTransport {
            last_method: None,
            last_timeout: None,
            response: json!({}),
        });
        let owned: Vec<String> = case.iter().map(|s| (*s).to_owned()).collect();
        let out = crate::execute_with_office("agent-os", &owned, &mut t);
        assert_eq!(out.exit_code, 1, "case {case:?} stdout={}", out.stdout);
        assert!(
            t.0.last_method.is_none(),
            "case {case:?} invoked RPC {:?} despite --dry-run!",
            t.0.last_method
        );
        let parsed: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(parsed["ok"], Value::Bool(false), "case {case:?}");
        assert_eq!(parsed["error"]["code"], "unsupported", "case {case:?}");
        assert_eq!(
            parsed["meta"]["dry_run"],
            Value::Bool(true),
            "case {case:?}"
        );
    }
}

/// Records every office call with its params.
struct CallLog {
    calls: Vec<(String, Value)>,
    response: Value,
}

impl OfficeTransport for CallLog {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, org_command::Refusal> {
        self.calls.push((method.to_owned(), params));
        Ok(self.response.clone())
    }
}

fn run_logged(args: &[&str], response: Value) -> (org_command::Output, Vec<(String, Value)>) {
    let mut t = TransportOffice(CallLog {
        calls: Vec::new(),
        response,
    });
    let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let out = crate::execute_with_office("agent-os", &owned, &mut t);
    (out, t.0.calls)
}

const M0: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a0";
const M2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a2";

#[test]
fn msg_list_catchup_flags_without_a_channel_conversation_or_thread_are_refused_with_zero_rpcs() {
    // Without --channel, --conversation or --thread, `msg list` reads a DM history over
    // `msg.list`, which would drop these flags: refuse instead.
    let cases: [&[&str]; 4] = [
        &["msg", "list", "--after-id", M2],
        &["msg", "list", "--top-level-only"],
        &["msg", "list", "--headers-only"],
        &[
            "msg",
            "list",
            "--after-id",
            M2,
            "--limit",
            "5",
            "--headers-only",
        ],
    ];
    for case in cases {
        let (out, calls) = run_logged(case, json!({}));
        assert_eq!(out.exit_code, 1, "{case:?}: {}", out.stdout);
        assert!(calls.is_empty(), "{case:?} called the office: {calls:?}");
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["error"]["code"], "invalid_request", "{case:?}");
        let text = format!("{} {}", env["error"]["message"], env["error"]["hint"]);
        assert!(text.contains("--channel"), "{case:?}: {text}");
    }
    // With a thread alone they read that thread over `chat.messages`, flags kept.
    let (out, calls) = run_logged(
        &[
            "msg",
            "list",
            "--thread",
            M0,
            "--after-id",
            M2,
            "--headers-only",
        ],
        json!({ "conversation_id": M0, "messages": [] }),
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "chat.messages");
    assert_eq!(calls[0].1["after_id"], M2);
    assert_eq!(calls[0].1["headers_only"], true);
}

#[test]
fn msg_list_delta_cursor_pages_without_replacing_the_after_id_anchor() {
    let page = json!({
        "conversation_id": M0, "messages": [], "next_after_id": M2, "total": 4
    });
    let (out, calls) = run_logged(
        &[
            "msg",
            "list",
            "--channel",
            "#ops",
            "--after-id",
            M2,
            "--cursor",
            M0,
        ],
        page.clone(),
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let params = &calls[0].1;
    assert_eq!(
        (&params["after_id"], &params["page_after_id"]),
        (&json!(M2), &json!(M0)),
        "{params}"
    );
    let env: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(
        (&env["meta"]["total"], &env["meta"]["next_cursor"]),
        (&json!(4), &json!(M2))
    );

    // A since-read page sends the cursor as the page anchor and no after_id at all.
    let (_, calls) = run_logged(
        &[
            "msg",
            "list",
            "--channel",
            "#ops",
            "--since-read",
            "--cursor",
            M2,
        ],
        page,
    );
    let params = &calls[0].1;
    assert_eq!(params["since_read"], true);
    assert_eq!(params["page_after_id"], M2);
    assert!(params.get("after_id").is_none(), "{params}");

    // A non-message cursor on a delta read is refused before any call.
    let (out, calls) = run_logged(
        &[
            "msg",
            "list",
            "--channel",
            "#ops",
            "--after-id",
            M2,
            "--cursor",
            "7",
        ],
        json!({}),
    );
    assert_eq!(out.exit_code, 1, "{}", out.stdout);
    assert!(calls.is_empty(), "{calls:?}");
}

#[test]
fn msg_send_reply_ask_attach_flags_and_zero_overhead_when_omitted() {
    use crate::runtime::file_guard::{with_test_roots, FileRoots, RootEntry};
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    const STAGED_ID: &str = "0193a5c0-0000-7000-8000-000000000801";
    const EXISTING_ID: &str = "0193a5c0-0000-7000-8000-000000000802";

    struct AttachMock {
        calls: Vec<(String, Value)>,
    }
    impl OfficeTransport for AttachMock {
        fn call_method(
            &mut self,
            method: &str,
            params: Value,
        ) -> Result<Value, org_command::Refusal> {
            self.calls.push((method.to_owned(), params));
            match method {
                "attachment.upload_begin" => Ok(json!({
                    "upload": {
                        "upload_id": STAGED_ID,
                        "filename": "note.md",
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
                        "filename": "note.md",
                        "mime_type": "text/markdown",
                        "byte_count": 5,
                        "received_bytes": 5,
                        "committed": true,
                        "expires_at": "2026-11-27T01:00:00.000Z",
                        "content_sha256": "0".repeat(64)
                    }
                })),
                "chat.send" => Ok(json!({
                    "id": M0,
                    "conversation_id": M2,
                    "thread_id": M0,
                    "local_seq": 1,
                    "origin_seq": 1,
                    "mentions": [],
                    "unmatched_mentions": []
                })),
                "msg.reply" => Ok(json!({
                    "id": M2,
                    "thread_id": M0,
                    "local_seq": 2,
                    "origin_seq": 2
                })),
                "chat.answer" => Ok(json!({
                    "ask_id": M0,
                    "conversation_id": M2,
                    "answer": { "by": M0, "at": "2026-11-27T00:00:00.000Z", "answers": [] }
                })),
                "chat.ask" => Ok(json!({
                    "status": "waiting",
                    "ask_id": M0,
                    "conversation_id": M2,
                    "thread_id": M0,
                    "local_seq": 3,
                    "origin_seq": 3
                })),
                other => panic!("unexpected RPC: {other}"),
            }
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("note.md"), b"hello").unwrap();
    let roots = FileRoots {
        cwd: tmp.path().to_path_buf(),
        workspace_roots: vec![RootEntry::from_path(tmp.path().to_path_buf()).unwrap()],
        ..FileRoots::default()
    };

    with_test_roots(roots, || {
        // 1. Plain `msg send` without `--attach`: zero upload RPCs, no `attachment_ids` key.
        let mut t = TransportOffice(AttachMock { calls: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "msg".into(),
                "send".into(),
                "--to".into(),
                "owner".into(),
                "--body".into(),
                "plain".into(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(t.0.calls.len(), 1);
        assert_eq!(t.0.calls[0].0, "chat.send");
        assert!(t.0.calls[0].1.get("attachment_ids").is_none());

        // 2. `msg send` with `--attach` + `--attachment-id` and omitted `--body`.
        let mut t = TransportOffice(AttachMock { calls: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "msg".into(),
                "send".into(),
                "--to".into(),
                "owner".into(),
                "--attach".into(),
                "note.md".into(),
                "--attachment-id".into(),
                EXISTING_ID.into(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        let send = t.0.calls.last().unwrap();
        assert_eq!(send.0, "chat.send");
        assert_eq!(send.1["body"], "");
        assert_eq!(send.1["attachment_ids"], json!([STAGED_ID, EXISTING_ID]));
        assert_eq!(t.0.calls[1].1["data_base64"], STANDARD.encode(b"hello"));

        // 3. `msg send` with neither `--body` nor attachments fails closed with zero RPCs.
        let mut t = TransportOffice(AttachMock { calls: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &["msg".into(), "send".into(), "--to".into(), "owner".into()],
            &mut t,
        );
        assert_ne!(out.exit_code, 0, "{}", out.stdout);
        assert!(t.0.calls.is_empty());

        // 4. `msg reply` (thread) with `--attach` and omitted `--body`.
        let mut t = TransportOffice(AttachMock { calls: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "msg".into(),
                "reply".into(),
                "--id".into(),
                M0.into(),
                "--attach".into(),
                "note.md".into(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        let reply = t.0.calls.last().unwrap();
        assert_eq!(reply.0, "msg.reply");
        assert_eq!(reply.1["attachment_ids"], json!([STAGED_ID]));

        // 5. `msg reply --answer` with `--attach`.
        let mut t = TransportOffice(AttachMock { calls: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "msg".into(),
                "reply".into(),
                "--id".into(),
                M0.into(),
                "--answer".into(),
                r#"[{"question_id":"q1","option_ids":["yes"]}]"#.into(),
                "--attach".into(),
                "note.md".into(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        let ans = t.0.calls.last().unwrap();
        assert_eq!(ans.0, "chat.answer");
        assert_eq!(ans.1["attachment_ids"], json!([STAGED_ID]));

        // 6. `msg ask` with `--attach` and `--attachment-id`.
        let mut t = TransportOffice(AttachMock { calls: vec![] });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "msg".into(),
                "ask".into(),
                "--to".into(),
                "owner".into(),
                "--prompt".into(),
                "Approve?".into(),
                "--attach".into(),
                "note.md".into(),
                "--attachment-id".into(),
                EXISTING_ID.into(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        let ask = t.0.calls.last().unwrap();
        assert_eq!(ask.0, "chat.ask");
        assert_eq!(ask.1["attachment_ids"], json!([STAGED_ID, EXISTING_ID]));
    });
}

#[test]
fn msg_attach_aborts_staged_uploads_on_downstream_error_and_accounts_for_answer_attachment_cap() {
    use org_command::Refusal;
    use org_proto::attachments::MESSAGE_ATTACHMENTS_MAX;
    use org_proto::ErrorCode;

    use crate::runtime::file_guard::{with_test_roots, FileRoots, RootEntry};
    use crate::runtime::transport::{OfficeTransport, TransportOffice};

    const STAGED_ID: &str = "0193a5c0-0000-7000-8000-000000000771";
    const EXISTING_ID: &str = "0193a5c0-0000-7000-8000-000000000772";

    struct FailingMsgMock {
        fail_method: &'static str,
        calls: Vec<(String, Value)>,
    }

    impl OfficeTransport for FailingMsgMock {
        fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
            self.calls.push((method.to_owned(), params));
            match method {
                "attachment.upload_begin" => Ok(json!({
                    "upload": {
                        "upload_id": STAGED_ID,
                        "filename": "note.md",
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
                        "filename": "note.md",
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
                    ErrorCode::Unavailable,
                    format!("{m} failed"),
                    "retry",
                )),
                "chat.answer" => Ok(json!({
                    "ask_id": M0,
                    "conversation_id": M2,
                    "answer": { "by": M0, "at": "2026-11-27T00:00:00.000Z", "answers": [] }
                })),
                other => panic!("unexpected RPC: {other}"),
            }
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("note.md"), b"hello").unwrap();
    let roots = FileRoots {
        cwd: tmp.path().to_path_buf(),
        workspace_roots: vec![RootEntry::from_path(tmp.path().to_path_buf()).unwrap()],
        ..FileRoots::default()
    };

    with_test_roots(roots, || {
        // 1. Downstream failure on chat.send, msg.reply, chat.ask, and chat.answer aborts ONLY the
        //    freshly staged upload (never the pre-existing `--attachment-id`).
        let cases: &[(&'static str, Vec<&str>)] = &[
            (
                "chat.send",
                vec![
                    "msg",
                    "send",
                    "--to",
                    "owner",
                    "--attach",
                    "note.md",
                    "--attachment-id",
                    EXISTING_ID,
                ],
            ),
            (
                "msg.reply",
                vec![
                    "msg",
                    "reply",
                    "--id",
                    M0,
                    "--attach",
                    "note.md",
                    "--attachment-id",
                    EXISTING_ID,
                ],
            ),
            (
                "chat.ask",
                vec![
                    "msg",
                    "ask",
                    "--to",
                    "owner",
                    "--prompt",
                    "Approve?",
                    "--attach",
                    "note.md",
                    "--attachment-id",
                    EXISTING_ID,
                ],
            ),
            (
                "chat.answer",
                vec![
                    "msg",
                    "reply",
                    "--id",
                    M0,
                    "--answer",
                    r#"[{"question_id":"q1","option_ids":["yes"]}]"#,
                    "--attach",
                    "note.md",
                    "--attachment-id",
                    EXISTING_ID,
                ],
            ),
        ];
        for (fail_method, args) in cases {
            let mut t = TransportOffice(FailingMsgMock {
                fail_method,
                calls: vec![],
            });
            let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
            let out = crate::execute_with_office("agent-os", &owned, &mut t);
            assert_ne!(out.exit_code, 0, "{fail_method}: {}", out.stdout);
            let aborted: Vec<&str> =
                t.0.calls
                    .iter()
                    .filter(|(m, _)| m == "attachment.upload_abort")
                    .filter_map(|(_, p)| p["upload_id"].as_str())
                    .collect();
            assert_eq!(
                aborted,
                vec![STAGED_ID],
                "{fail_method} must abort only staged upload, calls: {:?}",
                t.0.calls
            );
        }

        // 2. `--answer` JSON with `MESSAGE_ATTACHMENTS_MAX` pre-existing IDs + 1 `--attach` fails
        //    with `too_large` BEFORE staging any upload (zero RPCs).
        let existing_ten: Vec<String> = (0..MESSAGE_ATTACHMENTS_MAX)
            .map(|i| format!("0193a5c0-0000-7000-8000-0000000008{i:02}"))
            .collect();
        let over_cap_answer = json!({
            "answers": [{"question_id": "q1", "option_ids": ["yes"]}],
            "attachment_ids": existing_ten,
        })
        .to_string();
        let mut t = TransportOffice(FailingMsgMock {
            fail_method: "none",
            calls: vec![],
        });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "msg".into(),
                "reply".into(),
                "--id".into(),
                M0.into(),
                "--answer".into(),
                over_cap_answer,
                "--attach".into(),
                "note.md".into(),
            ],
            &mut t,
        );
        assert_ne!(out.exit_code, 0, "{}", out.stdout);
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["error"]["code"], "too_large");
        assert!(
            t.0.calls.is_empty(),
            "over-cap --answer must fail before staging any upload: {:?}",
            t.0.calls
        );

        // 3. `MESSAGE_ATTACHMENTS_MAX - 1` pre-existing IDs + 1 `--attach` succeeds at exact cap.
        let existing_nine: Vec<String> = (0..(MESSAGE_ATTACHMENTS_MAX - 1))
            .map(|i| format!("0193a5c0-0000-7000-8000-0000000009{i:02}"))
            .collect();
        let at_cap_answer = json!({
            "answers": [{"question_id": "q1", "option_ids": ["yes"]}],
            "attachment_ids": existing_nine,
        })
        .to_string();
        let mut t = TransportOffice(FailingMsgMock {
            fail_method: "none",
            calls: vec![],
        });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "msg".into(),
                "reply".into(),
                "--id".into(),
                M0.into(),
                "--answer".into(),
                at_cap_answer,
                "--attach".into(),
                "note.md".into(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        let ans = t.0.calls.last().unwrap();
        assert_eq!(ans.0, "chat.answer");
        assert_eq!(
            ans.1["attachment_ids"].as_array().unwrap().len(),
            MESSAGE_ATTACHMENTS_MAX
        );
    });
}
