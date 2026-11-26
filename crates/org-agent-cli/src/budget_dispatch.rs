//! Dispatch for `agent-os budget status [--task <ref>]` (`TASK-53`, `doc-92`, `doc-51` §1).

use org_command::{Answer, Refusal, Request};
use org_proto::budgets::{BudgetAgentOwn, BudgetAgentOwnParams, BudgetAgentOwnResult};
use org_proto::{Call, ErrorCode};

use crate::transport::OfficeTransport;

pub fn try_execute<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    if request.command.resource != "budget" || request.command.verb != "status" {
        return None;
    }
    Some(read_budget_status(transport, request))
}

fn read_budget_status<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let task_id = request
        .input
        .get("task")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let params = BudgetAgentOwnParams { task_id };
    let wire = serde_json::to_value(params).map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "could not encode budget.agent_own request",
            "Check your input flags.",
        )
    })?;
    let raw = transport.call_method(BudgetAgentOwn::METHOD, wire)?;
    let res: BudgetAgentOwnResult = serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "the office returned an unreadable budget status response",
            "Retry after checking the office.",
        )
    })?;
    let val = serde_json::to_value(res).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "could not format budget status",
            "Retry the command.",
        )
    })?;
    Ok(Answer::completed(val))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use crate::execute_with_office;
    use crate::transport::OfficeTransport;
    use crate::transport::TransportOffice;

    struct FakeBudgetTransport {
        last_method: Option<String>,
        last_params: Option<Value>,
    }

    impl OfficeTransport for FakeBudgetTransport {
        fn call_method(
            &mut self,
            method: &str,
            params: Value,
        ) -> Result<Value, org_command::Refusal> {
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
}
