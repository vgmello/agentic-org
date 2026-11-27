//! Requests and notifications from ACP runtimes: permissions, cancellations, and `session/update`.

use serde_json::Value;

use org_agent_runtime::{Activity, ApprovalId, NoticeKind, Output, SessionEvent};

use crate::protocol::{
    encode_jsonrpc_error, encode_permission_cancelled, encode_session_cancel, first_str,
    is_banned_mode, parse_acp_usage, parse_permission_request, resolve_tool_classification,
    rpc_id_key,
};

use super::handshake::fail_opening_mode_mismatch;
use super::{is_opening, AcpSession, PendingApproval, Phase};

pub(super) fn handle_server_request(s: &mut AcpSession, val: &Value) -> Vec<Output> {
    let method = val.get("method").and_then(Value::as_str).unwrap_or("");
    let raw_id = val.get("id").cloned().unwrap_or(Value::Null);
    if method != "session/request_permission" {
        return vec![
            encode_jsonrpc_error(&raw_id, -32601, "unsupported client request"),
            Output::Event(SessionEvent::Unknown {
                type_name: method.to_owned(),
            }),
        ];
    }
    if matches!(s.phase, Phase::Closing | Phase::Failed | Phase::Ended) {
        return vec![encode_permission_cancelled(&raw_id)];
    }
    let Some(id_key) = rpc_id_key(&raw_id) else {
        return vec![Output::Event(SessionEvent::ProtocolError {
            detail: "session/request_permission missing valid JSON-RPC id".to_owned(),
        })];
    };
    let approval_id = ApprovalId::new(id_key);
    let parsed = parse_permission_request(
        approval_id.clone(),
        val.get("params").unwrap_or(&Value::Null),
    );
    s.pending_approvals.insert(
        approval_id,
        PendingApproval {
            raw_id,
            allow_option_id: parsed.allow_option_id,
            deny_option_id: parsed.deny_option_id,
            tool_call_id: parsed.tool_call_id,
        },
    );
    vec![
        Output::Event(SessionEvent::Status(Activity::WaitingOnApproval)),
        Output::Event(SessionEvent::ApprovalRequested(parsed.request)),
    ]
}

pub(super) fn handle_notification(s: &mut AcpSession, val: &Value) -> Vec<Output> {
    let method = val.get("method").and_then(Value::as_str).unwrap_or("");
    let params = val.get("params").unwrap_or(&Value::Null);
    match method {
        "session/update" => handle_session_update(s, params),
        "$/cancel_request" => handle_cancel_request(s, params),
        other => vec![Output::Event(SessionEvent::Unknown {
            type_name: other.to_owned(),
        })],
    }
}

fn handle_cancel_request(s: &mut AcpSession, params: &Value) -> Vec<Output> {
    let Some(key) = params.get("id").and_then(rpc_id_key) else {
        return Vec::new();
    };
    let id = ApprovalId::new(key);
    remove_cancelled_approval(s, &id)
}

fn remove_cancelled_approval(s: &mut AcpSession, id: &ApprovalId) -> Vec<Output> {
    if s.pending_approvals.remove(id).is_none() {
        return Vec::new();
    }
    let mut out = vec![Output::Event(SessionEvent::ApprovalCancelled {
        id: id.clone(),
    })];
    if s.pending_approvals.is_empty() {
        match s.phase {
            Phase::Busy => out.push(Output::Event(SessionEvent::Status(Activity::Busy))),
            Phase::Idle => out.push(Output::Event(SessionEvent::Status(Activity::Idle))),
            _ => {}
        }
    }
    out
}

fn handle_session_update(s: &mut AcpSession, params: &Value) -> Vec<Output> {
    let Some(update) = params.get("update").and_then(Value::as_object) else {
        return Vec::new();
    };
    let kind = update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or("");
    match kind {
        "agent_message_chunk" => handle_message_chunk(s, update),
        "agent_thought_chunk" => vec![Output::Event(SessionEvent::Thinking)],
        "tool_call" => handle_tool_call_update_start(update),
        "tool_call_update" => handle_tool_call_update_status(s, update),
        "usage_update" => handle_usage_update(s, update),
        "current_mode_update" => handle_current_mode_update(s, update),
        "config_option_update" => {
            if update.get("configOptionId").and_then(Value::as_str) == Some("model") {
                if let Some(m) = update.get("currentValue").and_then(Value::as_str) {
                    s.active_model = Some(m.to_owned());
                }
            }
            Vec::new()
        }
        "available_commands_update" | "session_info_update" | "plan" | "user_message_chunk" => {
            Vec::new()
        }
        other => vec![Output::Event(SessionEvent::Unknown {
            type_name: format!("session/update/{other}"),
        })],
    }
}

