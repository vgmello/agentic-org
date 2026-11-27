use serde_json::{json, Value};

use crate::execute_with_office;
use crate::runtime::transport::{OfficeTransport, TransportOffice};

struct StubTessTransport {
    calls: Vec<(String, Value)>,
}

impl OfficeTransport for StubTessTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, org_command::Refusal> {
        self.calls.push((method.to_owned(), params));
        let sample = json!({
            "id": "0193a5c0-0000-7000-8000-000000000901",
            "status": "pending",
            "command": "task create",
            "call": "tasks.create",
            "input": {"title": "Review release"},
            "summary": "Create release review task",
            "created_by_member_id": "0193a5c0-0000-7000-8000-000000000010",
            "for_caller_id": "0193a5c0-0000-7000-8000-000000000002",
            "preflight_ok": true,
            "created_at": "2026-11-27T00:00:00.000Z",
            "expires_at": "2026-11-27T00:15:00.000Z"
        });
        let block = json!({
            "title": "Create release review task",
            "suggestion_id": "0193a5c0-0000-7000-8000-000000000901"
        });
        match method {
            "suggestion.create" => Ok(json!({ "suggestion": sample, "ui_block": block })),
            "suggestion.list" => Ok(json!({ "suggestions": [sample] })),
            "suggestion.inspect" => Ok(json!({ "suggestion": sample, "ui_block": block })),
            "suggestion.withdraw" => {
                let mut w = sample;
                w["status"] = json!("withdrawn");
                Ok(json!({ "suggestion": w }))
            }
            "ui.present" => Ok(json!({ "ui_block": { "title": "Brief", "metrics": [] } })),
            other => panic!("unexpected method `{other}`"),
        }
    }
}

#[test]
fn tess_suggestion_and_ui_commands_dispatch_expected_rpcs() {
    let mut office = TransportOffice(StubTessTransport { calls: Vec::new() });
    for line in [
        &[
            "suggestion",
            "create",
            "--command",
            "task create",
            "--summary",
            "Create release review task",
            "--expected-revision",
            "2",
            "--input-json",
            r#"{"input":{"title":"Review release"}}"#,
        ][..],
        &["suggestion", "list", "--status", "pending"],
        &[
            "suggestion",
            "inspect",
            "--suggestion",
            "0193a5c0-0000-7000-8000-000000000901",
        ],
        &[
            "suggestion",
            "withdraw",
            "--suggestion",
            "0193a5c0-0000-7000-8000-000000000901",
            "--reason",
            "Superseded",
        ],
        &["ui", "present", "--title", "Brief"],
    ] {
        let args: Vec<String> = line.iter().map(|s| (*s).to_owned()).collect();
        let out = execute_with_office("agent-os", &args, &mut office);
        assert_eq!(out.exit_code, 0, "{line:?}: {}", out.stdout);
    }
    let methods: Vec<&str> = office.0.calls.iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(
        methods,
        vec![
            "suggestion.create",
            "suggestion.list",
            "suggestion.inspect",
            "suggestion.withdraw",
            "ui.present",
        ]
    );
    assert_eq!(office.0.calls[0].1["expected_revision"], json!(2));
}
