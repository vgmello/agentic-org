//! Office-side read-only conversation transcript discovery, parsing, `SUB-6` sub-agent nesting,
//! toggle filtering, and secret redaction (`TASK-43` / `C4`, `REQ-05`, `FC-150`).
//!
//! All parsing and redaction happen inside the office over an agent's private home directory.
//! Symlinks are refused (both at walk time and with `O_NOFOLLOW | O_NONBLOCK` + `dev`/`ino`
//! verification on open) and raw host paths are never exposed over RPC/HTTP.

pub mod agy;
pub mod claude;
pub mod codex;
pub mod redact;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use org_proto::sessions::{
    FileEventKind, FileTouchSummary, SessionListResult, SessionSummaryView,
    SessionTranscriptResult, TranscriptEntry, TranscriptEntryKind, TranscriptFormat,
    TranscriptUsage,
};
use serde_json::Value;

use crate::credentials::format_rfc3339_ms;

/// Maximum bytes read from one transcript file (8 MiB).
pub const MAX_TRANSCRIPT_READ_BYTES: u64 = 8 * 1024 * 1024;
/// Default page size when `limit` is omitted.
pub const DEFAULT_TRANSCRIPT_LIMIT: usize = 500;
/// Maximum page size for one transcript response.
pub const MAX_TRANSCRIPT_LIMIT: usize = 2_000;
/// Maximum sessions returned by `member.session_list`.
pub const MAX_SESSION_LIST_LIMIT: usize = 200;
const MAX_META_READ_BYTES: u64 = 64 * 1024;
const MAX_TOOL_OUTPUT_CHARS: usize = 8_000;
const MAX_PREVIEW_CHARS: usize = 160;
const MAX_WALK_DEPTH: usize = 8;

/// Internal parsed transcript before filtering, pagination, and redaction.
#[derive(Debug, Clone)]
pub(crate) struct ParsedTranscript {
    pub format: TranscriptFormat,
    pub entries: Vec<TranscriptEntry>,
    pub files: Vec<FileTouchSummary>,
    pub usage: TranscriptUsage,
}

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
    format: TranscriptFormat,
    path: PathBuf,
    modified_ms: u64,
    size_bytes: u64,
    subagent_files: Vec<SubagentFile>,
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

