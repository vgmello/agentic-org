//! Shared JSON-RPC 2.0 ACP wire encoding, usage accumulation, and fail-closed classification.

mod classify;
mod encode;

use serde_json::Value;

pub(crate) use self::classify::{
    classify_rpc_error_outcome, classify_stop_reason, extract_rpc_error_message, is_auth_rpc_error,
    is_banned_mode, parse_acp_usage, parse_permission_request, parse_stderr_line,
    resolve_tool_classification,
};
pub(crate) use self::encode::{
    encode_initialize, encode_jsonrpc_error, encode_permission_cancelled,
    encode_permission_selected, encode_session_cancel, encode_session_close, encode_session_open,
    encode_session_prompt, encode_set_mode, encode_set_model,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct AcpUsage {
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) cache_read: u64,
    pub(crate) cache_write: u64,
    pub(crate) reasoning: u64,
}

impl AcpUsage {
    pub(crate) fn add_assign(&mut self, other: &AcpUsage) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
        self.reasoning = self.reasoning.saturating_add(other.reasoning);
    }

    pub(crate) fn merge_max(&mut self, other: &AcpUsage) {
        self.input = self.input.max(other.input);
        self.output = self.output.max(other.output);
        self.cache_read = self.cache_read.max(other.cache_read);
        self.cache_write = self.cache_write.max(other.cache_write);
        self.reasoning = self.reasoning.max(other.reasoning);
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.input == 0
            && self.output == 0
            && self.cache_read == 0
            && self.cache_write == 0
            && self.reasoning == 0
    }
}

/// Returns the non-null JSON-RPC `error` payload, if present.
pub(crate) fn rpc_error(val: &Value) -> Option<&Value> {
    val.get("error").filter(|v| !v.is_null())
}

/// Returns the non-null JSON-RPC `result` payload, if present.
pub(crate) fn rpc_result(val: &Value) -> Option<&Value> {
    val.get("result").filter(|v| !v.is_null())
}

/// Returns the first non-null JSON string across `candidates`, avoiding `null` short-circuiting.
pub(crate) fn first_str<'a>(candidates: &[Option<&'a Value>]) -> Option<&'a str> {
    candidates.iter().find_map(|c| c.and_then(Value::as_str))
}

/// Returns the first non-null JSON `u64` across `candidates`, defaulting to `0`.
pub(crate) fn first_u64(candidates: &[Option<&Value>]) -> u64 {
    candidates
        .iter()
        .find_map(|c| c.and_then(Value::as_u64))
        .unwrap_or(0)
}

/// Converts a JSON-RPC request/response `id` (string or integer) into a stable lookup key.
pub(crate) fn rpc_id_key(id: &Value) -> Option<String> {
    match id {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}
