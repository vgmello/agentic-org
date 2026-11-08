//! `can_use_tool` requests as [`ApprovalRequest`]s, and how they're answered.

use org_runtime::{ApprovalId, ApprovalKind, ApprovalRequest, Decision, Escalation};
use serde_json::{json, Value};

/// What a deny tells the model when the office gave no message. Claude needs one: a deny
/// without `message` is "an invalid permission result" (spike, `approval-deny-invalid`).
pub(crate) const DEFAULT_DENY_MESSAGE: &str = "The office denied this action.";

/// Tools that switch Claude's permission mode when they run.
const MODE_SWITCH_TOOLS: &[&str] = &["ExitPlanMode", "EnterPlanMode"];

/// The file tools, which run in the unsandboxed CLI, and the input key naming the file.
const FILE_WRITE_TOOLS: &[(&str, &str)] = &[
    ("Edit", "file_path"),
    ("Write", "file_path"),
    ("MultiEdit", "file_path"),
    ("NotebookEdit", "notebook_path"),
];

/// Whether allowing a `can_use_tool` request for `tool` with `input` would widen the session
/// beyond its spec:
/// - running outside the sandbox: a `dangerouslyDisableSandbox` that isn't `false`, or, while
///   the writable roots are enforced (`writable` is `Some`), a file tool writing anywhere but
///   inside them (Claude's file tools run in the unsandboxed CLI, so only this keeps them to
///   the roots; a path that isn't a plain absolute one counts as outside);
/// - switching the permission mode (`ExitPlanMode`, `EnterPlanMode`);
/// - a request too malformed to tell.
pub(crate) fn escalation(tool: &str, input: &Value, writable: Option<&[String]>) -> Escalation {
    if tool.is_empty() || !input.is_object() {
        return Escalation::Unknown;
    }
    if input
        .get("dangerouslyDisableSandbox")
        .is_some_and(|v| *v != Value::Bool(false))
    {
        return Escalation::SandboxBypass;
    }
    if let (Some(roots), Some((_, key))) =
        (writable, FILE_WRITE_TOOLS.iter().find(|(t, _)| *t == tool))
    {
        let inside = input
            .get(*key)
            .and_then(Value::as_str)
            .and_then(|p| org_runtime::SpecPath::try_from(p).ok())
            .is_some_and(|p| roots.iter().any(|r| crate::rules::within(p.as_str(), r)));
        if !inside {
            return Escalation::SandboxBypass;
        }
    }
    if MODE_SWITCH_TOOLS.contains(&tool) {
        Escalation::PermissionChange
    } else {
        Escalation::None
    }
}

/// What kind of action a tool call is, from its tool name.
pub(crate) fn kind(tool: &str) -> ApprovalKind {
    match tool {
        "Bash" => ApprovalKind::Command,
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => ApprovalKind::FileChange,
        t if MODE_SWITCH_TOOLS.contains(&t) => ApprovalKind::Permissions,
        _ => ApprovalKind::Other,
    }
}

/// A `can_use_tool` request (the `request` object) as an approval, or why it can't be one.
/// `writable` is the writable roots while they're enforced (see [`escalation`]).
pub(crate) fn request(
    id: &str,
    request: &Value,
    writable: Option<&[String]>,
) -> Result<ApprovalRequest, &'static str> {
    let tool = request
        .get("tool_name")
        .and_then(Value::as_str)
        .ok_or("a can_use_tool request without a tool_name")?;
    let input = request
        .get("input")
        .cloned()
        .ok_or("a can_use_tool request without an input")?;
    let text = |key: &str| request.get(key).and_then(Value::as_str).map(str::to_owned);
    let escalation = escalation(tool, &input, writable);
    let mut approval =
        ApprovalRequest::new(ApprovalId::new(id), kind(tool), escalation, tool, input);
    approval.display_name = text("display_name");
    // The agent's own words about the call: untrusted.
    approval.justification_untrusted = text("description");
    approval.blocked_path = text("blocked_path");
    approval.tool_use_id = text("tool_use_id");
    Ok(approval)
}

