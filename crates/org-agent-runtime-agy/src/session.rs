use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use org_agent_runtime::{
    encode_line, Activity, ApprovalId, Control, ControlError, CostBasis, Decision, ExitInfo,
    NoticeKind, Output, Session, SessionEvent, SessionId, StartFailure, TurnOutcome, Usage,
    UsageScope,
};

use crate::plan::ExpectedMode;
use crate::protocol::{
    build_approval_request, classify_result_outcome, encode_allow_response, encode_deny_response,
    extract_agent_names, handle_rate_limit, has_remote_control, is_banned_mode, parse_agy_usage,
    parse_assistant_blocks, parse_permission_denials, parse_result_errors, parse_stderr_line,
    parse_stream_event, parse_user_blocks, MessageUsage,
};

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Opening,
    Idle,
    Busy,
    Failed,
    Closing,
    Ended,
}

/// Pure sans-I/O state machine for one Antigravity (`agy -p --input-format stream-json --output-format stream-json`) child process.
pub(crate) struct AgySession {
    expected_agent: String,
    expected_mode: ExpectedMode,
    allowed_tools: Option<BTreeSet<String>>,
    allowed_mcp_servers: BTreeSet<String>,
    allowed_mcp_prefixes: Vec<String>,
    resumed: bool,
    awaiting_resume_baseline: bool,
    phase: Phase,
    aborting: bool,
    turn_open: bool,
    stop_reason: Option<String>,
    next_req_id: u64,
    init_request_id: Option<String>,
    init_deadline: Option<Instant>,
    emitted_session_id: Option<SessionId>,
    pending_approvals: BTreeSet<ApprovalId>,
    turn_usage_by_msg: BTreeMap<String, MessageUsage>,
    resume_baseline: Option<MessageUsage>,
    turn_model: Option<String>,
}

impl AgySession {
    pub(crate) fn new(
        expected_agent: String,
        expected_mode: ExpectedMode,
        allowed_tools: Option<BTreeSet<String>>,
        allowed_mcp_servers: BTreeSet<String>,
        allowed_mcp_prefixes: Vec<String>,
        resumed: bool,
    ) -> Self {
        Self {
            expected_agent,
            expected_mode,
            allowed_tools,
            allowed_mcp_servers,
            allowed_mcp_prefixes,
            resumed,
            awaiting_resume_baseline: resumed,
            phase: Phase::Opening,
            aborting: false,
            turn_open: false,
            stop_reason: None,
            next_req_id: 1,
            init_request_id: None,
            init_deadline: None,
            emitted_session_id: None,
            pending_approvals: BTreeSet::new(),
            turn_usage_by_msg: BTreeMap::new(),
            resume_baseline: None,
            turn_model: None,
        }
    }

    fn next_host_req_id(&mut self) -> String {
        let id = format!("host-{}", self.next_req_id);
        self.next_req_id += 1;
        id
    }

    fn busy_status(&self) -> Activity {
        if self.pending_approvals.is_empty() {
            Activity::Busy
        } else {
            Activity::WaitingOnApproval
        }
    }

    fn encode_interrupt_request(&mut self) -> Output {
        let req_id = self.next_host_req_id();
        let line = encode_line(&json!({
            "type": "control_request",
            "request_id": req_id,
            "request": { "subtype": "interrupt" }
        }))
        .expect("static interrupt encodes");
        Output::Write(line)
    }

