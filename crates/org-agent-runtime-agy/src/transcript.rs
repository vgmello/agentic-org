//! Google Antigravity (`agy`) JSONL conversation transcript parser + `SUB-6` sub-agent nesting.
//!
//! Handles `.gemini/antigravity-cli/brain/<conversation-id>/.system_generated/logs/transcript.jsonl`
//! steps (`USER_INPUT`, `PLANNER_RESPONSE`, tool results, and `invoke_subagent` links).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use org_agent_runtime::transcript::{
    aggregate_file_touches, base_entry, extract_file_event_from_tool, merge_usage, truncate_output,
    LoadedSubagentTranscript, ParsedTranscript, SubagentTranscript, SubagentTranscriptRef,
    TranscriptDiscoveryLayout, TranscriptEntry, TranscriptEntryKind, TranscriptParser,
    TranscriptUsage,
};
use serde_json::Value;

/// Sans-I/O Antigravity transcript parser registered by [`crate::AgyRuntime`].
pub(crate) struct AgyTranscriptParser;

/// Static parser instance returned by [`crate::AgyRuntime::transcript_parser`].
pub(crate) static AGY_TRANSCRIPT_PARSER: AgyTranscriptParser = AgyTranscriptParser;

impl TranscriptParser for AgyTranscriptParser {
    fn format_id(&self) -> &'static str {
        "agy"
    }

    fn discovery_layout(&self) -> TranscriptDiscoveryLayout {
        TranscriptDiscoveryLayout::ConversationDir {
            brain_root: ".gemini/antigravity-cli/brain",
            transcript_rel_path: ".system_generated/logs/transcript.jsonl",
        }
    }

    fn extract_subagent_refs(
        &self,
        raw: &str,
        parent_session_id: &str,
    ) -> Vec<SubagentTranscriptRef> {
        extract_agy_subagent_refs(raw, parent_session_id)
    }

    fn parse(&self, raw: &str) -> ParsedTranscript {
        parse_agy_jsonl(raw)
    }

    fn attach_subagents(
        &self,
        parent: &mut ParsedTranscript,
        subagents: Vec<LoadedSubagentTranscript>,
    ) {
        attach_agy_subagents(parent, subagents);
    }
}

/// Parses an Antigravity (`agy`) `transcript.jsonl` file.
pub(crate) fn parse_agy_jsonl(raw: &str) -> ParsedTranscript {
    let mut state = AgyParseState::default();
    for (line_idx, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(val) => state.handle_step(line_idx, &val),
            Err(_) => state.push_raw(line_idx, None, "malformed_json", trimmed),
        }
    }
    let files = aggregate_file_touches(&state.entries);
    ParsedTranscript {
        format_id: "agy".to_owned(),
        entries: state.entries,
        files,
        usage: state.usage,
    }
}

