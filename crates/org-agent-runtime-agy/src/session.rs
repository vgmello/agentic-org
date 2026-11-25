use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::Value;

use org_agent_runtime::{
    Activity, ApprovalId, Control, ControlError, CostBasis, Decision, ExitInfo, Output, Session,
    SessionEvent, SessionId, StartFailure, TurnOutcome, Usage, UsageScope,
};

use crate::plan::AgySessionConfig;
use crate::protocol::{
    classify_rpc_error_outcome, classify_stop_reason, classify_tool_capability, encode_initialize,
    encode_jsonrpc_error, encode_permission_cancelled, encode_permission_selected,
    encode_session_cancel, encode_session_close, encode_session_open, encode_session_prompt,
    encode_set_model, extract_rpc_error_message, parse_acp_usage, parse_permission_request,
    parse_stderr_line, rpc_id_key, AcpUsage,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    OpeningInit,
    OpeningSession,
    OpeningModel,
    Idle,
    Busy,
    Failed,
    Closing,
    Ended,
}

#[derive(Debug, Clone)]
struct PendingApproval {
    raw_id: Value,
    allow_option_id: String,
    deny_option_id: String,
    tool_call_id: Option<String>,
}

/// Pure sans-I/O ACP JSON-RPC 2.0 protocol state machine for `agy acp --stdio`.
pub(crate) struct AgySession {
    cfg: AgySessionConfig,
    phase: Phase,
    turn_open: bool,
    next_id: u64,
    init_req_id: Option<u64>,
    session_req_id: Option<u64>,
    model_req_id: Option<u64>,
    handshake_deadline: Option<Instant>,
    session_id: Option<SessionId>,
    runtime_version: Option<String>,
    active_model: Option<String>,
    /// Current owning `session/prompt` RPC id for the open turn.
    active_prompt_id: Option<u64>,
    /// Prior `session/prompt` RPC ids superseded by mid-turn steering (`old_id -> delivery_id`).
    /// On live `agy 1.5.17`, sending a mid-turn `session/prompt` resolves the prior prompt id
    /// immediately without aborting running tools and transfers turn ownership to the new id.
    superseded_prompts: BTreeMap<u64, String>,
    /// New mid-turn `session/prompt` RPC ids mapped to their unacknowledged `delivery_id`.
    steer_prompt_ids: BTreeMap<u64, String>,
    pending_approvals: BTreeMap<ApprovalId, PendingApproval>,
    turn_text_buf: String,
    turn_usage: AcpUsage,
    saw_turn_usage: bool,
}

impl AgySession {
    pub(crate) fn new(cfg: AgySessionConfig) -> Self {
        Self {
            cfg,
            phase: Phase::OpeningInit,
            turn_open: false,
            next_id: 1,
            init_req_id: None,
            session_req_id: None,
            model_req_id: None,
            handshake_deadline: None,
            session_id: None,
            runtime_version: None,
            active_model: None,
            active_prompt_id: None,
            superseded_prompts: BTreeMap::new(),
            steer_prompt_ids: BTreeMap::new(),
            pending_approvals: BTreeMap::new(),
            turn_text_buf: String::new(),
            turn_usage: AcpUsage::default(),
            saw_turn_usage: false,
        }
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

    fn fail_pending_mid_turn_deliveries(&mut self, reason: &str) -> Vec<Output> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for delivery_id in std::mem::take(&mut self.superseded_prompts)
            .into_values()
            .chain(std::mem::take(&mut self.steer_prompt_ids).into_values())
        {
            if seen.insert(delivery_id.clone()) {
                out.push(Output::Event(SessionEvent::MidTurnDeliveryFailed {
                    delivery_id,
                    reason: reason.to_owned(),
                }));
            }
        }
        out
    }

    fn flush_assistant_text(&mut self) -> Option<Output> {
        if self.turn_text_buf.is_empty() {
            return None;
        }
        let text = std::mem::take(&mut self.turn_text_buf);
        Some(Output::Event(SessionEvent::AssistantText {
            message_id: None,
            text,
        }))
    }

