use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use org_agent_runtime::{
    encode_line, Activity, ApprovalId, ApprovalKind, Control, ControlError, CostBasis, Decision,
    Escalation, ExitInfo, NoticeKind, Output, Session, SessionEvent, SessionId, StartFailure,
    TurnOutcome, Usage, UsageScope,
};

use crate::plan::CodexThreadConfig;
use crate::protocol::{
    build_codex_approval, encode_approval_result, encode_jsonrpc_error, extract_sandbox_mode,
    handle_item_completed, handle_item_started, handle_rate_limits, parse_codex_tokens, rpc_id_key,
    sandbox_mode_matches, CodexTokens,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    OpeningInit,
    OpeningThread,
    Idle,
    Busy,
    Failed,
    Closing,
    Ended,
}

/// Pure sans-I/O JSON-RPC 2.0 protocol state machine for `codex app-server --stdio`.
pub(crate) struct CodexSession {
    cfg: CodexThreadConfig,
    phase: Phase,
    turn_open: bool,
    next_id: u64,
    init_req_id: Option<u64>,
    thread_req_id: Option<u64>,
    turn_start_req_id: Option<u64>,
    handshake_deadline: Option<Instant>,
    thread_id: Option<SessionId>,
    active_turn_id: Option<String>,
    runtime_version: Option<String>,
    pending_approvals: BTreeMap<ApprovalId, Value>,
    pending_file_changes: BTreeMap<String, Value>,
    active_subagents: BTreeSet<String>,
    turn_tokens: Option<CodexTokens>,
    last_total_tokens: Option<CodexTokens>,
    /// Tracks in-flight `turn/steer` JSON-RPC request ids -> office `delivery_id` (`doc-101`).
    in_flight_deliveries: BTreeMap<u64, String>,
}

impl CodexSession {
    pub(crate) fn new(cfg: CodexThreadConfig) -> Self {
        Self {
            cfg,
            phase: Phase::OpeningInit,
            turn_open: false,
            next_id: 1,
            init_req_id: None,
            thread_req_id: None,
            turn_start_req_id: None,
            handshake_deadline: None,
            thread_id: None,
            active_turn_id: None,
            runtime_version: None,
            pending_approvals: BTreeMap::new(),
            pending_file_changes: BTreeMap::new(),
            active_subagents: BTreeSet::new(),
            turn_tokens: None,
            last_total_tokens: None,
            in_flight_deliveries: BTreeMap::new(),
        }
    }

    fn fail_in_flight_deliveries(&mut self, reason: &str) -> Vec<Output> {
        std::mem::take(&mut self.in_flight_deliveries)
            .into_values()
            .map(|delivery_id| {
                Output::Event(SessionEvent::MidTurnDeliveryFailed {
                    delivery_id,
                    reason: reason.to_owned(),
                })
            })
            .collect()
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn busy_status(&self) -> Activity {
        if self.pending_approvals.is_empty() {
            Activity::Busy
        } else {
            Activity::WaitingOnApproval
        }
    }

    fn cancel_pending_approvals(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.pending_approvals)
            .into_keys()
            .map(|id| Output::Event(SessionEvent::ApprovalCancelled { id }))
            .collect()
    }

    fn flush_turn_usage(&mut self) -> Option<Output> {
        let tokens = self.turn_tokens.take()?;
        let mut usage = Usage::new(UsageScope::TurnDelta, UsageScope::TurnDelta);
        usage.cost_basis = CostBasis::None;
        usage.input = tokens.input;
        usage.output = tokens.output;
        usage.cache_read = tokens.cache_read;
        usage.cache_write = tokens.cache_write;
        usage.reasoning = tokens.reasoning;
        usage.model = self.cfg.model.clone();
        Some(Output::Event(SessionEvent::Usage(usage)))
    }
}