/// Lists all discovered conversation sessions in `private_home`, newest first, with redacted previews.
pub fn list_agent_sessions(
    employee_id: &str,
    private_home: &Path,
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
    let mut discovered = discover_sessions(&canon_home);
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
        let (started_at, preview) = extract_summary_head(&item.path, &canon_home, item.format);
        let mut view = SessionSummaryView {
            session_id: item.session_id.clone(),
            format: item.format,
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
    requested_session_id: Option<&str>,
    live_runtime_session_id: Option<&str>,
    query: TranscriptQuery,
    active_secrets: &[&str],
) -> io::Result<Option<SessionTranscriptResult>> {
    let Ok(canon_home) = private_home.canonicalize() else {
        return Ok(None);
    };
    let mut discovered = discover_sessions(&canon_home);
    if discovered.is_empty() {
        return Ok(None);
    }
    discovered.sort_by_key(|a| std::cmp::Reverse(a.modified_ms));
    let Some(target) = select_session(&discovered, requested_session_id, live_runtime_session_id)
    else {
        return Ok(None);
    };

    let (raw, read_truncated) = read_bounded_utf8(&target.path, &canon_home)?;
    let mut parsed = parse_by_format(target.format, &raw);
    if query.include_subagents {
        attach_discovered_subagents(&mut parsed, target, &canon_home, &raw);
    }

    let mut filtered_entries = filter_entries(parsed.entries, query);
    let total_entries = filtered_entries.len() as u64;
    let start = (query.cursor as usize).min(filtered_entries.len());
    let limit = query.limit.clamp(1, MAX_TRANSCRIPT_LIMIT);
    let end = start.saturating_add(limit).min(filtered_entries.len());
    let page = filtered_entries.drain(start..end).collect();
    let next_cursor = (end < total_entries as usize).then_some(end as u64);

    let files = if query.include_file_changes {
        parsed.files
    } else {
        Vec::new()
    };
    let mut result = SessionTranscriptResult {
        employee_id: employee_id.to_owned(),
        session_id: target.session_id.clone(),
        format: parsed.format,
        is_live: live_runtime_session_id == Some(target.session_id.as_str()),
        file_size_bytes: target.size_bytes,
        total_entries,
        truncated: read_truncated,
        next_cursor,
        usage: parsed.usage,
        files,
        entries: page,
    };
    redact::redact_transcript(&mut result, active_secrets);
    Ok(Some(result))
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

fn parse_by_format(format: TranscriptFormat, raw: &str) -> ParsedTranscript {
    match format {
        TranscriptFormat::Claude => claude::parse_claude_jsonl(raw),
        TranscriptFormat::Codex => codex::parse_codex_jsonl(raw),
        TranscriptFormat::Agy => agy::parse_agy_jsonl(raw),
        _ => claude::parse_claude_jsonl(raw),
    }
}

fn attach_discovered_subagents(
    parsed: &mut ParsedTranscript,
    target: &DiscoveredSession,
    canon_home: &Path,
    raw: &str,
) {
    match target.format {
        TranscriptFormat::Claude => {
            let mut loaded = Vec::new();
            for sf in &target.subagent_files {
                if let Ok((sub_raw, _)) = read_bounded_utf8(&sf.path, canon_home) {
                    let sub_parsed = claude::parse_claude_jsonl(&sub_raw);
                    loaded.push((
                        sf.agent_id.clone(),
                        sf.label.clone(),
                        sf.parent_tool_use_id.clone(),
                        sub_parsed,
                    ));
                }
            }
            if !loaded.is_empty() {
                claude::attach_claude_subagents(parsed, loaded);
            }
        }
        TranscriptFormat::Agy => {
            let refs = agy::extract_agy_subagent_refs(raw, &target.session_id);
            let brain_root = canon_home.join(".gemini/antigravity-cli/brain");
            let mut loaded = Vec::new();
            for r in refs {
                if !is_safe_session_id(&r.conversation_id) {
                    continue;
                }
                let sub_path = brain_root
                    .join(&r.conversation_id)
                    .join(".system_generated/logs/transcript.jsonl");
                if let Ok((sub_raw, _)) = read_bounded_utf8(&sub_path, canon_home) {
                    loaded.push((r, agy::parse_agy_jsonl(&sub_raw)));
                }
            }
            if !loaded.is_empty() {
                agy::attach_agy_subagents(parsed, loaded);
            }
        }
        _ => {}
    }
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

fn hoist_subagent_entry(
    parent: TranscriptEntry,
    sub: org_proto::sessions::SubagentTranscriptView,
) -> TranscriptEntry {
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

fn discover_sessions(canon_home: &Path) -> Vec<DiscoveredSession> {
    let mut out = Vec::new();
    for rel in [".claude/projects", ".config/claude/projects"] {
        discover_claude_under(&canon_home.join(rel), canon_home, &mut out);
    }
    for rel in [".codex/sessions", ".codex/archived_sessions"] {
        discover_codex_under(&canon_home.join(rel), canon_home, &mut out);
    }
    discover_agy_under(
        &canon_home.join(".gemini/antigravity-cli/brain"),
        canon_home,
        &mut out,
    );
    out
}

fn discover_claude_under(root: &Path, canon_home: &Path, out: &mut Vec<DiscoveredSession>) {
    let mut files = Vec::new();
    walk_regular_files(root, canon_home, 0, &mut files);
    for path in files {
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("subagents")
        {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !is_safe_session_id(stem) {
            continue;
        }
        let (modified_ms, size_bytes) = file_meta(&path);
        let subagent_files = discover_claude_subagents(&path, canon_home);
        out.push(DiscoveredSession {
            session_id: stem.to_owned(),
            format: TranscriptFormat::Claude,
            path,
            modified_ms,
            size_bytes,
            subagent_files,
        });
    }
}

fn discover_claude_subagents(session_file: &Path, canon_home: &Path) -> Vec<SubagentFile> {
    let Some(stem) = session_file.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let Some(parent) = session_file.parent() else {
        return Vec::new();
    };
    let sub_dir = parent.join(stem).join("subagents");
    let mut files = Vec::new();
    walk_regular_files(&sub_dir, canon_home, 0, &mut files);
    let mut out = Vec::new();
    for path in files {
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(raw_name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let agent_id = raw_name
            .strip_prefix("agent-")
            .unwrap_or(raw_name)
            .to_owned();
        let (label, parent_tool_use_id) = read_claude_subagent_meta(&path, canon_home);
        out.push(SubagentFile {
            agent_id,
            label,
            parent_tool_use_id,
            path,
        });
    }
    out.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    out
}

fn read_claude_subagent_meta(
    jsonl_path: &Path,
    canon_home: &Path,
) -> (Option<String>, Option<String>) {
    let meta_path = jsonl_path.with_extension("meta.json");
    let mut label = None;
    let mut parent_tool_use_id = None;
    if let Ok(raw) = read_prefix_utf8(&meta_path, canon_home, MAX_META_READ_BYTES) {
        if let Ok(val) = serde_json::from_str::<Value>(&raw) {
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
        parent_tool_use_id = sniff_first_line_field(jsonl_path, canon_home, "parentToolUseId")
            .or_else(|| sniff_first_line_field(jsonl_path, canon_home, "parent_tool_use_id"));
    }
    (label, parent_tool_use_id)
}

fn discover_codex_under(root: &Path, canon_home: &Path, out: &mut Vec<DiscoveredSession>) {
    let mut files = Vec::new();
    walk_regular_files(root, canon_home, 0, &mut files);
    for path in files {
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let session_id = extract_codex_session_id(&path, canon_home)
            .filter(|id| is_safe_session_id(id))
            .unwrap_or_else(|| stem.to_owned());
        if !is_safe_session_id(&session_id) {
            continue;
        }
        let (modified_ms, size_bytes) = file_meta(&path);
        out.push(DiscoveredSession {
            session_id,
            format: TranscriptFormat::Codex,
            path,
            modified_ms,
            size_bytes,
            subagent_files: Vec::new(),
        });
    }
}

fn extract_codex_session_id(path: &Path, canon_home: &Path) -> Option<String> {
    let raw = read_prefix_utf8(path, canon_home, 4096).ok()?;
    let first = raw.lines().next()?.trim();
    let val: Value = serde_json::from_str(first).ok()?;
    val.pointer("/payload/id")
        .or_else(|| val.get("thread_id"))
        .and_then(Value::as_str)
        .filter(|s| is_safe_session_id(s))
        .map(ToOwned::to_owned)
}

fn discover_agy_under(root: &Path, canon_home: &Path, out: &mut Vec<DiscoveredSession>) {
    let Ok(meta) = fs::symlink_metadata(root) else {
        return;
    };
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return;
    }
    let Ok(canon_root) = root.canonicalize() else {
        return;
    };
    if !canon_root.starts_with(canon_home) {
        return;
    }
    let Ok(rd) = fs::read_dir(&canon_root) else {
        return;
    };
    for entry in rd.flatten().take(512) {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() || ft.is_symlink() {
            continue;
        }
        let Some(conv_id) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        if !is_safe_session_id(&conv_id) {
            continue;
        }
        let transcript = entry.path().join(".system_generated/logs/transcript.jsonl");
        if is_safe_regular_file(&transcript, canon_home) {
            let (modified_ms, size_bytes) = file_meta(&transcript);
            let subagent_files =
                discover_agy_subagent_files(&transcript, &canon_root, canon_home, &conv_id);
            out.push(DiscoveredSession {
                session_id: conv_id,
                format: TranscriptFormat::Agy,
                path: transcript,
                modified_ms,
                size_bytes,
                subagent_files,
            });
        }
    }
}

fn discover_agy_subagent_files(
    transcript: &Path,
    brain_root: &Path,
    canon_home: &Path,
    parent_id: &str,
) -> Vec<SubagentFile> {
    let Ok(prefix) = read_prefix_utf8(transcript, canon_home, MAX_META_READ_BYTES) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for r in agy::extract_agy_subagent_refs(&prefix, parent_id) {
        if !is_safe_session_id(&r.conversation_id) {
            continue;
        }
        let sub_path = brain_root
            .join(&r.conversation_id)
            .join(".system_generated/logs/transcript.jsonl");
        if is_safe_regular_file(&sub_path, canon_home) {
            out.push(SubagentFile {
                agent_id: r.conversation_id,
                label: r.label,
                parent_tool_use_id: r.tool_call_id,
                path: sub_path,
            });
        }
    }
    out
}

fn walk_regular_files(dir: &Path, canon_home: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_WALK_DEPTH || out.len() >= 512 {
        return;
    }
    let Ok(meta) = fs::symlink_metadata(dir) else {
        return;
    };
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return;
    }
    let Ok(canon_dir) = dir.canonicalize() else {
        return;
    };
    if !canon_dir.starts_with(canon_home) {
        return;
    }
    let Ok(entries) = fs::read_dir(&canon_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= 512 {
            break;
        }
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            walk_regular_files(&path, canon_home, depth + 1, out);
        } else if ft.is_file() && is_safe_regular_file(&path, canon_home) {
            out.push(path);
        }
    }
}

fn is_safe_regular_file(path: &Path, canon_home: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.file_type().is_symlink() {
        return false;
    }
    path.canonicalize()
        .is_ok_and(|canon| canon.starts_with(canon_home))
}

/// Opens a regular file inside `canon_home` with `O_NOFOLLOW | O_NONBLOCK` and verifies
/// `(dev, ino)` identity against the canonicalized check to close symlink/FIFO TOCTOU races.
fn open_safe_regular_file(path: &Path, canon_home: &Path) -> io::Result<File> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "transcript path must be a regular non-symlink file",
        ));
    }
    let canon = path.canonicalize()?;
    if !canon.starts_with(canon_home) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "transcript path escapes agent private home",
        ));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&canon)?;
    let fmeta = file.metadata()?;
    if !fmeta.is_file() || fmeta.dev() != meta.dev() || fmeta.ino() != meta.ino() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "transcript file changed or is not a regular file",
        ));
    }
    Ok(file)
}

