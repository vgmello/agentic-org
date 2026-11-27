//! Claude Code JSONL conversation transcript parser + `SUB-6` sub-agent nesting (`REQ-05`).
//!
//! Handles Claude Code `projects/<encoded-cwd>/<session-id>.jsonl` and its sibling
//! `<session-id>/subagents/agent-<agent-id>.jsonl` files. Unrecognized frames are
//! preserved as `TranscriptEntryKind::Raw` rather than failing the viewer.

use std::collections::BTreeMap;

use org_agent_runtime::transcript::{
    aggregate_file_touches, base_entry, extract_file_event_from_tool, merge_usage, truncate_output,
    LoadedSubagentTranscript, ParsedTranscript, SubagentTranscript, TranscriptDiscoveryLayout,
    TranscriptEntry, TranscriptEntryKind, TranscriptFileEventKind, TranscriptParser,
    TranscriptUsage,
};
use serde_json::Value;

/// Sans-I/O Claude Code transcript parser registered by [`crate::ClaudeRuntime`].
pub(crate) struct ClaudeTranscriptParser;

/// Static parser instance returned by [`crate::ClaudeRuntime::transcript_parser`].
pub(crate) static CLAUDE_TRANSCRIPT_PARSER: ClaudeTranscriptParser = ClaudeTranscriptParser;