fn handle_frame(s: &mut CodexSession, val: Value, now: Instant) -> Vec<Output> {
    if let Some(method) = val.get("method").and_then(Value::as_str) {
        if let Some(rpc_id) = val.get("id") {
            return handle_server_request(s, rpc_id.clone(), method, val.get("params"));
        }
        return handle_notification(s, method, val.get("params"));
    }
    if let Some(id) = val.get("id").and_then(Value::as_u64) {
        return handle_rpc_response(s, id, &val, now);
    }
    vec![Output::Event(SessionEvent::ProtocolError {
        detail: "Codex frame missing `method` and numeric `id`".to_owned(),
    })]
}

fn handle_rpc_response(s: &mut CodexSession, id: u64, val: &Value, now: Instant) -> Vec<Output> {
    if s.phase == Phase::OpeningInit && s.init_req_id == Some(id) {
        return handle_init_response(s, val, now);
    }
    if s.phase == Phase::OpeningThread && s.thread_req_id == Some(id) {
        return handle_thread_response(s, val);
    }
    if let Some(delivery_id) = s.in_flight_deliveries.remove(&id) {
        if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
            let reason = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Codex turn/steer RPC error")
                .to_owned();
            return vec![Output::Event(SessionEvent::MidTurnDeliveryFailed {
                delivery_id,
                reason,
            })];
        }
        return vec![Output::Event(SessionEvent::MidTurnDelivered {
            delivery_id,
        })];
    }
    if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex RPC error")
            .to_owned();
        if s.turn_start_req_id == Some(id) {
            s.turn_start_req_id = None;
            s.active_turn_id = None;
            s.turn_tokens = None;
            let mut out = s.fail_in_flight_deliveries("turn failed to start");
            if s.turn_open {
                s.turn_open = false;
                out.push(Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Failed {
                        reason: msg.clone(),
                    },
                    errors: vec![msg],
                }));
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
            return out;
        }
        return vec![Output::Event(SessionEvent::Notice {
            kind: NoticeKind::Warning,
            text: msg,
        })];
    }
    if s.turn_start_req_id == Some(id) {
        s.turn_start_req_id = None;
    }
    if let Some(turn_id) = val.pointer("/result/turn/id").and_then(Value::as_str) {
        s.active_turn_id = Some(turn_id.to_owned());
    }
    Vec::new()
}

fn handle_init_response(s: &mut CodexSession, val: &Value, now: Instant) -> Vec<Output> {
    if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
        s.phase = Phase::Failed;
        s.handshake_deadline = None;
        let detail = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex initialize failed")
            .to_owned();
        return vec![Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::Other { detail },
        })];
    }
    s.runtime_version = val
        .pointer("/result/serverInfo/version")
        .or_else(|| val.pointer("/result/version"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let initialized_notif = encode_line(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }))
    .expect("initialized encodes");

    let thread_req_id = s.alloc_id();
    s.thread_req_id = Some(thread_req_id);
    s.phase = Phase::OpeningThread;
    s.handshake_deadline = Some(now + HANDSHAKE_TIMEOUT);

    let thread_line = encode_thread_open(&s.cfg, thread_req_id);
    vec![Output::Write(initialized_notif), Output::Write(thread_line)]
}

fn granular_approval_policy_json() -> Value {
    json!({
        "granular": {
            "sandbox_approval": false,
            "rules": true,
            "mcp_elicitations": false,
            "request_permissions": false,
            "skill_approval": false
        }
    })
}

