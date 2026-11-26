use super::classify::{classify_approval_kind, classify_escalation};
use org_agent_runtime::Escalation;
use serde_json::json;

#[test]
fn dangerously_disable_sandbox_fails_closed_on_any_non_false_value() {
    let kind = classify_approval_kind("Bash");
    for flag in [
        json!(true),
        json!("true"),
        json!("false"),
        json!(1),
        json!(0),
        json!({}),
        json!([]),
        json!(""),
    ] {
        let input = json!({ "command": "ls", "dangerouslyDisableSandbox": flag });
        assert_eq!(
            classify_escalation("Bash", kind, &input),
            Escalation::SandboxBypass,
            "{flag} must be treated as a sandbox bypass request"
        );
    }
    for input in [
        json!({ "command": "ls" }),
        json!({ "command": "ls", "dangerouslyDisableSandbox": null }),
        json!({ "command": "ls", "dangerouslyDisableSandbox": false }),
    ] {
        assert_eq!(
            classify_escalation("Bash", kind, &input),
            Escalation::None,
            "{input}"
        );
    }
    assert_eq!(
        classify_escalation("SandboxNetworkAccess", kind, &json!({})),
        Escalation::SandboxBypass
    );
}