impl TranscriptParser for ClaudeTranscriptParser {
    fn format_id(&self) -> &'static str {
        "claude"
    }

    fn discovery_layout(&self) -> TranscriptDiscoveryLayout {
        TranscriptDiscoveryLayout::RecursiveJsonl {
            search_roots: &[".claude/projects", ".config/claude/projects"],
            skip_parent_dir: Some("subagents"),
            subagents_subdir: Some("subagents"),
            subagent_stem_prefix: Some("agent-"),
        }
    }

    fn parse_subagent_meta(
        &self,
        meta_json: Option<&str>,
        transcript_prefix: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        let mut label = None;
        let mut parent_tool_use_id = None;
        if let Some(raw) = meta_json {
            if let Ok(val) = serde_json::from_str::<Value>(raw) {
                label = val
                    .get("description")
                    .or_else(|| val.get("agentType"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                parent_tool_use_id = val
                    .get("parent_tool_use_id")
                    .or_else(|| val.get("parentToolUseId"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
        }
        if parent_tool_use_id.is_none() {
            if let Some(prefix) = transcript_prefix {
                parent_tool_use_id = sniff_first_lines_field(prefix, "parentToolUseId")
                    .or_else(|| sniff_first_lines_field(prefix, "parent_tool_use_id"));
            }
        }
        (label, parent_tool_use_id)
    }

    fn parse(&self, raw: &str) -> ParsedTranscript {
        parse_claude_jsonl(raw)
    }

    fn attach_subagents(
        &self,
        parent: &mut ParsedTranscript,
        subagents: Vec<LoadedSubagentTranscript>,
    ) {
        attach_claude_subagents(parent, subagents);
    }
}

fn sniff_first_lines_field(prefix: &str, field: &str) -> Option<String> {
    for line in prefix.lines().take(5) {
        if let Ok(val) = serde_json::from_str::<Value>(line.trim()) {
            if let Some(s) = val.get(field).and_then(Value::as_str) {
                return Some(s.to_owned());
            }
        }
    }
    None
}

/// Parses a single Claude Code JSONL transcript (main or sub-agent).
pub(crate) fn parse_claude_jsonl(raw: &str) -> ParsedTranscript {
    let mut state = ClaudeParseState::default();
    for (line_idx, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(val) => state.handle_value(line_idx, &val),
            Err(_) => state.push_raw(line_idx, None, "malformed_json", trimmed),
        }
    }
    let files = aggregate_file_touches(&state.entries);
    ParsedTranscript {
        format_id: "claude".to_owned(),
        entries: state.entries,
        files,
        usage: state.usage,
    }
}

/// Attaches parsed sub-agent transcripts onto the launching parent tool-call entry (or inserts a
/// `SubagentRun` entry in the same turn if multiple sub-agents share one `parent_tool_use_id` or
/// the parent tool-use ID is not present in the main log; `SUB-6`).
pub(crate) fn attach_claude_subagents(
    parent: &mut ParsedTranscript,
    subagents: Vec<LoadedSubagentTranscript>,
) {
    let mut attached_by_tool: BTreeMap<String, Vec<SubagentTranscript>> = BTreeMap::new();
    let mut unlinked = Vec::new();
    for sub in subagents {
        let LoadedSubagentTranscript {
            agent_id,
            label,
            parent_tool_use_id,
            parsed,
        } = sub;
        merge_usage(&mut parent.usage, &parsed.usage);
        let view = SubagentTranscript {
            agent_id,
            label,
            parent_tool_use_id: parent_tool_use_id.clone(),
            format_id: parsed.format_id,
            entry_count: parsed.entries.len() as u32,
            entries: parsed.entries,
            files: parsed.files,
            usage: parsed.usage,
        };
        if let Some(tid) = parent_tool_use_id {
            attached_by_tool.entry(tid).or_default().push(view);
        } else {
            unlinked.push(view);
        }
    }

    let mut rebuilt = Vec::with_capacity(parent.entries.len() + unlinked.len());
    for mut entry in std::mem::take(&mut parent.entries) {
        let extras = attach_matching_subagents(&mut entry, &mut attached_by_tool);
        let turn = entry.turn_index;
        rebuilt.push(entry);
        for sub in extras {
            rebuilt.push(make_subagent_run_entry(turn, sub));
        }
    }

    let last_turn = rebuilt.last().map_or(1, |e| e.turn_index);
    for sub in attached_by_tool.into_values().flatten().chain(unlinked) {
        rebuilt.push(make_subagent_run_entry(last_turn, sub));
    }
    parent.entries = rebuilt;
}

fn attach_matching_subagents(
    entry: &mut TranscriptEntry,
    attached_by_tool: &mut BTreeMap<String, Vec<SubagentTranscript>>,
) -> Vec<SubagentTranscript> {
    if entry.kind != TranscriptEntryKind::ToolCall {
        return Vec::new();
    }
    let Some(tid) = entry.tool_use_id.as_deref() else {
        return Vec::new();
    };
    let Some(mut list) = attached_by_tool.remove(tid) else {
        return Vec::new();
    };
    if !list.is_empty() {
        entry.subagent = Some(list.remove(0));
    }
    list
}

fn make_subagent_run_entry(turn_index: u32, sub: SubagentTranscript) -> TranscriptEntry {
    let id = format!("subagent-{}", sub.agent_id);
    let label = sub
        .label
        .clone()
        .unwrap_or_else(|| format!("Sub-agent {}", sub.agent_id));
    TranscriptEntry {
        id,
        turn_index,
        kind: TranscriptEntryKind::SubagentRun,
        timestamp: None,
        role: Some("assistant".to_owned()),
        text: Some(label),
        tool_name: Some("Agent".to_owned()),
        tool_use_id: sub.parent_tool_use_id.clone(),
        tool_input: None,
        tool_output: None,
        is_error: false,
        file_path: None,
        file_action: None,
        diff_preview: None,
        usage: Some(sub.usage.clone()),
        raw_type: None,
        subagent: Some(sub),
    }
}

#[derive(Default)]
struct ClaudeParseState {
    entries: Vec<TranscriptEntry>,
    usage: TranscriptUsage,
    seen_msg_usage: BTreeMap<String, TranscriptUsage>,
    turn_index: u32,
    seq: u64,
}

impl ClaudeParseState {
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

    fn handle_value(&mut self, line_idx: usize, val: &Value) {
        let ts = val
            .get("timestamp")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let msg_type = val.get("type").and_then(Value::as_str).unwrap_or("");
        match msg_type {
            "user" => self.handle_user(val, ts),
            "assistant" => self.handle_assistant(val, ts),
            "system" | "summary" => self.handle_system(val, msg_type, ts),
            "result" => self.handle_result(val, ts),
            "progress" | "file-history-snapshot" => self.handle_aux(val, msg_type, ts),
            other => self.push_raw(
                line_idx,
                ts,
                if other.is_empty() { "unknown" } else { other },
                &val.to_string(),
            ),
        }
    }

    fn handle_user(&mut self, val: &Value, ts: Option<String>) {
        let Some(msg) = val.get("message") else {
            return;
        };
        let content = msg.get("content");
        if let Some(text) = content.and_then(Value::as_str) {
            if !text.trim().is_empty() {
                self.turn_index += 1;
                let turn = self.turn_index;
                self.push_base(
                    "u",
                    turn,
                    TranscriptEntryKind::UserMessage,
                    ts,
                    Some("user"),
                    Some(text.to_owned()),
                );
            }
            return;
        }
        let Some(blocks) = content.and_then(Value::as_array) else {
            return;
        };
        let has_user_text = blocks
            .iter()
            .any(|b| b.get("type").and_then(Value::as_str) == Some("text"));
        if has_user_text {
            self.turn_index += 1;
        }
        let turn = self.turn_index.max(1);
        for block in blocks {
            self.handle_user_block(block, turn, &ts);
        }
    }

    fn handle_user_block(&mut self, block: &Value, turn: u32, ts: &Option<String>) {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        self.push_base(
                            "u",
                            turn,
                            TranscriptEntryKind::UserMessage,
                            ts.clone(),
                            Some("user"),
                            Some(text.to_owned()),
                        );
                    }
                }
            }
            Some("tool_result") => {
                let tool_use_id = block
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                let is_error = block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let output = extract_tool_result_text(block.get("content"));
                let mut entry = base_entry(
                    self.next_id("tr"),
                    turn,
                    TranscriptEntryKind::ToolResult,
                    ts.clone(),
                    Some("tool"),
                    None,
                );
                entry.tool_use_id = tool_use_id;
                entry.tool_output = output.map(truncate_output);
                entry.is_error = is_error;
                self.entries.push(entry);
            }
            _ => {}
        }
    }

    fn handle_assistant(&mut self, val: &Value, ts: Option<String>) {
        let Some(msg) = val.get("message") else {
            return;
        };
        let turn = self.turn_index.max(1);
        if let Some(u) = extract_claude_usage(msg.get("usage"), msg.get("model")) {
            let msg_id = msg.get("id").and_then(Value::as_str);
            if let Some(delta) = self.record_assistant_usage(msg_id, &u) {
                let mut entry = base_entry(
                    self.next_id("usage"),
                    turn,
                    TranscriptEntryKind::Usage,
                    ts.clone(),
                    Some("assistant"),
                    None,
                );
                entry.usage = Some(delta);
                self.entries.push(entry);
            }
        }
        let Some(blocks) = msg.get("content").and_then(Value::as_array) else {
            return;
        };
        for block in blocks {
            self.handle_assistant_block(block, turn, &ts);
        }
    }

    fn record_assistant_usage(
        &mut self,
        msg_id: Option<&str>,
        observed: &TranscriptUsage,
    ) -> Option<TranscriptUsage> {
        let Some(id) = msg_id.filter(|s| !s.is_empty()) else {
            merge_usage(&mut self.usage, observed);
            return Some(observed.clone());
        };
        let prev = self.seen_msg_usage.entry(id.to_owned()).or_default();
        let delta = TranscriptUsage {
            input_tokens: observed.input_tokens.saturating_sub(prev.input_tokens),
            output_tokens: observed.output_tokens.saturating_sub(prev.output_tokens),
            cache_read_tokens: observed
                .cache_read_tokens
                .saturating_sub(prev.cache_read_tokens),
            cache_write_tokens: observed
                .cache_write_tokens
                .saturating_sub(prev.cache_write_tokens),
            reasoning_tokens: observed
                .reasoning_tokens
                .saturating_sub(prev.reasoning_tokens),
            model: observed.model.clone(),
        };
        prev.input_tokens = prev.input_tokens.max(observed.input_tokens);
        prev.output_tokens = prev.output_tokens.max(observed.output_tokens);
        prev.cache_read_tokens = prev.cache_read_tokens.max(observed.cache_read_tokens);
        prev.cache_write_tokens = prev.cache_write_tokens.max(observed.cache_write_tokens);
        prev.reasoning_tokens = prev.reasoning_tokens.max(observed.reasoning_tokens);
        if delta.input_tokens == 0
            && delta.output_tokens == 0
            && delta.cache_read_tokens == 0
            && delta.cache_write_tokens == 0
            && delta.reasoning_tokens == 0
        {
            return None;
        }
        merge_usage(&mut self.usage, &delta);
        Some(delta)
    }

    fn handle_assistant_block(&mut self, block: &Value, turn: u32, ts: &Option<String>) {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        self.push_base(
                            "a",
                            turn,
                            TranscriptEntryKind::AssistantMessage,
                            ts.clone(),
                            Some("assistant"),
                            Some(text.to_owned()),
                        );
                    }
                }
            }
            Some("thinking") => {
                if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        self.push_base(
                            "th",
                            turn,
                            TranscriptEntryKind::Thinking,
                            ts.clone(),
                            Some("assistant"),
                            Some(truncate_output(text.to_owned())),
                        );
                    }
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                let tool_use_id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                let input = block.get("input").cloned().unwrap_or(Value::Null);
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
                        ts.clone(),
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
            _ => {}
        }
    }

    fn handle_system(&mut self, val: &Value, msg_type: &str, ts: Option<String>) {
        let text = val
            .get("summary")
            .or_else(|| val.get("content"))
            .or_else(|| val.get("message"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                val.get("subtype")
                    .and_then(Value::as_str)
                    .map(|s| format!("{msg_type}/{s}"))
            });
        if text.is_some() {
            let turn = self.turn_index.max(1);
            self.push_base(
                "sys",
                turn,
                TranscriptEntryKind::Notice,
                ts,
                Some("system"),
                text,
            );
        }
    }

    fn handle_result(&mut self, val: &Value, ts: Option<String>) {
        let turn = self.turn_index.max(1);
        if let Some(u) = extract_claude_usage(val.get("usage"), val.get("model")) {
            if self.usage.input_tokens == 0 && self.usage.output_tokens == 0 {
                merge_usage(&mut self.usage, &u);
            }
            let mut entry = base_entry(
                self.next_id("res"),
                turn,
                TranscriptEntryKind::Usage,
                ts,
                Some("system"),
                val.get("subtype")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            );
            entry.usage = Some(u);
            self.entries.push(entry);
        }
    }

    fn handle_aux(&mut self, val: &Value, msg_type: &str, ts: Option<String>) {
        if msg_type == "file-history-snapshot" {
            if let Some(files) = val
                .pointer("/snapshot/trackedFileBackups")
                .and_then(Value::as_object)
            {
                let turn = self.turn_index.max(1);
                for path in files.keys() {
                    let mut fe = base_entry(
                        self.next_id("fh"),
                        turn,
                        TranscriptEntryKind::FileEvent,
                        ts.clone(),
                        Some("system"),
                        None,
                    );
                    fe.file_path = Some(path.clone());
                    fe.file_action = Some(TranscriptFileEventKind::Edit);
                    self.entries.push(fe);
                }
            }
        }
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

fn extract_tool_result_text(content: Option<&Value>) -> Option<String> {
    let val = content?;
    if let Some(s) = val.as_str() {
        return Some(s.to_owned());
    }
    if let Some(arr) = val.as_array() {
        let parts: Vec<&str> = arr
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect();
        if !parts.is_empty() {
            return Some(parts.join("\n"));
        }
    }
    Some(val.to_string())
}

fn extract_claude_usage(
    usage_val: Option<&Value>,
    model_val: Option<&Value>,
) -> Option<TranscriptUsage> {
    let u = usage_val?.as_object()?;
    let input = u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
    let output = u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
    let cache_read = u
        .get("cache_read_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_write = u
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
        return None;
    }
    Some(TranscriptUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: 0,
        model: model_val.and_then(Value::as_str).map(ToOwned::to_owned),
    })
}
