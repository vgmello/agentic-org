//! Codex (OpenAI) rollout JSONL conversation transcript parser (`REQ-05`).
//!
//! Handles Codex rollout JSONL lines (`session_meta`, `response_item`, `event_msg`,
//! `turn_context`, plus flat `item.*` / `thread.*` JSONL events from `codex exec --json`).
//! Unrecognized frames are preserved as `TranscriptEntryKind::Raw`.

use org_agent_runtime::transcript::{
    aggregate_file_touches, base_entry, extract_file_event_from_tool, merge_usage, truncate_output,
    ParsedTranscript, TranscriptDiscoveryLayout, TranscriptEntry, TranscriptEntryKind,
    TranscriptFileEventKind, TranscriptParser, TranscriptUsage,
};
use serde_json::Value;

/// Sans-I/O Codex transcript parser registered by [`crate::CodexRuntime`].
pub(crate) struct CodexTranscriptParser;

/// Static parser instance returned by [`crate::CodexRuntime::transcript_parser`].
pub(crate) static CODEX_TRANSCRIPT_PARSER: CodexTranscriptParser = CodexTranscriptParser;

impl TranscriptParser for CodexTranscriptParser {
    fn format_id(&self) -> &'static str {
        "codex"
    }

    fn discovery_layout(&self) -> TranscriptDiscoveryLayout {
        TranscriptDiscoveryLayout::RecursiveJsonl {
            search_roots: &[".codex/sessions", ".codex/archived_sessions"],
            skip_parent_dir: None,
            subagents_subdir: None,
            subagent_stem_prefix: None,
        }
    }

    fn extract_session_id_from_prefix(&self, prefix: &str) -> Option<String> {
        let first = prefix.lines().next()?.trim();
        let val: Value = serde_json::from_str(first).ok()?;
        val.pointer("/payload/id")
            .or_else(|| val.get("thread_id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    }

    fn parse(&self, raw: &str) -> ParsedTranscript {
        parse_codex_jsonl(raw)
    }
}

/// Parses a Codex rollout or `codex exec --json` JSONL transcript.
pub(crate) fn parse_codex_jsonl(raw: &str) -> ParsedTranscript {
    let mut state = CodexParseState::default();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(val) => state.handle_value(&val),
            Err(_) => state.push_raw(None, "malformed_json", trimmed),
        }
    }
    let files = aggregate_file_touches(&state.entries);
    ParsedTranscript {
        format_id: "codex".to_owned(),
        entries: state.entries,
        files,
        usage: state.usage,
    }
}

#[derive(Default)]
struct CodexParseState {
    entries: Vec<TranscriptEntry>,
    usage: TranscriptUsage,
    turn_index: u32,
    seq: u64,
    model: Option<String>,
}

impl CodexParseState {
    fn next_id(&mut self, prefix: &str) -> String {
        self.seq += 1;
        format!("{prefix}-{}", self.seq)
    }

    fn push_base(
        &mut self,
        prefix: &str,
        turn: u32,
        kind: TranscriptEntryKind,
        ts: Option<String>,
        role: Option<&str>,
        text: Option<String>,
    ) {
        let id = self.next_id(prefix);
        self.entries
            .push(base_entry(id, turn, kind, ts, role, text));
    }

    fn handle_value(&mut self, val: &Value) {
        let ts = val
            .get("timestamp")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let entry_type = val.get("type").and_then(Value::as_str).unwrap_or("");
        match entry_type {
            "session_meta" => self.handle_session_meta(val, ts),
            "turn_context" => {
                if let Some(m) = val.pointer("/payload/model").and_then(Value::as_str) {
                    self.model = Some(m.to_owned());
                }
            }
            "response_item" => {
                if let Some(payload) = val.get("payload") {
                    self.handle_response_item(payload, ts);
                }
            }
            "event_msg" => {
                if let Some(payload) = val.get("payload") {
                    self.handle_event_msg(payload, ts);
                }
            }
            "item.completed" => {
                if let Some(item) = val.get("item") {
                    self.handle_exec_item(item, ts);
                }
            }
            "item.started" | "thread.started" | "turn.started" => {}
            "turn.completed" => self.handle_exec_turn_usage(val, ts),
            other => self.push_raw(
                ts,
                if other.is_empty() { "unknown" } else { other },
                &val.to_string(),
            ),
        }
    }

