use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::Value;

use org_agent_runtime::{
    Activity, ApprovalId, Control, ControlError, CostBasis, Decision, ExitInfo, NoticeKind, Output,
    Session, SessionEvent, SessionId, StartFailure, TurnOutcome, Usage, UsageScope,
};

use crate::plan::AgySessionConfig;
use crate::protocol::{
    classify_rpc_error_outcome, classify_stop_reason, encode_initialize, encode_jsonrpc_error,
    encode_permission_cancelled, encode_permission_selected, encode_session_cancel,
    encode_session_close, encode_session_open, encode_session_prompt, encode_set_mode,
    encode_set_model, extract_rpc_error_message, first_str, is_auth_rpc_error, is_banned_mode,
    parse_acp_usage, parse_permission_request, parse_stderr_line, resolve_tool_classification,
    rpc_error, rpc_id_key, rpc_result, AcpUsage,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    OpeningInit,
    OpeningSession,
    OpeningMode,
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
    allow_option_id: Option<String>,
    deny_option_id: Option<String>,
    tool_call_id: Option<String>,
}

/// One live `session/prompt` RPC slot in an active turn (initial prompt + any mid-turn steers).
#[derive(Debug, Clone)]
struct PromptSlot {
    req_id: u64,
    delivery_id: Option<String>,
    acked: bool,
}

/// Pure sans-I/O ACP JSON-RPC 2.0 protocol state machine for `agy acp --stdio`.
pub(crate) struct AgySession {
    cfg: AgySessionConfig,
    phase: Phase,
    turn_open: bool,
    next_id: u64,
    init_req_id: Option<u64>,
    session_req_id: Option<u64>,
    mode_req_id: Option<u64>,
    model_req_id: Option<u64>,
    handshake_deadline: Option<Instant>,
    session_id: Option<SessionId>,
    runtime_version: Option<String>,
    reported_mode: Option<String>,
    active_model: Option<String>,
    /// Ordered chain of live `session/prompt` slots for the open turn (`[0]` is oldest,
    /// `last()` is newest owning prompt). Replacing split maps prevents chained-steer corruption.
    prompt_chain: Vec<PromptSlot>,
    pending_approvals: BTreeMap<ApprovalId, PendingApproval>,
    turn_text_buf: String,
    /// Authoritative finalized token usage summed across `session/prompt` `result.usage` payloads.
    confirmed_usage: AcpUsage,
    saw_confirmed_usage: bool,
    /// Current-segment streaming `usage_update` snapshot (cleared when prompt `result.usage`
    /// finalizes the segment; only folded into `flush_turn_usage` if unfinalized on crash/EOF).
    streamed_usage: Option<AcpUsage>,
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
            mode_req_id: None,
            model_req_id: None,
            handshake_deadline: None,
            session_id: None,
            runtime_version: None,
            reported_mode: None,
            active_model: None,
            prompt_chain: Vec::new(),
            pending_approvals: BTreeMap::new(),
            turn_text_buf: String::new(),
            confirmed_usage: AcpUsage::default(),
            saw_confirmed_usage: false,
            streamed_usage: None,
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
        std::mem::take(&mut self.prompt_chain)
            .into_iter()
            .filter_map(|slot| (!slot.acked).then_some(slot.delivery_id).flatten())
            .map(|delivery_id| {
                Output::Event(SessionEvent::MidTurnDeliveryFailed {
                    delivery_id,
                    reason: reason.to_owned(),
                })
            })
            .collect()
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
        let mut total = std::mem::take(&mut self.confirmed_usage);
        let saw_confirmed = std::mem::replace(&mut self.saw_confirmed_usage, false);
        let streamed = self.streamed_usage.take();
        if let Some(est) = &streamed {
            total.add_assign(est);
        }
        if !saw_confirmed && streamed.is_none() && total.is_zero() {
            return None;
        }
        let mut u = Usage::new(UsageScope::TurnDelta, UsageScope::TurnDelta);
        u.input = total.input;
        u.output = total.output;
        u.cache_read = total.cache_read;
        u.cache_write = total.cache_write;
        u.reasoning = total.reasoning;
        u.cost_basis = CostBasis::None;
        u.model = self.active_model.clone().or_else(|| self.cfg.model.clone());
        Some(Output::Event(SessionEvent::Usage(u)))
    }
}

fn is_opening(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::OpeningInit | Phase::OpeningSession | Phase::OpeningMode | Phase::OpeningModel
    )
}

fn fail_opening_mode_mismatch(
    s: &mut AgySession,
    reported: &str,
    close_stdin: bool,
) -> Vec<Output> {
    s.handshake_deadline = None;
    s.phase = Phase::Failed;
    let mut out = vec![Output::Event(SessionEvent::StartFailed {
        reason: StartFailure::ModeMismatch {
            requested: s.cfg.expected_mode.reported_name.to_owned(),
            reported: reported.to_owned(),
        },
    })];
    if close_stdin {
        out.push(Output::CloseStdin);
    }
    out
}