    fn cancel_pending_approvals(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.pending_approvals)
            .into_iter()
            .map(|id| Output::Event(SessionEvent::ApprovalCancelled { id }))
            .collect()
    }

    fn fail_or_fast_stop(&mut self, detail: String) -> Vec<Output> {
        if matches!(self.phase, Phase::Closing | Phase::Failed | Phase::Ended) {
            return Vec::new();
        }
        if self.phase == Phase::Opening {
            self.init_deadline = None;
            self.phase = Phase::Failed;
            return vec![Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::Other { detail },
            })];
        }
        self.aborting = true;
        self.phase = Phase::Closing;
        self.stop_reason = Some(detail.clone());
        let interrupt = self.encode_interrupt_request();
        vec![
            Output::Event(SessionEvent::Notice {
                kind: NoticeKind::Warning,
                text: detail,
            }),
            interrupt,
            Output::CloseStdin,
        ]
    }

    fn flush_usage(&mut self, cost_usd: Option<f64>) -> Option<Output> {
        if self.turn_usage_by_msg.is_empty() && cost_usd.is_none() {
            return None;
        }
        let mut total = MessageUsage::default();
        for u in self.turn_usage_by_msg.values() {
            total.input = total.input.saturating_add(u.input);
            total.output = total.output.saturating_add(u.output);
            total.cache_read = total.cache_read.saturating_add(u.cache_read);
            total.cache_write = total.cache_write.saturating_add(u.cache_write);
            total.reasoning = total.reasoning.saturating_add(u.reasoning);
        }
        self.turn_usage_by_msg.clear();
        let mut u = Usage::new(UsageScope::TurnDelta, UsageScope::Cumulative);
        u.input = total.input;
        u.output = total.output;
        u.cache_read = total.cache_read;
        u.cache_write = total.cache_write;
        u.reasoning = total.reasoning;
        u.cost_usd = cost_usd;
        u.cost_basis = if cost_usd.is_some() {
            CostBasis::ListPriceEstimate
        } else {
            CostBasis::None
        };
        u.model = self.turn_model.clone();
        Some(Output::Event(SessionEvent::Usage(u)))
    }

    fn end_open_turn(&mut self, reported: Option<(TurnOutcome, Vec<String>)>) -> Vec<Output> {
        if !self.turn_open {
            return Vec::new();
        }
        self.turn_open = false;
        let (outcome, errors) = match (&self.stop_reason, reported) {
            (Some(detail), rep) => (
                TurnOutcome::Failed {
                    reason: format!("session stopped: {detail}"),
                },
                rep.map(|(_, e)| e).unwrap_or_default(),
            ),
            (None, Some(rep)) => rep,
            (None, None) => (TurnOutcome::Interrupted, Vec::new()),
        };
        vec![Output::Event(SessionEvent::TurnEnded { outcome, errors })]
    }
}

fn handle_frame(s: &mut AgySession, val: Value) -> Vec<Output> {
    let Some(msg_type) = val.get("type").and_then(Value::as_str) else {
        return vec![Output::Event(SessionEvent::ProtocolError {
            detail: "Agy stdout frame missing string `type` field".to_owned(),
        })];
    };
    match msg_type {
        "control_response" => handle_control_response(s, &val),
        "control_request" => handle_control_request(s, &val),
        "control_cancel_request" => handle_control_cancel(s, &val),
        "system" => handle_system(s, &val),
        "stream_event" => parse_stream_event(&val),
        "assistant" => handle_assistant(s, &val),
        "user" => parse_user_blocks(&val),
        "rate_limit_event" => handle_rate_limit(&val),
        "result" => handle_result(s, &val),
        other => vec![Output::Event(SessionEvent::Unknown {
            type_name: other.to_owned(),
        })],
    }
}

fn handle_control_response(s: &mut AgySession, val: &Value) -> Vec<Output> {
    let Some(resp) = val.get("response").and_then(Value::as_object) else {
        return vec![Output::Event(SessionEvent::ProtocolError {
            detail: "control_response missing `response` object".to_owned(),
        })];
    };
    let req_id = resp.get("request_id").and_then(Value::as_str).unwrap_or("");
    if s.phase != Phase::Opening || s.init_request_id.as_deref() != Some(req_id) {
        return Vec::new();
    }
    s.init_deadline = None;
    if resp.get("subtype").and_then(Value::as_str) != Some("success") {
        s.phase = Phase::Failed;
        let detail = resp
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("initialize control request failed")
            .to_owned();
        let reason = if s.resumed {
            StartFailure::ResumeFailed { detail }
        } else {
            StartFailure::Other { detail }
        };
        return vec![Output::Event(SessionEvent::StartFailed { reason })];
    }
    let body = resp.get("response").unwrap_or(&Value::Null);
    let reported_mode = body
        .get("mode")
        .or_else(|| body.get("permissionMode"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if reported_mode != s.expected_mode.reported_name || is_banned_mode(reported_mode) {
        s.phase = Phase::Failed;
        return vec![Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch {
                requested: s.expected_mode.reported_name.to_owned(),
                reported: reported_mode.to_owned(),
            },
        })];
    }
    if has_remote_control(body) {
        s.phase = Phase::Failed;
        return vec![Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::Other {
                detail: "Agy reported remote control active".to_owned(),
            },
        })];
    }
    let agents = extract_agent_names(body.get("agents"));
    if !agents.iter().any(|a| a == &s.expected_agent) {
        s.phase = Phase::Failed;
        return vec![Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::AgentMismatch {
                requested: s.expected_agent.clone(),
                available: agents,
            },
        })];
    }
    if s.resumed {
        if let Some(usage_val) = body.get("usage") {
            s.resume_baseline = Some(parse_agy_usage(usage_val));
            s.awaiting_resume_baseline = false;
        }
    }
    s.phase = Phase::Idle;
    vec![Output::Event(SessionEvent::Ready {
        runtime_version: body
            .get("version")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        pid: body
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        agents,
        permission_mode: Some(reported_mode.to_owned()),
    })]
}

