//! Office-side read-only conversation transcript discovery, vendor-agnostic parser dispatch,
//! `SUB-6` sub-agent nesting, toggle filtering, and secret redaction (`TASK-43` / `C4`, `REQ-05`,
//! `FC-150`, `TASK-216`).
//!
//! All filesystem discovery, bounded reads, and redaction happen inside the office over an
//! agent's private home directory. Symlinks are refused (both at walk time and via component-wise
//! `open_regular_beneath` with `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`), raw host paths are never
//! exposed over RPC/HTTP, and vendor-specific JSONL parsing is delegated to each registered
//! [`org_agent_runtime::AgentRuntime::transcript_parser`].

pub mod redact;

use std::io;
use std::path::{Path, PathBuf};

use org_agent_runtime::transcript as rt;
use org_agent_runtime::transcript::{
    LoadedSubagentTranscript, TranscriptDiscoveryLayout, TranscriptParser,
};
use org_proto::sessions::{
    FileEventKind, FileTouchSummary, SessionListResult, SessionSummaryView,
    SessionTranscriptResult, SubagentTranscriptView, TranscriptEntry, TranscriptEntryKind,
    TranscriptFormat, TranscriptUsage,
};
use serde_json::Value;

use crate::credentials::format_rfc3339_ms;
use crate::runtime_registry::RuntimeRegistry;

mod discover;

use discover::{discover_sessions, read_bounded_utf8_with_limit, read_prefix_utf8};

/// Maximum bytes read from one transcript file (8 MiB).
pub const MAX_TRANSCRIPT_READ_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum cumulative raw bytes read across the main transcript and all attached sub-agents (16 MiB).
pub const MAX_TOTAL_TRANSCRIPT_BYTES: usize = 16 * 1024 * 1024;
/// Maximum sub-agent transcripts discovered or attached for a single session.
pub const MAX_SUBAGENTS_PER_TRANSCRIPT: usize = 32;
/// Maximum entries retained per attached sub-agent transcript.
pub const MAX_SUBAGENT_ENTRIES: usize = 256;
/// Default page size when `limit` is omitted.
pub const DEFAULT_TRANSCRIPT_LIMIT: usize = 500;
/// Maximum page size for one transcript response.
pub const MAX_TRANSCRIPT_LIMIT: usize = 2_000;
/// Maximum sessions returned by `member.session_list`.
pub const MAX_SESSION_LIST_LIMIT: usize = 200;
const MAX_META_READ_BYTES: u64 = 64 * 1024;
const MAX_PREVIEW_CHARS: usize = 160;
const MAX_WALK_DEPTH: usize = 8;

/// Filter and pagination options for [`load_agent_transcript`].
#[derive(Debug, Clone, Copy)]
pub struct TranscriptQuery {
    pub include_tool_calls: bool,
    pub include_file_changes: bool,
    pub include_subagents: bool,
    pub cursor: u64,
    pub limit: usize,
}

impl Default for TranscriptQuery {
    fn default() -> Self {
        Self {
            include_tool_calls: true,
            include_file_changes: true,
            include_subagents: true,
            cursor: 0,
            limit: DEFAULT_TRANSCRIPT_LIMIT,
        }
    }
}

#[derive(Debug, Clone)]
struct DiscoveredSession {
    session_id: String,
    format_id: &'static str,
    path: PathBuf,
    modified_ms: u64,
    size_bytes: u64,
    subagent_files: Vec<SubagentFile>,
    subagents_capped: bool,
}

#[derive(Debug, Clone)]
struct SubagentFile {
    agent_id: String,
    label: Option<String>,
    parent_tool_use_id: Option<String>,
    path: PathBuf,
}

/// Strict allowlist for discovered and requested session identifiers.
pub fn is_safe_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.contains("..")
        && id != "."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Lists all discovered conversation sessions in `private_home`, newest first, with redacted