fn encode_thread_open(cfg: &CodexThreadConfig, req_id: u64) -> org_agent_runtime::Line {
    let mut params = json!({
        "cwd": cfg.cwd,
        "approvalPolicy": granular_approval_policy_json(),
        "approvalsReviewer": "user",
    });
    // Select the named `permissions.office` profile on start/resume when sandbox is enabled so
    // Codex never restores a persisted profile (e.g. `:read-only`) over `office`, and never
    // combine it with legacy `sandbox`/`writableRoots` which override the profile (`doc-29` §5).
    if cfg.uses_office_profile {
        params["permissions"] = Value::String("office".to_owned());
    } else {
        params["sandbox"] = Value::String(cfg.sandbox_mode.to_owned());
        params["writableRoots"] = json!(cfg.writable_roots);
    }
    if let Some(model) = &cfg.model {
        params["model"] = Value::String(model.clone());
    }
    if let Some(effort) = &cfg.effort {
        params["reasoningEffort"] = Value::String(effort.clone());
    }
    if !cfg.instructions.is_empty() {
        params["developerInstructions"] = Value::String(cfg.instructions.clone());
    }
    let method = if let Some(resume) = &cfg.resume_thread_id {
        params["threadId"] = Value::String(resume.as_str().to_owned());
        "thread/resume"
    } else {
        "thread/start"
    };
    encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params
    }))
    .expect("thread open encodes")
}

fn approval_policy_matches_office_granular(policy: Option<&Value>) -> bool {
    let Some(granular) = policy
        .and_then(|v| v.get("granular"))
        .and_then(Value::as_object)
    else {
        return false;
    };
    granular.get("sandbox_approval").and_then(Value::as_bool) == Some(false)
        && granular.get("rules").and_then(Value::as_bool) == Some(true)
        && granular.get("mcp_elicitations").and_then(Value::as_bool) == Some(false)
        && granular.get("request_permissions").and_then(Value::as_bool) == Some(false)
        && matches!(
            granular.get("skill_approval"),
            None | Some(Value::Bool(false))
        )
}

