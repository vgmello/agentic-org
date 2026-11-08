//! [`ClaudeSession`]: Claude Code's headless stream-json protocol as a sans-I/O
//! [`Session`].
//!
//! ## The protocol (Claude Code 2.1.311, from the spike's fixtures)
//! - **Opening.** The session writes `control_request{subtype:"initialize"}`. Claude answers
//!   with a `control_response` listing its `agents`, its `current_permission_mode`, its
//!   `pid` and `claude_code_version`. No `system/init` comes until the first user line.
//!   Claude silently runs its default agent when `--agent` names one it doesn't know, so
//!   the session checks the agent and the mode here and refuses to go on otherwise.
//! - **A turn.** A `user` line starts it; `system/init` comes first (every turn), then
//!   `assistant` events (one per content block, sharing `message.id`), `user` events with
//!   `tool_result`s, `stream_event`s (partial messages), `rate_limit_event`s, and exactly
//!   one `result`.
//! - **Approvals.** `control_request{subtype:"can_use_tool"}`, answered with a
//!   `control_response` (`allow`, or `deny` with a `message` and maybe `interrupt`). On an
//!   interrupt Claude withdraws a pending one with `control_cancel_request`.
//! - **Interrupt.** `control_request{subtype:"interrupt"}`, acknowledged at once; the turn
//!   ends with `result` `error_during_execution` / `aborted_tools` | `aborted_streaming`.
//!   While idle there's an acknowledgement and no `result`.
//!
//! ## Mapping decisions
//! - `system/init` is the turn's start: [`SessionEvent::TurnStarted`], `solicited` when it
//!   answers a [`Control::Send`]. Turn content that arrives with no turn running (background
//!   work finishing after a `result`) starts an unsolicited turn, so every turn's events
//!   sit between a `TurnStarted` and a `TurnEnded`.
//! - Only the main thread's text and thinking become [`SessionEvent::AssistantText`],
//!   [`SessionEvent::TextDelta`] and [`SessionEvent::Thinking`]; a subagent's (with a
//!   `parent_tool_use_id`) don't. Every tool call, a subagent's included, becomes
//!   [`SessionEvent::ToolStarted`] / [`SessionEvent::ToolFinished`], and every message
//!   counts towards [`SessionEvent::Usage`].
//! - At a `result`: pending approvals are withdrawn ([`SessionEvent::ApprovalCancelled`]),
//!   then the turn's denials Claude made by itself ([`SessionEvent::ActionDenied`]; the ones
//!   that went through the approval bridge aren't repeated), then [`SessionEvent::Usage`]
//!   (one per model), then [`SessionEvent::TurnEnded`].
//! - A turn cut short (stdout ends, or the process exits) ends `Failed`.
//! - Claude's other requests (hooks, MCP messages, elicitations) need content back, which
//!   the port can't give: they're declined with an error response and reported as
//!   [`SessionEvent::Unknown`].

use std::time::{Duration, Instant};

use org_runtime::{
    encode_line, ApprovalId, Control, ControlError, ExitInfo, Line, NoticeKind, Output, Session,
    SessionEvent, SessionId, StartFailure, TurnOutcome,
};
use serde_json::{json, Value};

use crate::{approval, outcome, ratelimit, usage::TurnUsage};

/// How long Claude may take to answer `initialize` (the spike saw about 0.6 s).
pub(crate) const OPEN_TIMEOUT: Duration = Duration::from_secs(30);
/// How long Claude may take to acknowledge an interrupt (the spike saw 1-2 ms).
pub(crate) const INTERRUPT_ACK_TIMEOUT: Duration = Duration::from_secs(10);

/// The prefix of the session's own control request ids (`org-1`, `org-2`, ...).
const REQUEST_ID_PREFIX: &str = "org-";

/// The longest stderr text a start failure quotes.
const MAX_STDERR_DETAIL: usize = 500;

/// `system` subtypes the session reads or knowingly ignores. Any other is reported as
/// [`SessionEvent::Unknown`].
const IGNORED_SYSTEM: &[&str] = &[
    "status",
    "commands_changed",
    "thinking_tokens",
    "task_started",
    "task_notification",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Before `start`.
    New,
    /// `initialize` sent, no answer yet.
    Opening,
    /// Ready: controls are accepted.
    Ready,
    /// The start failed; the session waits for the exit.
    Failed,
    /// `Ended` was emitted.
    Ended,
}

