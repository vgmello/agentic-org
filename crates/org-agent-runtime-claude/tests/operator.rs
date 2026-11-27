//! Claude Code's operator session (`OperatorSandboxed`, `TASK-80.1`): the read-only preset with
//! its Bash confined to `agent-os` by Claude's own rules and the office's approval bridge, the
//! sandbox on with no writable root, and an empty strict MCP config. A malformed operator spec
//! is refused before anything is planned.

use org_agent_runtime::testkit::{malformed_operator_specs, operator_ctx, operator_spec};
use org_agent_runtime::{AgentRuntime, LaunchPlan, PlanError, SessionSpec};
use org_agent_runtime_claude::ClaudeRuntime;
use serde_json::Value;

fn plan(spec: &SessionSpec) -> Result<LaunchPlan, PlanError> {
    ClaudeRuntime
        .plan_session(spec, &operator_ctx("/usr/local/bin/claude"))
        .map(|(plan, _)| plan)
}

fn file(plan: &LaunchPlan, name: &str) -> Value {
    let file = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == name)
        .unwrap_or_else(|| panic!("{name}"));
    serde_json::from_slice(&file.contents).unwrap()
}

fn flag_value<'a>(plan: &'a LaunchPlan, flag: &str) -> &'a str {
    let at = plan.args.iter().position(|a| a == flag).expect(flag);
    &plan.args[at + 1]
}

#[test]
fn the_operator_plan_is_read_only_with_a_confined_bash_and_no_mcp() {
    let plan = plan(&operator_spec("claude")).expect("an operator plan");
    assert!(plan.args.contains(&"--strict-mcp-config".to_owned()));
    let mcp = file(&plan, "claude-mcp.json");
    assert_eq!(mcp["mcpServers"], serde_json::json!({}), "{mcp}");
    assert_eq!(flag_value(&plan, "--permission-mode"), "plan");
    let settings = file(&plan, "claude-settings.json");
    let text = settings.to_string();
    // Claude may pre-allow `Bash(agent-os *)` or, when a deny it can't enforce exactly was
    // dropped, nothing at all (then every call reaches the office's approval bridge); never an
    // unscoped Bash, an edit or a wildcard.
    let allow = settings["permissions"]["allow"]
        .as_array()
        .expect("allow list");
    for rule in allow {
        let rule = rule.as_str().unwrap_or_default();
        assert!(
            rule == "Bash(agent-os *)"
                || rule.starts_with("Read")
                || rule.starts_with("Grep")
                || rule.starts_with("Glob"),
            "{rule} in {text}"
        );
    }
    assert_eq!(
        settings["permissions"]["disableBypassPermissionsMode"],
        "disable"
    );
    assert_eq!(
        settings["sandbox"]["allowUnsandboxedCommands"],
        Value::Bool(false)
    );
    // Nothing is writable but the run's own TMPDIR: never the home, never a repository.
    assert_eq!(
        settings["sandbox"]["filesystem"]["allowWrite"],
        serde_json::json!(["/private/tmp/agentic-org/tess-run"])
    );
    assert_eq!(
        settings["sandbox"]["network"]["allowUnixSockets"],
        serde_json::json!([org_agent_runtime::testkit::OPERATOR_SOCKET])
    );
    for arg in &plan.args {
        assert!(
            !arg.contains("bypassPermissions") && !arg.contains("dangerously"),
            "{arg}"
        );
    }
    // The sandbox is on, and nothing is writable but the run's TMPDIR.
    assert_eq!(settings["sandbox"]["enabled"], Value::Bool(true), "{text}");
}

#[test]
fn a_malformed_operator_spec_is_refused() {
    for (what, spec) in malformed_operator_specs("claude") {
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
fn an_operator_take_over_is_checked_the_same_way() {
    let mut ctx = operator_ctx("/usr/local/bin/claude");
    ctx.resume = Some(
        org_agent_runtime::SessionId::try_from("0193a5c0-0000-4000-8000-000000000081").unwrap(),
    );
    assert!(ClaudeRuntime
        .plan_take_over(&operator_spec("claude"), &ctx, 100, 30)
        .is_ok());
    for (what, spec) in malformed_operator_specs("claude") {
        assert!(
            matches!(
                ClaudeRuntime.plan_take_over(&spec, &ctx, 100, 30),
                Err(PlanError::InvalidSpec(_))
            ),
            "{what}"
        );
    }
}