/// previews.
pub fn list_agent_sessions(
    employee_id: &str,
    private_home: &Path,
    runtimes: &RuntimeRegistry,
    live_runtime_session_id: Option<&str>,
    limit: Option<u32>,
    active_secrets: &[&str],
) -> SessionListResult {
    let cap = limit
        .map(|n| (n as usize).clamp(1, MAX_SESSION_LIST_LIMIT))
        .unwrap_or(50);
    let Ok(canon_home) = private_home.canonicalize() else {
        return SessionListResult {
            employee_id: employee_id.to_owned(),
            live_session_id: live_runtime_session_id.map(ToOwned::to_owned),
            sessions: Vec::new(),
        };
    };
    let mut discovered = discover_sessions(&canon_home, runtimes);
    discovered.sort_by(|a, b| {
        let a_live = live_runtime_session_id == Some(a.session_id.as_str());
        let b_live = live_runtime_session_id == Some(b.session_id.as_str());
        b_live
            .cmp(&a_live)
            .then_with(|| b.modified_ms.cmp(&a.modified_ms))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    let mut sessions = Vec::new();
    for item in discovered.into_iter().take(cap) {
        let (started_at, preview) = find_parser(runtimes, item.format_id)
            .map(|parser| extract_summary_head(&item.path, &canon_home, parser))
            .unwrap_or((None, None));
        let mut view = SessionSummaryView {
            session_id: item.session_id.clone(),
            format: format_from_id(item.format_id),
            is_live: live_runtime_session_id == Some(item.session_id.as_str()),
            started_at,
            updated_at: (item.modified_ms > 0).then(|| format_rfc3339_ms(item.modified_ms)),
            size_bytes: item.size_bytes,
            subagent_count: item.subagent_files.len() as u32,
            preview,
        };
        redact::redact_summary(&mut view, active_secrets);
        sessions.push(view);
    }
    SessionListResult {
        employee_id: employee_id.to_owned(),
        live_session_id: live_runtime_session_id.map(ToOwned::to_owned),
        sessions,
    }
}

/// Parses, nests sub-agents (`SUB-6`), applies visibility toggles, paginates, and redacts a
/// session transcript from `private_home`.
pub fn load_agent_transcript(
    employee_id: &str,
    private_home: &Path,
    runtimes: &RuntimeRegistry,
    requested_session_id: Option<&str>,
    live_runtime_session_id: Option<&str>,
    query: TranscriptQuery,
    active_secrets: &[&str],
) -> io::Result<Option<SessionTranscriptResult>> {
    let Ok(canon_home) = private_home.canonicalize() else {
        return Ok(None);
    };
    let mut discovered = discover_sessions(&canon_home, runtimes);
    if discovered.is_empty() {
        return Ok(None);
    }
    discovered.sort_by_key(|a| std::cmp::Reverse(a.modified_ms));
    let Some(target) = select_session(&discovered, requested_session_id, live_runtime_session_id)
    else {
        return Ok(None);
    };
    let Some(parser) = find_parser(runtimes, target.format_id) else {
        return Ok(None);
    };

    let (raw, mut read_truncated, main_bytes) =
        read_bounded_utf8_with_limit(&target.path, &canon_home, MAX_TRANSCRIPT_READ_BYTES)?;
    let mut parsed = parser.parse(&raw);
    if query.include_subagents {
        if target.subagents_capped {
            read_truncated = true;
        }
        let remaining_budget = MAX_TOTAL_TRANSCRIPT_BYTES.saturating_sub(main_bytes);
        if attach_discovered_subagents(
            parser,
            &mut parsed,
            target,
            &canon_home,
            &raw,
            remaining_budget,
        ) {
            read_truncated = true;
        }
    }

    let format = format_from_id(&parsed.format_id);
    let usage = convert_usage(parsed.usage);
    let parsed_files = convert_files(parsed.files);
    let proto_entries = parsed.entries.into_iter().map(convert_entry).collect();

    let mut filtered_entries = filter_entries(proto_entries, query);
    let total_entries = filtered_entries.len() as u64;
    let start = (query.cursor as usize).min(filtered_entries.len());
    let limit = query.limit.clamp(1, MAX_TRANSCRIPT_LIMIT);
    let end = start.saturating_add(limit).min(filtered_entries.len());
    let page = filtered_entries.drain(start..end).collect();
    let next_cursor = (end < total_entries as usize).then_some(end as u64);

    let files = if query.include_file_changes {
        parsed_files
    } else {
        Vec::new()
    };
    let mut result = SessionTranscriptResult {
        employee_id: employee_id.to_owned(),
        session_id: target.session_id.clone(),
        format,
        is_live: live_runtime_session_id == Some(target.session_id.as_str()),
        file_size_bytes: target.size_bytes,
        total_entries,
        truncated: read_truncated,
        next_cursor,
        usage,
        files,
        entries: page,
    };
    redact::redact_transcript(&mut result, active_secrets);
    Ok(Some(result))
}

fn find_parser<'a>(
    runtimes: &'a RuntimeRegistry,
    format_id: &str,
) -> Option<&'a dyn TranscriptParser> {
    runtimes
        .adapters()
        .filter_map(|adapter| adapter.transcript_parser())
        .find(|parser| parser.format_id() == format_id)
}