fn format_reported_policy(policy: Option<&Value>) -> String {
    match policy {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

fn validate_thread_modes(cfg: &CodexThreadConfig, val: &Value) -> Result<String, StartFailure> {
    if let Some(reported_sb) = extract_sandbox_mode(val) {
        let lower = reported_sb.to_ascii_lowercase();
        if lower == "danger-full-access" || lower == "dangerfullaccess" {
            return Err(StartFailure::ModeMismatch {
                requested: if cfg.uses_office_profile {
                    "office".to_owned()
                } else {
                    cfg.sandbox_mode.to_owned()
                },
                reported: reported_sb.to_owned(),
            });
        }
    }
    if cfg.uses_office_profile {
        let profile_id = val
            .pointer("/result/activePermissionProfile/id")
            .or_else(|| val.pointer("/result/thread/activePermissionProfile/id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if profile_id != "office" {
            return Err(StartFailure::ModeMismatch {
                requested: "office".to_owned(),
                reported: profile_id.to_owned(),
            });
        }
    } else {
        let reported_sb = extract_sandbox_mode(val).unwrap_or("");
        if reported_sb.is_empty() || !sandbox_mode_matches(cfg.sandbox_mode, reported_sb) {
            return Err(StartFailure::ModeMismatch {
                requested: cfg.sandbox_mode.to_owned(),
                reported: reported_sb.to_owned(),
            });
        }
    }
    let reported_policy = val
        .pointer("/result/approvalPolicy")
        .or_else(|| val.pointer("/result/thread/approvalPolicy"));
    if !approval_policy_matches_office_granular(reported_policy) {
        return Err(StartFailure::ModeMismatch {
            requested: "granular(sandbox_approval=false,rules=true)".to_owned(),
            reported: format_reported_policy(reported_policy),
        });
    }
    Ok("granular:rules".to_owned())
}

fn handle_thread_response(s: &mut CodexSession, val: &Value) -> Vec<Output> {
    s.handshake_deadline = None;
    if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
        s.phase = Phase::Failed;
        let detail = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex thread start failed")
            .to_owned();
        let reason = if s.cfg.resume_thread_id.is_some() {
            StartFailure::ResumeFailed { detail }
        } else {
            StartFailure::Other { detail }
        };
        return vec![Output::Event(SessionEvent::StartFailed { reason })];
    }
    let raw_tid = val
        .pointer("/result/thread/id")
        .or_else(|| val.pointer("/result/threadId"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let Ok(tid) = SessionId::try_from(raw_tid) else {
        s.phase = Phase::Failed;
        return vec![
            Output::Event(SessionEvent::ProtocolError {
                detail: "Codex thread/start returned an invalid thread id".to_owned(),
            }),
            Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::Other {
                    detail: "invalid thread id from Codex".to_owned(),
                },
            }),
        ];
    };
    let policy = match validate_thread_modes(&s.cfg, val) {
        Ok(p) => p,
        Err(reason) => {
            s.phase = Phase::Failed;
            return vec![Output::Event(SessionEvent::StartFailed { reason })];
        }
    };
    s.thread_id = Some(tid.clone());
    s.phase = Phase::Idle;
    vec![
        Output::Event(SessionEvent::SessionStarted {
            runtime_session_id: tid,
        }),
        Output::Event(SessionEvent::Ready {
            runtime_version: s.runtime_version.clone(),
            pid: None,
            agents: Vec::new(),
            permission_mode: Some(policy),
        }),
    ]
}

fn handle_server_request(
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

fn handle_notification(s: &mut CodexSession, method: &str, params: Option<&Value>) -> Vec<Output> {
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

fn handle_send(s: &mut CodexSession, text: String) -> Result<Vec<Output>, ControlError> {
    match s.phase {
        Phase::OpeningInit | Phase::OpeningThread => Err(ControlError::NotReady),
        Phase::Busy => Err(ControlError::Busy),
        Phase::Idle => {
            let tid = s
                .thread_id
                .as_ref()
                .ok_or(ControlError::NotReady)?
                .as_str()
                .to_owned();
            let req_id = s.alloc_id();
            let mut params = json!({
                "threadId": tid,
                "approvalPolicy": granular_approval_policy_json(),
                "approvalsReviewer": "user",
                "input": [{ "type": "text", "text": text }]
            });
            if s.cfg.uses_office_profile {
                params["permissions"] = Value::String("office".to_owned());
            }
            let line = encode_line(&json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "method": "turn/start",
                "params": params
            }))
            .expect("turn/start encodes");
            s.phase = Phase::Busy;
            s.turn_open = true;
            s.turn_start_req_id = Some(req_id);
            s.turn_tokens = None;
            Ok(vec![
                Output::Write(line),
                Output::Event(SessionEvent::TurnStarted {
                    turn_id: None,
                    solicited: true,
                }),
                Output::Event(SessionEvent::Status(s.busy_status())),
            ])
        }
        _ => Err(ControlError::Closed),
    }
}

fn handle_deliver_mid_turn(
    s: &mut CodexSession,
    delivery_id: String,
    text: String,
) -> Result<Vec<Output>, ControlError> {
    match s.phase {
        Phase::OpeningInit | Phase::OpeningThread => Err(ControlError::NotReady),
        Phase::Idle => Err(ControlError::InvalidState),
        Phase::Busy => {
            let tid = s
                .thread_id
                .as_ref()
                .ok_or(ControlError::NotReady)?
                .as_str()
                .to_owned();
            let turn_id = s
                .active_turn_id
                .as_ref()
                .ok_or(ControlError::NotReady)?
                .clone();
            let req_id = s.alloc_id();
            let line = encode_line(&json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "method": "turn/steer",
                "params": {
                    "threadId": tid,
                    "expectedTurnId": turn_id,
                    "input": [{ "type": "text", "text": text }]
                }
            }))
            .expect("turn/steer encodes");
            s.in_flight_deliveries.insert(req_id, delivery_id);
            Ok(vec![Output::Write(line)])
        }
        _ => Err(ControlError::Closed),
    }
}

fn handle_interrupt(s: &mut CodexSession) -> Result<Vec<Output>, ControlError> {
    match s.phase {
        Phase::OpeningInit | Phase::OpeningThread => Err(ControlError::NotReady),
        Phase::Idle => Ok(Vec::new()),
        Phase::Busy => {
            let tid = s
                .thread_id
                .as_ref()
                .ok_or(ControlError::NotReady)?
                .as_str()
                .to_owned();
            let req_id = s.alloc_id();
            let mut params = json!({ "threadId": tid });
            if let Some(turn_id) = &s.active_turn_id {
                params["turnId"] = Value::String(turn_id.clone());
            }
            let line = encode_line(&json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "method": "turn/interrupt",
                "params": params
            }))
            .expect("turn/interrupt encodes");
            Ok(vec![Output::Write(line)])
        }
        _ => Err(ControlError::Closed),
    }
}

