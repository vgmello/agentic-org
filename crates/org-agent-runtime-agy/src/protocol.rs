use serde_json::{json, Value};

use org_agent_runtime::{
    encode_line, ApprovalId, ApprovalKind, ApprovalRequest, Escalation, LimitKind, LimitStatus,
    NoticeKind, Output, RateLimit, SessionEvent, ToolCapability, TurnOutcome, Window,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct MessageUsage {
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) cache_read: u64,
    pub(crate) cache_write: u64,
    pub(crate) reasoning: u64,
}

impl MessageUsage {
    pub(crate) fn merge_max(&mut self, other: &MessageUsage) {
        self.input = self.input.max(other.input);
        self.output = self.output.max(other.output);
        self.cache_read = self.cache_read.max(other.cache_read);
        self.cache_write = self.cache_write.max(other.cache_write);
        self.reasoning = self.reasoning.max(other.reasoning);
    }

    pub(crate) fn saturating_sub(&self, base: &MessageUsage) -> MessageUsage {
        MessageUsage {
            input: self.input.saturating_sub(base.input),
            output: self.output.saturating_sub(base.output),
            cache_read: self.cache_read.saturating_sub(base.cache_read),
            cache_write: self.cache_write.saturating_sub(base.cache_write),
            reasoning: self.reasoning.saturating_sub(base.reasoning),
        }
    }
}

pub(crate) fn is_banned_mode(mode: &str) -> bool {
    matches!(
        mode.to_ascii_lowercase().as_str(),
        "yolo" | "bypasspermissions" | "danger-full-access"
    )
}

pub(crate) fn has_remote_control(obj: &Value) -> bool {
    obj.get("remoteControl")
        .or_else(|| obj.get("remote_control"))
        .and_then(Value::as_bool)
        == Some(true)
}

pub(crate) fn extract_agent_names(val: Option<&Value>) -> Vec<String> {
    let Some(arr) = val.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            v.as_str()
                .or_else(|| v.get("name").and_then(Value::as_str))
                .map(ToOwned::to_owned)
        })
        .collect()
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
        "read_file" | "view_file" | "list_dir" | "find_by_name" | "grep_search" | "glob"
        | "grep" | "read" | "webfetch" | "websearch" | "invoke_subagent" | "spawn_subagent"
        | "subagent" => ApprovalKind::Other,
        other if other.starts_with("mcp_") => ApprovalKind::Other,
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
    if is_bypass_flag_triggered(input.get("dangerouslyDisableSandbox"))
        || is_bypass_flag_triggered(input.get("unsandboxed"))
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

pub(crate) fn build_approval_request(
    request_id: &str,
    req: &serde_json::Map<String, Value>,
) -> ApprovalRequest {
    let tool = req
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let input = req.get("input").cloned().unwrap_or(Value::Null);
    let kind = classify_tool_kind(tool);
    let escalation = classify_escalation(tool, kind, &input);
    let id = ApprovalId::new(request_id);
    let mut approval = ApprovalRequest::new(id, kind, escalation, tool, input.clone());
    approval.capability = classify_tool_capability(tool);
    approval.display_name = req
        .get("display_name")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    approval.justification_untrusted = req
        .get("description")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    approval.blocked_path = req
        .get("blocked_path")
        .or_else(|| req.get("blockedPath"))
        .or_else(|| input.get("path"))
        .or_else(|| input.get("file_path"))
        .or_else(|| input.get("TargetFile"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    approval.tool_use_id = req
        .get("tool_use_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    approval
}

pub(crate) fn encode_allow_response(request_id: &str) -> Output {
    let line = encode_line(&json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": { "behavior": "allow" }
        }
    }))
    .expect("static allow encodes");
    Output::Write(line)
}

pub(crate) fn encode_deny_response(request_id: &str, message: &str, interrupt: bool) -> Output {
    let mut body = json!({ "behavior": "deny", "message": message });
    if interrupt {
        body["interrupt"] = Value::Bool(true);
    }
    let line = encode_line(&json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": body
        }
    }))
    .expect("static deny encodes");
    Output::Write(line)
}

