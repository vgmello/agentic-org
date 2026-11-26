//! JSON-RPC responses: routed to the handshake or to the turn's `session/prompt` slots, and
//! how a turn finishes.

use serde_json::Value;

use org_agent_runtime::{Activity, Output, SessionEvent, TurnOutcome};

use crate::protocol::{
    classify_rpc_error_outcome, classify_stop_reason, extract_rpc_error_message, parse_acp_usage,
    rpc_error, rpc_result,
};

use super::handshake::{
    handle_init_response, handle_session_open_response, handle_set_mode_response,
    handle_set_model_response,
};
use super::{AgySession, Phase};

pub(super) fn handle_rpc_response(s: &mut AgySession, val: &Value) -> Vec<Output> {
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

pub(super) fn finish_turn(
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
