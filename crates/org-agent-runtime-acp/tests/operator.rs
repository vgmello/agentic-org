//! Operator sessions (`OperatorSandboxed`, `TASK-80.1`) across every ACP vendor adapter
//! (`agy`, `gemini`, `kimi`, `pi`): planned safely without bypass flags, and refused fail-closed
//! whenever malformed.

use org_agent_runtime::testkit::{malformed_operator_specs, operator_ctx, operator_spec};
use org_agent_runtime::{AgentRuntime, LaunchPlan, PlanError, SessionSpec};
use org_agent_runtime_acp::{AgyRuntime, GeminiRuntime, KimiRuntime, PiRuntime};

fn all_acp_runtimes() -> [(&'static dyn AgentRuntime, &'static str, &'static str, bool); 4] {
    [
        (&AgyRuntime, "agy", "/usr/local/bin/agy", true),
        (&GeminiRuntime, "gemini", "/usr/local/bin/gemini", true),
        (&KimiRuntime, "kimi", "/usr/local/bin/kimi", false),
        (&PiRuntime, "pi", "/usr/local/bin/pi-acp", false),
    ]
}

fn plan_for(
    runtime: &dyn AgentRuntime,
    bin: &str,
    spec: &SessionSpec,
) -> Result<LaunchPlan, PlanError> {
    runtime
        .plan_session(spec, &operator_ctx(bin))
        .map(|(plan, _)| plan)
}

#[test]
fn operator_plans_never_bypass_across_all_acp_runtimes() {
    for (runtime, id, bin, expects_sandbox_flag) in all_acp_runtimes() {
        let plan = plan_for(runtime, bin, &operator_spec(id))
            .unwrap_or_else(|e| panic!("{id}: operator plan failed: {e}"));
        let mut everything = plan.args.join(" ");
        for file in &plan.files {
            everything.push_str(&String::from_utf8_lossy(&file.contents));
        }
        for banned in ["yolo", "bypass", "dangerously", "mcpServers\":{\""] {
            assert!(
                !everything.contains(banned),
                "{id}: {banned} in {everything}"
            );
        }
        if expects_sandbox_flag {
            assert!(
                plan.args.contains(&"--sandbox".to_owned()),
                "{id}: missing --sandbox in {everything}"
            );
        }
    }
}

#[test]
fn malformed_operator_specs_are_refused_across_all_acp_runtimes() {
    for (runtime, id, bin, _) in all_acp_runtimes() {
        for (what, spec) in malformed_operator_specs(id) {
            match plan_for(runtime, bin, &spec) {
                Err(PlanError::InvalidSpec(reason)) => {
                    assert!(reason.contains("operator session"), "{id}/{what}: {reason}");
                }
                other => panic!(
                    "{id}/{what}: expected a refusal, got {:?}",
                    other.map(|p| p.args)
                ),
            }
        }
    }
}

#[test]
fn ordinary_read_only_specs_plan_across_all_acp_runtimes() {
    for (runtime, id, bin, _) in all_acp_runtimes() {
        let spec = SessionSpec {
            operator_shell_prefixes: Vec::new(),
            ..operator_spec(id)
        };
        assert!(plan_for(runtime, bin, &spec).is_ok(), "{id}");
    }
}
