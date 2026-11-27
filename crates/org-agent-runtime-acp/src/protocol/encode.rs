//! JSON-RPC 2.0 ACP wire encoders shared across all vendor profiles.

use serde_json::{json, Value};

use org_agent_runtime::{encode_line, Output};

pub(crate) fn encode_initialize(req_id: u64) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": {
                "name": "agentic-org",
                "version": env!("CARGO_PKG_VERSION"),
            },
        },
    }))
    .expect("initialize JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_session_open(
    req_id: u64,
    cwd: &str,
    mcp_servers: &[Value],
    antigravity_system_prompt: Option<&str>,
    resume_session_id: Option<&str>,
    resume_method: &str,
) -> Output {
    let mut params = json!({
        "cwd": cwd,
        "mcpServers": mcp_servers,
    });
    if let Some(prompt) = antigravity_system_prompt {
        params["_meta"] = json!({
            "antigravity": {
                "systemPrompt": {
                    "append": prompt,
                }
            }
        });
    }
    let method = match resume_session_id {
        Some(sid) => {
            params["sessionId"] = Value::String(sid.to_owned());
            resume_method
        }
        None => "session/new",
    };
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params,
    }))
    .expect("session open JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_set_mode(req_id: u64, session_id: &str, mode_id: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "session/set_mode",
        "params": {
            "sessionId": session_id,
            "modeId": mode_id,
        },
    }))
    .expect("session/set_mode JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_set_model(req_id: u64, session_id: &str, model_id: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "session/set_model",
        "params": {
            "sessionId": session_id,
            "modelId": model_id,
        },
    }))
    .expect("session/set_model JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_session_prompt(
    req_id: u64,
    session_id: &str,
    system_prompt: Option<&str>,
    text: &str,
) -> Output {
    let mut blocks = Vec::with_capacity(if system_prompt.is_some() { 2 } else { 1 });
    if let Some(system) = system_prompt {
        blocks.push(json!({ "type": "text", "text": system }));
    }
    blocks.push(json!({ "type": "text", "text": text }));
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": blocks,
        },
    }))
    .expect("session/prompt JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_session_cancel(session_id: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": { "sessionId": session_id },
    }))
    .expect("session/cancel notification encodes");
    Output::Write(line)
}

pub(crate) fn encode_session_close(req_id: u64, session_id: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "session/close",
        "params": { "sessionId": session_id },
    }))
    .expect("session/close JSON-RPC encodes");
    Output::Write(line)
}

pub(crate) fn encode_permission_selected(raw_id: &Value, option_id: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": raw_id,
        "result": {
            "outcome": {
                "outcome": "selected",
                "optionId": option_id,
            }
        }
    }))
    .expect("permission selected response encodes");
    Output::Write(line)
}

pub(crate) fn encode_permission_cancelled(raw_id: &Value) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": raw_id,
        "result": {
            "outcome": {
                "outcome": "cancelled",
            }
        }
    }))
    .expect("permission cancelled response encodes");
    Output::Write(line)
}

pub(crate) fn encode_jsonrpc_error(raw_id: &Value, code: i64, message: &str) -> Output {
    let line = encode_line(&json!({
        "jsonrpc": "2.0",
        "id": raw_id,
        "error": {
            "code": code,
            "message": message,
        }
    }))
    .expect("jsonrpc error encodes");
    Output::Write(line)
}