    fn handle_session_meta(&mut self, val: &Value, ts: Option<String>) {
        let model = val
            .pointer("/payload/model_provider")
            .or_else(|| val.pointer("/payload/model"))
            .and_then(Value::as_str);
        if let Some(m) = model {
            self.model = Some(m.to_owned());
        }
        let version = val
            .pointer("/payload/cli_version")
            .and_then(Value::as_str)
            .unwrap_or("codex");
        let turn = self.turn_index.max(1);
        self.push_base(
            "meta",
            turn,
            TranscriptEntryKind::Notice,
            ts,
            Some("system"),
            Some(format!("Codex session ({version})")),
        );
    }

    fn handle_response_item(&mut self, payload: &Value, ts: Option<String>) {
        match payload.get("type").and_then(Value::as_str) {
            Some("message") => self.handle_message(payload, ts),
            Some("reasoning") => self.handle_reasoning(payload, ts),
            Some("function_call") | Some("custom_tool_call") => self.handle_tool_call(payload, ts),
            Some("function_call_output") | Some("custom_tool_call_output") => {
                self.handle_tool_output(payload, ts)
            }
            Some(other) => self.push_raw(ts, other, &payload.to_string()),
            None => {}
        }
    }

    fn handle_message(&mut self, payload: &Value, ts: Option<String>) {
        let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
        let text = extract_codex_message_text(payload.get("content"));
        let Some(text) = text.filter(|t| !t.trim().is_empty()) else {
            return;
        };
        let kind = match role {
            "user" => {
                self.turn_index += 1;
                TranscriptEntryKind::UserMessage
            }
            "assistant" => TranscriptEntryKind::AssistantMessage,
            _ => TranscriptEntryKind::Notice,
        };
        let turn = self.turn_index.max(1);
        self.push_base("m", turn, kind, ts, Some(role), Some(text));
    }

