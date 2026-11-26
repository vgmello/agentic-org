//! Antigravity's operator session (`OperatorSandboxed`, `TASK-80.1`): planned like the read-only
//! preset (its shell confined to `agent-os` by the agent file's rules and the office's approval
//! bridge, no MCP server, never `yolo`), and refused when malformed.

use org_agent_runtime::testkit::{malformed_operator_specs, operator_ctx, operator_spec};
use org_agent_runtime::{AgentRuntime, LaunchPlan, PlanError, SessionSpec};
use org_agent_runtime_agy::AgyRuntime;

fn plan(spec: &SessionSpec) -> Result<LaunchPlan, PlanError> {
    AgyRuntime
        .plan_session(spec, &operator_ctx("/usr/local/bin/agy"))
        .map(|(plan, _)| plan)
}

#[test]
fn the_operator_plan_is_planned_and_never_bypasses() {
    let plan = plan(&operator_spec("agy")).expect("an operator plan");
    let mut everything = plan.args.join(" ");
    for file in &plan.files {
        everything.push_str(&String::from_utf8_lossy(&file.contents));
    }
    for banned in ["yolo", "bypass", "dangerously", "mcpServers\":{\""] {
        assert!(!everything.contains(banned), "{banned} in {everything}");
    }
    // Antigravity's own sandbox is on.
    assert!(plan.args.contains(&"--sandbox".to_owned()), "{everything}");
}

#[test]
fn a_malformed_operator_spec_is_refused() {
    for (what, spec) in malformed_operator_specs("agy") {
        match plan(&spec) {
            Err(PlanError::InvalidSpec(reason)) => {
                assert!(reason.contains("operator session"), "{what}: {reason}");
            }
            other => panic!(
                "{what}: expected a refusal, got {:?}",
                other.map(|p| p.args)
            ),
        }
    }
}

#[test]
fn an_ordinary_read_only_spec_still_plans() {
    let spec = SessionSpec {
        operator_shell_prefixes: Vec::new(),
        ..operator_spec("agy")
    };
    assert!(plan(&spec).is_ok());
}
