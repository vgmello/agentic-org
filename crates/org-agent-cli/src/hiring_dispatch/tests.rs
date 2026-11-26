//! Hiring dispatch: the RPC payloads of execution get and set, and member hiring rights.
use super::*;

struct MockTransport {
    last_method: Option<String>,
    last_params: Option<Value>,
    reply: Value,
}

impl OfficeTransport for MockTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.last_method = Some(method.to_owned());
        self.last_params = Some(params);
        Ok(self.reply.clone())
    }
}

#[test]
fn agent_execution_get_and_set_dispatch_expected_rpc_payloads() {
    let get_cmd = crate::hiring_table::HIRING_COMMANDS
        .iter()
        .find(|c| c.resource == "agent" && c.verb == "execution-get")
        .unwrap();
    let set_cmd = crate::hiring_table::HIRING_COMMANDS
        .iter()
        .find(|c| c.resource == "agent" && c.verb == "execution-set")
        .unwrap();

    let mut get_tx = MockTransport {
        last_method: None,
        last_params: None,
        reply: json!({
            "employee_id": "0193a5c0-0000-7000-8000-000000000051",
            "model_profile_id": "0193a5c0-0000-7000-8000-000000000060",
            "model_id": "sonnet",
            "tools_config": [],
            "permission_preset": "autonomous_sandboxed",
            "enabled": true,
            "system_prompt_markdown": "You are an engineer.",
            "revision": 2
        }),
    };
    let get_req = Request {
        command: get_cmd,
        input: json!({ "member": "0193a5c0-0000-7000-8000-000000000051" })
            .as_object()
            .unwrap()
            .clone(),
        limit: None,
        cursor: None,
        dry_run: false,
        idempotency_key: None,
        if_match: None,
        durable: false,
    };
    let ans = try_execute(&mut get_tx, &get_req).unwrap().unwrap();
    assert_eq!(
        get_tx.last_method.as_deref(),
        Some("member.agent_execution_get")
    );
    assert_eq!(ans.data["model_id"], "sonnet");

    let mut set_tx = MockTransport {
        last_method: None,
        last_params: None,
        reply: json!({
            "id": "0193a5c0-0000-7000-8000-000000000051",
            "revision": 3,
            "generation": 12
        }),
    };
    let set_req = Request {
        command: set_cmd,
        input: json!({
            "member": "0193a5c0-0000-7000-8000-000000000051",
            "model_profile_id": "0193a5c0-0000-7000-8000-000000000060",
            "model_id": "sonnet",
            "permission_preset": "autonomous_sandboxed",
            "enabled": true,
            "system_prompt_markdown": "Focus on Rust."
        })
        .as_object()
        .unwrap()
        .clone(),
        limit: None,
        cursor: None,
        dry_run: false,
        idempotency_key: Some("0193a5c0-0000-7000-8000-000000000099".into()),
        if_match: Some(2),
        durable: false,
    };
    let _ = try_execute(&mut set_tx, &set_req).unwrap().unwrap();
    assert_eq!(
        set_tx.last_method.as_deref(),
        Some("member.agent_execution_set")
    );
    let sent = set_tx.last_params.unwrap();
    assert_eq!(sent["expected_version"], 2);
    assert_eq!(sent["permission_preset"], "autonomous_sandboxed");
    assert_eq!(sent["system_prompt_markdown"], "Focus on Rust.");
}