/// The running turn.
#[derive(Debug, Default)]
struct Turn {
    /// Its `system/init` (or other content) arrived, so `TurnStarted` was emitted.
    started: bool,
    /// It answers a `Send`.
    solicited: bool,
    usage: TurnUsage,
    /// Tool calls whose denial the office already knows of: the bridged ones, and the ones
    /// reported by `system/permission_denied`.
    denial_known: Vec<String>,
}

/// An approval raised and not yet answered or withdrawn.
#[derive(Debug, Clone)]
struct Pending {
    /// The id as the office sees it ([`ApprovalId`]).
    request_id: String,
    /// The id as Claude sent it, for the answer.
    wire_id: Value,
}

/// One Claude Code process's protocol.
#[derive(Debug)]
pub(crate) struct ClaudeSession {
    agent: String,
    /// The mode Claude must report, in its words.
    reported_mode: &'static str,
    /// The conversation id the plan gave Claude (`--session-id` / `--resume`).
    planned_session: Option<SessionId>,
    phase: Phase,
    next_request: u64,
    init_request: Option<String>,
    open_deadline: Option<Instant>,
    /// Interrupts sent and not yet acknowledged, with their deadlines.
    interrupts: Vec<(String, Instant)>,
    /// Interrupts whose acknowledgement timed out: a late one is expected, not an error.
    late_interrupts: Vec<String>,
    turn: Option<Turn>,
    approvals: Vec<Pending>,
    /// `Control::Close` was accepted: stdin is closed.
    closing: bool,
    stdout_closed: bool,
    /// The last session id reported with `SessionStarted`.
    session_id: Option<SessionId>,
    /// The permission mode Claude last reported.
    mode: Option<String>,
    /// The writable roots (TMPDIR included) while they're enforced, for the file tools'
    /// approvals.
    writable: Option<Vec<String>>,
}

impl ClaudeSession {
    pub(crate) fn new(
        agent: &str,
        reported_mode: &'static str,
        planned_session: Option<SessionId>,
        writable: Option<Vec<String>>,
    ) -> Self {
        ClaudeSession {
            agent: agent.to_owned(),
            reported_mode,
            planned_session,
            phase: Phase::New,
            next_request: 1,
            init_request: None,
            open_deadline: None,
            interrupts: Vec::new(),
            late_interrupts: Vec::new(),
            turn: None,
            approvals: Vec::new(),
            closing: false,
            stdout_closed: false,
            session_id: None,
            mode: None,
            writable,
        }
    }

    fn request_id(&mut self) -> String {
        let id = format!("{REQUEST_ID_PREFIX}{}", self.next_request);
        self.next_request += 1;
        id
    }

    /// The start failed: say why, and close stdin so Claude doesn't go on (with its default
    /// agent, say).
    fn fail_start(&mut self, reason: StartFailure, out: &mut Vec<Output>) {
        self.phase = Phase::Failed;
        self.open_deadline = None;
        out.push(event(SessionEvent::StartFailed { reason }));
        if !self.closing {
            self.closing = true;
            out.push(Output::CloseStdin);
        }
    }

    /// The running turn, starting an unsolicited one if none is running, and emitting its
    /// `TurnStarted` if it hasn't been yet.
    fn ensure_turn(&mut self, out: &mut Vec<Output>) -> &mut Turn {
        let turn = self.turn.get_or_insert_with(Turn::default);
        if !turn.started {
            turn.started = true;
            out.push(event(SessionEvent::TurnStarted {
                turn_id: None,
                solicited: turn.solicited,
            }));
        }
        turn
    }

