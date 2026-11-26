//! [`Session`] for [`CodexSession`]: the pump's entry points, routing each frame in.

use std::time::Instant;

use serde_json::{json, Value};

use org_agent_runtime::{
    encode_line, Activity, Control, ControlError, ExitInfo, NoticeKind, Output, Session,
    SessionEvent, StartFailure, TurnOutcome,
};

use super::control::{handle_answer, handle_deliver_mid_turn, handle_interrupt, handle_send};
use super::notifications::{handle_notification, handle_server_request};
use super::responses::handle_rpc_response;
use super::{CodexSession, Phase, HANDSHAKE_TIMEOUT};

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
