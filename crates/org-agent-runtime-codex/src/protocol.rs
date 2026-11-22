use serde_json::{json, Value};

use org_agent_runtime::{
    encode_line, ApprovalId, ApprovalKind, ApprovalRequest, Escalation, LimitStatus, Output,
    RateLimit, SessionEvent, Window,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct CodexTokens {
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) cache_read: u64,
    pub(crate) cache_write: u64,
    pub(crate) reasoning: u64,
}

impl CodexTokens {
    pub(crate) fn add_assign(&mut self, other: &CodexTokens) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
        self.reasoning = self.reasoning.saturating_add(other.reasoning);
    }

    pub(crate) fn saturating_sub(&self, prev: &CodexTokens) -> CodexTokens {
        CodexTokens {
            input: self.input.saturating_sub(prev.input),
            output: self.output.saturating_sub(prev.output),
            cache_read: self.cache_read.saturating_sub(prev.cache_read),
            cache_write: self.cache_write.saturating_sub(prev.cache_write),
            reasoning: self.reasoning.saturating_sub(prev.reasoning),
        }
    }
}

pub(crate) fn parse_codex_tokens(obj: &serde_json::Map<String, Value>) -> CodexTokens {
    CodexTokens {
        input: obj
            .get("inputTokens")
            .or_else(|| obj.get("input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output: obj
            .get("outputTokens")
            .or_else(|| obj.get("output_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read: obj
            .get("cachedInputTokens")
            .or_else(|| obj.get("cached_input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_write: obj
            .get("cacheWriteInputTokens")
            .or_else(|| obj.get("cache_write_input_tokens"))
            .or_else(|| obj.get("cacheCreationInputTokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        reasoning: obj
            .get("reasoningOutputTokens")
            .or_else(|| obj.get("reasoning_output_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
    }
}

pub(crate) fn rpc_id_key(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn encode_jsonrpc_error(rpc_id: &Value, code: i64, message: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": rpc_id,
        "error": { "code": code, "message": message }
    }))
    .expect("jsonrpc error encodes");
    Output::Write(line)
}

pub(crate) fn encode_approval_result(rpc_id: &Value, result_val: Value) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": rpc_id,
        "result": result_val
    }))
    .expect("approval reply encodes");
    Output::Write(line)
}

pub(crate) fn classify_codex_escalation(p: &Value) -> Escalation {
    if p.get("withEscalatedPermissions")
        .or_else(|| p.get("unsandboxed"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Escalation::SandboxBypass;
    }
    if p.get("sandboxPermissions")
        .or_else(|| p.get("sandbox_permissions"))
        .and_then(Value::as_str)
        .is_some_and(|s| s.eq_ignore_ascii_case("require_escalated"))
    {
        return Escalation::SandboxBypass;
    }
    if p.get("sandbox")
        .and_then(Value::as_str)
        .is_some_and(is_danger_full_access)
    {
        return Escalation::SandboxBypass;
    }
    if p.pointer("/sandboxPolicy/type")
        .or_else(|| p.pointer("/sandbox_policy/type"))
        .and_then(Value::as_str)
        .is_some_and(is_danger_full_access)
    {
        return Escalation::SandboxBypass;
    }
    if has_non_empty_field(p.get("additionalPermissions"))
        || has_non_empty_field(p.get("additional_permissions"))
    {
        return Escalation::PermissionChange;
    }
    Escalation::None
}

fn is_danger_full_access(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower == "danger-full-access" || lower == "dangerfullaccess"
}

fn has_non_empty_field(val: Option<&Value>) -> bool {
    match val {
        Some(Value::Object(m)) => !m.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Bool(b)) => *b,
        _ => false,
    }
}

pub(crate) fn build_codex_approval(
    id_str: String,
    kind: ApprovalKind,
    tool: &'static str,
    forced_escalation: Option<Escalation>,
    p: &Value,
) -> ApprovalRequest {
    let escalation = forced_escalation.unwrap_or_else(|| classify_codex_escalation(p));
    let approval_id = ApprovalId::new(id_str);
    let mut req = ApprovalRequest::new(approval_id, kind, escalation, tool, p.clone());
    req.justification_untrusted = p
        .get("reason")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    req.blocked_path = p
        .get("blockedPath")
        .or_else(|| p.get("blocked_path"))
        .or_else(|| p.get("path"))
        .or_else(|| p.get("filePath"))
        .or_else(|| p.get("grantRoot"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    req.tool_use_id = p
        .get("itemId")
        .or_else(|| p.get("callId"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    req
}

pub(crate) fn handle_item_started(p: &Value) -> Vec<Output> {
    let Some(item) = p.get("item") else {
        return Vec::new();
    };
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    match item.get("type").and_then(Value::as_str) {
        Some("commandExecution") => vec![Output::Event(SessionEvent::ToolStarted {
            tool_use_id: id,
            name: "shell".to_owned(),
            input: json!({ "command": item.get("command").cloned().unwrap_or(Value::Null) }),
        })],
        Some("fileChange") => vec![Output::Event(SessionEvent::ToolStarted {
            tool_use_id: id,
            name: "apply_patch".to_owned(),
            input: item.get("changes").cloned().unwrap_or(Value::Null),
        })],
        Some("mcpToolCall") => vec![Output::Event(SessionEvent::ToolStarted {
            tool_use_id: id,
            name: item
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("mcp")
                .to_owned(),
            input: item.get("arguments").cloned().unwrap_or(Value::Null),
        })],
        _ => Vec::new(),
    }
}

pub(crate) fn handle_item_completed(p: &Value) -> Vec<Output> {
    let Some(item) = p.get("item") else {
        return Vec::new();
    };
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    match item.get("type").and_then(Value::as_str) {
        Some("agentMessage") => item
            .get("text")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(|t| {
                vec![Output::Event(SessionEvent::AssistantText {
                    message_id: (!id.is_empty()).then_some(id),
                    text: t.to_owned(),
                })]
            })
            .unwrap_or_default(),
        Some("commandExecution" | "fileChange" | "mcpToolCall") => {
            let is_error = item
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|s| s == "failed" || s == "declined")
                || item
                    .get("exitCode")
                    .and_then(Value::as_i64)
                    .is_some_and(|c| c != 0);
            vec![Output::Event(SessionEvent::ToolFinished {
                tool_use_id: id,
                is_error,
            })]
        }
        _ => Vec::new(),
    }
}

pub(crate) fn extract_sandbox_mode(val: &Value) -> Option<&str> {
    for ptr in [
        "/result/thread/sandbox",
        "/result/sandbox",
        "/result/thread/sandboxPolicy",
        "/result/sandboxPolicy",
    ] {
        let Some(v) = val.pointer(ptr) else {
            continue;
        };
        if let Some(s) = v.as_str() {
            return Some(s);
        }
        if let Some(ty) = v.get("type").and_then(Value::as_str) {
            return Some(ty);
        }
    }
    None
}

pub(crate) fn sandbox_mode_matches(requested: &str, reported: &str) -> bool {
    if is_danger_full_access(reported) {
        return false;
    }
    let norm = |s: &str| s.to_ascii_lowercase().replace(['-', '_'], "");
    norm(requested) == norm(reported)
}

pub(crate) fn format_unix_epoch_rfc3339(raw: i64) -> Option<String> {
    if raw < 0 {
        return None;
    }
    let secs = if raw >= 1_000_000_000_000 {
        raw / 1_000
    } else {
        raw
    };
    let days = secs.div_euclid(86_400);
    let sec_of_day = secs.rem_euclid(86_400);
    let hour = sec_of_day / 3_600;
    let minute = (sec_of_day % 3_600) / 60;
    let second = sec_of_day % 60;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(month <= 2);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

pub(crate) fn handle_rate_limits(p: &Value) -> Vec<Output> {
    let Some(rl_obj) = p.get("rateLimits").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, window) in [
        ("primary", Window::FiveHour),
        ("secondary", Window::SevenDay),
    ] {
        let Some(w) = rl_obj.get(key).and_then(Value::as_object) else {
            continue;
        };
        let used_pct = w.get("usedPercent").and_then(Value::as_f64);
        let status = match used_pct {
            Some(pct) if pct >= 100.0 => LimitStatus::Rejected,
            Some(pct) if pct >= 80.0 => LimitStatus::Warning,
            Some(_) => LimitStatus::Ok,
            None => LimitStatus::Unknown,
        };
        let mut rl = RateLimit::new(window, status);
        rl.used_fraction = used_pct.map(|pct| pct / 100.0);
        rl.resets_at = w.get("resetsAt").and_then(|v| match v {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Number(n) => n.as_i64().and_then(format_unix_epoch_rfc3339),
            _ => None,
        });
        out.push(Output::Event(SessionEvent::RateLimit(rl)));
    }
    out
}