/// Extracts deduplicated sub-agent references launched via `invoke_subagent`, skipping self-refs.
pub(crate) fn extract_agy_subagent_refs(
    raw: &str,
    parent_session_id: &str,
) -> Vec<SubagentTranscriptRef> {
    let mut seen = BTreeSet::new();
    let mut refs = Vec::new();
    for line in raw.lines() {
        let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let Some(conv_id) = val
            .pointer("/result/conversationId")
            .or_else(|| val.get("conversationId"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if conv_id == parent_session_id || !seen.insert(conv_id.to_owned()) {
            continue;
        }
        let role = val
            .pointer("/spec/role")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let tool_call_id = val
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        refs.push(SubagentTranscriptRef {
            agent_id: conv_id.to_owned(),
            label: role,
            parent_tool_use_id: tool_call_id,
        });
    }
    refs
}

/// Attaches parsed Agy sub-agent transcripts onto their launching `invoke_subagent` `ToolCall`
/// (matched by `tool_call_id` first, then FIFO for unlinked refs; `SUB-6`).
pub(crate) fn attach_agy_subagents(
    parent: &mut ParsedTranscript,
    subagents: Vec<LoadedSubagentTranscript>,
) {
    let mut by_call_id: BTreeMap<String, Vec<SubagentTranscript>> = BTreeMap::new();
    let mut fifo_unlinked: VecDeque<SubagentTranscript> = VecDeque::new();
    let mut seen_ids = BTreeSet::new();
    for sub in subagents {
        let LoadedSubagentTranscript {
            agent_id,
            label,
            parent_tool_use_id,
            parsed,
        } = sub;
        if !seen_ids.insert(agent_id.clone()) {
            continue;
        }
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
        if let Some(cid) = parent_tool_use_id {
            by_call_id.entry(cid).or_default().push(view);
        } else {
            fifo_unlinked.push_back(view);
        }
    }
    let mut rebuilt = Vec::with_capacity(parent.entries.len() + fifo_unlinked.len());
    for mut entry in std::mem::take(&mut parent.entries) {
        let extras = attach_matching_agy(&mut entry, &mut by_call_id, &mut fifo_unlinked);
        let turn = entry.turn_index;
        rebuilt.push(entry);
        for sub in extras {
            rebuilt.push(make_agy_subagent_entry(turn, sub));
        }
    }
    let last_turn = rebuilt.last().map_or(1, |e| e.turn_index);
    for sub in by_call_id.into_values().flatten().chain(fifo_unlinked) {
        rebuilt.push(make_agy_subagent_entry(last_turn, sub));
    }
    parent.entries = rebuilt;
}

fn attach_matching_agy(
    entry: &mut TranscriptEntry,
    by_call_id: &mut BTreeMap<String, Vec<SubagentTranscript>>,
    fifo_unlinked: &mut VecDeque<SubagentTranscript>,
) -> Vec<SubagentTranscript> {
    if entry.kind != TranscriptEntryKind::ToolCall
        || entry.tool_name.as_deref() != Some("invoke_subagent")
        || entry.subagent.is_some()
    {
        return Vec::new();
    }
    if let Some(mut list) = entry
        .tool_use_id
        .as_deref()
        .and_then(|cid| by_call_id.remove(cid))
    {
        if !list.is_empty() {
            entry.subagent = Some(list.remove(0));
        }
        return list;
    }
    if let Some(sub) = fifo_unlinked.pop_front() {
        entry.subagent = Some(sub);
    }
    Vec::new()
}

fn make_agy_subagent_entry(turn_index: u32, sub: SubagentTranscript) -> TranscriptEntry {
    let agent_id = sub.agent_id.clone();
    TranscriptEntry {
        id: format!("subagent-{agent_id}"),
        turn_index,
        kind: TranscriptEntryKind::SubagentRun,
        timestamp: None,
        role: Some("assistant".to_owned()),
        text: Some(
            sub.label
                .clone()
                .unwrap_or_else(|| format!("Sub-agent {agent_id}")),
        ),
        tool_name: Some("invoke_subagent".to_owned()),
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
struct AgyParseState {
    entries: Vec<TranscriptEntry>,
    usage: TranscriptUsage,
    turn_index: u32,
    seq: u64,
}

impl AgyParseState {
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

    fn handle_step(&mut self, line_idx: usize, val: &Value) {
        let ts = val
            .get("created_at")
            .or_else(|| val.get("timestamp"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let step_type = val.get("type").and_then(Value::as_str).unwrap_or("");
        match step_type {
            "USER_INPUT" => self.handle_user_input(val, ts),
            "PLANNER_RESPONSE" => self.handle_planner_response(val, ts),
            "TOOL_RESPONSE" | "TOOL_RESULT" => self.handle_tool_response(val, ts),
            "SYSTEM_MESSAGE" | "NOTICE" => self.handle_notice(val, ts),
            other => self.push_raw(
                line_idx,
                ts,
                if other.is_empty() { "unknown" } else { other },
                &val.to_string(),
            ),
        }
    }

    fn handle_user_input(&mut self, val: &Value, ts: Option<String>) {
        if let Some(text) = val.get("content").and_then(Value::as_str) {
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
        }
    }

    fn handle_planner_response(&mut self, val: &Value, ts: Option<String>) {
        let turn = self.turn_index.max(1);
        if let Some(thinking) = val.get("thinking").and_then(Value::as_str) {
            if !thinking.trim().is_empty() {
                self.push_base(
                    "th",
                    turn,
                    TranscriptEntryKind::Thinking,
                    ts.clone(),
                    Some("assistant"),
                    Some(truncate_output(thinking.to_owned())),
                );
            }
        }
        if let Some(text) = val.get("content").and_then(Value::as_str) {
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
        if let Some(calls) = val.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                self.handle_tool_call(call, turn, &ts);
            }
        }
        if let Some(u) = extract_agy_usage(val.get("usage")) {
            merge_usage(&mut self.usage, &u);
            let mut entry = base_entry(
                self.next_id("usage"),
                turn,
                TranscriptEntryKind::Usage,
                ts,
                Some("assistant"),
                None,
            );
            entry.usage = Some(u);
            self.entries.push(entry);
        }
    }

    fn handle_tool_call(&mut self, call: &Value, turn: u32, ts: &Option<String>) {
        let name = call
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let args = call
            .get("arguments")
            .or_else(|| call.get("args"))
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
        entry.tool_use_id = id.clone();
        entry.tool_input = Some(args.clone());
        self.entries.push(entry);

        if let Some((path, action, diff)) = extract_file_event_from_tool(&name, &args) {
            let mut fe = base_entry(
                self.next_id("fe"),
                turn,
                TranscriptEntryKind::FileEvent,
                ts.clone(),
                Some("tool"),
                None,
            );
            fe.tool_name = Some(name);
            fe.tool_use_id = id;
            fe.file_path = Some(path);
            fe.file_action = Some(action);
            fe.diff_preview = diff.map(truncate_output);
            self.entries.push(fe);
        }
    }

    fn handle_tool_response(&mut self, val: &Value, ts: Option<String>) {
        let output = val
            .get("content")
            .or_else(|| val.get("output"))
            .and_then(|v| {
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
        entry.tool_name = val
            .get("tool_name")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        entry.tool_use_id = val
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        entry.tool_output = output.map(truncate_output);
        entry.is_error = val.get("status").and_then(Value::as_str) == Some("ERROR");
        self.entries.push(entry);
    }

    fn handle_notice(&mut self, val: &Value, ts: Option<String>) {
        if let Some(text) = val.get("content").and_then(Value::as_str) {
            if !text.trim().is_empty() {
                let turn = self.turn_index.max(1);
                self.push_base(
                    "n",
                    turn,
                    TranscriptEntryKind::Notice,
                    ts,
                    Some("system"),
                    Some(text.to_owned()),
                );
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

fn extract_agy_usage(val: Option<&Value>) -> Option<TranscriptUsage> {
    let u = val?.as_object()?;
    let input = u
        .get("input_tokens")
        .or_else(|| u.get("prompt_token_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = u
        .get("output_tokens")
        .or_else(|| u.get("candidates_token_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_read = u
        .get("cache_read_tokens")
        .or_else(|| u.get("cached_content_token_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let reasoning = u
        .get("reasoning_tokens")
        .or_else(|| u.get("thoughts_token_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if input == 0 && output == 0 {
        return None;
    }
    Some(TranscriptUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: 0,
        reasoning_tokens: reasoning,
        model: u
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}