fn handle_control_request(s: &mut AgySession, val: &Value) -> Vec<Output> {
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
    if subtype != "can_use_tool" {
        return vec![Output::Event(SessionEvent::Unknown {
            type_name: format!("control_request/{subtype}"),
        })];
    }
    if s.aborting || matches!(s.phase, Phase::Closing | Phase::Failed | Phase::Ended) {
        return vec![encode_deny_response(
            request_id,
            "Session is closing or stopped",
            true,
        )];
    }
    let approval = build_approval_request(request_id, req);
    s.pending_approvals.insert(approval.id.clone());
    vec![
        Output::Event(SessionEvent::Status(Activity::WaitingOnApproval)),
        Output::Event(SessionEvent::ApprovalRequested(approval)),
    ]
}

fn handle_control_cancel(s: &mut AgySession, val: &Value) -> Vec<Output> {
    let Some(request_id) = val.get("request_id").and_then(Value::as_str) else {
        return vec![Output::Event(SessionEvent::ProtocolError {
            detail: "control_cancel_request missing string `request_id`".to_owned(),
        })];
    };
    let id = ApprovalId::new(request_id);
    if !s.pending_approvals.remove(&id) {
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

fn handle_system(s: &mut AgySession, val: &Value) -> Vec<Output> {
    let subtype = val.get("subtype").and_then(Value::as_str).unwrap_or("");
    match subtype {
        "init" => handle_system_init(s, val),
        "task_notification" | "task_started" | "task_updated" => {
            let text = val
                .get("summary")
                .or_else(|| val.get("message"))
                .and_then(Value::as_str)
                .map(|msg| format!("{subtype}: {msg}"))
                .unwrap_or_else(|| subtype.to_owned());
            vec![Output::Event(SessionEvent::Notice {
                kind: NoticeKind::Warning,
                text,
            })]
        }
        other => vec![Output::Event(SessionEvent::Unknown {
            type_name: format!("system/{other}"),
        })],
    }
}

fn is_tool_allowed(s: &AgySession, name: &str) -> bool {
    let Some(allowlist) = &s.allowed_tools else {
        return true;
    };
    allowlist.contains(name) || s.allowed_mcp_prefixes.iter().any(|p| name.starts_with(p))
}

fn check_init_violations(s: &AgySession, val: &Value) -> Option<String> {
    if has_remote_control(val) {
        return Some("Agy system/init reported remote control active".to_owned());
    }
    if let Some(tools) = val.get("tools").and_then(Value::as_array) {
        let disallowed: Vec<&str> = tools
            .iter()
            .filter_map(Value::as_str)
            .filter(|name| !is_tool_allowed(s, name))
            .collect();
        if !disallowed.is_empty() {
            return Some(format!(
                "Agy system/init exposed disallowed tools: {}",
                disallowed.join(", ")
            ));
        }
    }
    if let Some(servers) = val.get("mcp_servers").and_then(Value::as_array) {
        let unlisted: Vec<&str> = servers
            .iter()
            .filter_map(|v| v.as_str().or_else(|| v.get("name").and_then(Value::as_str)))
            .filter(|name| !s.allowed_mcp_servers.contains(*name))
            .collect();
        if !unlisted.is_empty() {
            return Some(format!(
                "Agy system/init exposed unlisted MCP servers: {}",
                unlisted.join(", ")
            ));
        }
    }
    None
}

fn check_init_mode(s: &mut AgySession, val: &Value) -> Result<Option<Output>, Vec<Output>> {
    let Some(mode) = val
        .get("mode")
        .or_else(|| val.get("permissionMode"))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    if is_banned_mode(mode) {
        if s.phase == Phase::Opening {
            s.init_deadline = None;
            s.phase = Phase::Failed;
            return Err(vec![Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ModeMismatch {
                    requested: s.expected_mode.reported_name.to_owned(),
                    reported: mode.to_owned(),
                },
            })]);
        }
        return Err(s.fail_or_fast_stop(format!(
            "Agy system/init reported banned permission mode `{mode}`"
        )));
    }
    if mode != s.expected_mode.reported_name {
        if s.phase == Phase::Opening {
            s.init_deadline = None;
            s.phase = Phase::Failed;
            return Err(vec![Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ModeMismatch {
                    requested: s.expected_mode.reported_name.to_owned(),
                    reported: mode.to_owned(),
                },
            })]);
        }
        return Ok(Some(Output::Event(SessionEvent::ModeChanged {
            mode: Some(mode.to_owned()),
        })));
    }
    Ok(None)
}