    fn flush_turn_usage(&mut self) -> Option<Output> {
        if !self.saw_turn_usage && self.turn_usage.is_zero() {
            return None;
        }
        let raw = std::mem::take(&mut self.turn_usage);
        self.saw_turn_usage = false;
        let mut u = Usage::new(UsageScope::TurnDelta, UsageScope::TurnDelta);
        u.input = raw.input;
        u.output = raw.output;
        u.cache_read = raw.cache_read;
        u.cache_write = raw.cache_write;
        u.reasoning = raw.reasoning;
        u.cost_basis = CostBasis::None;
        u.model = self.active_model.clone().or_else(|| self.cfg.model.clone());
        Some(Output::Event(SessionEvent::Usage(u)))
    }
}

fn is_opening(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::OpeningInit | Phase::OpeningSession | Phase::OpeningModel
    )
}

fn finish_opening_ready(s: &mut AgySession) -> Vec<Output> {
    s.handshake_deadline = None;
    s.phase = Phase::Idle;
    let mut out = Vec::new();
    if let Some(sid) = s.session_id.clone() {
        out.push(Output::Event(SessionEvent::SessionStarted {
            runtime_session_id: sid,
        }));
    }
    out.push(Output::Event(SessionEvent::Ready {
        runtime_version: s.runtime_version.clone(),
        pid: None,
        agents: vec![s.cfg.agent_name.clone()],
        permission_mode: Some(s.cfg.expected_mode.reported_name.to_owned()),
    }));
    out
}

fn fail_opening(s: &mut AgySession, detail: String) -> Vec<Output> {
    s.handshake_deadline = None;
    s.phase = Phase::Failed;
    let reason = if s.cfg.resume_session_id.is_some() {
        StartFailure::ResumeFailed { detail }
    } else {
        StartFailure::Other { detail }
    };
    vec![Output::Event(SessionEvent::StartFailed { reason })]
}

fn handle_frame(s: &mut AgySession, val: Value) -> Vec<Output> {
    let has_method = val.get("method").and_then(Value::as_str).is_some();
    let has_id = val.get("id").is_some_and(|v| !v.is_null());
    match (has_method, has_id) {
        (true, true) => handle_server_request(s, &val),
        (true, false) => handle_notification(s, &val),
        (false, true) => handle_rpc_response(s, &val),
        (false, false) => vec![Output::Event(SessionEvent::ProtocolError {
            detail: "ACP JSON-RPC frame missing both `method` and `id`".to_owned(),
        })],
    }
}

fn handle_rpc_response(s: &mut AgySession, val: &Value) -> Vec<Output> {
    let Some(req_id) = val.get("id").and_then(Value::as_u64) else {
        return Vec::new();
    };
    if s.init_req_id == Some(req_id) {
        return handle_init_response(s, val);
    }
    if s.session_req_id == Some(req_id) {
        return handle_session_open_response(s, val);
    }
    if s.model_req_id == Some(req_id) {
        return handle_set_model_response(s, val);
    }
    if s.superseded_prompts.contains_key(&req_id) {
        return handle_superseded_prompt_response(s, req_id, val);
    }
    if s.active_prompt_id == Some(req_id) {
        return handle_active_prompt_response(s, req_id, val);
    }
    Vec::new()
}

