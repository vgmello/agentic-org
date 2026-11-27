//! Fail-closed permission mode checks, tool classification, sandbox-bypass detection, and errors.

use serde_json::Value;

use org_agent_runtime::{
    ApprovalId, ApprovalKind, ApprovalRequest, Escalation, LimitKind, NoticeKind, Output,
    SessionEvent, ToolCapability, TurnOutcome,
};

use super::{first_str, first_u64, AcpUsage};

fn normalize_identifier(raw: &str) -> String {
    raw.chars()
        .filter(|&c| c != '_' && c != '-' && !c.is_ascii_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Returns `true` for ACP permission modes that bypass office approval enforcement.
pub(crate) fn is_banned_mode(mode: &str) -> bool {
    matches!(
        normalize_identifier(mode).as_str(),
        "yolo"
            | "bypasspermissions"
            | "dangerfullaccess"
            | "auto"
            | "fullauto"
            | "autoapprove"
            | "dontask"
            | "allowall"
            | "unrestricted"
    )
}

pub(crate) fn classify_tool_capability(tool: &str) -> Option<ToolCapability> {
    match tool.to_ascii_lowercase().as_str() {
        "read_file" | "view_file" | "read" | "read_many_files" | "readfile" => {
            Some(ToolCapability::Read)
        }
        "list_dir"
        | "list_directory"
        | "find_by_name"
        | "grep_search"
        | "search_file_content"
        | "glob"
        | "grep"
        | "ls"
        | "find" => Some(ToolCapability::Search),
        "write_file"
        | "edit_file"
        | "write"
        | "edit"
        | "delete_file"
        | "replace_file_content"
        | "multi_replace_file_content"
        | "replace"
        | "writefile"
        | "strreplacefile" => Some(ToolCapability::Edit),
        "run_command" | "run_shell_command" | "bash" | "shell" => Some(ToolCapability::Shell),
        "invoke_subagent" | "spawn_subagent" | "subagent" => Some(ToolCapability::Subagent),
        _ => None,
    }
}

pub(crate) fn classify_tool_kind(tool: &str) -> ApprovalKind {
    match tool.to_ascii_lowercase().as_str() {
        "run_command" | "run_shell_command" | "bash" | "shell" => ApprovalKind::Command,
        "write_file"
        | "edit_file"
        | "write"
        | "edit"
        | "delete_file"
        | "replace_file_content"
        | "multi_replace_file_content"
        | "replace"
        | "writefile"
        | "strreplacefile" => ApprovalKind::FileChange,
        "set_permission_mode"
        | "exitplanmode"
        | "enterplanmode"
        | "exit_plan_mode"
        | "enter_plan_mode" => ApprovalKind::Permissions,
        "read_file"
        | "view_file"
        | "read_many_files"
        | "readfile"
        | "list_dir"
        | "list_directory"
        | "find_by_name"
        | "grep_search"
        | "search_file_content"
        | "glob"
        | "grep"
        | "read"
        | "ls"
        | "find"
        | "webfetch"
        | "websearch"
        | "invoke_subagent"
        | "spawn_subagent"
        | "subagent"
        | "sandboxnetworkaccess" => ApprovalKind::Other,
        other if other.starts_with("mcp_") => ApprovalKind::Other,
        _ => ApprovalKind::Unknown,
    }
}

fn classify_acp_kind_str(kind: Option<&str>) -> Option<ToolCapability> {
    match kind.map(str::to_ascii_lowercase).as_deref() {
        Some("read") => Some(ToolCapability::Read),
        Some("search") => Some(ToolCapability::Search),
        Some("edit" | "delete" | "move") => Some(ToolCapability::Edit),
        Some("execute") => Some(ToolCapability::Shell),
        Some("subagent") => Some(ToolCapability::Subagent),
        _ => None,
    }
}

fn classify_acp_kind_approval(kind: Option<&str>) -> ApprovalKind {
    match kind.map(str::to_ascii_lowercase).as_deref() {
        Some("execute") => ApprovalKind::Command,
        Some("edit" | "delete" | "move") => ApprovalKind::FileChange,
        Some("switch_mode") => ApprovalKind::Permissions,
        Some("read" | "search") => ApprovalKind::Other,
        _ => ApprovalKind::Unknown,
    }
}

/// Classifies an ACP tool call's [`ApprovalKind`] and [`ToolCapability`].
///
/// Only falls back to ACP `kind` when `explicit_name` is absent or classifies as
/// [`ApprovalKind::Unknown`]; never classifies from display `title`.
pub(crate) fn resolve_tool_classification(
    explicit_name: Option<&str>,
    acp_kind: Option<&str>,
) -> (ApprovalKind, Option<ToolCapability>) {
    match explicit_name {
        Some(name) => {
            let name_kind = classify_tool_kind(name);
            if name_kind == ApprovalKind::Unknown {
                (
                    classify_acp_kind_approval(acp_kind),
                    classify_tool_capability(name).or_else(|| classify_acp_kind_str(acp_kind)),
                )
            } else {
                (name_kind, classify_tool_capability(name))
            }
        }
        None => (
            classify_acp_kind_approval(acp_kind),
            classify_acp_kind_str(acp_kind),
        ),
    }
}

fn is_bypass_flag_triggered(val: Option<&Value>) -> bool {
    match val {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        // Fail closed on malformed/non-boolean bypass fields.
        Some(_) => true,
    }
}

fn is_sandbox_disabled_value(val: &Value) -> bool {
    match val {
        Value::Bool(false) => true,
        Value::Bool(true) | Value::Null => false,
        Value::Number(n) => n.as_u64() == Some(0) || n.as_i64() == Some(0),
        Value::String(s) => matches!(
            normalize_identifier(s).as_str(),
            "false"
                | "off"
                | "none"
                | "disabled"
                | "disable"
                | "no"
                | "0"
                | "bypass"
                | "dangerfullaccess"
                | "unsandboxed"
        ),
        Value::Object(map) => map.get("enabled").is_some_and(is_sandbox_disabled_value),
        // Fail closed on unexpected non-object/non-bool values.
        Value::Array(_) => true,
    }
}

enum SandboxBypassKey {
    /// Positive-polarity bypass flag (`BypassSandbox: true`, `yolo: true`).
    BypassFlag,
    /// Negative-polarity sandbox toggle (`sandbox: false`, `useSandbox: false`, `sandboxed: false`).
    SandboxToggle,
}

fn classify_sandbox_bypass_key(key: &str) -> Option<SandboxBypassKey> {
    match normalize_identifier(key).as_str() {
        "bypasssandbox"
        | "dangerouslydisablesandbox"
        | "unsandboxed"
        | "nosandbox"
        | "disablesandbox"
        | "yolo" => Some(SandboxBypassKey::BypassFlag),
        "sandbox" | "usesandbox" | "sandboxed" => Some(SandboxBypassKey::SandboxToggle),
        _ => None,
    }
}

fn input_requests_sandbox_bypass(input: &Value) -> bool {
    let mut stack = vec![input];
    while let Some(node) = stack.pop() {
        match node {
            Value::Object(map) => {
                for (key, val) in map {
                    let triggered = match classify_sandbox_bypass_key(key) {
                        Some(SandboxBypassKey::BypassFlag) => is_bypass_flag_triggered(Some(val)),
                        Some(SandboxBypassKey::SandboxToggle) => is_sandbox_disabled_value(val),
                        None => false,
                    };
                    if triggered {
                        return true;
                    }
                    if matches!(val, Value::Object(_) | Value::Array(_)) {
                        stack.push(val);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    if matches!(item, Value::Object(_) | Value::Array(_)) {
                        stack.push(item);
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn classify_escalation(tool: &str, kind: ApprovalKind, input: &Value) -> Escalation {
    if tool.eq_ignore_ascii_case("SandboxNetworkAccess") || input_requests_sandbox_bypass(input) {
        return Escalation::SandboxBypass;
    }
    if kind == ApprovalKind::Permissions || tool.eq_ignore_ascii_case("set_permission_mode") {
        return Escalation::PermissionChange;
    }
    if kind == ApprovalKind::Unknown {
        return Escalation::Unknown;
    }
    Escalation::None
}

/// Extracted ACP `session/request_permission` payload plus selected one-shot allow/deny option IDs.
pub(crate) struct ParsedPermission {
    pub(crate) request: ApprovalRequest,
    pub(crate) allow_option_id: Option<String>,
    pub(crate) deny_option_id: Option<String>,
    pub(crate) tool_call_id: Option<String>,
}

pub(crate) fn parse_permission_request(
    approval_id: ApprovalId,
    params: &Value,
) -> ParsedPermission {
    let tc = params.get("toolCall").unwrap_or(&Value::Null);
    let explicit_name = tc
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty());
    let tool = explicit_name
        .or_else(|| {
            tc.get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or("unknown");
    let input = tc
        .get("rawInput")
        .filter(|v| !v.is_null())
        .or_else(|| tc.get("args").filter(|v| !v.is_null()))
        .cloned()
        .unwrap_or(Value::Null);
    let (kind, capability) =
        resolve_tool_classification(explicit_name, tc.get("kind").and_then(Value::as_str));
    let escalation = classify_escalation(tool, kind, &input);
    let mut request = ApprovalRequest::new(approval_id, kind, escalation, tool, input.clone());
    request.capability = capability;
    request.display_name = tc
        .get("title")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    request.blocked_path = first_str(&[
        input.get("path"),
        input.get("file_path"),
        input.get("Path"),
        input.get("TargetFile"),
        input.get("AbsolutePath"),
        tc.pointer("/locations/0/path"),
    ])
    .map(ToOwned::to_owned);
    let tool_call_id = tc
        .get("toolCallId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    request.tool_use_id = tool_call_id.clone();

    let (allow_option_id, deny_option_id) = extract_permission_options(params.get("options"));
    ParsedPermission {
        request,
        allow_option_id,
        deny_option_id,
        tool_call_id,
    }
}

/// Selects one-shot allow/deny option IDs, strictly preferring `allow_once` / `reject_once`
/// and NEVER falling back to `allow_always` / `reject_always` (`Decision::AllowOnce`).
/// Defaults to `("allow", "deny")` only when `options` is absent or empty (`[]`).
fn extract_permission_options(options: Option<&Value>) -> (Option<String>, Option<String>) {
    let Some(arr) = options.and_then(Value::as_array).filter(|a| !a.is_empty()) else {
        return (Some("allow".to_owned()), Some("deny".to_owned()));
    };
    let mut exact_allow = None;
    let mut exact_deny = None;
    let mut bare_allow = None;
    let mut bare_deny = None;
    for opt in arr {
        let Some(opt_id) = opt.get("optionId").and_then(Value::as_str) else {
            continue;
        };
        let kind = opt.get("kind").and_then(Value::as_str).unwrap_or("");
        if exact_allow.is_none() && kind == "allow_once" {
            exact_allow = Some(opt_id.to_owned());
        } else if bare_allow.is_none() && opt_id == "allow" && kind.is_empty() {
            bare_allow = Some(opt_id.to_owned());
        }
        if exact_deny.is_none() && kind == "reject_once" {
            exact_deny = Some(opt_id.to_owned());
        } else if bare_deny.is_none() && opt_id == "deny" && kind.is_empty() {
            bare_deny = Some(opt_id.to_owned());
        }
    }
    (exact_allow.or(bare_allow), exact_deny.or(bare_deny))
}

pub(crate) fn parse_acp_usage(val: &Value) -> AcpUsage {
    AcpUsage {
        input: first_u64(&[val.get("inputTokens"), val.get("input_tokens")]),
        output: first_u64(&[val.get("outputTokens"), val.get("output_tokens")]),
        cache_read: first_u64(&[val.get("cachedReadTokens"), val.get("cache_read_tokens")]),
        cache_write: first_u64(&[val.get("cachedWriteTokens"), val.get("cache_write_tokens")]),
        reasoning: first_u64(&[val.get("thoughtTokens"), val.get("reasoning_tokens")]),
    }
}

pub(crate) fn classify_stop_reason(reason: Option<&str>) -> TurnOutcome {
    match reason.unwrap_or("end_turn") {
        "end_turn" | "stop_sequence" => TurnOutcome::Completed,
        "cancelled" => TurnOutcome::Interrupted,
        "max_tokens" => TurnOutcome::Limit {
            kind: LimitKind::MaxTokens,
        },
        "max_turn_requests" | "max_turns" => TurnOutcome::Limit {
            kind: LimitKind::MaxTurns,
        },
        "timeout" => TurnOutcome::Limit {
            kind: LimitKind::Timeout,
        },
        "refusal" | "refused" => TurnOutcome::Refused,
        _ => TurnOutcome::Unknown,
    }
}

pub(crate) fn extract_rpc_error_message(err: &Value) -> String {
    let msg = err
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("ACP JSON-RPC error");
    let data = err
        .get("data")
        .and_then(|d| {
            d.as_str()
                .or_else(|| d.get("details").and_then(Value::as_str))
        })
        .filter(|s| !s.is_empty());
    match data {
        Some(detail) => format!("{msg}: {detail}"),
        None => msg.to_owned(),
    }
}

pub(crate) fn is_auth_rpc_error(err: &Value) -> bool {
    let code = err.get("code").and_then(Value::as_i64);
    let lower = extract_rpc_error_message(err).to_ascii_lowercase();
    code == Some(-32000)
        || lower.contains("authentication required")
        || lower.contains("not authenticated")
        || lower.contains("unauthenticated")
        || lower.contains("unauthorized")
        || lower.contains("not logged in")
        || lower.contains("login required")
        || lower.contains("invalid api key")
        || lower.contains("missing api key")
        || lower.contains("please sign in")
}

pub(crate) fn classify_rpc_error_outcome(err: &Value) -> (TurnOutcome, Vec<String>) {
    let msg = extract_rpc_error_message(err);
    let lower = msg.to_ascii_lowercase();
    let outcome = if is_auth_rpc_error(err) {
        TurnOutcome::SignedOut
    } else if lower.contains("quota") || lower.contains("rate limit") || lower.contains("exhausted")
    {
        TurnOutcome::Quota
    } else if lower.contains("cancelled") || lower.contains("canceled") {
        TurnOutcome::Interrupted
    } else {
        TurnOutcome::Failed {
            reason: msg.clone(),
        }
    };
    (outcome, vec![msg])
}

pub(crate) fn parse_stderr_line(
    display_name: &str,
    structured_prefix: Option<&str>,
    text: &str,
) -> Option<Output> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(prefix) = structured_prefix {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            if let Ok(val) = serde_json::from_str::<Value>(rest.trim()) {
                let code = val.get("code").and_then(Value::as_str).unwrap_or("error");
                let fallback = format!("{display_name} structured error");
                let msg = val
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or(&fallback);
                let kind = if code.eq_ignore_ascii_case("retry") {
                    NoticeKind::Retry
                } else {
                    NoticeKind::Warning
                };
                return Some(Output::Event(SessionEvent::Notice {
                    kind,
                    text: format!("{code}: {msg}"),
                }));
            }
        }
    }
    Some(Output::Event(SessionEvent::Notice {
        kind: NoticeKind::Warning,
        text: trimmed.to_owned(),
    }))
}
