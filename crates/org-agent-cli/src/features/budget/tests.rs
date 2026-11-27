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

struct MultiReplyTransport {
    last_method: Option<String>,
    last_params: Option<Value>,
}

impl OfficeTransport for MultiReplyTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, org_command::Refusal> {
        self.last_method = Some(method.to_owned());
        self.last_params = Some(params);
        match method {
            "budget.list" => Ok(json!({ "budgets": [] })),
            "budget.get" => Ok(json!({ "budget": null })),
            "cost.query" => Ok(json!({
                "time_zone": "UTC",
                "scoped_to_projects": false,
                "summary": {
                    "known_cost_micro_usd": null,
                    "known_tokens": null,
                    "input_tokens": null,
                    "output_tokens": null,
                    "cache_read_tokens": null,
                    "cache_write_tokens": null,
                    "reasoning_tokens": null,
                    "record_count": 0,
                    "unpriced_records": 0,
                    "incomplete_records": 0,
                    "includes_estimates": false,
                    "includes_api_equivalent": false,
                    "incomplete_streams": 0
                },
                "breakdown": []
            })),
            "cost.price.list" => Ok(json!({ "prices": [] })),
            "cost.subscription_usage.list" => Ok(json!({
                "snapshots": [],
                "totals_by_model": [],
                "partial_totals": false,
                "excluded_snapshot_count": 0,
                "is_api_equivalent_estimate": false
            })),
            other => panic!("unexpected method `{other}`"),
        }
    }
}

#[test]
fn budget_and_cost_operator_commands_dispatch_expected_rpcs() {
    for (line, expected_method) in [
        (
            &["budget", "list", "--scope-kind", "agent"][..],
            "budget.list",
        ),
        (
            &[
                "budget",
                "inspect",
                "--budget",
                "0193a5c0-0000-7000-8000-000000000701",
            ],
            "budget.get",
        ),
        (
            &[
                "cost",
                "query",
                "--period",
                "this_month",
                "--group-by",
                "project",
            ],
            "cost.query",
        ),
        (
            &["cost", "prices", "--provider", "example"],
            "cost.price.list",
        ),
        (
            &["cost", "subscription-usage", "--include-archived"],
            "cost.subscription_usage.list",
        ),
    ] {
        let mut office = TransportOffice(MultiReplyTransport {
            last_method: None,
            last_params: None,
        });
        let args: Vec<String> = line.iter().map(|s| (*s).to_owned()).collect();
        let out = execute_with_office("agent-os", &args, &mut office);
        assert_eq!(out.exit_code, 0, "{line:?}: {}", out.stdout);
        assert_eq!(office.0.last_method.as_deref(), Some(expected_method));
    }
}