fn handle_init_response(s: &mut AgySession, val: &Value) -> Vec<Output> {
    s.init_req_id = None;
    if s.phase != Phase::OpeningInit {
        return Vec::new();
    }
    if let Some(err) = val.get("error") {
        return fail_opening(
            s,
            format!("ACP initialize failed: {}", extract_rpc_error_message(err)),
        );
    }
    let Some(result) = val.get("result") else {
        return fail_opening(s, "ACP initialize response missing `result`".to_owned());
    };
    let proto_ver = result
        .get("protocolVersion")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if proto_ver < 1 {
        return fail_opening(
            s,
            format!("ACP initialize reported unsupported protocolVersion={proto_ver}"),
        );
    }
    s.runtime_version = result
        .pointer("/agentInfo/version")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let open_id = s.alloc_id();
    s.session_req_id = Some(open_id);
    s.phase = Phase::OpeningSession;
    vec![encode_session_open(
        open_id,
        &s.cfg.cwd,
        &s.cfg.mcp_servers,
        s.cfg.system_prompt_append.as_deref(),
        s.cfg.resume_session_id.as_ref().map(SessionId::as_str),
    )]
}

fn handle_session_open_response(s: &mut AgySession, val: &Value) -> Vec<Output> {
    s.session_req_id = None;
    if s.phase != Phase::OpeningSession {
        return Vec::new();
    }
    if let Some(err) = val.get("error") {
        return fail_opening(
            s,
            format!(
                "ACP session open failed: {}",
                extract_rpc_error_message(err)
            ),
        );
    }
    let Some(result) = val.get("result") else {
        return fail_opening(s, "ACP session open response missing `result`".to_owned());
    };
    let sid = match result
        .get("sessionId")
        .and_then(Value::as_str)
        .map(SessionId::try_from)
    {
        Some(Ok(id)) => id,
        Some(Err(_)) => {
            return fail_opening(s, "ACP session open carried invalid sessionId".to_owned());
        }
        None => match s.cfg.resume_session_id.clone() {
            Some(resumed) => resumed,
            None => {
                return fail_opening(s, "ACP session/new response missing sessionId".to_owned());
            }
        },
    };
    let current_model = result
        .pointer("/models/currentModelId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    s.active_model = current_model.clone();
    s.session_id = Some(sid.clone());

    if let Some(requested) = s.cfg.model.clone() {
        if current_model.as_deref() != Some(requested.as_str()) {
            let model_id = s.alloc_id();
            s.model_req_id = Some(model_id);
            s.phase = Phase::OpeningModel;
            return vec![encode_set_model(model_id, sid.as_str(), &requested)];
        }
    }
    finish_opening_ready(s)
}

fn handle_set_model_response(s: &mut AgySession, val: &Value) -> Vec<Output> {
    s.model_req_id = None;
    if s.phase != Phase::OpeningModel {
        return Vec::new();
    }
    if let Some(err) = val.get("error") {
        return fail_opening(
            s,
            format!(
                "ACP session/set_model failed: {}",
                extract_rpc_error_message(err)
            ),
        );
    }
    if let Some(requested) = &s.cfg.model {
        s.active_model = Some(requested.clone());
    }
    finish_opening_ready(s)
}

fn handle_superseded_prompt_response(s: &mut AgySession, req_id: u64, val: &Value) -> Vec<Output> {
    let Some(delivery_id) = s.superseded_prompts.remove(&req_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(result) = val.get("result") {
        if let Some(usage_val) = result.get("usage") {
            s.turn_usage.add_assign(&parse_acp_usage(usage_val));
            s.saw_turn_usage = true;
        }
        s.steer_prompt_ids.retain(|_, d| d != &delivery_id);
        if let Some(text) = s.flush_assistant_text() {
            out.push(text);
        }
        out.push(Output::Event(SessionEvent::MidTurnDelivered {
            delivery_id,
        }));
    }
    out
}

fn handle_active_prompt_response(s: &mut AgySession, req_id: u64, val: &Value) -> Vec<Output> {
    if let Some(err) = val.get("error") {
        return handle_active_prompt_error(s, req_id, err);
    }
    s.active_prompt_id = None;
    let mut out = Vec::new();
    if let Some(delivery_id) = s.steer_prompt_ids.remove(&req_id) {
        s.superseded_prompts.retain(|_, d| d != &delivery_id);
        out.push(Output::Event(SessionEvent::MidTurnDelivered {
            delivery_id,
        }));
    }
    out.extend(s.fail_pending_mid_turn_deliveries("turn ended before mid-turn prompt ack"));
    if let Some(text) = s.flush_assistant_text() {
        out.push(text);
    }
    let result = val.get("result").unwrap_or(&Value::Null);
    if let Some(usage_val) = result.get("usage") {
        s.turn_usage.add_assign(&parse_acp_usage(usage_val));
        s.saw_turn_usage = true;
    }
    if let Some(u) = s.flush_turn_usage() {
        out.push(u);
    }
    let outcome = classify_stop_reason(result.get("stopReason").and_then(Value::as_str));
    finish_turn(s, outcome, Vec::new(), &mut out);
    out
}

fn handle_active_prompt_error(s: &mut AgySession, req_id: u64, err: &Value) -> Vec<Output> {
    let mut out = Vec::new();
    if let Some(delivery_id) = s.steer_prompt_ids.remove(&req_id) {
        let reason = extract_rpc_error_message(err);
        let mut restored_prompt = None;
        s.superseded_prompts.retain(|&old_id, d| {
            if d == &delivery_id {
                restored_prompt = Some(old_id);
                false
            } else {
                true
            }
        });
        out.push(Output::Event(SessionEvent::MidTurnDeliveryFailed {
            delivery_id,
            reason,
        }));
        if let Some(old_id) = restored_prompt {
            // The steered prompt was rejected while the prior prompt is still running: restore
            // turn ownership to `old_id` so the running turn finishes cleanly.
            s.active_prompt_id = Some(old_id);
            return out;
        }
    }
    s.active_prompt_id = None;
    out.extend(s.fail_pending_mid_turn_deliveries("active turn failed"));
    if let Some(text) = s.flush_assistant_text() {
        out.push(text);
    }
    if let Some(u) = s.flush_turn_usage() {
        out.push(u);
    }
    let (outcome, errors) = classify_rpc_error_outcome(err);
    finish_turn(s, outcome, errors, &mut out);
    out
}

fn finish_turn(
    s: &mut AgySession,
    outcome: TurnOutcome,
    errors: Vec<String>,
    out: &mut Vec<Output>,
) {
    if s.turn_open {
        s.turn_open = false;
        out.push(Output::Event(SessionEvent::TurnEnded { outcome, errors }));
    }
    if s.phase == Phase::Busy {
        s.phase = Phase::Idle;
        let status = if s.pending_approvals.is_empty() {
            Activity::Idle
        } else {
            Activity::WaitingOnApproval
        };
        out.push(Output::Event(SessionEvent::Status(status)));
    }
}

fn handle_server_request(s: &mut AgySession, val: &Value) -> Vec<Output> {
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

fn handle_notification(s: &mut AgySession, val: &Value) -> Vec<Output> {
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

fn handle_cancel_request(s: &mut AgySession, params: &Value) -> Vec<Output> {
    let Some(key) = params.get("id").and_then(rpc_id_key) else {
        return Vec::new();
    };
    let id = ApprovalId::new(key);
    remove_cancelled_approval(s, &id)
}

fn remove_cancelled_approval(s: &mut AgySession, id: &ApprovalId) -> Vec<Output> {
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

fn handle_session_update(s: &mut AgySession, params: &Value) -> Vec<Output> {
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

fn handle_message_chunk(
    s: &mut AgySession,
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
    let name = update
        .get("name")
        .or_else(|| update.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let capability = classify_tool_capability(&name);
    let input = update
        .get("rawInput")
        .or_else(|| update.get("args"))
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
    s: &mut AgySession,
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

fn handle_usage_update(s: &mut AgySession, update: &serde_json::Map<String, Value>) -> Vec<Output> {
    if let Some(usage_val) = update.get("usage") {
        let parsed = parse_acp_usage(usage_val);
        if !parsed.is_zero() {
            s.turn_usage.add_assign(&parsed);
            s.saw_turn_usage = true;
        }
    }
    Vec::new()
}

fn control_send(s: &mut AgySession, text: String) -> Result<Vec<Output>, ControlError> {
    if is_opening(s.phase) {
        return Err(ControlError::NotReady);
    }
    if s.phase == Phase::Busy {
        return Err(ControlError::Busy);
    }
    let Some(sid) = s.session_id.clone() else {
        return Err(ControlError::NotReady);
    };
    let req_id = s.alloc_id();
    s.active_prompt_id = Some(req_id);
    s.phase = Phase::Busy;
    s.turn_open = true;
    s.turn_text_buf.clear();
    s.turn_usage = AcpUsage::default();
    s.saw_turn_usage = false;
    Ok(vec![
        encode_session_prompt(req_id, sid.as_str(), &text),
        Output::Event(SessionEvent::TurnStarted {
            turn_id: None,
            solicited: true,
        }),
        Output::Event(SessionEvent::Status(s.busy_status())),
    ])
}

fn control_deliver_mid_turn(
    s: &mut AgySession,
    delivery_id: String,
    text: String,
) -> Result<Vec<Output>, ControlError> {
    if is_opening(s.phase) {
        return Err(ControlError::NotReady);
    }
    if s.phase == Phase::Idle {
        return Err(ControlError::InvalidState);
    }
    let (Some(old_id), Some(sid)) = (s.active_prompt_id, s.session_id.clone()) else {
        return Err(ControlError::NotReady);
    };
    let new_id = s.alloc_id();
    s.superseded_prompts.insert(old_id, delivery_id.clone());
    s.steer_prompt_ids.insert(new_id, delivery_id);
    s.active_prompt_id = Some(new_id);
    Ok(vec![encode_session_prompt(new_id, sid.as_str(), &text)])
}

fn control_answer(
    s: &mut AgySession,
    request_id: ApprovalId,
    decision: Decision,
) -> Result<Vec<Output>, ControlError> {
    if !s.pending_approvals.contains_key(&request_id) {
        return Err(ControlError::UnknownApproval);
    }
    let (use_allow, stop_turn) = match decision {
        Decision::AllowOnce => (true, false),
        Decision::Deny { stop_turn, .. } => (false, stop_turn),
        _ => return Err(ControlError::Unsupported),
    };
    let pending = s
        .pending_approvals
        .remove(&request_id)
        .expect("verified present");
    let option_id = if use_allow {
        &pending.allow_option_id
    } else {
        &pending.deny_option_id
    };
    let mut out = vec![encode_permission_selected(&pending.raw_id, option_id)];
    if stop_turn {
        if let Some(sid) = &s.session_id {
            out.push(encode_session_cancel(sid.as_str()));
        }
    }
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

impl Session for AgySession {
    fn start(&mut self, now: Instant) -> Vec<Output> {
        if self.phase != Phase::OpeningInit || self.init_req_id.is_some() {
            return Vec::new();
        }
        let req_id = self.alloc_id();
        self.init_req_id = Some(req_id);
        self.handshake_deadline = Some(now + HANDSHAKE_TIMEOUT);
        vec![encode_initialize(req_id)]
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        let Ok(text) = std::str::from_utf8(line) else {
            return vec![Output::Event(SessionEvent::ProtocolError {
                detail: "Agy ACP stdout line was not valid UTF-8".to_owned(),
            })];
        };
        if text.trim().is_empty() {
            return Vec::new();
        }
        match serde_json::from_str::<Value>(text) {
            Ok(val) => handle_frame(self, val),
            Err(err) => vec![Output::Event(SessionEvent::ProtocolError {
                detail: format!("invalid JSON line from Agy ACP: {err}"),
            })],
        }
    }

    fn on_stderr_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(line);
        parse_stderr_line(&text).into_iter().collect()
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
        if is_opening(self.phase) {
            out.extend(fail_opening(
                self,
                "Agy ACP stdout closed before session ready".to_owned(),
            ));
            return out;
        }
        if self.phase == Phase::Busy || self.phase == Phase::Closing {
            out.extend(self.fail_pending_mid_turn_deliveries("Agy ACP stdout closed during turn"));
            if let Some(text) = self.flush_assistant_text() {
                out.push(text);
            }
            if let Some(u) = self.flush_turn_usage() {
                out.push(u);
            }
            let outcome = if self.phase == Phase::Closing {
                TurnOutcome::Interrupted
            } else {
                TurnOutcome::Failed {
                    reason: "Agy ACP stdout closed during turn".to_owned(),
                }
            };
            self.active_prompt_id = None;
            finish_turn(self, outcome, Vec::new(), &mut out);
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
        if is_opening(prev) {
            let reason = if self.cfg.resume_session_id.is_some() {
                StartFailure::ResumeFailed {
                    detail: format!("Agy ACP exited before ready ({exit:?})"),
                }
            } else {
                StartFailure::Other {
                    detail: format!("Agy ACP exited before ready ({exit:?})"),
                }
            };
            out.push(Output::Event(SessionEvent::StartFailed { reason }));
        } else if prev == Phase::Busy || prev == Phase::Closing {
            out.extend(self.fail_pending_mid_turn_deliveries("Agy ACP exited during turn"));
            if let Some(text) = self.flush_assistant_text() {
                out.push(text);
            }
            if let Some(u) = self.flush_turn_usage() {
                out.push(u);
            }
            if self.turn_open {
                self.turn_open = false;
                let outcome = if prev == Phase::Closing {
                    TurnOutcome::Interrupted
                } else {
                    TurnOutcome::Failed {
                        reason: "Agy ACP exited during turn".to_owned(),
                    }
                };
                out.push(Output::Event(SessionEvent::TurnEnded {
                    outcome,
                    errors: Vec::new(),
                }));
            }
        }
        self.active_prompt_id = None;
        out.push(Output::Event(SessionEvent::Ended { exit }));
        out
    }

    fn on_tick(&mut self, now: Instant) -> Vec<Output> {
        if !is_opening(self.phase) {
            return Vec::new();
        }
        if let Some(deadline) = self.handshake_deadline {
            if now >= deadline {
                self.handshake_deadline = None;
                self.phase = Phase::Failed;
                return vec![Output::Event(SessionEvent::StartFailed {
                    reason: StartFailure::ProtocolTimeout,
                })];
            }
        }
        Vec::new()
    }

    fn control(&mut self, cmd: Control, _now: Instant) -> Result<Vec<Output>, ControlError> {
        if matches!(self.phase, Phase::Ended | Phase::Closing | Phase::Failed) {
            return Err(ControlError::Closed);
        }
        match cmd {
            Control::Send { text } => control_send(self, text),
            Control::DeliverMidTurn { delivery_id, text } => {
                control_deliver_mid_turn(self, delivery_id, text)
            }
            Control::Interrupt => {
                if is_opening(self.phase) {
                    return Err(ControlError::NotReady);
                }
                let Some(sid) = self.session_id.as_ref() else {
                    return Err(ControlError::NotReady);
                };
                Ok(vec![encode_session_cancel(sid.as_str())])
            }
            Control::Answer {
                request_id,
                decision,
            } => control_answer(self, request_id, decision),
            Control::Close => {
                self.phase = Phase::Closing;
                self.handshake_deadline = None;
                let mut out = Vec::new();
                if let Some(sid) = self.session_id.clone() {
                    let req_id = self.alloc_id();
                    out.push(encode_session_close(req_id, sid.as_str()));
                }
                out.push(Output::CloseStdin);
                Ok(out)
            }
            _ => Err(ControlError::Unsupported),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        if is_opening(self.phase) {
            self.handshake_deadline
        } else {
            None
        }
    }
}