pub(crate) fn parse_agy_usage(val: &Value) -> MessageUsage {
    MessageUsage {
        input: val
            .get("input_tokens")
            .or_else(|| val.get("prompt_token_count"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output: val
            .get("output_tokens")
            .or_else(|| val.get("candidates_token_count"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read: val
            .get("cache_read_tokens")
            .or_else(|| val.get("cached_content_token_count"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_write: val
            .get("cache_write_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        reasoning: val
            .get("reasoning_tokens")
            .or_else(|| val.get("thoughts_token_count"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
    }
}

pub(crate) fn parse_stream_event(val: &Value) -> Vec<Output> {
    let Some(delta) = val.pointer("/event/delta").and_then(Value::as_object) else {
        return Vec::new();
    };
    match delta.get("type").and_then(Value::as_str) {
        Some("text_delta") => delta
            .get("text")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(|t| {
                vec![Output::Event(SessionEvent::TextDelta {
                    text: t.to_owned(),
                })]
            })
            .unwrap_or_default(),
        Some("thinking_delta") => vec![Output::Event(SessionEvent::Thinking)],
        _ => Vec::new(),
    }
}

pub(crate) fn parse_user_blocks(val: &Value) -> Vec<Output> {
    let mut out = Vec::new();
    let Some(blocks) = val.pointer("/message/content").and_then(Value::as_array) else {
        return out;
    };
    for block in blocks {
        if block.get("type").and_then(Value::as_str) == Some("tool_result") {
            out.push(Output::Event(SessionEvent::ToolFinished {
                tool_use_id: block
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                is_error: block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            }));
        }
    }
    out
}

pub(crate) fn parse_assistant_blocks(
    msg: &serde_json::Map<String, Value>,
    msg_id: &Option<String>,
) -> Vec<Output> {
    let mut out = Vec::new();
    let Some(blocks) = msg.get("content").and_then(Value::as_array) else {
        return out;
    };
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    out.push(Output::Event(SessionEvent::AssistantText {
                        message_id: msg_id.clone(),
                        text: t.to_owned(),
                    }));
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                let capability = classify_tool_capability(&name);
                out.push(Output::Event(SessionEvent::ToolStarted {
                    tool_use_id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    name,
                    capability,
                    input: block.get("input").cloned().unwrap_or(Value::Null),
                }));
            }
            Some("thinking") => out.push(Output::Event(SessionEvent::Thinking)),
            _ => {}
        }
    }
    out
}

pub(crate) fn parse_permission_denials(val: &Value) -> Vec<Output> {
    let Some(denials) = val.get("permission_denials").and_then(Value::as_array) else {
        return Vec::new();
    };
    denials
        .iter()
        .map(|entry| {
            let tool = entry
                .get("tool_name")
                .or_else(|| entry.get("tool"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let tool_use_id = entry
                .get("tool_use_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let message = entry
                .get("message")
                .or_else(|| entry.get("reason"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            Output::Event(SessionEvent::ActionDenied {
                tool,
                tool_use_id,
                message,
            })
        })
        .collect()
}

pub(crate) fn parse_result_errors(val: &Value) -> Vec<String> {
    val.get("errors")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str()
                        .or_else(|| v.get("message").and_then(Value::as_str))
                        .map(ToOwned::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn classify_result_outcome(val: &Value, errors: &[String]) -> TurnOutcome {
    let is_error = val
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let subtype = val.get("subtype").and_then(Value::as_str).unwrap_or("");
    let reason_text = val
        .get("result")
        .and_then(Value::as_str)
        .or_else(|| errors.first().map(String::as_str))
        .unwrap_or("");
    let lower = reason_text.to_ascii_lowercase();

    match subtype {
        "interrupted" => TurnOutcome::Interrupted,
        "error_max_turns" | "max_turns" => TurnOutcome::Limit {
            kind: LimitKind::MaxTurns,
        },
        "error_max_tokens" | "max_tokens" => TurnOutcome::Limit {
            kind: LimitKind::MaxTokens,
        },
        "error_timeout" | "timeout" => TurnOutcome::Limit {
            kind: LimitKind::Timeout,
        },
        "refused" => TurnOutcome::Refused,
        "quota_exceeded" | "error_quota" => TurnOutcome::Quota,
        "signed_out" | "auth_required" => TurnOutcome::SignedOut,
        _ if is_error || subtype.starts_with("error") => {
            if lower.contains("authentication required")
                || lower.contains("not authenticated")
                || lower.contains("not logged in")
                || lower.contains("unauthorized")
            {
                TurnOutcome::SignedOut
            } else if lower.contains("quota") || lower.contains("rate limit") {
                TurnOutcome::Quota
            } else if lower.contains("refused") {
                TurnOutcome::Refused
            } else {
                TurnOutcome::Failed {
                    reason: if reason_text.is_empty() {
                        "turn failed".to_owned()
                    } else {
                        reason_text.to_owned()
                    },
                }
            }
        }
        "" | "success" => TurnOutcome::Completed,
        _ => TurnOutcome::Unknown,
    }
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

pub(crate) fn handle_rate_limit(val: &Value) -> Vec<Output> {
    let Some(info) = val.get("rate_limit_info").and_then(Value::as_object) else {
        return Vec::new();
    };
    let status = match info.get("status").and_then(Value::as_str) {
        Some("allowed") => LimitStatus::Ok,
        Some("allowed_warning") => LimitStatus::Warning,
        Some("rejected") => LimitStatus::Rejected,
        _ => LimitStatus::Unknown,
    };
    let mut rl = RateLimit::new(
        Window::Other {
            id: info
                .get("rateLimitType")
                .and_then(Value::as_str)
                .unwrap_or("quota")
                .to_owned(),
            mins: None,
        },
        status,
    );
    rl.used_fraction = info.get("utilization").and_then(Value::as_f64);
    rl.resets_at = info
        .get("resetsAt")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    vec![Output::Event(SessionEvent::RateLimit(rl))]
}
