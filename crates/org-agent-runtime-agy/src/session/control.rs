//! The office's controls: send, deliver mid-turn, and answer an approval.

use org_agent_runtime::{
    Activity, ApprovalId, ControlError, Decision, NoticeKind, Output, SessionEvent,
};

use crate::protocol::{
    encode_permission_cancelled, encode_permission_selected, encode_session_cancel,
    encode_session_prompt, AcpUsage,
};

use super::{is_opening, AgySession, Phase, PromptSlot};

pub(super) fn control_send(s: &mut AgySession, text: String) -> Result<Vec<Output>, ControlError> {
    if is_opening(s.phase) {
        return Err(ControlError::NotReady);
    }
    if s.phase == Phase::Busy {
        return Err(ControlError::Busy);
    }
    let Some(sid) = s.session_id.clone() else {
        return Err(ControlError::NotReady);
    };
    let req_id = s.alloc_id();
    s.prompt_chain = vec![PromptSlot {
        req_id,
        delivery_id: None,
        acked: false,
    }];
    s.phase = Phase::Busy;
    s.turn_open = true;
    s.turn_text_buf.clear();
    s.confirmed_usage = AcpUsage::default();
    s.saw_confirmed_usage = false;
    s.streamed_usage = None;
    Ok(vec![
        encode_session_prompt(req_id, sid.as_str(), &text),
        Output::Event(SessionEvent::TurnStarted {
            turn_id: None,
            solicited: true,
        }),
        Output::Event(SessionEvent::Status(s.busy_status())),
    ])
}

pub(super) fn control_deliver_mid_turn(
    s: &mut AgySession,
    delivery_id: String,
    text: String,
) -> Result<Vec<Output>, ControlError> {
    if is_opening(s.phase) {
        return Err(ControlError::NotReady);
    }
    if s.phase == Phase::Idle {
        return Err(ControlError::InvalidState);
    }
    if s.prompt_chain.is_empty() {
        return Err(ControlError::NotReady);
    }
    let Some(sid) = s.session_id.clone() else {
        return Err(ControlError::NotReady);
    };
    let new_id = s.alloc_id();
    s.prompt_chain.push(PromptSlot {
        req_id: new_id,
        delivery_id: Some(delivery_id),
        acked: false,
    });
    Ok(vec![encode_session_prompt(new_id, sid.as_str(), &text)])
}

pub(super) fn control_answer(
    s: &mut AgySession,
    request_id: ApprovalId,
    decision: Decision,
) -> Result<Vec<Output>, ControlError> {
    if !s.pending_approvals.contains_key(&request_id) {
        return Err(ControlError::UnknownApproval);
    }
    let (use_allow, stop_turn) = match decision {
        Decision::AllowOnce => (true, false),
        Decision::Deny { stop_turn, .. } => (false, stop_turn),
        _ => return Err(ControlError::Unsupported),
    };
    let pending = s
        .pending_approvals
        .remove(&request_id)
        .expect("verified present");
    let mut out = Vec::new();
    let chosen = if use_allow {
        pending.allow_option_id.as_deref()
    } else {
        pending.deny_option_id.as_deref()
    };
    match chosen {
        Some(option_id) => out.push(encode_permission_selected(&pending.raw_id, option_id)),
        None => {
            out.push(encode_permission_cancelled(&pending.raw_id));
            if use_allow {
                out.push(Output::Event(SessionEvent::Notice {
                    kind: NoticeKind::Warning,
                    text: "Agy ACP permission request offered no one-shot allow option; failed closed with cancel".to_owned(),
                }));
            }
        }
    }
    if stop_turn {
        if let Some(sid) = &s.session_id {
            out.push(encode_session_cancel(sid.as_str()));
        }
    }
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
