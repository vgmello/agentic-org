//! The opening handshake: `initialize`, `session/new` or resume/load, then mode and model.

use serde_json::Value;

use org_agent_runtime::{Output, SessionEvent, SessionId, StartFailure};

use crate::profile::SystemPromptDelivery;
use crate::protocol::{
    encode_session_open, encode_set_mode, encode_set_model, extract_rpc_error_message,
    is_auth_rpc_error, is_banned_mode, rpc_error, rpc_result,
};

use super::{AcpSession, Phase};

pub(super) fn fail_opening_mode_mismatch(
    s: &mut AcpSession,
    reported: &str,
    close_stdin: bool,
) -> Vec<Output> {
    s.handshake_deadline = None;
    s.phase = Phase::Failed;
    let mut out = vec![Output::Event(SessionEvent::StartFailed {
        reason: StartFailure::ModeMismatch {
            requested: s.cfg.expected_mode.reported_name.to_owned(),
            reported: reported.to_owned(),
        },
    })];
    if close_stdin {
        out.push(Output::CloseStdin);
    }
    out
}

fn continue_opening_model_or_ready(s: &mut AcpSession) -> Vec<Output> {
    if let Some(requested) = s.cfg.model.clone() {
        if s.active_model.as_deref() != Some(requested.as_str()) {
            if let Some(sid) = s.session_id.clone() {
                let model_id = s.alloc_id();
                s.model_req_id = Some(model_id);
                s.phase = Phase::OpeningModel;
                return vec![encode_set_model(model_id, sid.as_str(), &requested)];
            }
        }
    }
    finish_opening_ready(s)
}

fn finish_opening_ready(s: &mut AcpSession) -> Vec<Output> {
    s.handshake_deadline = None;
    s.phase = Phase::Idle;
    let mut out = Vec::new();
    if let Some(sid) = s.session_id.clone() {
        out.push(Output::Event(SessionEvent::SessionStarted {
            runtime_session_id: sid,
        }));
    }
    out.push(Output::Event(SessionEvent::Ready {
        runtime_version: s.runtime_version.clone(),
        pid: None,
        agents: vec![s.cfg.agent_name.clone()],
        permission_mode: s.reported_mode.clone(),
    }));
    out
}

pub(super) fn fail_opening(
    s: &mut AcpSession,
    detail: String,
    err_val: Option<&Value>,
) -> Vec<Output> {
    let prev = s.phase;
    s.handshake_deadline = None;
    s.phase = Phase::Failed;
    let reason = if err_val.is_some_and(is_auth_rpc_error) {
        StartFailure::AuthRequired
    } else if prev == Phase::OpeningSession && s.cfg.resume_session_id.is_some() {
        StartFailure::ResumeFailed { detail }
    } else {
        StartFailure::Other { detail }
    };
    vec![Output::Event(SessionEvent::StartFailed { reason })]
}

pub(super) fn handle_init_response(s: &mut AcpSession, val: &Value) -> Vec<Output> {
    s.init_req_id = None;
    if s.phase != Phase::OpeningInit {
        return Vec::new();
    }
    if let Some(err) = rpc_error(val) {
        return fail_opening(
            s,
            format!("ACP initialize failed: {}", extract_rpc_error_message(err)),
            Some(err),
        );
    }
    let Some(result) = rpc_result(val) else {
        return fail_opening(
            s,
            "ACP initialize response missing `result`".to_owned(),
            None,
        );
    };
    let proto_ver = result
        .get("protocolVersion")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if proto_ver < 1 {
        return fail_opening(
            s,
            format!("ACP initialize reported unsupported protocolVersion={proto_ver}"),
            None,
        );
    }
    s.runtime_version = result
        .pointer("/agentInfo/version")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let open_id = s.alloc_id();
    s.session_req_id = Some(open_id);
    s.phase = Phase::OpeningSession;
    let antigravity_prompt =
        if s.cfg.profile.system_prompt == SystemPromptDelivery::MetaAntigravityAppend {
            s.cfg.system_prompt_append.as_deref()
        } else {
            None
        };
    vec![encode_session_open(
        open_id,
        &s.cfg.cwd,
        &s.cfg.mcp_servers,
        antigravity_prompt,
        s.cfg.resume_session_id.as_ref().map(SessionId::as_str),
        s.cfg.profile.resume_method,
    )]
}

