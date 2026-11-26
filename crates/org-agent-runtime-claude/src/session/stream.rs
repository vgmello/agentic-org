//! Streaming frames: `stream_event`, `assistant`, `user` and `rate_limit_event`.

use serde_json::Value;

use org_agent_runtime::{Output, RateLimit, SessionEvent, Window};

use super::classify::{classify_tool_capability, parse_claude_usage, parse_limit_status};
use super::{ClaudeSession, MessageUsage};

impl ClaudeSession {
    pub(super) fn handle_stream_event(&mut self, val: &Value) -> Vec<Output> {
        let Some(event) = val.get("event").and_then(Value::as_object) else {
            return Vec::new();
        };
        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        match event_type {
            "message_start" => {
                if let Some(msg) = event.get("message").and_then(Value::as_object) {
                    if let Some(model) = msg.get("model").and_then(Value::as_str) {
                        self.turn_model = Some(model.to_owned());
                    }
                    if let Some(id) = msg.get("id").and_then(Value::as_str) {
                        self.streaming_message_id = Some(id.to_owned());
                        if let Some(usage_val) = msg.get("usage") {
                            let parsed = parse_claude_usage(usage_val);
                            self.turn_usage_by_msg
                                .entry(id.to_owned())
                                .and_modify(|existing| existing.merge_max(&parsed))
                                .or_insert(parsed);
                        }
                    }
                }
                Vec::new()
            }
            "content_block_delta" => {
                let Some(delta) = event.get("delta").and_then(Value::as_object) else {
                    return Vec::new();
                };
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => delta
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|t| !t.is_empty())
                        .map(|t| {
                            vec![Output::Event(SessionEvent::TextDelta {
                                text: t.to_owned(),
                            })]
                        })
                        .unwrap_or_default(),
                    Some("thinking_delta") => vec![Output::Event(SessionEvent::Thinking)],
                    _ => Vec::new(),
                }
            }
            "message_delta" => {
                if let Some(usage_obj) = event.get("usage").and_then(Value::as_object) {
                    if let Some(out_tokens) = usage_obj.get("output_tokens").and_then(Value::as_u64)
                    {
                        if let Some(msg_id) = self.streaming_message_id.clone() {
                            self.turn_usage_by_msg
                                .entry(msg_id)
                                .and_modify(|entry| entry.output = entry.output.max(out_tokens))
                                .or_insert_with(|| MessageUsage {
                                    output: out_tokens,
                                    ..MessageUsage::default()
                                });
                        }
                    }
                }
                Vec::new()
            }
            "message_stop" => {
                self.streaming_message_id = None;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn handle_assistant(&mut self, val: &Value) -> Vec<Output> {
        let Some(msg) = val.get("message").and_then(Value::as_object) else {
            return vec![Output::Event(SessionEvent::ProtocolError {
                detail: "assistant frame missing `message` object".to_owned(),
            })];
        };

        let message_id = msg.get("id").and_then(Value::as_str).map(ToOwned::to_owned);
        if let Some(model) = msg.get("model").and_then(Value::as_str) {
            self.turn_model = Some(model.to_owned());
        }
        if let Some(usage_val) = msg.get("usage") {
            let parsed = parse_claude_usage(usage_val);
            let key = message_id
                .clone()
                .unwrap_or_else(|| format!("anon-{}", self.turn_usage_by_msg.len()));
            self.turn_usage_by_msg
                .entry(key)
                .and_modify(|existing| existing.merge_max(&parsed))
                .or_insert(parsed);
        }

        let mut out = Vec::new();
        if let Some(blocks) = msg.get("content").and_then(Value::as_array) {
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                out.push(Output::Event(SessionEvent::AssistantText {
                                    message_id: message_id.clone(),
                                    text: text.to_owned(),
                                }));
                            }
                        }
                    }
                    Some("tool_use") => {
                        let tool_use_id = block
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        let name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_owned();
                        let input = block.get("input").cloned().unwrap_or(Value::Null);
                        let capability = classify_tool_capability(&name);
                        out.push(Output::Event(SessionEvent::ToolStarted {
                            tool_use_id,
                            name,
                            capability,
                            input,
                        }));
                    }
                    Some("thinking") => {
                        out.push(Output::Event(SessionEvent::Thinking));
                    }
                    _ => {}
                }
            }
        }
        out
    }

    pub(super) fn handle_user(&mut self, val: &Value) -> Vec<Output> {
        let mut out = Vec::new();
        if let Some(blocks) = val
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                    let tool_use_id = block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    let is_error = block
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    out.push(Output::Event(SessionEvent::ToolFinished {
                        tool_use_id,
                        is_error,
                    }));
                }
            }
        }
        out
    }

    pub(super) fn handle_rate_limit(&mut self, val: &Value) -> Vec<Output> {
        let Some(info) = val.get("rate_limit_info").and_then(Value::as_object) else {
            return Vec::new();
        };
        let top_status = parse_limit_status(info.get("status").and_then(Value::as_str));
        let mut out = Vec::new();

        if let Some(windows) = info.get("unifiedWindows").and_then(Value::as_object) {
            for (name, wval) in windows {
                let window = match name.as_str() {
                    "five_hour" => Window::FiveHour,
                    "seven_day" => Window::SevenDay,
                    other => Window::Other {
                        id: other.to_owned(),
                        mins: None,
                    },
                };
                let status = wval
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|s| parse_limit_status(Some(s)))
                    .unwrap_or(top_status);
                let mut rl = RateLimit::new(window, status);
                rl.used_fraction = wval.get("utilization").and_then(Value::as_f64);
                rl.resets_at = wval
                    .get("resetsAt")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                out.push(Output::Event(SessionEvent::RateLimit(rl)));
            }
        } else if let Some(rl_type) = info.get("rateLimitType").and_then(Value::as_str) {
            let window = match rl_type {
                "five_hour" => Window::FiveHour,
                "seven_day" => Window::SevenDay,
                other => Window::Other {
                    id: other.to_owned(),
                    mins: None,
                },
            };
            let mut rl = RateLimit::new(window, top_status);
            rl.used_fraction = info.get("utilization").and_then(Value::as_f64);
            rl.resets_at = info
                .get("resetsAt")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            out.push(Output::Event(SessionEvent::RateLimit(rl)));
        }
        out
    }
}