fn select_session<'a>(
    discovered: &'a [DiscoveredSession],
    requested: Option<&str>,
    live: Option<&str>,
) -> Option<&'a DiscoveredSession> {
    if let Some(req) = requested {
        return discovered.iter().find(|s| s.session_id == req);
    }
    if let Some(live_id) = live {
        if let Some(found) = discovered.iter().find(|s| s.session_id == live_id) {
            return Some(found);
        }
    }
    discovered.first()
}

struct SubagentLoadState<'a> {
    parser: &'a dyn TranscriptParser,
    canon_home: &'a Path,
    remaining_bytes: usize,
    truncated: bool,
    loaded: Vec<LoadedSubagentTranscript>,
}

impl SubagentLoadState<'_> {
    fn try_load(
        &mut self,
        path: &Path,
        agent_id: String,
        label: Option<String>,
        parent_tool_use_id: Option<String>,
    ) -> bool {
        if !is_safe_session_id(&agent_id) {
            return true;
        }
        if self.loaded.len() >= MAX_SUBAGENTS_PER_TRANSCRIPT || self.remaining_bytes == 0 {
            self.truncated = true;
            return false;
        }
        let file_limit = (self.remaining_bytes as u64).min(MAX_TRANSCRIPT_READ_BYTES);
        let Ok((sub_raw, sub_truncated, bytes_used)) =
            read_bounded_utf8_with_limit(path, self.canon_home, file_limit)
        else {
            return true;
        };
        self.remaining_bytes = self.remaining_bytes.saturating_sub(bytes_used);
        if sub_truncated {
            self.truncated = true;
        }
        let mut sub_parsed = self.parser.parse(&sub_raw);
        if sub_parsed.entries.len() > MAX_SUBAGENT_ENTRIES {
            sub_parsed.entries.truncate(MAX_SUBAGENT_ENTRIES);
            sub_parsed.files = rt::aggregate_file_touches(&sub_parsed.entries);
            self.truncated = true;
        }
        self.loaded.push(LoadedSubagentTranscript {
            agent_id,
            label,
            parent_tool_use_id,
            parsed: sub_parsed,
        });
        true
    }
}

