//! The opening handshake: `initialize`, then `thread/start` or `thread/resume`, with the
//! thread's approval policy and sandbox checked against what the spec asked for.

use std::time::Instant;

use serde_json::{json, Value};

use org_agent_runtime::{encode_line, Output, SessionEvent, SessionId, StartFailure};

use crate::plan::CodexThreadConfig;
use crate::protocol::{extract_sandbox_mode, sandbox_mode_matches};

use super::{CodexSession, Phase, HANDSHAKE_TIMEOUT};

pub(super) fn handle_init_response(s: &mut CodexSession, val: &Value, now: Instant) -> Vec<Output> {
    if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
        s.phase = Phase::Failed;
        s.handshake_deadline = None;
        let detail = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex initialize failed")
            .to_owned();
        return vec![Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::Other { detail },
        })];
    }
    s.runtime_version = val
        .pointer("/result/serverInfo/version")
        .or_else(|| val.pointer("/result/version"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let initialized_notif = encode_line(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }))
    .expect("initialized encodes");

    let thread_req_id = s.alloc_id();
    s.thread_req_id = Some(thread_req_id);
    s.phase = Phase::OpeningThread;
    s.handshake_deadline = Some(now + HANDSHAKE_TIMEOUT);

    let thread_line = encode_thread_open(&s.cfg, thread_req_id);
    vec![Output::Write(initialized_notif), Output::Write(thread_line)]
}

pub(super) fn granular_approval_policy_json() -> Value {
    json!({
        "granular": {
            "sandbox_approval": false,
            "rules": true,
            "mcp_elicitations": false,
            "request_permissions": false,
            "skill_approval": false
        }
    })
}

fn encode_thread_open(cfg: &CodexThreadConfig, req_id: u64) -> org_agent_runtime::Line {
    let mut params = json!({
        "cwd": cfg.cwd,
        "approvalPolicy": granular_approval_policy_json(),
        "approvalsReviewer": "user",
    });
    // Select the named `permissions.office` profile on start/resume when sandbox is enabled so
    // Codex never restores a persisted profile (e.g. `:read-only`) over `office`, and never
    // combine it with legacy `sandbox`/`writableRoots` which override the profile (`doc-29` §5).
    if cfg.uses_office_profile {
        params["permissions"] = Value::String("office".to_owned());
    } else {
        params["sandbox"] = Value::String(cfg.sandbox_mode.to_owned());
        params["writableRoots"] = json!(cfg.writable_roots);
    }
    if let Some(model) = &cfg.model {
        params["model"] = Value::String(model.clone());
    }
    if let Some(effort) = &cfg.effort {
        params["reasoningEffort"] = Value::String(effort.clone());
    }
    if !cfg.instructions.is_empty() {
        params["developerInstructions"] = Value::String(cfg.instructions.clone());
    }
    let method = if let Some(resume) = &cfg.resume_thread_id {
        params["threadId"] = Value::String(resume.as_str().to_owned());
        "thread/resume"
    } else {
        "thread/start"
    };
    encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params
    }))
    .expect("thread open encodes")
}

fn approval_policy_matches_office_granular(policy: Option<&Value>) -> bool {
    let Some(granular) = policy
        .and_then(|v| v.get("granular"))
        .and_then(Value::as_object)
    else {
        return false;
    };
    granular.get("sandbox_approval").and_then(Value::as_bool) == Some(false)
        && granular.get("rules").and_then(Value::as_bool) == Some(true)
        && granular.get("mcp_elicitations").and_then(Value::as_bool) == Some(false)
        && granular.get("request_permissions").and_then(Value::as_bool) == Some(false)
        && matches!(
            granular.get("skill_approval"),
            None | Some(Value::Bool(false))
        )
}

fn format_reported_policy(policy: Option<&Value>) -> String {
    match policy {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

fn validate_thread_modes(cfg: &CodexThreadConfig, val: &Value) -> Result<String, StartFailure> {
    if let Some(reported_sb) = extract_sandbox_mode(val) {
        let lower = reported_sb.to_ascii_lowercase();
        if lower == "danger-full-access" || lower == "dangerfullaccess" {
            return Err(StartFailure::ModeMismatch {
                requested: if cfg.uses_office_profile {
                    "office".to_owned()
                } else {
                    cfg.sandbox_mode.to_owned()
                },
                reported: reported_sb.to_owned(),
            });
        }
    }
    if cfg.uses_office_profile {
        let profile_id = val
            .pointer("/result/activePermissionProfile/id")
            .or_else(|| val.pointer("/result/thread/activePermissionProfile/id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if profile_id != "office" {
            return Err(StartFailure::ModeMismatch {
                requested: "office".to_owned(),
                reported: profile_id.to_owned(),
            });
        }
    } else {
        let reported_sb = extract_sandbox_mode(val).unwrap_or("");
        if reported_sb.is_empty() || !sandbox_mode_matches(cfg.sandbox_mode, reported_sb) {
            return Err(StartFailure::ModeMismatch {
                requested: cfg.sandbox_mode.to_owned(),
                reported: reported_sb.to_owned(),
            });
        }
    }
    let reported_policy = val
        .pointer("/result/approvalPolicy")
        .or_else(|| val.pointer("/result/thread/approvalPolicy"));
    if !approval_policy_matches_office_granular(reported_policy) {
        return Err(StartFailure::ModeMismatch {
            requested: "granular(sandbox_approval=false,rules=true)".to_owned(),
            reported: format_reported_policy(reported_policy),
        });
    }
    Ok("granular:rules".to_owned())
}

pub(super) fn handle_thread_response(s: &mut CodexSession, val: &Value) -> Vec<Output> {
    s.handshake_deadline = None;
    if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
        s.phase = Phase::Failed;
        let detail = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex thread start failed")
            .to_owned();
        let reason = if s.cfg.resume_thread_id.is_some() {
            StartFailure::ResumeFailed { detail }
        } else {
            StartFailure::Other { detail }
        };
        return vec![Output::Event(SessionEvent::StartFailed { reason })];
    }
    let raw_tid = val
        .pointer("/result/thread/id")
        .or_else(|| val.pointer("/result/threadId"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let Ok(tid) = SessionId::try_from(raw_tid) else {
        s.phase = Phase::Failed;
        return vec![
            Output::Event(SessionEvent::ProtocolError {
                detail: "Codex thread/start returned an invalid thread id".to_owned(),
            }),
            Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::Other {
                    detail: "invalid thread id from Codex".to_owned(),
                },
            }),
        ];
    };
    let policy = match validate_thread_modes(&s.cfg, val) {
        Ok(p) => p,
        Err(reason) => {
            s.phase = Phase::Failed;
            return vec![Output::Event(SessionEvent::StartFailed { reason })];
        }
    };
    s.thread_id = Some(tid.clone());
    s.phase = Phase::Idle;
    vec![
        Output::Event(SessionEvent::SessionStarted {
            runtime_session_id: tid,
        }),
        Output::Event(SessionEvent::Ready {
            runtime_version: s.runtime_version.clone(),
            pid: None,
            agents: Vec::new(),
            permission_mode: Some(policy),
        }),
    ]
}
