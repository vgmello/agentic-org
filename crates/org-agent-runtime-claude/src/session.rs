use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use org_agent_runtime::{
    encode_line, Activity, ApprovalId, ApprovalKind, ApprovalRequest, Control, ControlError,
    CostBasis, Decision, Escalation, ExitInfo, LimitKind, LimitStatus, NoticeKind, Output,
    RateLimit, Session, SessionEvent, SessionId, StartFailure, ToolCapability, TurnOutcome, Usage,
    UsageScope, Window,
};

use crate::plan::ExpectedMode;

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

#[derive(Debug, Clone, Default)]
struct MessageUsage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    reasoning: u64,
}

impl MessageUsage {
    fn merge_max(&mut self, other: &MessageUsage) {
        self.input = self.input.max(other.input);
        self.output = self.output.max(other.output);
        self.cache_read = self.cache_read.max(other.cache_read);
        self.cache_write = self.cache_write.max(other.cache_write);
        self.reasoning = self.reasoning.max(other.reasoning);
    }
}

/// Pure sans-I/O protocol state machine for one Claude Code `stream-json` child process.
///
/// ## Known protocol race (Send vs. unsolicited background wake)
/// Claude Code's `stream-json` stdin/stdout pipes are asynchronous: if the office invokes
/// `Control::Send` right as Claude emits an unsolicited `system/init` for a completing
/// background task/sub-agent, the session is already `Busy` (`solicited: true`), so Claude
/// runs the wake first and queues the office's `user` message behind it. Because Claude's
/// `result` frame does not carry a host-assigned request ID, the first `result` closes the
/// open turn and the follow-up turn arrives on the next `system/init` as `solicited: false`
/// (with a warning notice when `system/init` arrives mid-turn). The office must not rely on
/// `solicited` alone to distinguish races when background tasks are active.
pub(crate) struct ClaudeSession {
    expected_agent: String,
    expected_agent_description: String,
    expected_mode: ExpectedMode,
    allowed_tools: Option<BTreeSet<String>>,
    allowed_mcp_servers: BTreeSet<String>,
    allowed_mcp_prefixes: Vec<String>,
    phase: Phase,
    aborting: bool,
    /// A `TurnStarted` was emitted and its `TurnEnded` not yet (also while `Closing`).
    turn_open: bool,
    /// Why the adapter fast-stopped the session, if it did.
    stop_reason: Option<String>,
    next_req_id: u64,
    init_request_id: Option<String>,
    init_deadline: Option<Instant>,
    emitted_session_id: Option<SessionId>,
    pending_approvals: BTreeSet<ApprovalId>,
    turn_usage_by_msg: BTreeMap<String, MessageUsage>,
    streaming_message_id: Option<String>,
    turn_model: Option<String>,
}

impl ClaudeSession {
    pub(crate) fn new(
        expected_agent: String,
        expected_agent_description: String,
        expected_mode: ExpectedMode,
        allowed_tools: Option<BTreeSet<String>>,
        allowed_mcp_servers: BTreeSet<String>,
        allowed_mcp_prefixes: Vec<String>,
    ) -> Self {
        Self {
            expected_agent,
            expected_agent_description,
            expected_mode,
            allowed_tools,
            allowed_mcp_servers,
            allowed_mcp_prefixes,
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
            streaming_message_id: None,
            turn_model: None,
        }
    }