fn attach_discovered_subagents(
    parser: &dyn TranscriptParser,
    parsed: &mut rt::ParsedTranscript,
    target: &DiscoveredSession,
    canon_home: &Path,
    raw: &str,
    remaining_bytes: usize,
) -> bool {
    let mut state = SubagentLoadState {
        parser,
        canon_home,
        remaining_bytes,
        truncated: false,
        loaded: Vec::new(),
    };
    match parser.discovery_layout() {
        TranscriptDiscoveryLayout::RecursiveJsonl { .. } => {
            for sf in &target.subagent_files {
                if !state.try_load(
                    &sf.path,
                    sf.agent_id.clone(),
                    sf.label.clone(),
                    sf.parent_tool_use_id.clone(),
                ) {
                    break;
                }
            }
        }
        TranscriptDiscoveryLayout::ConversationDir {
            brain_root,
            transcript_rel_path,
        } => {
            let brain_dir = canon_home.join(brain_root);
            for r in parser.extract_subagent_refs(raw, &target.session_id) {
                if !is_safe_session_id(&r.agent_id) {
                    continue;
                }
                let sub_path = brain_dir.join(&r.agent_id).join(transcript_rel_path);
                if !state.try_load(&sub_path, r.agent_id, r.label, r.parent_tool_use_id) {
                    break;
                }
            }
        }
        _ => {}
    }
    if !state.loaded.is_empty() {
        parser.attach_subagents(parsed, state.loaded);
    }
    state.truncated
}

fn filter_entries(entries: Vec<TranscriptEntry>, query: TranscriptQuery) -> Vec<TranscriptEntry> {
    let mut out = Vec::with_capacity(entries.len());
    for mut entry in entries {
        if !query.include_file_changes && entry.kind == TranscriptEntryKind::FileEvent {
            continue;
        }
        if !query.include_file_changes {
            entry.diff_preview = None;
        }
        if let Some(ref mut sub) = entry.subagent {
            sub.entries = filter_entries(std::mem::take(&mut sub.entries), query);
            sub.entry_count = sub.entries.len() as u32;
            if !query.include_file_changes {
                sub.files.clear();
            }
        }
        if !query.include_tool_calls
            && matches!(
                entry.kind,
                TranscriptEntryKind::ToolCall | TranscriptEntryKind::ToolResult
            )
        {
            if query.include_subagents {
                if let Some(sub) = entry.subagent.take() {
                    out.push(hoist_subagent_entry(entry, sub));
                }
            }
            continue;
        }
        out.push(entry);
    }
    out
}

