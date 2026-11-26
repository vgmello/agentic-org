//! JSON-RPC responses to the session's own requests: the handshake's, `turn/start`'s and
//! `turn/steer`'s.

use std::time::Instant;

use serde_json::Value;

use org_agent_runtime::{Activity, NoticeKind, Output, SessionEvent, TurnOutcome};

use super::handshake::{handle_init_response, handle_thread_response};
use super::{CodexSession, Phase};

pub(super) fn handle_rpc_response(
    s: &mut CodexSession,
    id: u64,
    val: &Value,
    now: Instant,
) -> Vec<Output> {
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