fn record_init_session_id(s: &mut AgySession, val: &Value) -> Option<Output> {
    let raw_sid = val
        .get("conversation_id")
        .or_else(|| val.get("session_id"))
        .and_then(Value::as_str)?;
    match SessionId::try_from(raw_sid) {
        Ok(sid) if s.emitted_session_id.as_ref() != Some(&sid) => {
            s.emitted_session_id = Some(sid.clone());
            Some(Output::Event(SessionEvent::SessionStarted {
                runtime_session_id: sid,
            }))
        }
        Ok(_) => None,
        Err(_) => Some(Output::Event(SessionEvent::ProtocolError {
            detail: "Agy system/init carried an invalid session_id".to_owned(),
        })),
    }
}

fn handle_system_init(s: &mut AgySession, val: &Value) -> Vec<Output> {
    if let Some(reason) = check_init_violations(s, val) {
        return s.fail_or_fast_stop(reason);
    }
    let mut out = Vec::new();
    match check_init_mode(s, val) {
        Ok(Some(ev)) => out.push(ev),
        Ok(None) => {}
        Err(stop) => return stop,
    }
    if let Some(ev) = record_init_session_id(s, val) {
        out.push(ev);
    }
    if s.resumed && s.resume_baseline.is_none() {
        if let Some(usage_val) = val.get("usage") {
            s.resume_baseline = Some(parse_agy_usage(usage_val));
            s.awaiting_resume_baseline = false;
        }
    }
    if s.phase == Phase::Idle {
        if let Some(u) = s.flush_usage(None) {
            out.push(u);
        }
        s.phase = Phase::Busy;
        s.turn_open = true;
        out.push(Output::Event(SessionEvent::TurnStarted {
            turn_id: None,
            solicited: false,
        }));
        out.push(Output::Event(SessionEvent::Status(s.busy_status())));
    }
    out
}

fn handle_assistant(s: &mut AgySession, val: &Value) -> Vec<Output> {
    let Some(msg) = val.get("message").and_then(Value::as_object) else {
        return vec![Output::Event(SessionEvent::ProtocolError {
            detail: "assistant frame missing `message` object".to_owned(),
        })];
    };
    let msg_id = msg.get("id").and_then(Value::as_str).map(ToOwned::to_owned);
    if let Some(model) = msg.get("model").and_then(Value::as_str) {
        s.turn_model = Some(model.to_owned());
    }
    if let Some(usage_val) = msg.get("usage") {
        let parsed = parse_agy_usage(usage_val);
        let key = msg_id
            .clone()
            .unwrap_or_else(|| format!("anon-{}", s.turn_usage_by_msg.len()));
        s.turn_usage_by_msg
            .entry(key)
            .and_modify(|e| e.merge_max(&parsed))
            .or_insert(parsed);
    }
    parse_assistant_blocks(msg, &msg_id)
}