    fn fail_or_fast_stop(&mut self, detail: String) -> Vec<Output> {
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

    fn is_tool_allowed(&self, tool_name: &str) -> bool {
        let Some(allowlist) = &self.allowed_tools else {
            return true;
        };
        if allowlist.contains(tool_name) {
            return true;
        }
        self.allowed_mcp_prefixes
            .iter()
            .any(|prefix| tool_name.starts_with(prefix))
    }

    fn encode_deny_control_response(request_id: &str, message: &str, interrupt: bool) -> Output {
        let mut body = json!({
            "behavior": "deny",
            "message": message,
        });
        if interrupt {
            body["interrupt"] = Value::Bool(true);
        }
        let payload = json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": body,
            }
        });
        let line = encode_line(&payload).expect("static deny control_response encodes cleanly");
        Output::Write(line)
    }

    fn encode_interrupt_request(&mut self) -> Output {
        let req_id = format!("host-{}", self.next_req_id);
        self.next_req_id += 1;
        let payload = json!({
            "type": "control_request",
            "request_id": req_id,
            "request": { "subtype": "interrupt" }
        });
        let line = encode_line(&payload).expect("static interrupt control_request encodes cleanly");
        Output::Write(line)
    }

    fn reset_turn_usage(&mut self) {
        self.turn_usage_by_msg.clear();
        self.streaming_message_id = None;
        self.turn_model = None;
    }

    /// The status of a running turn: still `WaitingOnApproval` while a request (e.g. a
    /// background sub-agent's, kept across the previous turn's end) awaits an answer.
    fn busy_status(&self) -> Activity {
        if self.pending_approvals.is_empty() {
            Activity::Busy
        } else {
            Activity::WaitingOnApproval
        }
    }

    /// Withdraws every pending approval once the process is gone: nothing can take an answer
    /// to it any more.
    fn cancel_pending_approvals(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.pending_approvals)
            .into_iter()
            .map(|id| Output::Event(SessionEvent::ApprovalCancelled { id }))
            .collect()
    }

    /// Ends a turn still open after the session started closing (a fast stop or
    /// [`Control::Close`] during the turn), so every `TurnStarted` gets its `TurnEnded`.
    /// A fast stop fails the turn with its reason; otherwise the runtime's own outcome is
    /// used when it reported one (`result`), and `Interrupted` when the process ended first.
    fn end_closed_turn(&mut self, reported: Option<(TurnOutcome, Vec<String>)>) -> Vec<Output> {
        if !self.turn_open {
            return Vec::new();
        }
        self.turn_open = false;
        let (outcome, errors) = match (&self.stop_reason, reported) {
            (Some(detail), reported) => (
                TurnOutcome::Failed {
                    reason: format!("session stopped: {detail}"),
                },
                reported.map(|(_, errors)| errors).unwrap_or_default(),
            ),
            (None, Some(reported)) => reported,
            (None, None) => (TurnOutcome::Interrupted, Vec::new()),
        };
        vec![Output::Event(SessionEvent::TurnEnded { outcome, errors })]
    }

    fn sum_turn_usage(&self) -> Option<MessageUsage> {
        if self.turn_usage_by_msg.is_empty() {
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
        Some(total)
    }

    fn handle_frame(&mut self, val: Value) -> Vec<Output> {
        let Some(msg_type) = val.get("type").and_then(Value::as_str) else {
            return vec![Output::Event(SessionEvent::ProtocolError {
                detail: "Claude stdout frame missing string `type` field".to_owned(),
            })];
        };

        match msg_type {
            "control_response" => self.handle_control_response(&val),
            "control_request" => self.handle_control_request(&val),
            "control_cancel_request" => self.handle_control_cancel(&val),
            "system" => self.handle_system(&val),
            "stream_event" => self.handle_stream_event(&val),
            "assistant" => self.handle_assistant(&val),
            "user" => self.handle_user(&val),
            "rate_limit_event" => self.handle_rate_limit(&val),
            "result" => self.handle_result(&val),
            other => vec![Output::Event(SessionEvent::Unknown {
                type_name: other.to_owned(),
            })],
        }
    }

    fn handle_control_response(&mut self, val: &Value) -> Vec<Output> {
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

    fn handle_control_request(&mut self, val: &Value) -> Vec<Output> {
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

    fn handle_control_cancel(&mut self, val: &Value) -> Vec<Output> {
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

    fn handle_system(&mut self, val: &Value) -> Vec<Output> {
        let subtype = val.get("subtype").and_then(Value::as_str).unwrap_or("");
        let mut out = Vec::new();

        match subtype {
            "init" => {
                // Verify `init.tools` is a subset of the spec allowlist on every `system/init`.
                if let Some(tools) = val.get("tools").and_then(Value::as_array) {
                    let disallowed: Vec<String> = tools
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|name| !self.is_tool_allowed(name))
                        .map(ToOwned::to_owned)
                        .collect();
                    if !disallowed.is_empty() {
                        let detail = format!(
                            "Claude system/init exposed disallowed tools: {}",
                            disallowed.join(", ")
                        );
                        return self.fail_or_fast_stop(detail);
                    }
                }

                // Verify `init.plugins` holds only built-in plugins (S1, agent-sessions.md:168).
                if let Some(plugins) = val.get("plugins").and_then(Value::as_array) {
                    let non_builtin: Vec<String> = plugins
                        .iter()
                        .filter(|p| !is_builtin_plugin(p))
                        .map(plugin_display_name)
                        .collect();
                    if !non_builtin.is_empty() {
                        let detail = format!(
                            "Claude system/init loaded non-builtin plugins: {}",
                            non_builtin.join(", ")
                        );
                        return self.fail_or_fast_stop(detail);
                    }
                }

                // Verify `init.mcp_servers` lists only servers granted in `spec.mcp_servers` (S1).
                if let Some(servers) = val.get("mcp_servers").and_then(Value::as_array) {
                    let unlisted: Vec<String> = servers
                        .iter()
                        .filter_map(mcp_server_name)
                        .filter(|name| !self.allowed_mcp_servers.contains(name))
                        .collect();
                    if !unlisted.is_empty() {
                        let detail = format!(
                            "Claude system/init exposed unlisted MCP servers: {}",
                            unlisted.join(", ")
                        );
                        return self.fail_or_fast_stop(detail);
                    }
                }

                // Verify remote control is not active on any `system/init` (S1).
                if remote_control_active(val) {
                    return self.fail_or_fast_stop(
                        "Claude system/init reported remote control active".to_owned(),
                    );
                }

                if let Some(reported_mode) = val.get("permissionMode").and_then(Value::as_str) {
                    if reported_mode == "bypassPermissions" {
                        // Banned everywhere: fast-stop instead of emitting ModeChanged (B1).
                        if self.phase == Phase::Opening {
                            self.init_deadline = None;
                            self.phase = Phase::Failed;
                            return vec![Output::Event(SessionEvent::StartFailed {
                                reason: StartFailure::ModeMismatch {
                                    requested: self.expected_mode.reported_name.to_owned(),
                                    reported: reported_mode.to_owned(),
                                },
                            })];
                        }
                        return self.fail_or_fast_stop(
                            "Claude system/init reported banned permission mode `bypassPermissions`"
                                .to_owned(),
                        );
                    }
                    if !reported_mode.is_empty()
                        && !permission_mode_matches(&self.expected_mode, reported_mode)
                    {
                        if self.phase == Phase::Opening {
                            self.init_deadline = None;
                            self.phase = Phase::Failed;
                            return vec![Output::Event(SessionEvent::StartFailed {
                                reason: StartFailure::ModeMismatch {
                                    requested: self.expected_mode.reported_name.to_owned(),
                                    reported: reported_mode.to_owned(),
                                },
                            })];
                        }
                        out.push(Output::Event(SessionEvent::ModeChanged {
                            mode: Some(reported_mode.to_owned()),
                        }));
                    }
                }

                if let Some(raw_sid) = val.get("session_id").and_then(Value::as_str) {
                    match SessionId::try_from(raw_sid) {
                        Ok(sid) => {
                            if self.emitted_session_id.as_ref() != Some(&sid) {
                                self.emitted_session_id = Some(sid.clone());
                                out.push(Output::Event(SessionEvent::SessionStarted {
                                    runtime_session_id: sid,
                                }));
                            }
                        }
                        Err(_) => {
                            out.push(Output::Event(SessionEvent::ProtocolError {
                                detail: "Claude system/init carried an invalid session_id"
                                    .to_owned(),
                            }));
                        }
                    }
                }
                // Unsolicited turn started by Claude itself (e.g. background bash/sub-agent
                // completing after the previous turn's result; ADR-0025 decision 6).
                if self.phase == Phase::Idle {
                    // Flush any idle-time background sub-agent usage before resetting (S3).
                    if let Some(u) = self.flush_open_usage() {
                        out.push(u);
                    }
                    self.phase = Phase::Busy;
                    self.turn_open = true;
                    self.reset_turn_usage();
                    out.push(Output::Event(SessionEvent::TurnStarted {
                        turn_id: None,
                        solicited: false,
                    }));
                    out.push(Output::Event(SessionEvent::Status(self.busy_status())));
                }
            }
            "task_notification" | "task_started" | "task_updated" => {
                let text = val
                    .get("summary")
                    .or_else(|| val.get("message"))
                    .and_then(Value::as_str)
                    .map(|s| format!("{subtype}: {s}"))
                    .unwrap_or_else(|| subtype.to_owned());
                out.push(Output::Event(SessionEvent::Notice {
                    kind: NoticeKind::Warning,
                    text,
                }));
            }
            "commands_changed" | "background_tasks_changed" | "status" | "thinking_tokens" => {}
            other => {
                out.push(Output::Event(SessionEvent::Unknown {
                    type_name: format!("system/{other}"),
                }));
            }
        }
        out
    }

    fn handle_stream_event(&mut self, val: &Value) -> Vec<Output> {
        let Some(event) = val.get("event").and_then(Value::as_object) else {
            return Vec::new();
        };
        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        match event_type {
            "message_start" => {
                if let Some(msg) = event.get("message").and_then(Value::as_object) {
                    if let Some(model) = msg.get("model").and_then(Value::as_str) {
                        self.turn_model = Some(model.to_owned());
                    }
                    if let Some(id) = msg.get("id").and_then(Value::as_str) {
                        self.streaming_message_id = Some(id.to_owned());
                        if let Some(usage_val) = msg.get("usage") {
                            let parsed = parse_claude_usage(usage_val);
                            self.turn_usage_by_msg
                                .entry(id.to_owned())
                                .and_modify(|existing| existing.merge_max(&parsed))
                                .or_insert(parsed);
                        }
                    }
                }
                Vec::new()
            }
            "content_block_delta" => {
                let Some(delta) = event.get("delta").and_then(Value::as_object) else {
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
            "message_delta" => {
                if let Some(usage_obj) = event.get("usage").and_then(Value::as_object) {
                    if let Some(out_tokens) = usage_obj.get("output_tokens").and_then(Value::as_u64)
                    {
                        if let Some(msg_id) = self.streaming_message_id.clone() {
                            self.turn_usage_by_msg
                                .entry(msg_id)
                                .and_modify(|entry| entry.output = entry.output.max(out_tokens))
                                .or_insert_with(|| MessageUsage {
                                    output: out_tokens,
                                    ..MessageUsage::default()
                                });
                        }
                    }
                }
                Vec::new()
            }
            "message_stop" => {
                self.streaming_message_id = None;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn handle_assistant(&mut self, val: &Value) -> Vec<Output> {
        let Some(msg) = val.get("message").and_then(Value::as_object) else {
            return vec![Output::Event(SessionEvent::ProtocolError {
                detail: "assistant frame missing `message` object".to_owned(),
            })];
        };

        let message_id = msg.get("id").and_then(Value::as_str).map(ToOwned::to_owned);
        if let Some(model) = msg.get("model").and_then(Value::as_str) {
            self.turn_model = Some(model.to_owned());
        }
        if let Some(usage_val) = msg.get("usage") {
            let parsed = parse_claude_usage(usage_val);
            let key = message_id
                .clone()
                .unwrap_or_else(|| format!("anon-{}", self.turn_usage_by_msg.len()));
            self.turn_usage_by_msg
                .entry(key)
                .and_modify(|existing| existing.merge_max(&parsed))
                .or_insert(parsed);
        }

        let mut out = Vec::new();
        if let Some(blocks) = msg.get("content").and_then(Value::as_array) {
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                out.push(Output::Event(SessionEvent::AssistantText {
                                    message_id: message_id.clone(),
                                    text: text.to_owned(),
                                }));
                            }
                        }
                    }
                    Some("tool_use") => {
                        let tool_use_id = block
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        let name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_owned();
                        let input = block.get("input").cloned().unwrap_or(Value::Null);
                        let capability = classify_tool_capability(&name);
                        out.push(Output::Event(SessionEvent::ToolStarted {
                            tool_use_id,
                            name,
                            capability,
                            input,
                        }));
                    }
                    Some("thinking") => {
                        out.push(Output::Event(SessionEvent::Thinking));
                    }
                    _ => {}
                }
            }
        }
        out
    }

    fn handle_user(&mut self, val: &Value) -> Vec<Output> {
        let mut out = Vec::new();
        if let Some(blocks) = val
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                    let tool_use_id = block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    let is_error = block
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    out.push(Output::Event(SessionEvent::ToolFinished {
                        tool_use_id,
                        is_error,
                    }));
                }
            }
        }
        out
    }

    fn handle_rate_limit(&mut self, val: &Value) -> Vec<Output> {
        let Some(info) = val.get("rate_limit_info").and_then(Value::as_object) else {
            return Vec::new();
        };
        let top_status = parse_limit_status(info.get("status").and_then(Value::as_str));
        let mut out = Vec::new();

        if let Some(windows) = info.get("unifiedWindows").and_then(Value::as_object) {
            for (name, wval) in windows {
                let window = match name.as_str() {
                    "five_hour" => Window::FiveHour,
                    "seven_day" => Window::SevenDay,
                    other => Window::Other {
                        id: other.to_owned(),
                        mins: None,
                    },
                };
                let status = wval
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|s| parse_limit_status(Some(s)))
                    .unwrap_or(top_status);
                let mut rl = RateLimit::new(window, status);
                rl.used_fraction = wval.get("utilization").and_then(Value::as_f64);
                rl.resets_at = wval
                    .get("resetsAt")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                out.push(Output::Event(SessionEvent::RateLimit(rl)));
            }
        } else if let Some(rl_type) = info.get("rateLimitType").and_then(Value::as_str) {
            let window = match rl_type {
                "five_hour" => Window::FiveHour,
                "seven_day" => Window::SevenDay,
                other => Window::Other {
                    id: other.to_owned(),
                    mins: None,
                },
            };
            let mut rl = RateLimit::new(window, top_status);
            rl.used_fraction = info.get("utilization").and_then(Value::as_f64);
            rl.resets_at = info
                .get("resetsAt")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            out.push(Output::Event(SessionEvent::RateLimit(rl)));
        }
        out
    }

    fn handle_result(&mut self, val: &Value) -> Vec<Output> {
        let mut out = Vec::new();

        if let Some(raw_sid) = val.get("session_id").and_then(Value::as_str) {
            if let Ok(sid) = SessionId::try_from(raw_sid) {
                if self.emitted_session_id.as_ref() != Some(&sid) {
                    self.emitted_session_id = Some(sid.clone());
                    out.push(Output::Event(SessionEvent::SessionStarted {
                        runtime_session_id: sid,
                    }));
                }
            }
        }

        if let Some(denials) = val.get("permission_denials").and_then(Value::as_array) {
            for denial in denials {
                let tool = denial
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                let tool_use_id = denial
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                let message = denial
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                out.push(Output::Event(SessionEvent::ActionDenied {
                    tool,
                    tool_use_id,
                    message,
                }));
            }
        }

        // Always emit Usage BEFORE TurnEnded (usage rule).
        let msg_totals = self
            .sum_turn_usage()
            .or_else(|| val.get("usage").map(parse_claude_usage));
        let cost_usd = val.get("total_cost_usd").and_then(Value::as_f64);
        if msg_totals.is_some() || cost_usd.is_some() {
            let mut usage = Usage::new(UsageScope::TurnDelta, UsageScope::Cumulative);
            usage.model = self.turn_model.clone();
            if let Some(t) = msg_totals {
                usage.input = t.input;
                usage.output = t.output;
                usage.cache_read = t.cache_read;
                usage.cache_write = t.cache_write;
                usage.reasoning = t.reasoning;
            }
            if let Some(cost) = cost_usd {
                usage.cost_usd = Some(cost);
                usage.cost_basis = CostBasis::ListPriceEstimate;
            }
            out.push(Output::Event(SessionEvent::Usage(usage)));
        }

        let (outcome, errors) = map_result_outcome(val);
        self.reset_turn_usage();
        // Pending approvals stay answerable: a background sub-agent's `can_use_tool` outlives
        // the main turn's `result` (ADR-0025), and Claude has not withdrawn it. Only the
        // process ending (stdout closed or exit) cancels them.
        match self.phase {
            Phase::Busy => {
                self.phase = Phase::Idle;
                self.turn_open = false;
                out.push(Output::Event(SessionEvent::TurnEnded { outcome, errors }));
                let status = if self.pending_approvals.is_empty() {
                    Activity::Idle
                } else {
                    Activity::WaitingOnApproval
                };
                out.push(Output::Event(SessionEvent::Status(status)));
            }
            Phase::Closing => out.extend(self.end_closed_turn(Some((outcome, errors)))),
            Phase::Opening | Phase::Idle | Phase::Failed | Phase::Ended => {}
        }
        out
    }

    fn flush_open_usage(&mut self) -> Option<Output> {
        let totals = self.sum_turn_usage()?;
        let mut usage = Usage::new(UsageScope::TurnDelta, UsageScope::Cumulative);
        usage.model = self.turn_model.clone();
        usage.input = totals.input;
        usage.output = totals.output;
        usage.cache_read = totals.cache_read;
        usage.cache_write = totals.cache_write;
        usage.reasoning = totals.reasoning;
        self.reset_turn_usage();
        Some(Output::Event(SessionEvent::Usage(usage)))
    }
}