    /// Ends the running turn, if any: withdrawn approvals, Claude's own denials, usage, then
    /// `TurnEnded`. `result` is the `result` message, if the turn had one.
    fn end_turn(&mut self, result: Option<&Value>, cut_short: &str, out: &mut Vec<Output>) {
        for pending in std::mem::take(&mut self.approvals) {
            out.push(event(SessionEvent::ApprovalCancelled {
                id: ApprovalId::new(pending.request_id),
            }));
        }
        if self.turn.is_none() {
            return;
        }
        self.ensure_turn(out);
        let turn = self.turn.take().expect("ensure_turn made one");
        if let Some(result) = result {
            for denial in result
                .get("permission_denials")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let tool_use_id = denial.get("tool_use_id").and_then(Value::as_str);
                if tool_use_id.is_some_and(|id| turn.denial_known.iter().any(|k| k == id)) {
                    continue;
                }
                out.push(event(SessionEvent::ActionDenied {
                    tool: str_field(denial, "tool_name").unwrap_or_default(),
                    tool_use_id: tool_use_id.map(str::to_owned),
                    message: None,
                }));
            }
        }
        let costs = result
            .and_then(|r| r.get("model_usage_since_last_result"))
            .and_then(Value::as_object);
        for usage in turn.usage.finish(costs) {
            out.push(event(SessionEvent::Usage(usage)));
        }
        let (outcome, errors) = match result {
            Some(result) => outcome::from_result(result),
            None => (
                TurnOutcome::Failed {
                    reason: cut_short.to_owned(),
                },
                vec![],
            ),
        };
        out.push(event(SessionEvent::TurnEnded { outcome, errors }));
    }

    /// Claude reported its permission mode on a `system/init` (`initialize`'s was checked
    /// at the opening). The one place a change is noticed: a mode other than the last one
    /// reported (e.g. after an approved `ExitPlanMode`) is a [`SessionEvent::ModeChanged`].
    fn on_mode_report(&mut self, mode: &str, out: &mut Vec<Output>) {
        if self.mode.as_deref() != Some(mode) {
            self.mode = Some(mode.to_owned());
            out.push(event(SessionEvent::ModeChanged {
                mode: Some(mode.to_owned()),
            }));
        }
    }

    fn on_initialize(&mut self, response: &Value, out: &mut Vec<Output>) {
        self.init_request = None;
        if str_field(response, "subtype").as_deref() != Some("success") {
            let detail = str_field(response, "error")
                .unwrap_or_else(|| "Claude refused the initialize request".to_owned());
            return self.fail_start(StartFailure::Other { detail }, out);
        }
        let Some(body) = response.get("response").filter(|b| b.is_object()) else {
            let detail = "Claude's initialize reply has no body".to_owned();
            return self.fail_start(StartFailure::Other { detail }, out);
        };
        let agents: Vec<String> = body
            .get("agents")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|a| str_field(a, "name"))
            .collect();
        if !agents.contains(&self.agent) {
            let reason = StartFailure::AgentMismatch {
                requested: self.agent.clone(),
                available: agents,
            };
            return self.fail_start(reason, out);
        }
        let mode = str_field(body, "current_permission_mode");
        if mode.as_deref() != Some(self.reported_mode) {
            let reason = StartFailure::ModeMismatch {
                requested: self.reported_mode.to_owned(),
                reported: mode.unwrap_or_default(),
            };
            return self.fail_start(reason, out);
        }
        self.phase = Phase::Ready;
        self.open_deadline = None;
        self.mode = mode.clone();
        out.push(event(SessionEvent::Ready {
            runtime_version: str_field(body, "claude_code_version"),
            pid: body
                .get("pid")
                .and_then(Value::as_u64)
                .and_then(|p| u32::try_from(p).ok()),
            agents,
            permission_mode: mode,
        }));
    }

    fn on_control_response(&mut self, msg: &Value, out: &mut Vec<Output>) {
        let Some(response) = msg.get("response").filter(|r| r.is_object()) else {
            return protocol_error("a control_response without a response", out);
        };
        let id = str_field(response, "request_id");
        if id.is_some() && id == self.init_request {
            return self.on_initialize(response, out);
        }
        if let Some(pos) = self
            .interrupts
            .iter()
            .position(|(i, _)| Some(i) == id.as_ref())
        {
            self.interrupts.remove(pos);
            if str_field(response, "subtype").as_deref() != Some("success") {
                protocol_error("Claude refused the interrupt", out);
            }
            return;
        }
        if let Some(pos) = self
            .late_interrupts
            .iter()
            .position(|i| Some(i) == id.as_ref())
        {
            self.late_interrupts.remove(pos);
            return;
        }
        protocol_error(
            "a control_response to a request this session didn't send",
            out,
        )
    }

    fn on_control_request(&mut self, msg: &Value, out: &mut Vec<Output>) {
        let wire_id = msg.get("request_id").cloned().unwrap_or(Value::Null);
        let Some(id) = request_id(&wire_id) else {
            return protocol_error("a control_request without a request_id", out);
        };
        let request = msg.get("request").cloned().unwrap_or(Value::Null);
        let subtype = str_field(&request, "subtype").unwrap_or_default();
        if subtype == "can_use_tool" {
            match approval::request(&id, &request, self.writable.as_deref()) {
                Ok(approval) => {
                    // After Close, stdin is gone: Claude denies it itself when it reads EOF.
                    if self.closing {
                        return;
                    }
                    // Two pending requests with one id (`"42"` and `42` read alike) would
                    // make an answer ambiguous: the second is declined.
                    if self.approvals.iter().any(|p| p.request_id == id) {
                        protocol_error("a second pending request with the same id", out);
                        return self.decline(&wire_id, "a request with this id is pending", out);
                    }
                    let turn = self.ensure_turn(out);
                    if let Some(tool_use_id) = &approval.tool_use_id {
                        turn.denial_known.push(tool_use_id.clone());
                    }
                    self.approvals.push(Pending {
                        request_id: id.clone(),
                        wire_id: wire_id.clone(),
                    });
                    out.push(event(SessionEvent::ApprovalRequested(approval)));
                }
                Err(why) => {
                    protocol_error(why, out);
                    self.decline(
                        &wire_id,
                        "the office couldn't read this permission request",
                        out,
                    );
                }
            }
            return;
        }
        // Requests that need content back (hooks, MCP messages, elicitations): declined.
        self.decline(&wire_id, "the office doesn't support this request", out);
        out.push(event(SessionEvent::Unknown {
            type_name: format!("control_request/{subtype}"),
        }));
    }

    /// Answers a request of Claude's with an error (for `can_use_tool`, Claude takes it as
    /// a deny).
    fn decline(&self, id: &Value, why: &str, out: &mut Vec<Output>) {
        if !self.closing {
            out.push(write(&json!({
                "type": "control_response",
                "response": {"subtype": "error", "request_id": id, "error": why},
            })));
        }
    }

    fn on_system(&mut self, msg: &Value, out: &mut Vec<Output>) {
        let subtype = str_field(msg, "subtype").unwrap_or_default();
        match subtype.as_str() {
            "init" => self.on_init(msg, out),
            "permission_denied" => {
                let tool_use_id = str_field(msg, "tool_use_id");
                let turn = self.ensure_turn(out);
                if let Some(id) = &tool_use_id {
                    turn.denial_known.push(id.clone());
                }
                out.push(event(SessionEvent::ActionDenied {
                    tool: str_field(msg, "tool_name").unwrap_or_default(),
                    tool_use_id,
                    message: str_field(msg, "message"),
                }));
            }
            "api_retry" => out.push(event(SessionEvent::Notice {
                kind: NoticeKind::Retry,
                text: retry_text(msg),
            })),
            s if IGNORED_SYSTEM.contains(&s) => {}
            other => out.push(event(SessionEvent::Unknown {
                type_name: format!("system/{other}"),
            })),
        }
    }

    fn on_init(&mut self, msg: &Value, out: &mut Vec<Output>) {
        match msg.get("session_id").and_then(Value::as_str) {
            Some(raw) => match SessionId::try_from(raw) {
                Ok(id) => {
                    if self.session_id.as_ref() != Some(&id) {
                        if self.session_id.is_none()
                            && self.planned_session.as_ref().is_some_and(|p| *p != id)
                        {
                            out.push(event(SessionEvent::Notice {
                                kind: NoticeKind::Warning,
                                text: "Claude's session id isn't the one the session was \
                                       launched with"
                                    .to_owned(),
                            }));
                        }
                        self.session_id = Some(id.clone());
                        out.push(event(SessionEvent::SessionStarted {
                            runtime_session_id: id,
                        }));
                    }
                }
                Err(_) => protocol_error("system/init carries a session id that isn't valid", out),
            },
            None => protocol_error("system/init carries no session id", out),
        }
        self.ensure_turn(out);
        if let Some(mode) = msg.get("permissionMode").and_then(Value::as_str) {
            self.on_mode_report(mode, out);
        }
    }

    fn on_assistant(&mut self, msg: &Value, out: &mut Vec<Output>) {
        let Some(message) = msg.get("message").filter(|m| m.is_object()) else {
            return protocol_error("an assistant event without a message", out);
        };
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            return protocol_error("an assistant message without content", out);
        };
        let main = is_main_thread(msg);
        let message_id = str_field(message, "id");
        self.ensure_turn(out).usage.on_assistant(message);
        for block in content {
            match block.get("type").and_then(Value::as_str) {
                Some("text") if main => match str_field(block, "text") {
                    Some(text) => out.push(event(SessionEvent::AssistantText {
                        message_id: message_id.clone(),
                        text,
                    })),
                    None => protocol_error("a text block without text", out),
                },
                Some("thinking" | "redacted_thinking") if main => {
                    out.push(event(SessionEvent::Thinking))
                }
                Some("tool_use" | "server_tool_use" | "mcp_tool_use") => {
                    match (str_field(block, "id"), str_field(block, "name")) {
                        (Some(tool_use_id), Some(name)) => {
                            out.push(event(SessionEvent::ToolStarted {
                                tool_use_id,
                                name,
                                input: block.get("input").cloned().unwrap_or(Value::Null),
                            }))
                        }
                        _ => protocol_error("a tool_use block without an id or a name", out),
                    }
                }
                _ => {}
            }
        }
    }

    fn on_user(&mut self, msg: &Value, out: &mut Vec<Output>) {
        let Some(content) = msg.pointer("/message/content") else {
            return protocol_error("a user event without content", out);
        };
        // A plain string is an echoed prompt; text blocks are Claude's own markers
        // ("[Request interrupted by user]").
        for block in content.as_array().into_iter().flatten() {
            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let Some(tool_use_id) = str_field(block, "tool_use_id") else {
                protocol_error("a tool_result without a tool_use_id", out);
                continue;
            };
            self.ensure_turn(out);
            out.push(event(SessionEvent::ToolFinished {
                tool_use_id,
                is_error: block.get("is_error").and_then(Value::as_bool) == Some(true),
            }));
        }
    }

    fn on_stream_event(&mut self, msg: &Value, out: &mut Vec<Output>) {
        let Some(ev) = msg.get("event").filter(|e| e.is_object()) else {
            return protocol_error("a stream_event without an event", out);
        };
        let parent = msg.get("parent_tool_use_id").and_then(Value::as_str);
        match ev.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                if let Some(message) = ev.get("message") {
                    self.ensure_turn(out)
                        .usage
                        .on_message_start(parent, message);
                }
            }
            Some("message_delta") => {
                if let Some(usage) = ev.get("usage").filter(|u| u.is_object()) {
                    let id = msg.get("api_message_id").and_then(Value::as_str);
                    self.ensure_turn(out)
                        .usage
                        .on_message_delta(parent, id, usage);
                }
            }
            Some("content_block_delta")
                if parent.is_none()
                    && ev.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") =>
            {
                if let Some(text) = ev.pointer("/delta/text").and_then(Value::as_str) {
                    self.ensure_turn(out);
                    out.push(event(SessionEvent::TextDelta {
                        text: text.to_owned(),
                    }));
                }
            }
            _ => {}
        }
    }

    fn on_message(&mut self, msg: &Value, out: &mut Vec<Output>) {
        match msg.get("type").and_then(Value::as_str) {
            Some("control_response") => self.on_control_response(msg, out),
            Some("control_request") => self.on_control_request(msg, out),
            Some("control_cancel_request") => {
                let id = msg.get("request_id").and_then(request_id);
                if let Some(pos) = self
                    .approvals
                    .iter()
                    .position(|p| Some(&p.request_id) == id.as_ref())
                {
                    let pending = self.approvals.remove(pos);
                    out.push(event(SessionEvent::ApprovalCancelled {
                        id: ApprovalId::new(pending.request_id),
                    }));
                }
            }
            Some("system") => self.on_system(msg, out),
            Some("assistant") => self.on_assistant(msg, out),
            Some("user") => self.on_user(msg, out),
            Some("stream_event") => self.on_stream_event(msg, out),
            Some("result") => self.end_turn(Some(msg), "", out),
            Some("rate_limit_event") => match ratelimit::from_event(msg) {
                Some(limits) => out.extend(
                    limits
                        .into_iter()
                        .map(|l| event(SessionEvent::RateLimit(l))),
                ),
                None => protocol_error("a rate_limit_event without rate_limit_info", out),
            },
            Some(other) => out.push(event(SessionEvent::Unknown {
                type_name: other.to_owned(),
            })),
            None => protocol_error("a message without a type", out),
        }
    }

    /// What the session still owes when the output ends or the process exits.
    fn wind_up(&mut self, why: &str, out: &mut Vec<Output>) {
        self.open_deadline = None;
        self.interrupts.clear();
        if self.phase == Phase::Opening || self.phase == Phase::New {
            self.phase = Phase::Failed;
            out.push(event(SessionEvent::StartFailed {
                reason: StartFailure::Other {
                    detail: format!("{why} before the opening finished"),
                },
            }));
        }
        self.end_turn(None, &format!("{why} during the turn"), out);
    }
}

