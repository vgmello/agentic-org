//! Generic JSONL conversation transcript parser for standard ACP runtimes (`gemini`, `kimi`, `pi`).

use serde_json::Value;

use org_agent_runtime::transcript::{
    aggregate_file_touches, base_entry, extract_file_event_from_tool, merge_usage, truncate_output,
    ParsedTranscript, TranscriptEntry, TranscriptEntryKind, TranscriptUsage,
};

pub(super) fn parse_generic_acp_jsonl(format_id: &'static str, raw: &str) -> ParsedTranscript {
    let mut state = GenericParseState::default();
    for (line_idx, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(val) => state.handle_record(line_idx, &val),
            Err(_) => state.push_raw(line_idx, None, "malformed_json", trimmed),
        }
    }
    let files = aggregate_file_touches(&state.entries);
    ParsedTranscript {
        format_id: format_id.to_owned(),
        entries: state.entries,
        files,
        usage: state.usage,
    }
}

#[derive(Default)]
struct GenericParseState {
    entries: Vec<TranscriptEntry>,
    usage: TranscriptUsage,
    turn_index: u32,
    seq: u64,
}

impl GenericParseState {
    fn next_id(&mut self, prefix: &str) -> String {
        self.seq += 1;
        format!("{prefix}-{}", self.seq)
    }

    fn handle_record(&mut self, line_idx: usize, val: &Value) {
        let ts = val
            .get("timestamp")
            .or_else(|| val.get("created_at"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let role = val.get("role").and_then(Value::as_str).unwrap_or("");
        let kind = val.get("type").and_then(Value::as_str).unwrap_or("");
        match (kind, role) {
            ("user" | "user_message", _) | ("", "user") => self.handle_user(val, ts),
            ("assistant" | "assistant_message", _) | ("", "assistant" | "model") => {
                self.handle_assistant(val, ts);
            }
            ("thinking" | "thought", _) => self.handle_thinking(val, ts),
            ("tool_call" | "tool_use", _) => self.handle_tool_call(val, ts),
            ("tool_result" | "tool_response", _) | ("", "tool") => self.handle_tool_result(val, ts),
            ("usage", _) => self.record_usage(val.get("usage").unwrap_or(val), ts),
            ("notice" | "system", _) | ("", "system") => self.handle_notice(val, ts),
            _ => {
                let tag = if !kind.is_empty() {
                    kind
                } else if !role.is_empty() {
                    role
                } else {
                    "unknown"
                };
                self.push_raw(line_idx, ts, tag, &val.to_string());
            }
        }
    }

    fn handle_user(&mut self, val: &Value, ts: Option<String>) {
        if let Some(text) = extract_text(val) {
            self.turn_index += 1;
            let id = self.next_id("u");
            self.entries.push(base_entry(
                id,
                self.turn_index,
                TranscriptEntryKind::UserMessage,
                ts,
                Some("user"),
                Some(text),
            ));
        }
    }

    fn handle_assistant(&mut self, val: &Value, ts: Option<String>) {
        let turn = self.turn_index.max(1);
        if let Some(thinking) = val.get("thinking").and_then(Value::as_str) {
            if !thinking.trim().is_empty() {
                let id = self.next_id("th");
                self.entries.push(base_entry(
                    id,
                    turn,
                    TranscriptEntryKind::Thinking,
                    ts.clone(),
                    Some("assistant"),
                    Some(truncate_output(thinking.to_owned())),
                ));
            }
        }
        if let Some(text) = extract_text(val) {
            let id = self.next_id("a");
            self.entries.push(base_entry(
                id,
                turn,
                TranscriptEntryKind::AssistantMessage,
                ts.clone(),
                Some("assistant"),
                Some(text),
            ));
        }
        if let Some(calls) = val.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                self.handle_tool_call(call, ts.clone());
            }
        }
        if let Some(usage_val) = val.get("usage") {
            self.record_usage(usage_val, ts);
        }
    }

    fn handle_thinking(&mut self, val: &Value, ts: Option<String>) {
        if let Some(text) = extract_text(val) {
            let id = self.next_id("th");
            self.entries.push(base_entry(
                id,
                self.turn_index.max(1),
                TranscriptEntryKind::Thinking,
                ts,
                Some("assistant"),
                Some(truncate_output(text)),
            ));
        }
    }

