//! Unit & contract tests for the 9-verb `agent-os review` VSA slice (`TASK-92.1`).

use org_command::Refusal;
use serde_json::{json, Value};

use crate::execute_with_office;
use crate::runtime::transport::{OfficeTransport, TransportOffice};

struct StubTransport {
    calls: Vec<(String, Value)>,
    reply: Value,
}

impl StubTransport {
    fn new(reply: Value) -> Self {
        Self {
            calls: Vec::new(),
            reply,
        }
    }
}

impl OfficeTransport for StubTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        Ok(self.reply.clone())
    }
}

fn sample_review() -> Value {
    json!({
        "id": "018bce83-1200-7000-8000-000000000001",
        "key": "RV-A1",
        "office_code": "A",
        "number": 1,
        "organization_id": "018bce83-1200-7000-8000-000000000099",
        "target_kind": "code",
        "kind_slug": "code-review",
        "title": "Add parser",
        "independent": false,
        "status": "open",
        "current_round": 1,
        "pin_stale": false,
        "open_findings": 0,
        "revision": 1,
        "created_by": "018bce83-1200-7000-8000-000000000010",
        "created_at": "2026-11-27T00:00:00.000Z",
        "updated_at": "2026-11-27T00:00:00.000Z"
    })
}

fn sample_round(mode: &str, num: u32) -> Value {
    json!({
        "round_number": num,
        "session_mode": mode,
        "submitted_by": "018bce83-1200-7000-8000-000000000010",
        "submitted_at": "2026-11-27T00:00:00.000Z"
    })
}

fn run(transport: StubTransport, args: &[&str]) -> (Value, i32, Vec<(String, Value)>) {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let mut office = TransportOffice(transport);
    let out = execute_with_office("agent-os", &owned, &mut office);
    let env: Value = serde_json::from_str(&out.stdout).unwrap_or_else(|e| {
        panic!("non-JSON output for {args:?}: {e}: {}", out.stdout);
    });
    (env, i32::from(out.exit_code), office.0.calls)
}

#[test]
fn review_request_supports_auto_assign_fresh_and_input_json() {
    let reply = json!({ "review": sample_review(), "round": sample_round("fresh", 1) });
    let (env, code, calls) = run(
        StubTransport::new(reply),
        &[
            "review",
            "request",
            "--title",
            "Add parser",
            "--target-kind",
            "doc",
            "--target-ref",
            "docs/parser.md",
            "--auto-assign",
            "--fresh",
        ],
    );
    assert_eq!(code, 0, "{env}");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "review.request");
    assert_eq!(calls[0].1["routing"]["mode"], "single_agent");
    assert_eq!(calls[0].1["session_mode"], "fresh");
}

#[test]
fn review_inspect_forwards_fields_and_preserves_pruned_reply() {
    let pruned = json!({
        "review": {
            "id": "018bce83-1200-7000-8000-000000000001",
            "key": "RV-A1",
            "office_code": "A",
            "number": 1,
            "organization_id": "018bce83-1200-7000-8000-000000000099",
            "target_kind": "code",
            "kind_slug": "code-review",
            "title": "Add parser",
            "independent": false,
            "status": "open",
            "current_round": 1,
            "pin_stale": true,
            "open_findings": 0,
            "revision": 1,
            "created_by": "018bce83-1200-7000-8000-000000000010",
            "created_at": "2026-11-27T00:00:00.000Z",
            "updated_at": "2026-11-27T00:00:00.000Z"
        }
    });
    let (env, code, calls) = run(
        StubTransport::new(pruned),
        &[
            "review",
            "inspect",
            "--review",
            "RV-A1",
            "--fields",
            "review.status,review.pin_stale",
        ],
    );
    assert_eq!(code, 0, "{env}");
    assert_eq!(calls[0].0, "review.inspect");
    assert_eq!(calls[0].1["fields"], json!(["review"]));
    assert!(env["data"].get("rounds").is_none());
    assert_eq!(env["data"]["review"]["status"], "open");
    assert_eq!(env["data"]["review"]["pin_stale"], true);
    assert!(env["data"]["review"].get("title").is_none());
}

