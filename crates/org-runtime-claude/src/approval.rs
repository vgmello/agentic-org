//! `can_use_tool` requests as [`ApprovalRequest`]s, and how they're answered.

use org_runtime::{ApprovalId, ApprovalKind, ApprovalRequest, Decision};
use serde_json::{json, Value};

/// What a deny tells the model when the office gave no message. Claude needs one: a deny
/// without `message` is "an invalid permission result" (spike, `approval-deny-invalid`).
pub(crate) const DEFAULT_DENY_MESSAGE: &str = "The office denied this action.";

/// Tools that switch Claude's permission mode when they run.
const MODE_SWITCH_TOOLS: &[&str] = &["ExitPlanMode", "EnterPlanMode"];

/// Whether an approval would widen what the session may do beyond the call itself.
///
/// The port's `ApprovalRequest` doesn't carry this yet (an `escalation` field is coming);
/// it's computed here, in one place, so the session can set it as soon as it exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // wired into ApprovalRequest once the port has the field
pub(crate) enum Escalation {
    /// An ordinary tool call.
    None,
    /// The call asks to run outside the sandbox (`dangerouslyDisableSandbox`).
    SandboxBypass,
    /// The call changes the session's permission mode (e.g. `ExitPlanMode`).
    PermissionChange,
    /// The request is too malformed to tell.
    Unknown,
}

/// The escalation of a `can_use_tool` request for `tool` with `input`.
#[allow(dead_code)] // wired into ApprovalRequest once the port has the field
pub(crate) fn escalation(tool: &str, input: &Value) -> Escalation {
    if tool.is_empty() || !input.is_object() {
        Escalation::Unknown
    } else if input.get("dangerouslyDisableSandbox") == Some(&Value::Bool(true)) {
        Escalation::SandboxBypass
    } else if MODE_SWITCH_TOOLS.contains(&tool) {
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
pub(crate) fn request(id: &str, request: &Value) -> Result<ApprovalRequest, &'static str> {
    let tool = request
        .get("tool_name")
        .and_then(Value::as_str)
        .ok_or("a can_use_tool request without a tool_name")?;
    let input = request
        .get("input")
        .cloned()
        .ok_or("a can_use_tool request without an input")?;
    let text = |key: &str| request.get(key).and_then(Value::as_str).map(str::to_owned);
    Ok(ApprovalRequest {
        id: ApprovalId::new(id),
        kind: kind(tool),
        tool: tool.to_owned(),
        display_name: text("display_name"),
        input,
        // The agent's own words about the call: untrusted.
        justification_untrusted: text("description"),
        blocked_path: text("blocked_path"),
        tool_use_id: text("tool_use_id"),
    })
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
        let cmd = json!({"command": "ls"});
        assert_eq!(escalation("Bash", &cmd), Escalation::None);
        assert_eq!(escalation("Edit", &json!({})), Escalation::None);
        assert_eq!(
            escalation(
                "Bash",
                &json!({"command": "ls", "dangerouslyDisableSandbox": true})
            ),
            Escalation::SandboxBypass
        );
        assert_eq!(
            escalation(
                "Bash",
                &json!({"command": "ls", "dangerouslyDisableSandbox": false})
            ),
            Escalation::None
        );
        assert_eq!(
            escalation("ExitPlanMode", &json!({"plan": "x"})),
            Escalation::PermissionChange
        );
        assert_eq!(
            escalation("EnterPlanMode", &json!({})),
            Escalation::PermissionChange
        );
        assert_eq!(escalation("", &cmd), Escalation::Unknown);
        assert_eq!(escalation("Bash", &json!("ls")), Escalation::Unknown);
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
