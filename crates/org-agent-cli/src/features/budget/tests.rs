use serde_json::{json, Value};

use crate::execute_with_office;
use crate::runtime::transport::{OfficeTransport, TransportOffice};

struct FakeBudgetTransport {
    last_method: Option<String>,
    last_params: Option<Value>,
}

impl OfficeTransport for FakeBudgetTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, org_command::Refusal> {
        self.last_method = Some(method.to_owned());
        self.last_params = Some(params);
        Ok(json!({
            "agent_id": "0193a5c0-0000-7000-8000-000000000003"
        }))
    }
}

#[test]
fn budget_status_command_dispatches_budget_agent_own() {
    let transport = FakeBudgetTransport {
        last_method: None,
        last_params: None,
    };
    let args = vec![
        "budget".into(),
        "status".into(),
        "--task".into(),
        "T-A1".into(),
    ];
    let mut office = TransportOffice(transport);
    let out = execute_with_office("agent-os", &args, &mut office);
    assert_eq!(out.exit_code, 0, "stdout: {}", out.stdout);
    assert_eq!(office.0.last_method.as_deref(), Some("budget.agent_own"));
    assert_eq!(office.0.last_params, Some(json!({ "task_id": "T-A1" })));
}