fn handle_answer(
    s: &mut CodexSession,
    request_id: ApprovalId,
    decision: Decision,
) -> Result<Vec<Output>, ControlError> {
    let Some(rpc_id) = s.pending_approvals.get(&request_id).cloned() else {
        return Err(ControlError::UnknownApproval);
    };
    let result_val = match decision {
        Decision::AllowOnce => json!({ "decision": "accept" }),
        Decision::Deny { message, stop_turn } => {
            let mut obj = if stop_turn {
                json!({ "decision": "cancel" })
            } else {
                json!({ "decision": "decline" })
            };
            if let Some(m) = message {
                obj["reason"] = Value::String(m);
            }
            obj
        }
        _ => return Err(ControlError::Unsupported),
    };
    s.pending_approvals.remove(&request_id);
    let mut out = vec![encode_approval_result(&rpc_id, result_val)];
    if s.pending_approvals.is_empty() {
        let activity = if s.phase == Phase::Busy {
            Activity::Busy
        } else {
            Activity::Idle
        };
        out.push(Output::Event(SessionEvent::Status(activity)));
    }
    Ok(out)
}

impl Session for CodexSession {
    fn start(&mut self, now: Instant) -> Vec<Output> {
        if self.phase != Phase::OpeningInit || self.init_req_id.is_some() {
            return Vec::new();
        }
        let req_id = self.alloc_id();
        self.init_req_id = Some(req_id);
        self.handshake_deadline = Some(now + HANDSHAKE_TIMEOUT);
        let line = encode_line(&json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "agentic-org",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true
                }
            }
        }))
        .expect("initialize encodes");
        vec![Output::Write(line)]
    }

    fn on_line(&mut self, line: &[u8], now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        let Ok(text) = std::str::from_utf8(line) else {
            return vec![Output::Event(SessionEvent::ProtocolError {
                detail: "Codex stdout line was not valid UTF-8".to_owned(),
            })];
        };
        if text.trim().is_empty() {
            return Vec::new();
        }
        match serde_json::from_str::<Value>(text) {
            Ok(val) => handle_frame(self, val, now),
            Err(err) => vec![Output::Event(SessionEvent::ProtocolError {
                detail: format!("invalid JSON line from Codex: {err}"),
            })],
        }
    }

    fn on_stderr_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(line).trim().to_owned();
        if text.is_empty() {
            return Vec::new();
        }
        vec![Output::Event(SessionEvent::Notice {
            kind: NoticeKind::Warning,
            text,
        })]
    }

    fn on_truncated(&mut self, len: usize, _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        vec![Output::Event(SessionEvent::Truncated { len })]
    }

    fn on_stdout_closed(&mut self, _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        self.handshake_deadline = None;
        let mut out = self.cancel_pending_approvals();
        match self.phase {
            Phase::OpeningInit | Phase::OpeningThread => {
                let is_resume =
                    self.phase == Phase::OpeningThread && self.cfg.resume_thread_id.is_some();
                self.phase = Phase::Failed;
                let detail = "Codex stdout closed before ready".to_owned();
                let reason = if is_resume {
                    StartFailure::ResumeFailed { detail }
                } else {
                    StartFailure::Other { detail }
                };
                out.push(Output::Event(SessionEvent::StartFailed { reason }));
            }
            Phase::Busy => {
                out.extend(self.fail_in_flight_deliveries("Codex stdout closed before steer ack"));
                if let Some(u) = self.flush_turn_usage() {
                    out.push(u);
                }
                self.phase = Phase::Idle;
                self.turn_open = false;
                self.active_turn_id = None;
                self.turn_start_req_id = None;
                out.push(Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Failed {
                        reason: "Codex stdout closed during turn".to_owned(),
                    },
                    errors: Vec::new(),
                }));
                out.push(Output::Event(SessionEvent::Status(Activity::Idle)));
            }
            Phase::Closing => {
                out.extend(self.fail_in_flight_deliveries("Codex stdout closed before steer ack"));
                if let Some(u) = self.flush_turn_usage() {
                    out.push(u);
                }
                if self.turn_open {
                    self.turn_open = false;
                    self.active_turn_id = None;
                    out.push(Output::Event(SessionEvent::TurnEnded {
                        outcome: TurnOutcome::Interrupted,
                        errors: Vec::new(),
                    }));
                }
            }
            Phase::Idle | Phase::Failed | Phase::Ended => {}
        }
        out
    }

    fn on_exit(&mut self, exit: ExitInfo, _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        let prev = self.phase;
        self.phase = Phase::Ended;
        self.handshake_deadline = None;
        let mut out = self.cancel_pending_approvals();
        match prev {
            Phase::OpeningInit | Phase::OpeningThread => {
                let is_resume = prev == Phase::OpeningThread && self.cfg.resume_thread_id.is_some();
                let detail = format!("Codex exited before ready ({exit:?})");
                let reason = if is_resume {
                    StartFailure::ResumeFailed { detail }
                } else {
                    StartFailure::Other { detail }
                };
                out.push(Output::Event(SessionEvent::StartFailed { reason }));
            }
            Phase::Busy => {
                out.extend(self.fail_in_flight_deliveries("Codex exited before steer ack"));
                if let Some(u) = self.flush_turn_usage() {
                    out.push(u);
                }
                self.turn_open = false;
                self.active_turn_id = None;
                out.push(Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Failed {
                        reason: "Codex exited during turn".to_owned(),
                    },
                    errors: Vec::new(),
                }));
            }
            Phase::Closing => {
                out.extend(self.fail_in_flight_deliveries("Codex exited before steer ack"));
                if let Some(u) = self.flush_turn_usage() {
                    out.push(u);
                }
                if self.turn_open {
                    self.turn_open = false;
                    self.active_turn_id = None;
                    out.push(Output::Event(SessionEvent::TurnEnded {
                        outcome: TurnOutcome::Interrupted,
                        errors: Vec::new(),
                    }));
                }
            }
            Phase::Idle | Phase::Failed | Phase::Ended => {}
        }
        out.push(Output::Event(SessionEvent::Ended { exit }));
        out
    }

    fn control(&mut self, cmd: Control, _now: Instant) -> Result<Vec<Output>, ControlError> {
        if matches!(self.phase, Phase::Ended | Phase::Closing | Phase::Failed) {
            return Err(ControlError::Closed);
        }
        match cmd {
            Control::Send { text } => handle_send(self, text),
            Control::DeliverMidTurn { delivery_id, text } => {
                handle_deliver_mid_turn(self, delivery_id, text)
            }
            Control::Interrupt => handle_interrupt(self),
            Control::Answer {
                request_id,
                decision,
            } => handle_answer(self, request_id, decision),
            Control::Close => {
                self.phase = Phase::Closing;
                self.handshake_deadline = None;
                Ok(vec![Output::CloseStdin])
            }
            _ => Err(ControlError::Unsupported),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        if matches!(self.phase, Phase::OpeningInit | Phase::OpeningThread) {
            self.handshake_deadline
        } else {
            None
        }
    }

    fn on_tick(&mut self, now: Instant) -> Vec<Output> {
        if matches!(self.phase, Phase::OpeningInit | Phase::OpeningThread)
            && self.handshake_deadline.is_some_and(|d| now >= d)
        {
            self.handshake_deadline = None;
            self.phase = Phase::Failed;
            return vec![Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ProtocolTimeout,
            })];
        }
        Vec::new()
    }
}
