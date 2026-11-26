use serde_json::{json, Value};

use org_agent_runtime::{
    encode_line, ApprovalId, ApprovalKind, ApprovalRequest, Escalation, LimitKind, NoticeKind,
    Output, SessionEvent, ToolCapability, TurnOutcome,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct AcpUsage {
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) cache_read: u64,
    pub(crate) cache_write: u64,
    pub(crate) reasoning: u64,
}

impl AcpUsage {
    pub(crate) fn add_assign(&mut self, other: &AcpUsage) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
        self.reasoning = self.reasoning.saturating_add(other.reasoning);
    }

    pub(crate) fn merge_max(&mut self, other: &AcpUsage) {
        self.input = self.input.max(other.input);
        self.output = self.output.max(other.output);
        self.cache_read = self.cache_read.max(other.cache_read);
        self.cache_write = self.cache_write.max(other.cache_write);
        self.reasoning = self.reasoning.max(other.reasoning);
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.input == 0
            && self.output == 0
            && self.cache_read == 0
            && self.cache_write == 0
            && self.reasoning == 0
    }
}

/// Returns the non-null JSON-RPC `error` payload, if present.
pub(crate) fn rpc_error(val: &Value) -> Option<&Value> {
    val.get("error").filter(|v| !v.is_null())
}

/// Returns the non-null JSON-RPC `result` payload, if present.
pub(crate) fn rpc_result(val: &Value) -> Option<&Value> {
    val.get("result").filter(|v| !v.is_null())
}

/// Returns the first non-null JSON string across `candidates`, avoiding `null` short-circuiting.
pub(crate) fn first_str<'a>(candidates: &[Option<&'a Value>]) -> Option<&'a str> {
    candidates.iter().find_map(|c| c.and_then(Value::as_str))
}

/// Returns the first non-null JSON `u64` across `candidates`, defaulting to `0`.
pub(crate) fn first_u64(candidates: &[Option<&Value>]) -> u64 {
    candidates
        .iter()
        .find_map(|c| c.and_then(Value::as_u64))
        .unwrap_or(0)
}

/// Returns `true` for ACP permission modes that bypass office approval enforcement.
pub(crate) fn is_banned_mode(mode: &str) -> bool {
    matches!(
        mode.to_ascii_lowercase().as_str(),
        "yolo" | "bypasspermissions" | "danger-full-access" | "auto"
    )
}

/// Converts a JSON-RPC request/response `id` (string or integer) into a stable lookup key.
pub(crate) fn rpc_id_key(id: &Value) -> Option<String> {
    match id {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub(crate) fn encode_initialize(req_id: u64) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": {
                "name": "agentic-org",
                "version": env!("CARGO_PKG_VERSION"),
            },
        },
    }))
    .expect("initialize JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_session_open(
    req_id: u64,
    cwd: &str,
    mcp_servers: &[Value],
    system_prompt_append: Option<&str>,
    resume_session_id: Option<&str>,
) -> Output {
    let mut params = json!({
        "cwd": cwd,
        "mcpServers": mcp_servers,
    });
    if let Some(prompt) = system_prompt_append {
        params["_meta"] = json!({
            "antigravity": {
                "systemPrompt": {
                    "append": prompt,
                }
            }
        });
    }
    let method = match resume_session_id {
        Some(sid) => {
            params["sessionId"] = Value::String(sid.to_owned());
            "session/resume"
        }
        None => "session/new",
    };
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params,
    }))
    .expect("session open JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_set_mode(req_id: u64, session_id: &str, mode_id: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "session/set_mode",
        "params": {
            "sessionId": session_id,
            "modeId": mode_id,
        },
    }))
    .expect("session/set_mode JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_set_model(req_id: u64, session_id: &str, model_id: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "session/set_model",
        "params": {
            "sessionId": session_id,
            "modelId": model_id,
        },
    }))
    .expect("session/set_model JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_session_prompt(req_id: u64, session_id: &str, text: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": text }],
        },
    }))
    .expect("session/prompt JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_session_cancel(session_id: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": { "sessionId": session_id },
    }))
    .expect("session/cancel notification encodes");
    Output::Write(line)
}

pub(crate) fn encode_session_close(req_id: u64, session_id: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "session/close",
        "params": { "sessionId": session_id },
    }))
    .expect("session/close JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_permission_selected(raw_id: &Value, option_id: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": raw_id,
        "result": {
            "outcome": {
                "outcome": "selected",
                "optionId": option_id,
            }
        }
    }))
    .expect("permission selected response encodes");
    Output::Write(line)
}

pub(crate) fn encode_permission_cancelled(raw_id: &Value) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": raw_id,
        "result": {
            "outcome": {
                "outcome": "cancelled",
            }
        }
    }))
    .expect("permission cancelled response encodes");
    Output::Write(line)
}