fn apply_result_usage(s: &mut AgySession, val: &Value, out: &mut Vec<Output>) {
    if !matches!(s.phase, Phase::Busy | Phase::Idle | Phase::Closing) {
        return;
    }
    if let Some(usage_val) = val.get("usage") {
        let raw = parse_agy_usage(usage_val);
        if s.turn_usage_by_msg.is_empty() {
            if s.awaiting_resume_baseline {
                s.resume_baseline = Some(raw);
                s.awaiting_resume_baseline = false;
                out.push(Output::Event(SessionEvent::Notice {
                    kind: NoticeKind::Warning,
                    text: "resumed without prior usage baseline; skipping first turn cumulative token count".to_owned(),
                }));
                return;
            }
            let delta = match &s.resume_baseline {
                Some(base) => {
                    let diff = raw.saturating_sub(base);
                    s.resume_baseline = Some(raw);
                    diff
                }
                None => raw,
            };
            s.turn_usage_by_msg.insert("result".to_owned(), delta);
        }
    }
    let cost = val.get("total_cost_usd").and_then(Value::as_f64);
    if let Some(u) = s.flush_usage(cost) {
        out.push(u);
    }
}

fn handle_result(s: &mut AgySession, val: &Value) -> Vec<Output> {
    let mut out = parse_permission_denials(val);
    apply_result_usage(s, val, &mut out);
    let errors = parse_result_errors(val);
    let outcome = classify_result_outcome(val, &errors);
    if s.phase == Phase::Closing {
        out.extend(s.end_open_turn(Some((outcome, errors))));
        return out;
    }
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
    out
}

impl Session for AgySession {
    fn start(&mut self, now: Instant) -> Vec<Output> {
        if self.phase != Phase::Opening || self.init_request_id.is_some() {
            return Vec::new();
        }
        let req_id = self.next_host_req_id();
        self.init_request_id = Some(req_id.clone());
        self.init_deadline = Some(now + INITIALIZE_TIMEOUT);
        let line = encode_line(&json!({
            "type": "control_request",
            "request_id": req_id,
            "request": { "subtype": "initialize" }
        }))
        .expect("static initialize encodes");
        vec![Output::Write(line)]
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        let Ok(text) = std::str::from_utf8(line) else {
            return vec![Output::Event(SessionEvent::ProtocolError {
                detail: "Agy stdout line was not valid UTF-8".to_owned(),
            })];
        };
        if text.trim().is_empty() {
            return Vec::new();
        }
        match serde_json::from_str::<Value>(text) {
            Ok(val) => handle_frame(self, val),
            Err(err) => vec![Output::Event(SessionEvent::ProtocolError {
                detail: format!("invalid JSON line from Agy: {err}"),
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
        self.init_deadline = None;
        let mut out = self.cancel_pending_approvals();
        match self.phase {
            Phase::Opening => {
                self.phase = Phase::Failed;
                let detail = "Agy stdout closed before initialize response".to_owned();
                let reason = if self.resumed {
                    StartFailure::ResumeFailed { detail }
                } else {
                    StartFailure::Other { detail }
                };
                out.push(Output::Event(SessionEvent::StartFailed { reason }));
            }
            Phase::Busy => {
                if let Some(u) = self.flush_usage(None) {
                    out.push(u);
                }
                self.phase = Phase::Idle;
                self.turn_open = false;
                out.push(Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Failed {
                        reason: "Agy stdout closed during turn".to_owned(),
                    },
                    errors: Vec::new(),
                }));
                out.push(Output::Event(SessionEvent::Status(Activity::Idle)));
            }
            Phase::Idle => {
                if let Some(u) = self.flush_usage(None) {
                    out.push(u);
                }
            }
            Phase::Closing => {
                if let Some(u) = self.flush_usage(None) {
                    out.push(u);
                }
                out.extend(self.end_open_turn(None));
            }
            Phase::Failed | Phase::Ended => {}
        }
        out
    }

    fn on_exit(&mut self, exit: ExitInfo, _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        let prev = self.phase;
        self.phase = Phase::Ended;
        self.init_deadline = None;
        let mut out = self.cancel_pending_approvals();
        match prev {
            Phase::Opening => {
                let detail = format!("Agy exited before ready ({exit:?})");
                let reason = if self.resumed {
                    StartFailure::ResumeFailed { detail }
                } else {
                    StartFailure::Other { detail }
                };
                out.push(Output::Event(SessionEvent::StartFailed { reason }));
            }
            Phase::Busy => {
                if let Some(u) = self.flush_usage(None) {
                    out.push(u);
                }
                self.turn_open = false;
                out.push(Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Failed {
                        reason: "Agy exited during turn".to_owned(),
                    },
                    errors: Vec::new(),
                }));
            }
            Phase::Idle => {
                if let Some(u) = self.flush_usage(None) {
                    out.push(u);
                }
            }
            Phase::Closing => {
                if let Some(u) = self.flush_usage(None) {
                    out.push(u);
                }
                out.extend(self.end_open_turn(None));
            }
            Phase::Failed | Phase::Ended => {}
        }
        out.push(Output::Event(SessionEvent::Ended { exit }));
        out
    }

