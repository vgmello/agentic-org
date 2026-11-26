//! [`Session`] for [`ClaudeSession`]: the pump's entry points, routing each frame in.

use std::time::Instant;

use serde_json::{json, Value};

use org_agent_runtime::{
    encode_line, Activity, Control, ControlError, Decision, ExitInfo, Output, Session,
    SessionEvent, StartFailure, TurnOutcome,
};

use super::{
    fail_staged_mid_turn, ClaudeSession, Phase, INITIALIZE_TIMEOUT, POST_TOOL_USE_CALLBACK_ID,
};

impl ClaudeSession {
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
            "request": {
                "subtype": "initialize",
                "hooks": {
                    "PostToolUse": [
                        {
                            "matcher": "",
                            "hookCallbackIds": [POST_TOOL_USE_CALLBACK_ID]
                        }
                    ]
                }
            }
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
                if let Some(fail) =
                    fail_staged_mid_turn(self, "Claude stdout closed before next tool boundary")
                {
                    out.push(fail);
                }
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
                if let Some(fail) =
                    fail_staged_mid_turn(self, "Claude stdout closed before next tool boundary")
                {
                    out.push(fail);
                }
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
                if let Some(fail) =
                    fail_staged_mid_turn(self, "Claude exited before next tool boundary")
                {
                    out.push(fail);
                }
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
                if let Some(fail) =
                    fail_staged_mid_turn(self, "Claude exited before next tool boundary")
                {
                    out.push(fail);
                }
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
            Control::Send { text } => match self.phase {
                Phase::Opening | Phase::Failed => Err(ControlError::NotReady),
                Phase::Busy => Err(ControlError::Busy),
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
            Control::DeliverMidTurn { delivery_id, text } => match self.phase {
                Phase::Opening | Phase::Failed => Err(ControlError::NotReady),
                Phase::Idle => Err(ControlError::InvalidState),
                Phase::Busy => {
                    if self.staged_mid_turn.is_some() {
                        return Err(ControlError::NotReady);
                    }
                    self.staged_mid_turn = Some((delivery_id, text));
                    Ok(Vec::new())
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