fn read_bounded_utf8(path: &Path, canon_home: &Path) -> io::Result<(String, bool)> {
    let file = open_safe_regular_file(path, canon_home)?;
    let mut buf = Vec::new();
    file.take(MAX_TRANSCRIPT_READ_BYTES + 1)
        .read_to_end(&mut buf)?;
    let truncated = buf.len() as u64 > MAX_TRANSCRIPT_READ_BYTES;
    if truncated {
        buf.truncate(MAX_TRANSCRIPT_READ_BYTES as usize);
        if let Some(last_nl) = buf.iter().rposition(|&b| b == b'\n') {
            buf.truncate(last_nl);
        }
    }
    Ok((String::from_utf8_lossy(&buf).into_owned(), truncated))
}

fn read_prefix_utf8(path: &Path, canon_home: &Path, max_bytes: u64) -> io::Result<String> {
    let file = open_safe_regular_file(path, canon_home)?;
    let mut buf = Vec::new();
    file.take(max_bytes).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn sniff_first_line_field(path: &Path, canon_home: &Path, field: &str) -> Option<String> {
    let raw = read_prefix_utf8(path, canon_home, 8192).ok()?;
    for line in raw.lines().take(5) {
        if let Ok(val) = serde_json::from_str::<Value>(line.trim()) {
            if let Some(s) = val.get(field).and_then(Value::as_str) {
                return Some(s.to_owned());
            }
        }
    }
    None
}

fn file_meta(path: &Path) -> (u64, u64) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return (0, 0);
    };
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    (mtime_ms, meta.len())
}

