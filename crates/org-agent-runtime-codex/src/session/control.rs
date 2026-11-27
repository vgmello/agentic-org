//! The office's controls: send, deliver mid-turn, interrupt, and answer an approval.

use serde_json::{json, Value};

use org_agent_runtime::{
    encode_line, Activity, ApprovalId, ControlError, Decision, Output, SessionEvent, TurnImage,
    TurnImageSource,
};

use crate::protocol::encode_approval_result;

use super::handshake::granular_approval_policy_json;
use super::{CodexSession, Phase};

fn build_codex_turn_input(
    text: String,
    images: Vec<TurnImage>,
) -> Result<Vec<Value>, ControlError> {
    let mut items = Vec::with_capacity(images.len() + 1);
    items.push(json!({ "type": "text", "text": text }));
    for img in images {
        let TurnImageSource::LocalPath { path } = img.source else {
            return Err(ControlError::Unsupported);
        };
        items.push(json!({
            "type": "localImage",
            "path": path.as_str(),
        }));
    }
    Ok(items)
}

pub(super) fn handle_send(
    s: &mut CodexSession,
    text: String,
    images: Vec<TurnImage>,
) -> Result<Vec<Output>, ControlError> {
    match s.phase {
        Phase::OpeningInit | Phase::OpeningThread => Err(ControlError::NotReady),
        Phase::Busy => Err(ControlError::Busy),
        Phase::Idle => {
            let tid = s
                .thread_id
                .as_ref()
                .ok_or(ControlError::NotReady)?
                .as_str()
                .to_owned();
            let input = build_codex_turn_input(text, images)?;
            let req_id = s.next_id;
            let mut params = json!({
                "threadId": tid,
                "approvalPolicy": granular_approval_policy_json(),
                "approvalsReviewer": "user",
                "input": input
            });
            if s.cfg.uses_office_profile {
                params["permissions"] = Value::String("office".to_owned());
            }
            let line = encode_line(&json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "method": "turn/start",
                "params": params
            }))
            .map_err(|_| ControlError::Unsupported)?;
            s.alloc_id();
            s.phase = Phase::Busy;
            s.turn_open = true;
            s.turn_start_req_id = Some(req_id);
            s.turn_tokens = None;
            Ok(vec![
                Output::Write(line),
                Output::Event(SessionEvent::TurnStarted {
                    turn_id: None,
                    solicited: true,
                }),
                Output::Event(SessionEvent::Status(s.busy_status())),
            ])
        }
        _ => Err(ControlError::Closed),
    }
}

pub(super) fn handle_deliver_mid_turn(
    s: &mut CodexSession,
    delivery_id: String,
    text: String,
    images: Vec<TurnImage>,
) -> Result<Vec<Output>, ControlError> {
    match s.phase {
        Phase::OpeningInit | Phase::OpeningThread => Err(ControlError::NotReady),
        Phase::Idle => Err(ControlError::InvalidState),
        Phase::Busy => {
            let tid = s
                .thread_id
                .as_ref()
                .ok_or(ControlError::NotReady)?
                .as_str()
                .to_owned();
            let turn_id = s
                .active_turn_id
                .as_ref()
                .ok_or(ControlError::NotReady)?
                .clone();
            let input = build_codex_turn_input(text, images)?;
            let req_id = s.next_id;
            let line = encode_line(&json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "method": "turn/steer",
                "params": {
                    "threadId": tid,
                    "expectedTurnId": turn_id,
                    "input": input
                }
            }))
            .map_err(|_| ControlError::Unsupported)?;
            s.alloc_id();
            s.in_flight_deliveries.insert(req_id, delivery_id);
            Ok(vec![Output::Write(line)])
        }
        _ => Err(ControlError::Closed),
    }
}

pub(super) fn handle_interrupt(s: &mut CodexSession) -> Result<Vec<Output>, ControlError> {
    match s.phase {
        Phase::OpeningInit | Phase::OpeningThread => Err(ControlError::NotReady),
        Phase::Idle => Ok(Vec::new()),
        Phase::Busy => {
            let tid = s
                .thread_id
                .as_ref()
                .ok_or(ControlError::NotReady)?
                .as_str()
                .to_owned();
            let req_id = s.alloc_id();
            let mut params = json!({ "threadId": tid });
            if let Some(turn_id) = &s.active_turn_id {
                params["turnId"] = Value::String(turn_id.clone());
            }
            let line = encode_line(&json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "method": "turn/interrupt",
                "params": params
            }))
            .expect("turn/interrupt encodes");
            Ok(vec![Output::Write(line)])
        }
        _ => Err(ControlError::Closed),
    }
}

pub(super) fn handle_answer(
    s: &mut CodexSession,
    request_id: ApprovalId,
    decision: Decision,
) -> Result<Vec<Output>, ControlError> {
    let Some(rpc_id) = s.pending_approvals.get(&request_id).cloned() else {
        return Err(ControlError::UnknownApproval);
    };
    let result_val = match decision {
        Decision::AllowOnce => json!({ "decision": "accept" }),
        Decision::Deny { message, stop_turn } => {
            let mut obj = if stop_turn {
                json!({ "decision": "cancel" })
            } else {
                json!({ "decision": "decline" })
            };
            if let Some(m) = message {
                obj["reason"] = Value::String(m);
            }
            obj
        }
        _ => return Err(ControlError::Unsupported),
    };
    s.pending_approvals.remove(&request_id);
    let mut out = vec![encode_approval_result(&rpc_id, result_val)];
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