/// The `control_response` answering request `id` (as Claude sent it: a string or a number).
/// Never carries `updatedInput`: the call runs exactly as Claude asked for it, or not at all.
pub(crate) fn answer(id: &Value, decision: &Decision) -> Option<Value> {
    let response = match decision {
        Decision::AllowOnce => json!({"behavior": "allow"}),
        Decision::Deny { message, stop_turn } => {
            let message = message.as_deref().unwrap_or(DEFAULT_DENY_MESSAGE);
            if *stop_turn {
                json!({"behavior": "deny", "message": message, "interrupt": true})
            } else {
                json!({"behavior": "deny", "message": message})
            }
        }
        _ => return None,
    };
    Some(json!({
        "type": "control_response",
        "response": {"subtype": "success", "request_id": id, "response": response},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escalations() {
        let e = |tool: &str, input: Value| escalation(tool, &input, None);
        let cmd = json!({"command": "ls"});
        assert_eq!(e("Bash", cmd.clone()), Escalation::None);
        assert_eq!(e("Edit", json!({})), Escalation::None);
        for bypass in [json!(true), json!("true"), json!(1)] {
            assert_eq!(
                e(
                    "Bash",
                    json!({"command": "ls", "dangerouslyDisableSandbox": bypass})
                ),
                Escalation::SandboxBypass
            );
        }
        assert_eq!(
            e(
                "Bash",
                json!({"command": "ls", "dangerouslyDisableSandbox": false})
            ),
            Escalation::None
        );
        assert_eq!(
            e("ExitPlanMode", json!({"plan": "x"})),
            Escalation::PermissionChange
        );
        assert_eq!(e("EnterPlanMode", json!({})), Escalation::PermissionChange);
        assert_eq!(e("", cmd.clone()), Escalation::Unknown);
        assert_eq!(e("Bash", json!("ls")), Escalation::Unknown);
    }

    #[test]
    fn file_writes_outside_the_roots_are_a_sandbox_bypass() {
        let roots = vec!["/work/t1".to_owned(), "/tmp/s1".to_owned()];
        let e = |tool: &str, input: Value| escalation(tool, &input, Some(&roots));
        for (tool, key) in FILE_WRITE_TOOLS {
            assert_eq!(
                e(tool, json!({*key: "/work/t1/src/a.rs"})),
                Escalation::None,
                "{tool}"
            );
            assert_eq!(
                e(tool, json!({*key: "/tmp/s1/x"})),
                Escalation::None,
                "{tool}"
            );
            for outside in [
                json!("/work/t2/a.rs"),
                json!("/work/t1/../t2/a.rs"),
                json!("/work/t1x/a"),
                json!("src/a.rs"),
                json!("/etc/hosts"),
                json!(42),
            ] {
                assert_eq!(
                    e(tool, json!({*key: outside.clone()})),
                    Escalation::SandboxBypass,
                    "{tool} {outside}"
                );
            }
            // No path at all: fails closed.
            assert_eq!(e(tool, json!({})), Escalation::SandboxBypass, "{tool}");
        }
        // Reads and commands aren't file writes; and with the sandbox off nothing is held.
        assert_eq!(
            e("Read", json!({"file_path": "/etc/hosts"})),
            Escalation::None
        );
        assert_eq!(e("Bash", json!({"command": "ls /etc"})), Escalation::None);
        assert_eq!(
            escalation("Edit", &json!({"file_path": "/etc/hosts"}), None),
            Escalation::None
        );
    }

    #[test]
    fn kinds() {
        assert_eq!(kind("Bash"), ApprovalKind::Command);
        assert_eq!(kind("Write"), ApprovalKind::FileChange);
        assert_eq!(kind("NotebookEdit"), ApprovalKind::FileChange);
        assert_eq!(kind("mcp__office__send"), ApprovalKind::Other);
        assert_eq!(kind("WebFetch"), ApprovalKind::Other);
        assert_eq!(kind("ExitPlanMode"), ApprovalKind::Permissions);
    }

    #[test]
    fn answers_never_rewrite_input() {
        let allow = answer(&json!("r1"), &Decision::AllowOnce).unwrap();
        assert_eq!(
            allow,
            json!({"type": "control_response", "response": {
                "subtype": "success", "request_id": "r1", "response": {"behavior": "allow"}}})
        );
        let deny = answer(
            &json!(7),
            &Decision::Deny {
                message: None,
                stop_turn: false,
            },
        )
        .unwrap();
        assert_eq!(
            deny["response"]["response"],
            json!({"behavior": "deny", "message": DEFAULT_DENY_MESSAGE})
        );
        // A numeric id goes back as the number Claude sent.
        assert_eq!(deny["response"]["request_id"], json!(7));
        for v in [allow, deny] {
            assert!(!v.to_string().contains("updatedInput"));
        }
    }
}