impl Session for ClaudeSession {
    fn start(&mut self, now: Instant) -> Vec<Output> {
        if self.phase != Phase::Opening || self.init_request_id.is_some() {
            return Vec::new();
        }
        let req_id = format!("host-{}", self.next_req_id);
        self.next_req_id += 1;
        let payload = json!({
            "type": "control_request",
            "request_id": req_id,
            "request": { "subtype": "initialize" }
        });
        let line = encode_line(&payload).expect("static initialize JSON encodes cleanly");
        self.init_request_id = Some(req_id);
        self.init_deadline = Some(now + INITIALIZE_TIMEOUT);
        vec![Output::Write(line)]
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        let Ok(text) = std::str::from_utf8(line) else {
            return vec![Output::Event(SessionEvent::ProtocolError {
                detail: "Claude stdout line was not valid UTF-8".to_owned(),
            })];
        };
        if text.trim().is_empty() {
            return Vec::new();
        }
        match serde_json::from_str::<Value>(text) {
            Ok(val) => self.handle_frame(val),
            Err(_) => vec![Output::Event(SessionEvent::ProtocolError {
                detail: "Claude stdout line was not valid JSON".to_owned(),
            })],
        }
    }

    fn on_truncated(&mut self, len: usize, _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        vec![Output::Event(SessionEvent::Truncated { len })]
    }