    fn control(&mut self, cmd: Control, _now: Instant) -> Result<Vec<Output>, ControlError> {
        if matches!(self.phase, Phase::Ended | Phase::Closing | Phase::Failed) {
            return Err(ControlError::Closed);
        }
        match cmd {
            Control::Send { text, urgent } => match self.phase {
                Phase::Opening => Err(ControlError::NotReady),
                Phase::Busy if urgent => Err(ControlError::NeedsInterrupt),
                Phase::Busy => Err(ControlError::Busy),
                Phase::Idle => {
                    let line = encode_line(&json!({
                        "type": "user",
                        "message": { "role": "user", "content": text }
                    }))
                    .expect("user message encodes");
                    let mut out = Vec::new();
                    if let Some(u) = self.flush_usage(None) {
                        out.push(u);
                    }
                    self.turn_model = None;
                    self.phase = Phase::Busy;
                    self.turn_open = true;
                    out.push(Output::Write(line));
                    out.push(Output::Event(SessionEvent::TurnStarted {
                        turn_id: None,
                        solicited: true,
                    }));
                    out.push(Output::Event(SessionEvent::Status(self.busy_status())));
                    Ok(out)
                }
                _ => Err(ControlError::Closed),
            },
            Control::Interrupt => match self.phase {
                Phase::Opening => Err(ControlError::NotReady),
                Phase::Idle | Phase::Busy => Ok(vec![self.encode_interrupt_request()]),
                _ => Err(ControlError::Closed),
            },
            Control::Answer {
                request_id,
                decision,
            } => {
                if !self.pending_approvals.contains(&request_id) {
                    return Err(ControlError::UnknownApproval);
                }
                let output = match decision {
                    Decision::AllowOnce => encode_allow_response(request_id.as_str()),
                    Decision::Deny { message, stop_turn } => encode_deny_response(
                        request_id.as_str(),
                        message.as_deref().unwrap_or("Denied by office policy"),
                        stop_turn,
                    ),
                    _ => return Err(ControlError::Unsupported),
                };
                self.pending_approvals.remove(&request_id);
                let mut out = vec![output];
                if self.pending_approvals.is_empty() {
                    let activity = if self.phase == Phase::Busy {
                        Activity::Busy
                    } else {
                        Activity::Idle
                    };
                    out.push(Output::Event(SessionEvent::Status(activity)));
                }
                Ok(out)
            }
            Control::Close => {
                self.phase = Phase::Closing;
                self.init_deadline = None;
                Ok(vec![Output::CloseStdin])
            }
            _ => Err(ControlError::Unsupported),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        if self.phase == Phase::Opening {
            self.init_deadline
        } else {
            None
        }
    }

    fn on_tick(&mut self, now: Instant) -> Vec<Output> {
        if self.phase == Phase::Opening && self.init_deadline.is_some_and(|d| now >= d) {
            self.init_deadline = None;
            self.phase = Phase::Failed;
            return vec![Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ProtocolTimeout,
            })];
        }
        Vec::new()
    }
}