fn validate_session_open_mode<'a>(
    s: &mut AcpSession,
    result: &'a Value,
) -> Result<&'a str, Vec<Output>> {
    let Some(mode) = result
        .pointer("/modes/currentModeId")
        .and_then(Value::as_str)
        .filter(|m| !m.trim().is_empty())
    else {
        return Err(fail_opening_mode_mismatch(s, "<missing>", false));
    };
    if is_banned_mode(mode) {
        return Err(fail_opening_mode_mismatch(s, mode, true));
    }
    Ok(mode)
}

pub(super) fn handle_session_open_response(s: &mut AcpSession, val: &Value) -> Vec<Output> {
    s.session_req_id = None;
    if s.phase != Phase::OpeningSession {
        return Vec::new();
    }
    if let Some(err) = rpc_error(val) {
        return fail_opening(
            s,
            format!(
                "ACP session open failed: {}",
                extract_rpc_error_message(err)
            ),
            Some(err),
        );
    }
    let Some(result) = rpc_result(val) else {
        return fail_opening(
            s,
            "ACP session open response missing `result`".to_owned(),
            None,
        );
    };
    let mode = match validate_session_open_mode(s, result) {
        Ok(m) => m,
        Err(mismatch) => return mismatch,
    };
    let sid = match result
        .get("sessionId")
        .and_then(Value::as_str)
        .map(SessionId::try_from)
    {
        Some(Ok(id)) => id,
        Some(Err(_)) => {
            return fail_opening(
                s,
                "ACP session open carried invalid sessionId".to_owned(),
                None,
            );
        }
        None => match s.cfg.resume_session_id.clone() {
            Some(resumed) => resumed,
            None => {
                return fail_opening(
                    s,
                    "ACP session/new response missing sessionId".to_owned(),
                    None,
                );
            }
        },
    };
    s.active_model = result
        .pointer("/models/currentModelId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    s.session_id = Some(sid.clone());
    s.reported_mode = Some(mode.to_owned());

    if mode != s.cfg.expected_mode.reported_name {
        let mode_id = s.alloc_id();
        s.mode_req_id = Some(mode_id);
        s.phase = Phase::OpeningMode;
        return vec![encode_set_mode(
            mode_id,
            sid.as_str(),
            s.cfg.expected_mode.reported_name,
        )];
    }
    continue_opening_model_or_ready(s)
}

pub(super) fn handle_set_mode_response(s: &mut AcpSession, val: &Value) -> Vec<Output> {
    s.mode_req_id = None;
    if s.phase != Phase::OpeningMode {
        return Vec::new();
    }
    let err = rpc_error(val);
    if err.is_some() || rpc_result(val).is_none() {
        let expected = s.cfg.expected_mode.reported_name;
        let reported = s
            .reported_mode
            .as_deref()
            .filter(|m| *m != expected)
            .unwrap_or("<rejected>")
            .to_owned();
        return fail_opening_mode_mismatch(s, &reported, err.is_some_and(is_auth_rpc_error));
    }
    s.reported_mode = Some(s.cfg.expected_mode.reported_name.to_owned());
    continue_opening_model_or_ready(s)
}

pub(super) fn handle_set_model_response(s: &mut AcpSession, val: &Value) -> Vec<Output> {
    s.model_req_id = None;
    if s.phase != Phase::OpeningModel {
        return Vec::new();
    }
    if let Some(err) = rpc_error(val) {
        return fail_opening(
            s,
            format!(
                "ACP session/set_model failed: {}",
                extract_rpc_error_message(err)
            ),
            Some(err),
        );
    }
    if rpc_result(val).is_none() {
        return fail_opening(
            s,
            "ACP session/set_model response missing `result`".to_owned(),
            None,
        );
    }
    if let Some(requested) = &s.cfg.model {
        s.active_model = Some(requested.clone());
    }
    finish_opening_ready(s)
}
