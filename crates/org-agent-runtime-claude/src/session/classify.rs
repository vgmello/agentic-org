//! Pure readers of Claude's JSON: agent, plugin and MCP lists, permission modes, tool and
//! approval classification, usage and rate-limit status.

use serde_json::Value;

use org_agent_runtime::{ApprovalKind, Escalation, LimitStatus, ToolCapability};

use crate::plan::ExpectedMode;

use super::MessageUsage;

pub(super) fn extract_agent_names(val: Option<&Value>) -> Vec<String> {
    let Some(arr) = val.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            item.as_str().map(ToOwned::to_owned).or_else(|| {
                item.get("name")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
        })
        .collect()
}

pub(super) fn agent_matches(
    val: Option<&Value>,
    expected_name: &str,
    expected_description: &str,
) -> bool {
    let Some(arr) = val.and_then(Value::as_array) else {
        return false;
    };
    arr.iter().any(|item| {
        if let Some(name) = item.as_str() {
            return name == expected_name;
        }
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            return false;
        };
        if name != expected_name {
            return false;
        }
        match item.get("description").and_then(Value::as_str) {
            Some(desc) => desc == expected_description,
            None => true,
        }
    })
}

pub(super) fn is_builtin_plugin(val: &Value) -> bool {
    let Some(obj) = val.as_object() else {
        return false;
    };
    let source = obj.get("source").and_then(Value::as_str);
    let path = obj.get("path").and_then(Value::as_str);
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty());
    let builtin_2_1_323 = path == Some("builtin")
        && name.is_some()
        && source.and_then(|s| s.strip_suffix("@builtin")) == name;
    obj.get("builtin").and_then(Value::as_bool) == Some(true)
        || obj.get("isBuiltin").and_then(Value::as_bool) == Some(true)
        || source == Some("builtin")
        // Claude Code 2.1.323+ emits `{"name": "<id>", "path": "builtin", "source": "<id>@builtin"}`.
        || builtin_2_1_323
}

pub(super) fn plugin_display_name(val: &Value) -> String {
    val.as_str()
        .or_else(|| val.get("name").and_then(Value::as_str))
        .unwrap_or("<unnamed>")
        .to_owned()
}

pub(super) fn mcp_server_name(val: &Value) -> Option<String> {
    val.as_str()
        .or_else(|| val.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

pub(super) fn remote_control_active(body: &Value) -> bool {
    [
        "remote_control_active",
        "remote_control_enabled",
        "remote_control_auto_enable",
        "remote_control_auto_connect_default",
        "remote_control_auto_on_by_default",
    ]
    .iter()
    .any(|key| body.get(key).and_then(Value::as_bool) == Some(true))
}

pub(super) fn permission_mode_matches(expected: &ExpectedMode, reported: &str) -> bool {
    if reported.is_empty() {
        return false;
    }
    reported == expected.reported_name || reported == expected.cli_flag
}

pub(super) fn classify_tool_capability(tool_name: &str) -> Option<ToolCapability> {
    match tool_name {
        "Read" => Some(ToolCapability::Read),
        "Glob" | "Grep" | "LS" => Some(ToolCapability::Search),
        // `NotebookEdit` stays excluded from `ToolCapability::Edit` so it remains in
        // `RISKY_BUILTIN_TOOLS` and never auto-allows via `cap:edit`.
        "Edit" | "Write" | "MultiEdit" => Some(ToolCapability::Edit),
        "Bash" => Some(ToolCapability::Shell),
        "Task" | "Agent" => Some(ToolCapability::Subagent),
        _ => None,
    }
}

pub(super) fn classify_approval_kind(tool_name: &str) -> ApprovalKind {
    match tool_name {
        "Bash" => ApprovalKind::Command,
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => ApprovalKind::FileChange,
        "ExitPlanMode" | "EnterPlanMode" => ApprovalKind::Permissions,
        "Read"
        | "Glob"
        | "Grep"
        | "LS"
        | "Agent"
        | "Task"
        | "TaskOutput"
        | "TaskStop"
        | "SandboxNetworkAccess" => ApprovalKind::Other,
        other if other.starts_with("mcp__") => ApprovalKind::Other,
        _ => ApprovalKind::Unknown,
    }
}

pub(super) fn classify_escalation(
    tool_name: &str,
    kind: ApprovalKind,
    input: &Value,
) -> Escalation {
    if tool_name == "SandboxNetworkAccess"
        || input
            .get("dangerouslyDisableSandbox")
            .and_then(Value::as_bool)
            == Some(true)
    {
        return Escalation::SandboxBypass;
    }
    if kind == ApprovalKind::Permissions || matches!(tool_name, "ExitPlanMode" | "EnterPlanMode") {
        return Escalation::PermissionChange;
    }
    if kind == ApprovalKind::Unknown {
        return Escalation::Unknown;
    }
    Escalation::None
}

pub(super) fn parse_claude_usage(val: &Value) -> MessageUsage {
    let reasoning = val
        .get("output_tokens_details")
        .and_then(|d| d.get("thinking_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    MessageUsage {
        input: val.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
        output: val
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read: val
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_write: val
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        reasoning,
    }
}

pub(super) fn parse_limit_status(raw: Option<&str>) -> LimitStatus {
    match raw {
        Some("allowed") => LimitStatus::Ok,
        Some("allowed_warning" | "warning") => LimitStatus::Warning,
        Some("rejected" | "rate_limited") => LimitStatus::Rejected,
        _ => LimitStatus::Unknown,
    }
}