    fn handle_reasoning(&mut self, payload: &Value, ts: Option<String>) {
        let summary = payload
            .get("summary")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|s| s.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|t| !t.trim().is_empty());
        if let Some(text) = summary {
            let turn = self.turn_index.max(1);
            self.push_base(
                "r",
                turn,
                TranscriptEntryKind::Thinking,
                ts,
                Some("assistant"),
                Some(truncate_output(text)),
            );
        }
    }

    fn handle_tool_call(&mut self, payload: &Value, ts: Option<String>) {
        let turn = self.turn_index.max(1);
        let name = payload
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("tool")
            .to_owned();
        let call_id = payload
            .get("call_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let raw_args = payload.get("arguments").or_else(|| payload.get("input"));
        let input = parse_arguments_value(raw_args);
        let mut entry = base_entry(
            self.next_id("tc"),
            turn,
            TranscriptEntryKind::ToolCall,
            ts.clone(),
            Some("assistant"),
            None,
        );
        entry.tool_name = Some(name.clone());
        entry.tool_use_id = call_id.clone();
        entry.tool_input = Some(input.clone());
        self.entries.push(entry);

        self.record_codex_file_events(&name, &input, call_id, turn, ts);
    }

    fn record_codex_file_events(
        &mut self,
        name: &str,
        input: &Value,
        call_id: Option<String>,
        turn: u32,
        ts: Option<String>,
    ) {
        if name == "apply_patch" {
            let patch = input
                .as_str()
                .or_else(|| input.get("patch").and_then(Value::as_str))
                .or_else(|| input.get("input").and_then(Value::as_str))
                .unwrap_or("");
            for (path, action) in extract_apply_patch_files(patch) {
                let mut fe = base_entry(
                    self.next_id("fe"),
                    turn,
                    TranscriptEntryKind::FileEvent,
                    ts.clone(),
                    Some("tool"),
                    None,
                );
                fe.tool_name = Some(name.to_owned());
                fe.tool_use_id = call_id.clone();
                fe.file_path = Some(path);
                fe.file_action = Some(action);
                fe.diff_preview = Some(truncate_output(patch.to_owned()));
                self.entries.push(fe);
            }
            return;
        }
        if let Some((path, action, diff)) = extract_file_event_from_tool(name, input) {
            let mut fe = base_entry(
                self.next_id("fe"),
                turn,
                TranscriptEntryKind::FileEvent,
                ts,
                Some("tool"),
                None,
            );
            fe.tool_name = Some(name.to_owned());
            fe.tool_use_id = call_id;
            fe.file_path = Some(path);
            fe.file_action = Some(action);
            fe.diff_preview = diff.map(truncate_output);
            self.entries.push(fe);
        }
    }

    fn handle_tool_output(&mut self, payload: &Value, ts: Option<String>) {
        let output = payload.get("output").and_then(|v| {
            v.as_str()
                .map(ToOwned::to_owned)
                .or_else(|| Some(v.to_string()))
        });
        let mut entry = base_entry(
            self.next_id("tr"),
            self.turn_index.max(1),
            TranscriptEntryKind::ToolResult,
            ts,
            Some("tool"),
            None,
        );
        entry.tool_use_id = payload
            .get("call_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        entry.tool_output = output.map(truncate_output);
        self.entries.push(entry);
    }

    fn handle_event_msg(&mut self, payload: &Value, ts: Option<String>) {
        match payload.get("type").and_then(Value::as_str) {
            Some("token_count") => {
                if let Some((u, is_cumulative)) =
                    extract_codex_token_count(payload, self.model.clone())
                {
                    if is_cumulative {
                        self.usage.input_tokens = self.usage.input_tokens.max(u.input_tokens);
                        self.usage.output_tokens = self.usage.output_tokens.max(u.output_tokens);
                        self.usage.cache_read_tokens =
                            self.usage.cache_read_tokens.max(u.cache_read_tokens);
                        self.usage.reasoning_tokens =
                            self.usage.reasoning_tokens.max(u.reasoning_tokens);
                        if self.usage.model.is_none() {
                            self.usage.model = u.model.clone();
                        }
                    } else {
                        merge_usage(&mut self.usage, &u);
                    }
                    let mut entry = base_entry(
                        self.next_id("u"),
                        self.turn_index.max(1),
                        TranscriptEntryKind::Usage,
                        ts,
                        Some("system"),
                        None,
                    );
                    entry.usage = Some(u);
                    self.entries.push(entry);
                }
            }
            Some("agent_reasoning") => {
                if let Some(text) = payload.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        let turn = self.turn_index.max(1);
                        self.push_base(
                            "r",
                            turn,
                            TranscriptEntryKind::Thinking,
                            ts,
                            Some("assistant"),
                            Some(truncate_output(text.to_owned())),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_exec_item(&mut self, item: &Value, ts: Option<String>) {
        let turn = self.turn_index.max(1);
        match item.get("type").and_then(Value::as_str) {
            Some("agent_message") => {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    self.push_base(
                        "a",
                        turn,
                        TranscriptEntryKind::AssistantMessage,
                        ts,
                        Some("assistant"),
                        Some(text.to_owned()),
                    );
                }
            }
            Some("command_execution") => {
                let cmd = item
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let mut tc = base_entry(
                    self.next_id("tc"),
                    turn,
                    TranscriptEntryKind::ToolCall,
                    ts.clone(),
                    Some("assistant"),
                    None,
                );
                tc.tool_name = Some("shell".to_owned());
                tc.tool_use_id = item
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                tc.tool_input = Some(serde_json::json!({ "command": cmd }));
                self.entries.push(tc);
                if let Some(out) = item.get("aggregated_output").and_then(Value::as_str) {
                    let mut tr = base_entry(
                        self.next_id("tr"),
                        turn,
                        TranscriptEntryKind::ToolResult,
                        ts,
                        Some("tool"),
                        None,
                    );
                    tr.tool_use_id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    tr.tool_output = Some(truncate_output(out.to_owned()));
                    tr.is_error = item
                        .get("exit_code")
                        .and_then(Value::as_i64)
                        .is_some_and(|c| c != 0);
                    self.entries.push(tr);
                }
            }
            Some("file_change") => {
                if let Some(changes) = item.get("changes").and_then(Value::as_array) {
                    for ch in changes {
                        let Some(path) = ch.get("path").and_then(Value::as_str) else {
                            continue;
                        };
                        let action = match ch.get("kind").and_then(Value::as_str) {
                            Some("add" | "create") => TranscriptFileEventKind::Write,
                            Some("delete" | "remove") => TranscriptFileEventKind::Delete,
                            _ => TranscriptFileEventKind::Edit,
                        };
                        let mut fe = base_entry(
                            self.next_id("fe"),
                            turn,
                            TranscriptEntryKind::FileEvent,
                            ts.clone(),
                            Some("tool"),
                            None,
                        );
                        fe.file_path = Some(path.to_owned());
                        fe.file_action = Some(action);
                        self.entries.push(fe);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_exec_turn_usage(&mut self, val: &Value, ts: Option<String>) {
        let Some(u) = val.get("usage").and_then(Value::as_object) else {
            return;
        };
        let usage = TranscriptUsage {
            input_tokens: u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
            output_tokens: u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0),
            cache_read_tokens: u
                .get("cached_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            cache_write_tokens: 0,
            reasoning_tokens: u
                .get("reasoning_output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            model: self.model.clone(),
        };
        merge_usage(&mut self.usage, &usage);
        let mut entry = base_entry(
            self.next_id("u"),
            self.turn_index.max(1),
            TranscriptEntryKind::Usage,
            ts,
            Some("system"),
            None,
        );
        entry.usage = Some(usage);
        self.entries.push(entry);
    }

    fn push_raw(&mut self, ts: Option<String>, raw_type: &str, text: &str) {
        let mut entry = base_entry(
            self.next_id("raw"),
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

fn extract_codex_message_text(content: Option<&Value>) -> Option<String> {
    let val = content?;
    if let Some(s) = val.as_str() {
        return Some(s.to_owned());
    }
    let arr = val.as_array()?;
    let parts: Vec<&str> = arr
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn parse_arguments_value(raw: Option<&Value>) -> Value {
    match raw {
        Some(Value::String(s)) => {
            serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone()))
        }
        Some(other) => other.clone(),
        None => Value::Null,
    }
}

fn extract_apply_patch_files(patch: &str) -> Vec<(String, TranscriptFileEventKind)> {
    let mut out = Vec::new();
    for line in patch.lines() {
        let trimmed = line.trim();
        if let Some(path) = trimmed.strip_prefix("*** Update File: ") {
            out.push((path.trim().to_owned(), TranscriptFileEventKind::Edit));
        } else if let Some(path) = trimmed.strip_prefix("*** Add File: ") {
            out.push((path.trim().to_owned(), TranscriptFileEventKind::Write));
        } else if let Some(path) = trimmed.strip_prefix("*** Delete File: ") {
            out.push((path.trim().to_owned(), TranscriptFileEventKind::Delete));
        }
    }
    out
}

fn extract_codex_token_count(
    payload: &Value,
    model: Option<String>,
) -> Option<(TranscriptUsage, bool)> {
    let (info, is_cumulative) = if let Some(last) = payload
        .pointer("/info/last_token_usage")
        .and_then(Value::as_object)
    {
        (last, false)
    } else {
        let total = payload
            .pointer("/info/total_token_usage")
            .and_then(Value::as_object)?;
        (total, true)
    };
    let input = info
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = info
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_read = info
        .get("cached_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let reasoning = info
        .get("reasoning_output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if input == 0 && output == 0 {
        return None;
    }
    Some((
        TranscriptUsage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: 0,
            reasoning_tokens: reasoning,
            model,
        },
        is_cumulative,
    ))
}