impl Session for ClaudeSession {
    fn start(&mut self, now: Instant) -> Vec<Output> {
        if self.phase != Phase::New {
            return vec![];
        }
        self.phase = Phase::Opening;
        let id = self.request_id();
        self.init_request = Some(id.clone());
        self.open_deadline = Some(now + OPEN_TIMEOUT);
        vec![write(&json!({
            "type": "control_request",
            "request_id": id,
            "request": {"subtype": "initialize"},
        }))]
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        let mut out = Vec::new();
        if matches!(self.phase, Phase::Ended | Phase::Failed) {
            return out;
        }
        match serde_json::from_slice::<Value>(line) {
            Ok(msg) if msg.is_object() => self.on_message(&msg, &mut out),
            Ok(_) => protocol_error("a line from Claude that isn't a JSON object", &mut out),
            Err(e) => out.push(event(SessionEvent::ProtocolError {
                detail: format!(
                    "a line from Claude isn't valid JSON (at column {})",
                    e.column()
                ),
            })),
        }
        out
    }

    fn on_truncated(&mut self, len: usize, _now: Instant) -> Vec<Output> {
        if self.phase == Phase::Ended {
            return vec![];
        }
        vec![event(SessionEvent::Truncated { len })]
    }

    fn on_stderr_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        let mut out = Vec::new();
        if self.phase != Phase::Opening {
            return out;
        }
        // How Claude says a start failed, before it answers `initialize` (the CLI's own
        // texts, **\[binary\]**; the spike didn't record a failed start).
        let text = String::from_utf8_lossy(line);
        let detail = || {
            text.trim()
                .chars()
                .take(MAX_STDERR_DETAIL)
                .collect::<String>()
        };
        let reason = if text.contains("No conversation found") {
            Some(StartFailure::ResumeFailed { detail: detail() })
        } else if ["Not logged in", "Please run /login", "Invalid API key"]
            .iter()
            .any(|s| text.contains(s))
        {
            Some(StartFailure::AuthRequired)
        } else if text.contains("is already in use") {
            Some(StartFailure::Other { detail: detail() })
        } else {
            None
        };
        if let Some(reason) = reason {
            self.fail_start(reason, &mut out);
        }
        out
    }

    fn on_stdout_closed(&mut self, _now: Instant) -> Vec<Output> {
        let mut out = Vec::new();
        if self.phase == Phase::Ended || self.stdout_closed {
            return out;
        }
        self.stdout_closed = true;
        self.wind_up("Claude's output ended", &mut out);
        out
    }

    fn on_exit(&mut self, exit: ExitInfo, _now: Instant) -> Vec<Output> {
        let mut out = Vec::new();
        if self.phase == Phase::Ended {
            return out;
        }
        self.wind_up("Claude exited", &mut out);
        self.phase = Phase::Ended;
        self.approvals.clear();
        out.push(event(SessionEvent::Ended { exit }));
        out
    }

    fn on_tick(&mut self, now: Instant) -> Vec<Output> {
        let mut out = Vec::new();
        if self.phase == Phase::Ended {
            return out;
        }
        if self.phase == Phase::Opening && self.open_deadline.is_some_and(|d| d <= now) {
            self.fail_start(StartFailure::ProtocolTimeout, &mut out);
        }
        let (due, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut self.interrupts)
            .into_iter()
            .partition(|(_, due)| *due <= now);
        self.interrupts = pending;
        for (id, _) in due {
            self.late_interrupts.push(id);
            protocol_error(
                "Claude didn't acknowledge an interrupt in time; the session may be stuck",
                &mut out,
            );
        }
        out
    }

    fn control(&mut self, control: Control, now: Instant) -> Result<Vec<Output>, ControlError> {
        match self.phase {
            Phase::Ended | Phase::Failed => return Err(ControlError::Closed),
            _ if self.closing => return Err(ControlError::Closed),
            // A graceful close works before Ready too.
            Phase::New | Phase::Opening if control == Control::Close => {
                self.closing = true;
                return Ok(vec![Output::CloseStdin]);
            }
            Phase::New | Phase::Opening => return Err(ControlError::NotReady),
            Phase::Ready => {}
        }
        match control {
            Control::Send { text, urgent } => {
                if self.turn.is_some() {
                    return Err(if urgent {
                        ControlError::NeedsInterrupt
                    } else {
                        ControlError::Busy
                    });
                }
                self.turn = Some(Turn {
                    solicited: true,
                    ..Turn::default()
                });
                Ok(vec![write(&json!({
                    "type": "user",
                    "message": {"role": "user", "content": text},
                }))])
            }
            Control::Interrupt => {
                let id = self.request_id();
                self.interrupts
                    .push((id.clone(), now + INTERRUPT_ACK_TIMEOUT));
                Ok(vec![write(&json!({
                    "type": "control_request",
                    "request_id": id,
                    "request": {"subtype": "interrupt"},
                }))])
            }
            Control::Answer {
                request_id,
                decision,
            } => {
                let pos = self
                    .approvals
                    .iter()
                    .position(|p| p.request_id == request_id.as_str())
                    .ok_or(ControlError::UnknownApproval)?;
                let answer = approval::answer(&self.approvals[pos].wire_id, &decision)
                    .ok_or(ControlError::Unsupported)?;
                self.approvals.remove(pos);
                Ok(vec![write(&answer)])
            }
            Control::Close => {
                self.closing = true;
                let mut out: Vec<Output> = std::mem::take(&mut self.approvals)
                    .into_iter()
                    .map(|p| {
                        event(SessionEvent::ApprovalCancelled {
                            id: ApprovalId::new(p.request_id),
                        })
                    })
                    .collect();
                out.push(Output::CloseStdin);
                Ok(out)
            }
            _ => Err(ControlError::Unsupported),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        if self.phase == Phase::Ended {
            return None;
        }
        let open = self.open_deadline.filter(|_| self.phase == Phase::Opening);
        open.into_iter()
            .chain(self.interrupts.iter().map(|(_, due)| *due))
            .min()
    }
}