    fn handle_tool_call(&mut self, val: &Value, ts: Option<String>) {
        let turn = self.turn_index.max(1);
        let name = val
            .get("name")
            .or_else(|| val.get("tool_name"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let tool_use_id = val
            .get("id")
            .or_else(|| val.get("tool_use_id"))
            .or_else(|| val.get("toolCallId"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let input = val
            .get("input")
            .or_else(|| val.get("arguments"))
            .or_else(|| val.get("rawInput"))
            .cloned()
            .unwrap_or(Value::Null);
        let mut entry = base_entry(
            self.next_id("tc"),
            turn,
            TranscriptEntryKind::ToolCall,
            ts.clone(),
            Some("assistant"),
            None,
        );
        entry.tool_name = Some(name.clone());
        entry.tool_use_id = tool_use_id.clone();
        entry.tool_input = Some(input.clone());
        self.entries.push(entry);

        if let Some((path, action, diff)) = extract_file_event_from_tool(&name, &input) {
            let mut fe = base_entry(
                self.next_id("fe"),
                turn,
                TranscriptEntryKind::FileEvent,
                ts,
                Some("tool"),
                None,
            );
            fe.tool_name = Some(name);
            fe.tool_use_id = tool_use_id;
            fe.file_path = Some(path);
            fe.file_action = Some(action);
            fe.diff_preview = diff.map(truncate_output);
            self.entries.push(fe);
        }
    }

    fn handle_tool_result(&mut self, val: &Value, ts: Option<String>) {
        let output = val
            .get("output")
            .or_else(|| val.get("content"))
            .map(|v| v.as_str().map_or_else(|| v.to_string(), ToOwned::to_owned));
        let mut entry = base_entry(
            self.next_id("tr"),
            self.turn_index.max(1),
            TranscriptEntryKind::ToolResult,
            ts,
            Some("tool"),
            None,
        );
        entry.tool_name = val
            .get("tool_name")
            .or_else(|| val.get("name"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        entry.tool_use_id = val
            .get("tool_call_id")
            .or_else(|| val.get("tool_use_id"))
            .or_else(|| val.get("toolCallId"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        entry.tool_output = output.map(truncate_output);
        entry.is_error = val
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || val.get("status").and_then(Value::as_str) == Some("ERROR");
        self.entries.push(entry);
    }

    fn handle_notice(&mut self, val: &Value, ts: Option<String>) {
        if let Some(text) = extract_text(val) {
            let id = self.next_id("n");
            self.entries.push(base_entry(
                id,
                self.turn_index.max(1),
                TranscriptEntryKind::Notice,
                ts,
                Some("system"),
                Some(text),
            ));
        }
    }

    fn record_usage(&mut self, val: &Value, ts: Option<String>) {
        let Some(u) = extract_generic_usage(val) else {
            return;
        };
        merge_usage(&mut self.usage, &u);
        let mut entry = base_entry(
            self.next_id("usage"),
            self.turn_index.max(1),
            TranscriptEntryKind::Usage,
            ts,
            Some("assistant"),
            None,
        );
        entry.usage = Some(u);
        self.entries.push(entry);
    }

    fn push_raw(&mut self, line_idx: usize, ts: Option<String>, raw_type: &str, text: &str) {
        let mut entry = base_entry(
            format!("raw-{}", line_idx + 1),
            self.turn_index.max(1),
            TranscriptEntryKind::Raw,
            ts,
            None,
            Some(truncate_output(text.to_owned())),
        );
        entry.raw_type = Some(raw_type.to_owned());
        self.entries.push(entry);
    }
}

fn extract_text(val: &Value) -> Option<String> {
    let raw = val
        .get("text")
        .or_else(|| val.get("content"))
        .and_then(Value::as_str)?;
    (!raw.trim().is_empty()).then(|| raw.to_owned())
}

fn extract_generic_usage(val: &Value) -> Option<TranscriptUsage> {
    let obj = val.as_object()?;
    let input = obj
        .get("input_tokens")
        .or_else(|| obj.get("inputTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = obj
        .get("output_tokens")
        .or_else(|| obj.get("outputTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_read = obj
        .get("cache_read_tokens")
        .or_else(|| obj.get("cachedReadTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_write = obj
        .get("cache_write_tokens")
        .or_else(|| obj.get("cachedWriteTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let reasoning = obj
        .get("reasoning_tokens")
        .or_else(|| obj.get("thoughtTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if input == 0 && output == 0 {
        return None;
    }
    Some(TranscriptUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
        model: obj
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use org_agent_runtime::transcript::{TranscriptEntryKind, TranscriptFileEventKind};

    use super::parse_generic_acp_jsonl;

    #[test]
    fn parses_messages_tools_file_events_usage_and_malformed_lines() {
        let raw = [
            r#"{"role":"user","content":"Inspect src/main.rs"}"#,
            r#"{"role":"assistant","thinking":"Checking file","content":"Reading now","tool_calls":[{"id":"c1","name":"read_file","input":{"path":"src/main.rs"}}],"usage":{"inputTokens":40,"outputTokens":12,"model":"gemini-2.5-pro"}}"#,
            r#"{"role":"tool","tool_call_id":"c1","name":"read_file","output":"fn main() {}"}"#,
            "not valid json",
        ]
        .join("\n");
        let parsed = parse_generic_acp_jsonl("gemini", &raw);
        assert_eq!(parsed.format_id, "gemini");
        assert_eq!(parsed.usage.input_tokens, 40);
        assert_eq!(parsed.usage.output_tokens, 12);
        assert_eq!(parsed.usage.model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(parsed.files[0].path, "src/main.rs");
        assert_eq!(
            parsed.files[0].operations,
            vec![TranscriptFileEventKind::Read]
        );
        assert!(parsed
            .entries
            .iter()
            .any(|e| e.kind == TranscriptEntryKind::Raw
                && e.raw_type.as_deref() == Some("malformed_json")));
    }
}
