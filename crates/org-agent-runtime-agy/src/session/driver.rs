//! [`Session`] for [`AgySession`]: the pump's entry points, routing each frame in.

use std::time::Instant;

use serde_json::Value;

use org_agent_runtime::{
    Control, ControlError, ExitInfo, Output, Session, SessionEvent, StartFailure, TurnOutcome,
};

use crate::protocol::{
    encode_initialize, encode_session_cancel, encode_session_close, parse_stderr_line,
};

use super::control::{control_answer, control_deliver_mid_turn, control_send};
use super::handshake::fail_opening;
use super::responses::{finish_turn, handle_rpc_response};
use super::updates::{handle_notification, handle_server_request};
use super::{is_opening, AgySession, Phase, HANDSHAKE_TIMEOUT};

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
            Control::Send { text, images } => control_send(self, text, images),
            Control::DeliverMidTurn {
                delivery_id,
                text,
                images,
            } => control_deliver_mid_turn(self, delivery_id, text, images),
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