fn event(e: SessionEvent) -> Output {
    Output::Event(e)
}

fn write(value: &Value) -> Output {
    Output::Write(line(value))
}

fn line(value: &Value) -> Line {
    // A `Value` always serializes, and compact JSON escapes every line break.
    encode_line(value).expect("a JSON value always encodes as one line")
}

fn protocol_error(detail: &str, out: &mut Vec<Output>) {
    out.push(event(SessionEvent::ProtocolError {
        detail: detail.to_owned(),
    }));
}

/// A request id as the office sees it: Claude's string, or its number written out.
fn request_id(wire: &Value) -> Option<String> {
    match wire {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn is_main_thread(msg: &Value) -> bool {
    msg.get("parent_tool_use_id").is_none_or(Value::is_null)
}

/// A `system/api_retry`'s text, from whatever fields it has (not in the spike's fixtures).
fn retry_text(msg: &Value) -> String {
    let attempt = msg.get("attempt").and_then(Value::as_u64);
    let max = msg.get("max_retries").and_then(Value::as_u64);
    let status = msg.get("error_status").and_then(Value::as_u64);
    let mut text = "Claude is retrying an API request".to_owned();
    if let (Some(a), Some(m)) = (attempt, max) {
        text.push_str(&format!(" (attempt {a} of {m})"));
    }
    if let Some(s) = status {
        text.push_str(&format!(" after HTTP {s}"));
    }
    text
}