#[test]
fn review_diff_list_route_resubmit_and_close_wire_correctly() {
    let diff_reply = json!({
        "review_id": "018bce83-1200-7000-8000-000000000001",
        "round_number": 2,
        "since_round": 1,
        "base_oid": "0000000000000000000000000000000000000001",
        "head_oid": "0000000000000000000000000000000000000002",
        "files": [],
        "total_files": 0,
        "truncated": false
    });
    let (env, code, calls) = run(
        StubTransport::new(diff_reply),
        &[
            "review",
            "diff",
            "--review",
            "RV-A1",
            "--round",
            "2",
            "--since-round",
            "1",
            "--stat-only",
        ],
    );
    assert_eq!(code, 0, "{env}");
    assert_eq!(calls[0].0, "review.diff");
    assert_eq!(calls[0].1["since_round"], 1);
    assert_eq!(calls[0].1["stat_only"], true);

    let list_reply = json!({ "reviews": [sample_review()], "total": 1 });
    let (env, code, calls) = run(
        StubTransport::new(list_reply),
        &["review", "list", "--status", "open"],
    );
    assert_eq!(code, 0, "{env}");
    assert_eq!(calls[0].0, "review.list");
    assert_eq!(env["meta"]["total"], 1);

    let route_reply = json!({ "review": sample_review(), "reviewers": [] });
    let (env, code, calls) = run(
        StubTransport::new(route_reply),
        &[
            "review",
            "route",
            "--review",
            "RV-A1",
            "--team",
            "018bce83-1200-7000-8000-000000000044",
            "--if-match",
            "1",
        ],
    );
    assert_eq!(code, 0, "{env}");
    assert_eq!(calls[0].0, "review.route");
    assert_eq!(calls[0].1["routing"]["mode"], "pool");

    let resub_reply = json!({ "review": sample_review(), "round": sample_round("fresh", 2) });
    let (env, code, calls) = run(
        StubTransport::new(resub_reply),
        &[
            "review",
            "resubmit",
            "--review",
            "RV-A1",
            "--fresh",
            "--note",
            "Final check",
            "--if-match",
            "2",
        ],
    );
    assert_eq!(code, 0, "{env}");
    assert_eq!(calls[0].0, "review.resubmit");
    assert_eq!(calls[0].1["fresh"], true);
    assert_eq!(calls[0].1["session_mode"], "fresh");

    let close_reply = json!({ "review": sample_review() });
    let (env, code, calls) = run(
        StubTransport::new(close_reply),
        &[
            "review",
            "close",
            "--review",
            "RV-A1",
            "--reason",
            "merged",
            "--if-match",
            "3",
        ],
    );
    assert_eq!(code, 0, "{env}");
    assert_eq!(calls[0].0, "review.close");
    assert_eq!(calls[0].1["reason"], "merged");
}

#[test]
fn review_finding_and_verdict_support_attachments_and_structured_findings() {
    let finding_reply = json!({
        "review": sample_review(),
        "finding": {
            "id": "018bce83-1200-7000-8000-000000000020",
            "review_id": "018bce83-1200-7000-8000-000000000001",
            "number": 1,
            "round_number": 1,
            "severity": "blocker",
            "status": "open",
            "anchor": {"kind": "file_line", "path": "src/lib.rs", "start_line": 10, "end_line": 12},
            "body_markdown": "Fix overflow",
            "author_id": "018bce83-1200-7000-8000-000000000011",
            "attachment_ids": ["018bce83-1200-7000-8000-000000000077"],
            "revision": 1,
            "created_at": "2026-11-27T00:00:00.000Z",
            "updated_at": "2026-11-27T00:00:00.000Z"
        }
    });
    let (env, code, calls) = run(
        StubTransport::new(finding_reply),
        &[
            "review",
            "finding",
            "--review",
            "RV-A1",
            "--action",
            "add",
            "--severity",
            "blocker",
            "--path",
            "src/lib.rs",
            "--start-line",
            "10",
            "--end-line",
            "12",
            "--body",
            "Fix overflow",
            "--attachment-id",
            "018bce83-1200-7000-8000-000000000077",
        ],
    );
    assert_eq!(code, 0, "{env}");
    assert_eq!(calls[0].0, "review.finding");
    assert_eq!(calls[0].1["op"]["action"], "add");
    assert_eq!(calls[0].1["op"]["anchor"]["kind"], "file_line");
    assert_eq!(
        calls[0].1["op"]["attachment_ids"],
        json!(["018bce83-1200-7000-8000-000000000077"])
    );

    let verdict_reply = json!({
        "review": sample_review(),
        "verdict": {
            "reviewer_id": "018bce83-1200-7000-8000-000000000011",
            "round_number": 1,
            "verdict": "changes_requested",
            "submitted_at": "2026-11-27T00:00:00.000Z"
        },
        "findings": []
    });
    let (env, code, calls) = run(
        StubTransport::new(verdict_reply),
        &[
            "review",
            "verdict",
            "--input-json",
            r#"{"review":"RV-A1","round":1,"verdict":"changes_requested","summary":"See findings","findings":[{"severity":"major","body_markdown":"Check bounds"}]}"#,
        ],
    );
    assert_eq!(code, 0, "{env}");
    assert_eq!(calls[0].0, "review.verdict");
    assert_eq!(calls[0].1["findings"].as_array().unwrap().len(), 1);
    assert!(org_proto::is_uuid_v7(
        calls[0].1["findings"][0]["finding_id"].as_str().unwrap()
    ));
}
