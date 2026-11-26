//! Requests and notifications from the runtime: approval requests, items, token usage and
//! turn completion.

use serde_json::{json, Value};

use org_agent_runtime::{
    Activity, ApprovalId, ApprovalKind, Escalation, Output, SessionEvent, TurnOutcome,
};

use crate::protocol::{
    build_codex_approval, encode_approval_result, encode_jsonrpc_error, handle_item_completed,
    handle_item_started, handle_rate_limits, parse_codex_tokens, rpc_id_key,
};

use super::{CodexSession, Phase};

pub(super) fn handle_server_request(
    s: &mut CodexSession,
    rpc_id: Value,
    method: &str,
    params: Option<&Value>,
) -> Vec<Output> {
    let p = params.unwrap_or(&Value::Null);
    let (kind, tool, forced_escalation) = match method {
        "item/commandExecution/requestApproval" | "execCommandApproval" => {
            (ApprovalKind::Command, "shell", None)
        }
        "item/fileChange/requestApproval" | "applyPatchApproval" => {
            (ApprovalKind::FileChange, "apply_patch", None)
        }
        "item/permissions/requestApproval" => (
            ApprovalKind::Permissions,
            "permissions",
            Some(Escalation::PermissionChange),
        ),
        "tool/requestUserInput" | "mcpServer/elicitation/request" => {
            return vec![encode_jsonrpc_error(
                &rpc_id,
                -32601,
                "headless office declines interactive user-input requests",
            )];
        }
        other => {
            return vec![
                encode_jsonrpc_error(&rpc_id, -32601, "unsupported server request method"),
                Output::Event(SessionEvent::Unknown {
                    type_name: other.to_owned(),
                }),
            ];
        }
    };
    if matches!(s.phase, Phase::Closing | Phase::Failed | Phase::Ended) {
        return vec![encode_approval_result(
            &rpc_id,
            json!({ "decision": "cancel" }),
        )];
    }
    let id_str = rpc_id_key(&rpc_id);
    let correlated_changes = if kind == ApprovalKind::FileChange {
        p.get("itemId")
            .or_else(|| p.get("callId"))
            .and_then(Value::as_str)
            .and_then(|item_id| s.pending_file_changes.get(item_id))
    } else {
        None
    };
    let req = build_codex_approval(id_str, kind, tool, forced_escalation, p, correlated_changes);
    s.pending_approvals.insert(req.id.clone(), rpc_id);
    vec![
        Output::Event(SessionEvent::Status(Activity::WaitingOnApproval)),
        Output::Event(SessionEvent::ApprovalRequested(req)),
    ]
}

pub(super) fn handle_notification(
    s: &mut CodexSession,
    method: &str,
    params: Option<&Value>,
) -> Vec<Output> {
    let p = params.unwrap_or(&Value::Null);
    match method {
        "item/agentMessage/delta" => p
            .get("delta")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(|t| {
                vec![Output::Event(SessionEvent::TextDelta {
                    text: t.to_owned(),
                })]
            })
            .unwrap_or_default(),
        "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
            vec![Output::Event(SessionEvent::Thinking)]
        }
        "item/started" => {
            handle_item_started(p, &mut s.pending_file_changes, &mut s.active_subagents)
        }
        "item/completed" => {
            handle_item_completed(p, &mut s.pending_file_changes, &mut s.active_subagents)
        }
        "thread/tokenUsage/updated" => {
            record_token_usage(s, p);
            Vec::new()
        }
        "account/rateLimits/updated" => handle_rate_limits(p),
        "serverRequest/resolved" => handle_approval_resolved(s, p),
        "turn/started" => {
            if let Some(tid) = p.pointer("/turn/id").and_then(Value::as_str) {
                s.active_turn_id = Some(tid.to_owned());
            }
            Vec::new()
        }
        "turn/completed" => handle_turn_completed(s, p),
        other => vec![Output::Event(SessionEvent::Unknown {
            type_name: other.to_owned(),
        })],
    }
}

fn record_token_usage(s: &mut CodexSession, p: &Value) {
    let total_obj = p
        .pointer("/tokenUsage/total")
        .or_else(|| p.get("total"))
        .and_then(Value::as_object);
    let last_obj = p
        .pointer("/tokenUsage/last")
        .or_else(|| p.get("last"))
        .or_else(|| p.get("usage"))
        .and_then(Value::as_object);

    if !s.turn_open {
        // Record cumulative baseline during idle/resume without emitting phantom turn deltas.
        if let Some(tot) = total_obj {
            s.last_total_tokens = Some(parse_codex_tokens(tot));
        }
        return;
    }

    let delta = if let Some(last) = last_obj {
        let parsed = parse_codex_tokens(last);
        if let Some(tot) = total_obj {
            s.last_total_tokens = Some(parse_codex_tokens(tot));
        }
        parsed
    } else if let Some(tot) = total_obj {
        let current = parse_codex_tokens(tot);
        let d = match &s.last_total_tokens {
            Some(prev) => current.saturating_sub(prev),
            None => current.clone(),
        };
        s.last_total_tokens = Some(current);
        d
    } else {
        return;
    };

    match &mut s.turn_tokens {
        Some(acc) => acc.add_assign(&delta),
        None => s.turn_tokens = Some(delta),
    }
}

fn handle_approval_resolved(s: &mut CodexSession, p: &Value) -> Vec<Output> {
    let Some(raw) = p.get("requestId") else {
        return Vec::new();
    };
    let id = ApprovalId::new(rpc_id_key(raw));
    if s.pending_approvals.remove(&id).is_none() {
        return Vec::new();
    }
    let mut out = vec![Output::Event(SessionEvent::ApprovalCancelled { id })];
    if s.pending_approvals.is_empty() {
        match s.phase {
            Phase::Busy => out.push(Output::Event(SessionEvent::Status(Activity::Busy))),
            Phase::Idle => out.push(Output::Event(SessionEvent::Status(Activity::Idle))),
            _ => {}
        }
    }
    out
}

fn handle_turn_completed(s: &mut CodexSession, p: &Value) -> Vec<Output> {
    let mut out = Vec::new();
    if let Some(u) = s.flush_turn_usage() {
        out.push(u);
    }
    let status = p
        .pointer("/turn/status")
        .or_else(|| p.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let (outcome, errors) = match status {
        "completed" => (TurnOutcome::Completed, Vec::new()),
        "interrupted" => (TurnOutcome::Interrupted, Vec::new()),
        "failed" => {
            let reason = p
                .pointer("/turn/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Codex turn failed")
                .to_owned();
            (
                TurnOutcome::Failed {
                    reason: reason.clone(),
                },
                vec![reason],
            )
        }
        _ => (TurnOutcome::Unknown, Vec::new()),
    };
    s.active_turn_id = None;
    s.turn_start_req_id = None;
    s.pending_file_changes.clear();
    out.extend(s.fail_in_flight_deliveries("turn ended before steer acknowledgement"));
    if s.turn_open {
        s.turn_open = false;
        out.push(Output::Event(SessionEvent::TurnEnded { outcome, errors }));
    }
    if s.phase == Phase::Busy {
        s.phase = Phase::Idle;
        let activity = if s.pending_approvals.is_empty() {
            Activity::Idle
        } else {
            Activity::WaitingOnApproval
        };
        out.push(Output::Event(SessionEvent::Status(activity)));
    }
    out
}