    fn on_stderr_line(&mut self, _line: &[u8], _now: Instant) -> Vec<Output> {
        // Stderr lines are logged by the pump; they never end a Claude session.
        Vec::new()
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
                out.push(Output::Event(SessionEvent::StartFailed {
                    reason: StartFailure::Other {
                        detail: "Claude stdout closed before initialize response".to_owned(),
                    },
                }));
            }
            Phase::Busy => {
                if let Some(u) = self.flush_open_usage() {
                    out.push(u);
                }
                self.phase = Phase::Idle;
                self.turn_open = false;
                out.push(Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Failed {
                        reason: "Claude stdout closed during turn".to_owned(),
                    },
                    errors: Vec::new(),
                }));
                out.push(Output::Event(SessionEvent::Status(Activity::Idle)));
            }
            Phase::Idle => {
                if let Some(u) = self.flush_open_usage() {
                    out.push(u);
                }
            }
            Phase::Closing => {
                if let Some(u) = self.flush_open_usage() {
                    out.push(u);
                }
                out.extend(self.end_closed_turn(None));
            }
            Phase::Failed | Phase::Ended => {}
        }
        out
    }

    fn on_exit(&mut self, exit: ExitInfo, _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        self.init_deadline = None;
        let mut out = self.cancel_pending_approvals();
        match self.phase {
            Phase::Opening => {
                out.push(Output::Event(SessionEvent::StartFailed {
                    reason: StartFailure::Other {
                        detail: "Claude exited before initialize response".to_owned(),
                    },
                }));
            }
            Phase::Busy => {
                if let Some(u) = self.flush_open_usage() {
                    out.push(u);
                }
                self.turn_open = false;
                out.push(Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Failed {
                        reason: "Claude exited during turn".to_owned(),
                    },
                    errors: Vec::new(),
                }));
            }
            Phase::Idle => {
                if let Some(u) = self.flush_open_usage() {
                    out.push(u);
                }
            }
            Phase::Closing => {
                if let Some(u) = self.flush_open_usage() {
                    out.push(u);
                }
                out.extend(self.end_closed_turn(None));
            }
            Phase::Failed | Phase::Ended => {}
        }
        self.phase = Phase::Ended;
        out.push(Output::Event(SessionEvent::Ended { exit }));
        out
    }

    fn on_tick(&mut self, now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        if self.phase == Phase::Opening {
            if let Some(deadline) = self.init_deadline {
                if now >= deadline {
                    self.init_deadline = None;
                    self.phase = Phase::Failed;
                    return vec![Output::Event(SessionEvent::StartFailed {
                        reason: StartFailure::ProtocolTimeout,
                    })];
                }
            }
        }
        Vec::new()
    }

    fn control(&mut self, control: Control, _now: Instant) -> Result<Vec<Output>, ControlError> {
        if matches!(self.phase, Phase::Closing | Phase::Ended) {
            return Err(ControlError::Closed);
        }

        match control {
            Control::Send { text, urgent } => match self.phase {
                Phase::Opening | Phase::Failed => Err(ControlError::NotReady),
                Phase::Busy => {
                    if urgent {
                        Err(ControlError::NeedsInterrupt)
                    } else {
                        Err(ControlError::Busy)
                    }
                }
                Phase::Idle => {
                    let payload = json!({
                        "type": "user",
                        "message": {
                            "role": "user",
                            "content": text,
                        }
                    });
                    let line = encode_line(&payload).map_err(|_| ControlError::Unsupported)?;
                    // Flush any idle-time background sub-agent usage before starting the turn (S3).
                    let mut out = Vec::new();
                    if let Some(u) = self.flush_open_usage() {
                        out.push(u);
                    }
                    self.phase = Phase::Busy;
                    self.turn_open = true;
                    self.reset_turn_usage();
                    out.push(Output::Write(line));
                    out.push(Output::Event(SessionEvent::TurnStarted {
                        turn_id: None,
                        solicited: true,
                    }));
                    out.push(Output::Event(SessionEvent::Status(self.busy_status())));
                    Ok(out)
                }
                Phase::Closing | Phase::Ended => Err(ControlError::Closed),
            },
            Control::Interrupt => match self.phase {
                Phase::Opening | Phase::Failed => Err(ControlError::NotReady),
                Phase::Idle | Phase::Busy => {
                    let req_id = format!("host-{}", self.next_req_id);
                    let payload = json!({
                        "type": "control_request",
                        "request_id": req_id,
                        "request": { "subtype": "interrupt" }
                    });
                    let line = encode_line(&payload).map_err(|_| ControlError::Unsupported)?;
                    self.next_req_id += 1;
                    Ok(vec![Output::Write(line)])
                }
                Phase::Closing | Phase::Ended => Err(ControlError::Closed),
            },
            Control::Answer {
                request_id,
                decision,
            } => {
                if !self.pending_approvals.contains(&request_id) {
                    return Err(ControlError::UnknownApproval);
                }
                let decision_json = match decision {
                    Decision::AllowOnce => json!({ "behavior": "allow" }),
                    Decision::Deny { message, stop_turn } => {
                        let msg = message
                            .filter(|m| !m.is_empty())
                            .unwrap_or_else(|| "Denied by office policy".to_owned());
                        let mut body = json!({
                            "behavior": "deny",
                            "message": msg,
                        });
                        if stop_turn {
                            body["interrupt"] = Value::Bool(true);
                        }
                        body
                    }
                    _ => return Err(ControlError::Unsupported),
                };
                let payload = json!({
                    "type": "control_response",
                    "response": {
                        "subtype": "success",
                        "request_id": request_id.as_str(),
                        "response": decision_json,
                    }
                });
                let line = encode_line(&payload).map_err(|_| ControlError::Unsupported)?;
                self.pending_approvals.remove(&request_id);
                let mut out = vec![Output::Write(line)];
                if self.pending_approvals.is_empty() {
                    match self.phase {
                        Phase::Busy => {
                            out.push(Output::Event(SessionEvent::Status(Activity::Busy)));
                        }
                        Phase::Idle => {
                            out.push(Output::Event(SessionEvent::Status(Activity::Idle)));
                        }
                        _ => {}
                    }
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
}

fn extract_agent_names(val: Option<&Value>) -> Vec<String> {
    let Some(arr) = val.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            item.as_str().map(ToOwned::to_owned).or_else(|| {
                item.get("name")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
        })
        .collect()
}

fn agent_matches(val: Option<&Value>, expected_name: &str, expected_description: &str) -> bool {
    let Some(arr) = val.and_then(Value::as_array) else {
        return false;
    };
    arr.iter().any(|item| {
        if let Some(name) = item.as_str() {
            return name == expected_name;
        }
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            return false;
        };
        if name != expected_name {
            return false;
        }
        match item.get("description").and_then(Value::as_str) {
            Some(desc) => desc == expected_description,
            None => true,
        }
    })
}

fn is_builtin_plugin(val: &Value) -> bool {
    let Some(obj) = val.as_object() else {
        return false;
    };
    let source = obj.get("source").and_then(Value::as_str);
    let path = obj.get("path").and_then(Value::as_str);
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty());
    let builtin_2_1_323 = path == Some("builtin")
        && name.is_some()
        && source.and_then(|s| s.strip_suffix("@builtin")) == name;
    obj.get("builtin").and_then(Value::as_bool) == Some(true)
        || obj.get("isBuiltin").and_then(Value::as_bool) == Some(true)
        || source == Some("builtin")
        // Claude Code 2.1.323+ emits `{"name": "<id>", "path": "builtin", "source": "<id>@builtin"}`.
        || builtin_2_1_323
}

fn plugin_display_name(val: &Value) -> String {
    val.as_str()
        .or_else(|| val.get("name").and_then(Value::as_str))
        .unwrap_or("<unnamed>")
        .to_owned()
}

fn mcp_server_name(val: &Value) -> Option<String> {
    val.as_str()
        .or_else(|| val.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

fn remote_control_active(body: &Value) -> bool {
    [
        "remote_control_active",
        "remote_control_enabled",
        "remote_control_auto_enable",
        "remote_control_auto_connect_default",
        "remote_control_auto_on_by_default",
    ]
    .iter()
    .any(|key| body.get(key).and_then(Value::as_bool) == Some(true))
}

fn permission_mode_matches(expected: &ExpectedMode, reported: &str) -> bool {
    if reported.is_empty() {
        return false;
    }
    reported == expected.reported_name || reported == expected.cli_flag
}

fn classify_tool_capability(tool_name: &str) -> Option<ToolCapability> {
    match tool_name {
        "Read" => Some(ToolCapability::Read),
        "Glob" | "Grep" | "LS" => Some(ToolCapability::Search),
        // `NotebookEdit` stays excluded from `ToolCapability::Edit` so it remains in
        // `RISKY_BUILTIN_TOOLS` and never auto-allows via `cap:edit`.
        "Edit" | "Write" | "MultiEdit" => Some(ToolCapability::Edit),
        "Bash" => Some(ToolCapability::Shell),
        "Task" | "Agent" => Some(ToolCapability::Subagent),
        _ => None,
    }
}

fn classify_approval_kind(tool_name: &str) -> ApprovalKind {
    match tool_name {
        "Bash" => ApprovalKind::Command,
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => ApprovalKind::FileChange,
        "ExitPlanMode" | "EnterPlanMode" => ApprovalKind::Permissions,
        "Read" | "Glob" | "Grep" | "LS" | "Agent" | "Task" | "TaskOutput" | "TaskStop" => {
            ApprovalKind::Other
        }
        other if other.starts_with("mcp__") => ApprovalKind::Other,
        _ => ApprovalKind::Unknown,
    }
}

fn classify_escalation(tool_name: &str, kind: ApprovalKind, input: &Value) -> Escalation {
    if input
        .get("dangerouslyDisableSandbox")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Escalation::SandboxBypass;
    }
    if kind == ApprovalKind::Permissions || matches!(tool_name, "ExitPlanMode" | "EnterPlanMode") {
        return Escalation::PermissionChange;
    }
    if kind == ApprovalKind::Unknown {
        return Escalation::Unknown;
    }
    Escalation::None
}

fn parse_claude_usage(val: &Value) -> MessageUsage {
    let reasoning = val
        .get("output_tokens_details")
        .and_then(|d| d.get("thinking_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    MessageUsage {
        input: val.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
        output: val
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read: val
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_write: val
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        reasoning,
    }
}

fn parse_limit_status(raw: Option<&str>) -> LimitStatus {
    match raw {
        Some("allowed") => LimitStatus::Ok,
        Some("allowed_warning" | "warning") => LimitStatus::Warning,
        Some("rejected" | "rate_limited") => LimitStatus::Rejected,
        _ => LimitStatus::Unknown,
    }
}

fn map_result_outcome(val: &Value) -> (TurnOutcome, Vec<String>) {
    let subtype = val.get("subtype").and_then(Value::as_str).unwrap_or("");
    let is_error = val
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let terminal_reason = val
        .get("terminal_reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    let stop_reason = val.get("stop_reason").and_then(Value::as_str).unwrap_or("");
    let result_text = val.get("result").and_then(Value::as_str).unwrap_or("");

    let mut errors = Vec::new();
    if let Some(arr) = val.get("errors").and_then(Value::as_array) {
        for item in arr {
            if let Some(s) = item.as_str() {
                errors.push(s.to_owned());
            }
        }
    }

    if matches!(terminal_reason, "aborted_tools" | "aborted_streaming") {
        return (TurnOutcome::Interrupted, errors);
    }
    if result_text.contains("Not logged in") || result_text.contains("Please run /login") {
        return (TurnOutcome::SignedOut, errors);
    }
    if stop_reason == "refusal" {
        return (TurnOutcome::Refused, errors);
    }
    if subtype == "error_max_turns" {
        return (
            TurnOutcome::Limit {
                kind: LimitKind::MaxTurns,
            },
            errors,
        );
    }
    if stop_reason == "max_tokens" {
        return (
            TurnOutcome::Limit {
                kind: LimitKind::MaxTokens,
            },
            errors,
        );
    }
    if subtype == "error_max_budget_usd" {
        return (TurnOutcome::Quota, errors);
    }
    if is_error || subtype.starts_with("error") {
        let reason = if !result_text.is_empty() {
            result_text.to_owned()
        } else if !errors.is_empty() {
            errors.join("; ")
        } else {
            format!("Claude turn failed ({subtype})")
        };
        return (TurnOutcome::Failed { reason }, errors);
    }
    (TurnOutcome::Completed, errors)
}

#[cfg(test)]
mod plugin_tests {
    use super::is_builtin_plugin;
    use serde_json::json;

    #[test]
    fn accepts_claude_2_1_323_builtin_plugins_and_rejects_disk_spoofs() {
        assert!(is_builtin_plugin(&json!({
            "name": "cc-plugin-agents-md",
            "path": "builtin",
            "source": "cc-plugin-agents-md@builtin"
        })));
        assert!(is_builtin_plugin(&json!({
            "name": "legacy",
            "source": "builtin"
        })));
        assert!(!is_builtin_plugin(&json!({
            "name": "spoofed",
            "path": "/home/user/.claude/plugins/spoofed",
            "source": "spoofed@builtin"
        })));
        assert!(!is_builtin_plugin(&json!({
            "name": "user-plugin",
            "path": "/home/user/.claude/plugins/user-plugin",
            "source": "user-plugin@marketplace"
        })));
        assert!(!is_builtin_plugin(&json!({
            "name": "",
            "path": "builtin",
            "source": "@builtin"
        })));
        assert!(!is_builtin_plugin(&json!({
            "name": "evil",
            "path": "builtin",
            "source": "other@builtin"
        })));
    }
}
