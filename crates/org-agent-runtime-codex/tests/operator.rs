//! Codex's operator session (`OperatorSandboxed`, `TASK-80.1`, `doc-97` §3.1): the Chief of
//! Staff gets a shell for `agent-os`, confined to a read-only, office-socket-only sandbox with
//! no writable root, no MCP server and an execpolicy that forbids every shell, interpreter and
//! wrapper; an ordinary `ReadOnly` agent keeps the shell off, as before.

use org_agent_runtime::testkit::{
    malformed_operator_specs, operator_ctx, operator_spec, OPERATOR_HOME, OPERATOR_SOCKET,
};
use org_agent_runtime::{
    AgentRuntime, FileLocation, LaunchPlan, PlanError, SessionId, SessionSpec,
};
use org_agent_runtime_codex::CodexRuntime;

const CODEX: &str = "/opt/homebrew/bin/codex";

fn plan(spec: &SessionSpec) -> Result<LaunchPlan, PlanError> {
    CodexRuntime
        .plan_session(spec, &operator_ctx(CODEX))
        .map(|(plan, _)| plan)
}

fn rules_of(plan: &LaunchPlan) -> String {
    let file = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == ".codex/rules/office-operator.rules")
        .expect("the execpolicy");
    assert_eq!(file.location, FileLocation::Workspace);
    String::from_utf8(file.contents.clone()).unwrap()
}

fn filesystem_arg(plan: &LaunchPlan) -> &str {
    plan.args
        .iter()
        .find(|a| a.starts_with("permissions.office.filesystem="))
        .expect("the office profile")
}

#[test]
fn the_operator_shell_is_on_and_confined() {
    let plan = plan(&operator_spec("codex")).expect("an operator plan");
    // The shell is on: the lockout is gone.
    assert!(
        !plan.args.iter().any(|a| a == "features.shell_tool=false"),
        "{:?}",
        plan.args
    );
    // Read-only: zero `write` grants anywhere, the workspace and TMPDIR included.
    let fs = filesystem_arg(&plan);
    assert!(!fs.contains("\"write\""), "{fs}");
    assert!(
        fs.contains(&format!("\"{OPERATOR_HOME}\"=\"read\"")),
        "{fs}"
    );
    assert!(
        fs.contains("\"/office/offices/*\"=\"none\"") || fs.contains("=\"none\""),
        "{fs}"
    );
    // Only the office socket, and no MCP server at all.
    let network = plan
        .args
        .iter()
        .find(|a| a.starts_with("permissions.office.network="))
        .expect("network");
    assert_eq!(
        network,
        &format!("permissions.office.network={{enabled=true,unix_sockets={{\"{OPERATOR_SOCKET}\"=\"allow\"}}}}")
    );
    assert!(plan.args.contains(&"mcp_servers={}".to_owned()));
    assert!(!plan.args.iter().any(|a| a.starts_with("mcp_servers.")));
    // Never a bypass or an unsandboxed mode.
    for arg in &plan.args {
        for banned in [
            "danger-full-access",
            "bypass",
            "never\"",
            "yolo",
            "workspace-write",
        ] {
            assert!(!arg.contains(banned), "{arg}");
        }
    }
    // The workspace is trusted so Codex loads the office's project layer, and nothing else
    // configures the session.
    assert!(plan.args.contains(&format!(
        "projects={{\"{OPERATOR_HOME}\"={{trust_level=\"trusted\"}}}}"
    )));
    let dirs: Vec<&str> = plan.dirs.iter().map(|d| d.rel_path.as_str()).collect();
    assert_eq!(dirs, [".codex", ".codex/rules"]);
    assert!(plan
        .dirs
        .iter()
        .all(|d| d.location == FileLocation::Workspace));
    let config = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == ".codex/config.toml")
        .expect("the empty project config");
    assert!(String::from_utf8_lossy(&config.contents)
        .lines()
        .all(|l| l.starts_with('#')));
}

#[test]
fn the_execpolicy_forbids_shells_and_never_allows_agent_os_out_of_the_sandbox() {
    let rules = rules_of(&plan(&operator_spec("codex")).unwrap());
    assert!(rules.contains("decision = \"forbidden\""), "{rules}");
    // An `allow` would run the command outside the sandbox: there is none.
    assert!(!rules.contains("\"allow\""), "{rules}");
    for program in [
        "\"bash\"",
        "\"sh\"",
        "\"env\"",
        "\"xargs\"",
        "\"python3\"",
        "\"curl\"",
        "\"rm\"",
        "\"git\"",
    ] {
        assert!(rules.contains(program), "{program} in {rules}");
    }
    // `agent-os` is in no pattern, only in the `not_match` examples Codex checks at load.
    let pattern = rules
        .lines()
        .find(|l| l.trim_start().starts_with("pattern"))
        .expect("a pattern");
    assert!(!pattern.contains("agent-os"), "{pattern}");
    assert!(
        rules.contains("not_match = [\"agent-os --help\"]"),
        "{rules}"
    );
    assert!(rules.contains("\"bash x\""), "{rules}");
}

#[test]
fn an_ordinary_read_only_agent_keeps_the_shell_off() {
    let read_only = SessionSpec {
        operator_shell_prefixes: Vec::new(),
        ..operator_spec("codex")
    };
    let plan = plan(&read_only).expect("a read-only plan");
    assert!(plan.args.contains(&"features.shell_tool=false".to_owned()));
    assert!(plan.dirs.is_empty());
    assert!(!plan
        .files
        .iter()
        .any(|f| f.location == FileLocation::Workspace));
    assert!(!plan.args.iter().any(|a| a.starts_with("projects=")));
}

#[test]
fn a_malformed_operator_spec_is_refused() {
    for (what, spec) in malformed_operator_specs("codex") {
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
fn an_operator_prefix_that_is_a_shell_or_not_a_plain_word_is_refused() {
    for prefix in [
        "bash",
        "env agent-os",
        "agent-os;rm",
        "agent-os $(id)",
        "agent-os|sh",
    ] {
        let Ok(prefix) = org_agent_runtime::CommandPrefix::try_from(prefix) else {
            continue;
        };
        let mut spec = operator_spec("codex");
        spec.operator_shell_prefixes = vec![prefix.clone()];
        spec.policy.allow[2].scope = org_agent_runtime::RuleScope::CommandPrefix(prefix.clone());
        let refused = plan(&spec);
        assert!(
            matches!(refused, Err(PlanError::InvalidSpec(_))),
            "{prefix:?}: {:?}",
            refused.map(|p| p.args)
        );
    }
}

#[test]
fn a_take_over_of_an_operator_session_keeps_its_confinement() {
    let mut ctx = operator_ctx(CODEX);
    ctx.resume = Some(SessionId::try_from("thr_tess").unwrap());
    let plan = CodexRuntime
        .plan_take_over(&operator_spec("codex"), &ctx, 120, 40)
        .expect("a take-over plan");
    assert!(!plan.args.iter().any(|a| a == "features.shell_tool=false"));
    assert!(!filesystem_arg(&plan).contains("\"write\""));
    assert!(rules_of(&plan).contains("decision = \"forbidden\""));
}