fn handle_current_mode_update(
    s: &mut AcpSession,
    update: &serde_json::Map<String, Value>,
) -> Vec<Output> {
    if matches!(s.phase, Phase::Failed | Phase::Ended) {
        return Vec::new();
    }
    let Some(mode) = first_str(&[update.get("currentModeId"), update.get("mode")]) else {
        return Vec::new();
    };
    if is_opening(s.phase) {
        if is_banned_mode(mode) {
            return fail_opening_mode_mismatch(s, mode, true);
        }
        if mode == s.cfg.expected_mode.reported_name {
            s.reported_mode = Some(mode.to_owned());
            return Vec::new();
        }
        if s.phase == Phase::OpeningMode && s.reported_mode.as_deref() == Some(mode) {
            return Vec::new();
        }
        return fail_opening_mode_mismatch(s, mode, false);
    }
    s.reported_mode = Some(mode.to_owned());
    let mut out = vec![Output::Event(SessionEvent::ModeChanged {
        mode: Some(mode.to_owned()),
    })];
    // Fail closed on any runtime mode drift outside the requested mode (allowing at most strict
    // narrowing to `plan`, matching opening validation semantics).
    let mode_disallowed =
        is_banned_mode(mode) || (mode != s.cfg.expected_mode.reported_name && mode != "plan");
    if mode_disallowed && !matches!(s.phase, Phase::Closing | Phase::Failed | Phase::Ended) {
        s.phase = Phase::Closing;
        s.handshake_deadline = None;
        out.push(Output::Event(SessionEvent::Notice {
            kind: NoticeKind::Warning,
            text: format!(
                "{} ACP switched to disallowed permission mode `{mode}`; stopping session",
                s.display_name()
            ),
        }));
        if let Some(sid) = s.session_id.as_ref() {
            out.push(encode_session_cancel(sid.as_str()));
        }
        out.push(Output::CloseStdin);
    }
    out
}

fn handle_message_chunk(
    s: &mut AcpSession,
    update: &serde_json::Map<String, Value>,
) -> Vec<Output> {
    let Some(text) = update
        .get("content")
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
    else {
        return Vec::new();
    };
    s.turn_text_buf.push_str(text);
    vec![Output::Event(SessionEvent::TextDelta {
        text: text.to_owned(),
    })]
}

fn handle_tool_call_update_start(update: &serde_json::Map<String, Value>) -> Vec<Output> {
    let explicit_name = update
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty());
    let name = explicit_name
        .or_else(|| {
            update
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or("unknown")
        .to_owned();
    let (_, capability) =
        resolve_tool_classification(explicit_name, update.get("kind").and_then(Value::as_str));
    let input = update
        .get("rawInput")
        .filter(|v| !v.is_null())
        .or_else(|| update.get("args").filter(|v| !v.is_null()))
        .cloned()
        .unwrap_or(Value::Null);
    vec![Output::Event(SessionEvent::ToolStarted {
        tool_use_id: update
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        name,
        capability,
        input,
    })]
}

fn handle_tool_call_update_status(
    s: &mut AcpSession,
    update: &serde_json::Map<String, Value>,
) -> Vec<Output> {
    let status = update.get("status").and_then(Value::as_str).unwrap_or("");
    if !matches!(status, "completed" | "failed" | "cancelled") {
        return Vec::new();
    }
    let tool_use_id = update
        .get("toolCallId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut out = Vec::new();
    if status == "cancelled" && !tool_use_id.is_empty() {
        let matched: Vec<ApprovalId> = s
            .pending_approvals
            .iter()
            .filter(|(_, p)| p.tool_call_id.as_deref() == Some(tool_use_id.as_str()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in matched {
            out.extend(remove_cancelled_approval(s, &id));
        }
    }
    out.push(Output::Event(SessionEvent::ToolFinished {
        tool_use_id,
        is_error: matches!(status, "failed" | "cancelled"),
    }));
    out
}

fn handle_usage_update(s: &mut AcpSession, update: &serde_json::Map<String, Value>) -> Vec<Output> {
    if !s.turn_open {
        return Vec::new();
    }
    if let Some(usage_val) = update.get("usage").filter(|v| v.is_object()) {
        let parsed = parse_acp_usage(usage_val);
        if !parsed.is_zero() {
            match &mut s.streamed_usage {
                Some(existing) => existing.merge_max(&parsed),
                None => s.streamed_usage = Some(parsed),
            }
        }
    }
    Vec::new()
}