fn extract_summary_head(
    path: &Path,
    canon_home: &Path,
    format: TranscriptFormat,
) -> (Option<String>, Option<String>) {
    let Ok(prefix) = read_prefix_utf8(path, canon_home, 16_384) else {
        return (None, None);
    };
    let parsed = parse_by_format(format, &prefix);
    let started_at = parsed.entries.iter().find_map(|e| e.timestamp.clone());
    let preview = parsed
        .entries
        .iter()
        .find(|e| {
            matches!(
                e.kind,
                TranscriptEntryKind::UserMessage | TranscriptEntryKind::AssistantMessage
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

pub(crate) fn truncate_output(text: String) -> String {
    if text.chars().count() <= MAX_TOOL_OUTPUT_CHARS {
        text
    } else {
        let mut s: String = text.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
        s.push_str("\n… [truncated]");
        s
    }
}

pub(crate) fn merge_usage(dst: &mut TranscriptUsage, src: &TranscriptUsage) {
    dst.input_tokens = dst.input_tokens.saturating_add(src.input_tokens);
    dst.output_tokens = dst.output_tokens.saturating_add(src.output_tokens);
    dst.cache_read_tokens = dst.cache_read_tokens.saturating_add(src.cache_read_tokens);
    dst.cache_write_tokens = dst
        .cache_write_tokens
        .saturating_add(src.cache_write_tokens);
    dst.reasoning_tokens = dst.reasoning_tokens.saturating_add(src.reasoning_tokens);
    if dst.model.is_none() && src.model.is_some() {
        dst.model = src.model.clone();
    }
}

pub(crate) fn extract_file_event_from_tool(
    tool_name: &str,
    input: &Value,
) -> Option<(String, FileEventKind, Option<String>)> {
    let lower = tool_name.to_ascii_lowercase();
    let path = input
        .get("file_path")
        .or_else(|| input.get("path"))
        .or_else(|| input.get("Path"))
        .or_else(|| input.get("target_file"))
        .and_then(Value::as_str)
        .filter(|p| !p.trim().is_empty())?;
    let (action, diff) = match lower.as_str() {
        "read" | "read_file" | "view_file" | "glob" | "grep" => (FileEventKind::Read, None),
        "write" | "write_file" | "write_to_file" => (
            FileEventKind::Write,
            input
                .get("content")
                .or_else(|| input.get("Content"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        ),
        "edit" | "multiedit" | "notebookedit" | "edit_file" | "replace_file_content" => {
            let old = input
                .get("old_string")
                .or_else(|| input.get("Target"))
                .and_then(Value::as_str);
            let new = input
                .get("new_string")
                .or_else(|| input.get("Replacement"))
                .and_then(Value::as_str);
            let preview = match (old, new) {
                (Some(o), Some(n)) => Some(format!("- {o}\n+ {n}")),
                (_, Some(n)) => Some(format!("+ {n}")),
                _ => None,
            };
            (FileEventKind::Edit, preview)
        }
        "delete" | "delete_file" => (FileEventKind::Delete, None),
        _ => return None,
    };
    Some((path.to_owned(), action, diff))
}

pub(crate) fn aggregate_file_touches(entries: &[TranscriptEntry]) -> Vec<FileTouchSummary> {
    let mut map: BTreeMap<String, (BTreeSet<FileEventKind>, u32, u32)> = BTreeMap::new();
    for entry in entries {
        if entry.kind == TranscriptEntryKind::FileEvent {
            if let (Some(path), Some(action)) = (&entry.file_path, entry.file_action) {
                let rec = map
                    .entry(path.clone())
                    .or_insert_with(|| (BTreeSet::new(), 0, 0));
                rec.0.insert(action);
                rec.1 = rec.1.saturating_add(1);
                rec.2 = rec.2.max(entry.turn_index);
            }
        }
    }
    map.into_iter()
        .map(|(path, (ops, count, last_turn_index))| FileTouchSummary {
            path,
            operations: ops.into_iter().collect(),
            count,
            last_turn_index,
        })
        .collect()
}

#[cfg(test)]
mod tests;