fn continue_opening_model_or_ready(s: &mut AgySession) -> Vec<Output> {
    if let Some(requested) = s.cfg.model.clone() {
        if s.active_model.as_deref() != Some(requested.as_str()) {
            if let Some(sid) = s.session_id.clone() {
                let model_id = s.alloc_id();
                s.model_req_id = Some(model_id);
                s.phase = Phase::OpeningModel;
                return vec![encode_set_model(model_id, sid.as_str(), &requested)];
            }
        }
    }
    finish_opening_ready(s)
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
        permission_mode: s.reported_mode.clone(),
    }));
    out
}

fn fail_opening(s: &mut AgySession, detail: String, err_val: Option<&Value>) -> Vec<Output> {
    let prev = s.phase;
    s.handshake_deadline = None;
    s.phase = Phase::Failed;
    let reason = if err_val.is_some_and(is_auth_rpc_error) {
        StartFailure::AuthRequired
    } else if prev == Phase::OpeningSession && s.cfg.resume_session_id.is_some() {
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
    if s.mode_req_id == Some(req_id) {
        return handle_set_mode_response(s, val);
    }
    if s.model_req_id == Some(req_id) {
        return handle_set_model_response(s, val);
    }
    if let Some(idx) = s.prompt_chain.iter().position(|slot| slot.req_id == req_id) {
        return handle_prompt_slot_response(s, idx, val);
    }
    Vec::new()
}

fn handle_init_response(s: &mut AgySession, val: &Value) -> Vec<Output> {
    s.init_req_id = None;
    if s.phase != Phase::OpeningInit {
        return Vec::new();
    }
    if let Some(err) = rpc_error(val) {
        return fail_opening(
            s,
            format!("ACP initialize failed: {}", extract_rpc_error_message(err)),
            Some(err),
        );
    }
    let Some(result) = rpc_result(val) else {
        return fail_opening(
            s,
            "ACP initialize response missing `result`".to_owned(),
            None,
        );
    };
    let proto_ver = result
        .get("protocolVersion")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if proto_ver < 1 {
        return fail_opening(
            s,
            format!("ACP initialize reported unsupported protocolVersion={proto_ver}"),
            None,
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

fn validate_session_open_mode<'a>(
    s: &mut AgySession,
    result: &'a Value,
) -> Result<&'a str, Vec<Output>> {
    let Some(mode) = result
        .pointer("/modes/currentModeId")
        .and_then(Value::as_str)
        .filter(|m| !m.trim().is_empty())
    else {
        return Err(fail_opening_mode_mismatch(s, "<missing>", false));
    };
    if is_banned_mode(mode) {
        return Err(fail_opening_mode_mismatch(s, mode, true));
    }
    Ok(mode)
}

fn handle_session_open_response(s: &mut AgySession, val: &Value) -> Vec<Output> {
    s.session_req_id = None;
    if s.phase != Phase::OpeningSession {
        return Vec::new();
    }
    if let Some(err) = rpc_error(val) {
        return fail_opening(
            s,
            format!(
                "ACP session open failed: {}",
                extract_rpc_error_message(err)
            ),
            Some(err),
        );
    }
    let Some(result) = rpc_result(val) else {
        return fail_opening(
            s,
            "ACP session open response missing `result`".to_owned(),
            None,
        );
    };
    let mode = match validate_session_open_mode(s, result) {
        Ok(m) => m,
        Err(mismatch) => return mismatch,
    };
    let sid = match result
        .get("sessionId")
        .and_then(Value::as_str)
        .map(SessionId::try_from)
    {
        Some(Ok(id)) => id,
        Some(Err(_)) => {
            return fail_opening(
                s,
                "ACP session open carried invalid sessionId".to_owned(),
                None,
            );
        }
        None => match s.cfg.resume_session_id.clone() {
            Some(resumed) => resumed,
            None => {
                return fail_opening(
                    s,
                    "ACP session/new response missing sessionId".to_owned(),
                    None,
                );
            }
        },
    };
    let current_model = result
        .pointer("/models/currentModelId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    s.active_model = current_model;
    s.session_id = Some(sid.clone());
    s.reported_mode = Some(mode.to_owned());

    if mode != s.cfg.expected_mode.reported_name {
        let mode_id = s.alloc_id();
        s.mode_req_id = Some(mode_id);
        s.phase = Phase::OpeningMode;
        return vec![encode_set_mode(
            mode_id,
            sid.as_str(),
            s.cfg.expected_mode.reported_name,
        )];
    }
    continue_opening_model_or_ready(s)
}

fn handle_set_mode_response(s: &mut AgySession, val: &Value) -> Vec<Output> {
    s.mode_req_id = None;
    if s.phase != Phase::OpeningMode {
        return Vec::new();
    }
    let err = rpc_error(val);
    if err.is_some() || rpc_result(val).is_none() {
        let expected = s.cfg.expected_mode.reported_name;
        let reported = s
            .reported_mode
            .as_deref()
            .filter(|m| *m != expected)
            .unwrap_or("<rejected>")
            .to_owned();
        return fail_opening_mode_mismatch(s, &reported, err.is_some_and(is_auth_rpc_error));
    }
    s.reported_mode = Some(s.cfg.expected_mode.reported_name.to_owned());
    continue_opening_model_or_ready(s)
}

fn handle_set_model_response(s: &mut AgySession, val: &Value) -> Vec<Output> {
    s.model_req_id = None;
    if s.phase != Phase::OpeningModel {
        return Vec::new();
    }
    if let Some(err) = rpc_error(val) {
        return fail_opening(
            s,
            format!(
                "ACP session/set_model failed: {}",
                extract_rpc_error_message(err)
            ),
            Some(err),
        );
    }
    if rpc_result(val).is_none() {
        return fail_opening(
            s,
            "ACP session/set_model response missing `result`".to_owned(),
            None,
        );
    }
    if let Some(requested) = &s.cfg.model {
        s.active_model = Some(requested.clone());
    }
    finish_opening_ready(s)
}

fn handle_prompt_slot_response(s: &mut AgySession, idx: usize, val: &Value) -> Vec<Output> {
    if let Some(err) = rpc_error(val) {
        return handle_prompt_slot_error(s, idx, err);
    }
    let Some(result) = rpc_result(val) else {
        return vec![Output::Event(SessionEvent::ProtocolError {
            detail: "ACP session/prompt response missing `result` and `error`".to_owned(),
        })];
    };
    let slot = s.prompt_chain.remove(idx);
    let mut out = Vec::new();
    if let Some(did) = slot.delivery_id.filter(|_| !slot.acked) {
        out.push(Output::Event(SessionEvent::MidTurnDelivered {
            delivery_id: did,
        }));
    }
    if idx < s.prompt_chain.len() {
        // Live `agy 1.5.17` resolves a superseded prompt immediately when a mid-turn steer
        // arrives and transfers turn ownership to the next surviving slot without aborting tools.
        let next = &mut s.prompt_chain[idx];
        if !next.acked {
            if let Some(did) = next.delivery_id.clone() {
                next.acked = true;
                out.push(Output::Event(SessionEvent::MidTurnDelivered {
                    delivery_id: did,
                }));
            }
        }
    }
    if let Some(usage_val) = result.get("usage").filter(|v| v.is_object()) {
        s.confirmed_usage.add_assign(&parse_acp_usage(usage_val));
        s.saw_confirmed_usage = true;
        s.streamed_usage = None;
    }
    if let Some(text) = s.flush_assistant_text() {
        out.push(text);
    }
    if !s.prompt_chain.is_empty() {
        return out;
    }
    if let Some(u) = s.flush_turn_usage() {
        out.push(u);
    }
    let outcome = classify_stop_reason(result.get("stopReason").and_then(Value::as_str));
    finish_turn(s, outcome, Vec::new(), &mut out);
    out
}

fn handle_prompt_slot_error(s: &mut AgySession, idx: usize, err: &Value) -> Vec<Output> {
    let slot = s.prompt_chain.remove(idx);
    let mut out = Vec::new();
    if let Some(did) = slot.delivery_id.filter(|_| !slot.acked) {
        out.push(Output::Event(SessionEvent::MidTurnDeliveryFailed {
            delivery_id: did,
            reason: extract_rpc_error_message(err),
        }));
    }
    if !s.prompt_chain.is_empty() {
        // Another live prompt slot still owns this turn; keep `Phase::Busy` until it settles.
        return out;
    }
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
    s: &mut AgySession,
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
                "Agy ACP switched to disallowed permission mode `{mode}`; stopping session"
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
    s.prompt_chain = vec![PromptSlot {
        req_id,
        delivery_id: None,
        acked: false,
    }];
    s.phase = Phase::Busy;
    s.turn_open = true;
    s.turn_text_buf.clear();
    s.confirmed_usage = AcpUsage::default();
    s.saw_confirmed_usage = false;
    s.streamed_usage = None;
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
    if s.prompt_chain.is_empty() {
        return Err(ControlError::NotReady);
    }
    let Some(sid) = s.session_id.clone() else {
        return Err(ControlError::NotReady);
    };
    let new_id = s.alloc_id();
    s.prompt_chain.push(PromptSlot {
        req_id: new_id,
        delivery_id: Some(delivery_id),
        acked: false,
    });
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
    let mut out = Vec::new();
    let chosen = if use_allow {
        pending.allow_option_id.as_deref()
    } else {
        pending.deny_option_id.as_deref()
    };
    match chosen {
        Some(option_id) => out.push(encode_permission_selected(&pending.raw_id, option_id)),
        None => {
            out.push(encode_permission_cancelled(&pending.raw_id));
            if use_allow {
                out.push(Output::Event(SessionEvent::Notice {
                    kind: NoticeKind::Warning,
                    text: "Agy ACP permission request offered no one-shot allow option; failed closed with cancel".to_owned(),
                }));
            }
        }
    }
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
                None,
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
            let detail = format!("Agy ACP exited before ready ({exit:?})");
            let reason = if prev == Phase::OpeningSession && self.cfg.resume_session_id.is_some() {
                StartFailure::ResumeFailed { detail }
            } else {
                StartFailure::Other { detail }
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
        self.prompt_chain.clear();
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
                if self.phase == Phase::Idle {
                    return Ok(Vec::new());
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
