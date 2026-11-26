//! The Claude Code `stream-json` session: [`ClaudeSession`], its state, and the helpers every
//! part of the protocol shares. Each kind of stdout frame is handled in its own submodule.

mod classify;
mod control_frames;
mod driver;
mod result;
mod stream;
mod system;

#[cfg(test)]
mod plugin_tests;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use org_agent_runtime::{
    encode_line, Activity, ApprovalId, NoticeKind, Output, SessionEvent, SessionId, StartFailure,
    TurnOutcome, Usage, UsageScope,
};

use crate::plan::ExpectedMode;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(15);
const POST_TOOL_USE_CALLBACK_ID: &str = "office-mid-turn-post-tool-use";

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
    /// Staged non-destructive mid-turn delivery `(delivery_id, text)` injected on the next
    /// in-band `PostToolUse` `hook_callback` (`doc-101`, `ADR-0035`).
    staged_mid_turn: Option<(String, String)>,
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
            staged_mid_turn: None,
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
        let mut out = Vec::new();
        if let Some(fail) = fail_staged_mid_turn(self, "session stopped before next tool boundary")
        {
            out.push(fail);
        }
        let interrupt = self.encode_interrupt_request();
        out.push(Output::Event(SessionEvent::Notice {
            kind: NoticeKind::Warning,
            text: detail,
        }));
        out.push(interrupt);
        out.push(Output::CloseStdin);
        out
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

fn fail_staged_mid_turn(s: &mut ClaudeSession, reason: &str) -> Option<Output> {
    let (delivery_id, _) = s.staged_mid_turn.take()?;
    Some(Output::Event(SessionEvent::MidTurnDeliveryFailed {
        delivery_id,
        reason: reason.to_owned(),
    }))
}