fn hoist_subagent_entry(parent: TranscriptEntry, sub: SubagentTranscriptView) -> TranscriptEntry {
    let label = sub
        .label
        .clone()
        .unwrap_or_else(|| format!("Sub-agent {}", sub.agent_id));
    TranscriptEntry {
        id: format!("sub-{}", parent.id),
        turn_index: parent.turn_index,
        kind: TranscriptEntryKind::SubagentRun,
        timestamp: parent.timestamp,
        role: Some("assistant".to_owned()),
        text: Some(label),
        tool_name: parent.tool_name,
        tool_use_id: parent.tool_use_id,
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

fn extract_summary_head(
    path: &Path,
    canon_home: &Path,
    parser: &dyn TranscriptParser,
) -> (Option<String>, Option<String>) {
    let Ok(prefix) = read_prefix_utf8(path, canon_home, 16_384) else {
        return (None, None);
    };
    let parsed = parser.parse(&prefix);
    let started_at = parsed.entries.iter().find_map(|e| e.timestamp.clone());
    let preview = parsed
        .entries
        .iter()
        .find(|e| {
            matches!(
                e.kind,
                rt::TranscriptEntryKind::UserMessage | rt::TranscriptEntryKind::AssistantMessage
            )
        })
        .and_then(|e| e.text.as_deref())
        .map(clip_preview);
    (started_at, preview)
}

fn clip_preview(text: &str) -> String {
    let clean: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() <= MAX_PREVIEW_CHARS {
        clean
    } else {
        let mut s: String = clean.chars().take(MAX_PREVIEW_CHARS - 1).collect();
        s.push('…');
        s
    }
}

fn format_from_id(format_id: &str) -> TranscriptFormat {
    serde_json::from_value(Value::String(format_id.to_owned())).unwrap_or(TranscriptFormat::Unknown)
}

fn convert_entry_kind(kind: rt::TranscriptEntryKind) -> TranscriptEntryKind {
    match kind {
        rt::TranscriptEntryKind::UserMessage => TranscriptEntryKind::UserMessage,
        rt::TranscriptEntryKind::AssistantMessage => TranscriptEntryKind::AssistantMessage,
        rt::TranscriptEntryKind::Thinking => TranscriptEntryKind::Thinking,
        rt::TranscriptEntryKind::ToolCall => TranscriptEntryKind::ToolCall,
        rt::TranscriptEntryKind::ToolResult => TranscriptEntryKind::ToolResult,
        rt::TranscriptEntryKind::FileEvent => TranscriptEntryKind::FileEvent,
        rt::TranscriptEntryKind::Usage => TranscriptEntryKind::Usage,
        rt::TranscriptEntryKind::Notice => TranscriptEntryKind::Notice,
        rt::TranscriptEntryKind::SubagentRun => TranscriptEntryKind::SubagentRun,
        rt::TranscriptEntryKind::Raw => TranscriptEntryKind::Raw,
        _ => TranscriptEntryKind::Unknown,
    }
}

fn convert_file_event_kind(kind: rt::TranscriptFileEventKind) -> FileEventKind {
    match kind {
        rt::TranscriptFileEventKind::Read => FileEventKind::Read,
        rt::TranscriptFileEventKind::Write => FileEventKind::Write,
        rt::TranscriptFileEventKind::Edit => FileEventKind::Edit,
        rt::TranscriptFileEventKind::Delete => FileEventKind::Delete,
        rt::TranscriptFileEventKind::Reference => FileEventKind::Reference,
        _ => FileEventKind::Unknown,
    }
}

fn convert_usage(usage: rt::TranscriptUsage) -> TranscriptUsage {
    TranscriptUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        model: usage.model,
    }
}

fn convert_files(files: Vec<rt::TranscriptFileTouch>) -> Vec<FileTouchSummary> {
    files
        .into_iter()
        .map(|f| FileTouchSummary {
            path: f.path,
            operations: f
                .operations
                .into_iter()
                .map(convert_file_event_kind)
                .collect(),
            count: f.count,
            last_turn_index: f.last_turn_index,
        })
        .collect()
}

fn convert_subagent(mut sub: rt::SubagentTranscript) -> SubagentTranscriptView {
    if sub.entries.len() > MAX_SUBAGENT_ENTRIES {
        sub.entries.truncate(MAX_SUBAGENT_ENTRIES);
    }
    let entries: Vec<TranscriptEntry> = sub.entries.into_iter().map(convert_entry).collect();
    SubagentTranscriptView {
        agent_id: sub.agent_id,
        label: sub.label,
        parent_tool_use_id: sub.parent_tool_use_id,
        format: format_from_id(&sub.format_id),
        entry_count: entries.len() as u32,
        entries,
        files: convert_files(sub.files),
        usage: convert_usage(sub.usage),
    }
}

fn convert_entry(entry: rt::TranscriptEntry) -> TranscriptEntry {
    TranscriptEntry {
        id: entry.id,
        turn_index: entry.turn_index,
        kind: convert_entry_kind(entry.kind),
        timestamp: entry.timestamp,
        role: entry.role,
        text: entry.text,
        tool_name: entry.tool_name,
        tool_use_id: entry.tool_use_id,
        tool_input: entry.tool_input,
        tool_output: entry.tool_output,
        is_error: entry.is_error,
        file_path: entry.file_path,
        file_action: entry.file_action.map(convert_file_event_kind),
        diff_preview: entry.diff_preview,
        usage: entry.usage.map(convert_usage),
        raw_type: entry.raw_type,
        subagent: entry.subagent.map(convert_subagent),
    }
}

#[cfg(test)]
mod tests;