pub(crate) fn encode_jsonrpc_error(raw_id: &Value, code: i64, message: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": raw_id,
        "error": {
            "code": code,
            "message": message,
        }
    }))
    .expect("jsonrpc error encodes");
    Output::Write(line)
}

pub(crate) fn classify_tool_capability(tool: &str) -> Option<ToolCapability> {
    match tool.to_ascii_lowercase().as_str() {
        "read_file" | "view_file" => Some(ToolCapability::Read),
        "list_dir" | "find_by_name" | "grep_search" => Some(ToolCapability::Search),
        "write_file"
        | "edit_file"
        | "delete_file"
        | "replace_file_content"
        | "multi_replace_file_content" => Some(ToolCapability::Edit),
        "run_command" => Some(ToolCapability::Shell),
        "invoke_subagent" | "spawn_subagent" | "subagent" => Some(ToolCapability::Subagent),
        _ => None,
    }
}

pub(crate) fn classify_tool_kind(tool: &str) -> ApprovalKind {
    match tool.to_ascii_lowercase().as_str() {
        "run_command" | "bash" | "shell" => ApprovalKind::Command,
        "write_file"
        | "edit_file"
        | "write"
        | "edit"
        | "delete_file"
        | "replace_file_content"
        | "multi_replace_file_content" => ApprovalKind::FileChange,
        "set_permission_mode"
        | "exitplanmode"
        | "enterplanmode"
        | "exit_plan_mode"
        | "enter_plan_mode" => ApprovalKind::Permissions,
        "read_file"
        | "view_file"
        | "list_dir"
        | "find_by_name"
        | "grep_search"
        | "glob"
        | "grep"
        | "read"
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

pub(crate) fn classify_acp_kind_str(kind: Option<&str>) -> Option<ToolCapability> {
    match kind.map(str::to_ascii_lowercase).as_deref() {
        Some("read") => Some(ToolCapability::Read),
        Some("search") => Some(ToolCapability::Search),
        Some("edit" | "delete" | "move") => Some(ToolCapability::Edit),
        Some("execute") => Some(ToolCapability::Shell),
        Some("subagent") => Some(ToolCapability::Subagent),
        _ => None,
    }
}

pub(crate) fn classify_acp_tool_call_capability(tc: &Value) -> Option<ToolCapability> {
    classify_acp_kind_str(tc.get("kind").and_then(Value::as_str))
}

fn classify_acp_tool_call_kind(tc: &Value) -> ApprovalKind {
    match tc
        .get("kind")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("execute") => ApprovalKind::Command,
        Some("edit" | "delete" | "move") => ApprovalKind::FileChange,
        Some("switch_mode") => ApprovalKind::Permissions,
        Some("read" | "search") => ApprovalKind::Other,
        _ => ApprovalKind::Unknown,
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

pub(crate) fn classify_escalation(tool: &str, kind: ApprovalKind, input: &Value) -> Escalation {
    if tool.eq_ignore_ascii_case("SandboxNetworkAccess")
        || is_bypass_flag_triggered(input.get("dangerouslyDisableSandbox"))
        || is_bypass_flag_triggered(input.get("unsandboxed"))
        || is_bypass_flag_triggered(input.get("BypassSandbox"))
        || is_bypass_flag_triggered(input.get("bypassSandbox"))
        || is_bypass_flag_triggered(input.get("bypass_sandbox"))
    {
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
    let tool = first_str(&[tc.get("name"), tc.get("title")]).unwrap_or("unknown");
    let input = tc
        .get("rawInput")
        .filter(|v| !v.is_null())
        .or_else(|| tc.get("args").filter(|v| !v.is_null()))
        .cloned()
        .unwrap_or(Value::Null);
    let kind = match classify_tool_kind(tool) {
        ApprovalKind::Unknown => classify_acp_tool_call_kind(tc),
        known => known,
    };
    let escalation = classify_escalation(tool, kind, &input);
    let mut request = ApprovalRequest::new(approval_id, kind, escalation, tool, input.clone());
    request.capability =
        classify_tool_capability(tool).or_else(|| classify_acp_tool_call_capability(tc));
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
        || lower.contains("unauthorized")
        || lower.contains("not logged in")
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

pub(crate) fn parse_stderr_line(text: &str) -> Option<Output> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("AGY_ERROR:") {
        if let Ok(val) = serde_json::from_str::<Value>(rest.trim()) {
            let code = val.get("code").and_then(Value::as_str).unwrap_or("error");
            let msg = val
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Agy structured error");
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
    Some(Output::Event(SessionEvent::Notice {
        kind: NoticeKind::Warning,
        text: trimmed.to_owned(),
    }))
}
