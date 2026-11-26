//! Claude's control protocol: `control_response` (the `initialize` answer),
//! `control_request` (tool permission prompts and hook callbacks) and
//! `control_cancel_request`.

use serde_json::{json, Value};

use org_agent_runtime::{
    encode_line, Activity, ApprovalId, ApprovalRequest, Output, SessionEvent, StartFailure,
};

use super::classify::{
    agent_matches, classify_approval_kind, classify_escalation, classify_tool_capability,
    extract_agent_names, permission_mode_matches, remote_control_active,
};
use super::{ClaudeSession, Phase, POST_TOOL_USE_CALLBACK_ID};

impl ClaudeSession {
    pub(super) fn handle_control_response(&mut self, val: &Value) -> Vec<Output> {
        let Some(resp_obj) = val.get("response").and_then(Value::as_object) else {
            return vec![Output::Event(SessionEvent::ProtocolError {
                detail: "control_response missing `response` object".to_owned(),
            })];
        };
        let req_id = resp_obj
            .get("request_id")
            .and_then(Value::as_str)
            .unwrap_or("");

        if self.phase == Phase::Opening && self.init_request_id.as_deref() == Some(req_id) {
            self.init_deadline = None;
            let subtype = resp_obj
                .get("subtype")
                .and_then(Value::as_str)
                .unwrap_or("");
            if subtype != "success" {
                self.phase = Phase::Failed;
                let detail = resp_obj
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("initialize control request failed")
                    .to_owned();
                return vec![Output::Event(SessionEvent::StartFailed {
                    reason: StartFailure::Other { detail },
                })];
            }

            let body = resp_obj.get("response").unwrap_or(&Value::Null);
            let agents = extract_agent_names(body.get("agents"));
            if !agent_matches(
                body.get("agents"),
                &self.expected_agent,
                &self.expected_agent_description,
            ) {
                self.phase = Phase::Failed;
                return vec![Output::Event(SessionEvent::StartFailed {
                    reason: StartFailure::AgentMismatch {
                        requested: self.expected_agent.clone(),
                        available: agents,
                    },
                })];
            }

            if remote_control_active(body) {
                self.phase = Phase::Failed;
                return vec![Output::Event(SessionEvent::StartFailed {
                    reason: StartFailure::Other {
                        detail: "Claude reported remote control active".to_owned(),
                    },
                })];
            }

            let reported_mode = body
                .get("current_permission_mode")
                .or_else(|| body.get("permissionMode"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if !permission_mode_matches(&self.expected_mode, reported_mode) {
                self.phase = Phase::Failed;
                return vec![Output::Event(SessionEvent::StartFailed {
                    reason: StartFailure::ModeMismatch {
                        requested: self.expected_mode.reported_name.to_owned(),
                        reported: reported_mode.to_owned(),
                    },
                })];
            }

            let pid = body
                .get("pid")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok());
            let runtime_version = body
                .get("version")
                .or_else(|| body.get("claude_code_version"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);

            self.phase = Phase::Idle;
            return vec![Output::Event(SessionEvent::Ready {
                runtime_version,
                pid,
                agents,
                permission_mode: Some(reported_mode.to_owned()),
            })];
        }

        // Non-initialize control responses (e.g. interrupt ack) produce no state transition.
        Vec::new()
    }

    pub(super) fn handle_control_request(&mut self, val: &Value) -> Vec<Output> {
        let Some(request_id) = val.get("request_id").and_then(Value::as_str) else {
            return vec![Output::Event(SessionEvent::ProtocolError {
                detail: "control_request missing string `request_id`".to_owned(),
            })];
        };
        let Some(req) = val.get("request").and_then(Value::as_object) else {
            return vec![Output::Event(SessionEvent::ProtocolError {
                detail: "control_request missing `request` object".to_owned(),
            })];
        };
        let subtype = req.get("subtype").and_then(Value::as_str).unwrap_or("");
        if subtype == "hook_callback" {
            return handle_hook_callback(self, request_id, req);
        }
        if subtype != "can_use_tool" {
            return vec![Output::Event(SessionEvent::Unknown {
                type_name: format!("control_request/{subtype}"),
            })];
        }

        let tool_name = req
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let tool_use_id = req
            .get("tool_use_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        if self.aborting {
            return vec![Self::encode_deny_control_response(
                request_id,
                "Session is stopping due to a policy violation",
                true,
            )];
        }

        let input = req.get("input").cloned().unwrap_or(Value::Null);
        let kind = classify_approval_kind(tool_name);
        let escalation = classify_escalation(tool_name, kind, &input);
        let id = ApprovalId::new(request_id);

        let mut approval = ApprovalRequest::new(id.clone(), kind, escalation, tool_name, input);
        approval.capability = classify_tool_capability(tool_name);
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
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        approval.tool_use_id = tool_use_id;

        self.pending_approvals.insert(id);
        vec![
            Output::Event(SessionEvent::Status(Activity::WaitingOnApproval)),
            Output::Event(SessionEvent::ApprovalRequested(approval)),
        ]
    }

    pub(super) fn handle_control_cancel(&mut self, val: &Value) -> Vec<Output> {
        let Some(request_id) = val.get("request_id").and_then(Value::as_str) else {
            return vec![Output::Event(SessionEvent::ProtocolError {
                detail: "control_cancel_request missing string `request_id`".to_owned(),
            })];
        };
        let id = ApprovalId::new(request_id);
        if self.pending_approvals.remove(&id) {
            let mut out = vec![Output::Event(SessionEvent::ApprovalCancelled { id })];
            if self.pending_approvals.is_empty() {
                match self.phase {
                    Phase::Busy => out.push(Output::Event(SessionEvent::Status(Activity::Busy))),
                    Phase::Idle => out.push(Output::Event(SessionEvent::Status(Activity::Idle))),
                    _ => {}
                }
            }
            out
        } else {
            Vec::new()
        }
    }
}

fn handle_hook_callback(
    s: &mut ClaudeSession,
    request_id: &str,
    req: &serde_json::Map<String, Value>,
) -> Vec<Output> {
    let callback_id = req.get("callback_id").and_then(Value::as_str).unwrap_or("");
    let delivered = if callback_id == POST_TOOL_USE_CALLBACK_ID
        && !s.aborting
        && s.phase == Phase::Busy
        && s.pending_approvals.is_empty()
    {
        s.staged_mid_turn.take()
    } else {
        None
    };
    let (body, event) = match delivered {
        Some((delivery_id, text)) => (
            json!({
                "continue": true,
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "additionalContext": text,
                }
            }),
            Some(Output::Event(SessionEvent::MidTurnDelivered {
                delivery_id,
            })),
        ),
        None => (json!({ "continue": true }), None),
    };
    let payload = json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": body,
        }
    });
    let line = encode_line(&payload).expect("hook_callback control_response encodes");
    let mut out = vec![Output::Write(line)];
    if let Some(ev) = event {
        out.push(ev);
    }
    out
}
