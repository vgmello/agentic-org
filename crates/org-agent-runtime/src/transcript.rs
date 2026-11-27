//! Sans-I/O conversation transcript IR, layout descriptors, and [`TranscriptParser`] port
//! (`REQ-05`, `SUB-6`, `TASK-216`).
//!
//! Adapters implement [`TranscriptParser`] purely in memory without touching the filesystem.
//! The office (`org-office-sessions`) performs all symlink-safe, bounded file discovery and
//! reads, then routes every parsed summary and entry through its single redaction choke-point.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// Maximum character count retained for a single tool output, thinking block, or diff preview.
pub const MAX_TOOL_OUTPUT_CHARS: usize = 8_000;

/// Normalized conversation entry kind shared across runtime adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TranscriptEntryKind {
    /// A user prompt or instruction turn.
    UserMessage,
    /// An assistant text response.
    AssistantMessage,
    /// Assistant reasoning or thinking text.
    Thinking,
    /// A tool invocation emitted by the assistant.
    ToolCall,
    /// Output returned from a tool call.
    ToolResult,
    /// A file read, write, edit, or delete inferred from tool activity.
    FileEvent,
    /// Token usage metrics observed for a message or turn.
    Usage,
    /// A system notice, session banner, or summary.
    Notice,
    /// A hoisted or unlinked sub-agent conversation block.
    SubagentRun,
    /// An unrecognized or malformed JSONL line preserved verbatim for transparency.
    Raw,
}

/// File operation classified for file-based visualization panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum TranscriptFileEventKind {
    /// File read or search operation.
    Read,
    /// File creation or full write.
    Write,
    /// In-place file modification or patch.
    Edit,
    /// File deletion.
    Delete,
    /// Non-mutating reference to a file.
    Reference,
}

/// Aggregated file touch summary extracted from a parsed transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptFileTouch {
    /// Path reported by the runtime tool call.
    pub path: String,
    /// Distinct operations performed on `path`, in deterministic order.
    pub operations: Vec<TranscriptFileEventKind>,
    /// Number of file events targeting `path`.
    pub count: u32,
    /// Highest turn index that touched `path`.
    pub last_turn_index: u32,
}

/// Aggregated or per-entry token usage metrics extracted from a transcript.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscriptUsage {
    /// Prompt/input token count.
    pub input_tokens: u64,
    /// Completion/output token count.
    pub output_tokens: u64,
    /// Cached input tokens read.
    pub cache_read_tokens: u64,
    /// Cached input tokens written.
    pub cache_write_tokens: u64,
    /// Reasoning/thinking output tokens.
    pub reasoning_tokens: u64,
    /// Model identifier reported in the transcript, if any.
    pub model: Option<String>,
}

/// Nested sub-agent conversation transcript attached to the parent turn that launched it.
#[derive(Debug, Clone, PartialEq)]
pub struct SubagentTranscript {
    /// Runtime-assigned sub-agent identifier.
    pub agent_id: String,
    /// Human-readable description or role label, if available.
    pub label: Option<String>,
    /// Parent tool-call identifier that spawned this sub-agent, if known.
    pub parent_tool_use_id: Option<String>,
    /// Runtime format identifier (`"claude"`, `"codex"`, `"agy"`, ...).
    pub format_id: String,
    /// Number of entries in `entries`.
    pub entry_count: u32,
    /// Normalized sub-agent conversation entries.
    pub entries: Vec<TranscriptEntry>,
    /// Aggregated file touches within the sub-agent conversation.
    pub files: Vec<TranscriptFileTouch>,
    /// Aggregated token usage within the sub-agent conversation.
    pub usage: TranscriptUsage,
}

/// One normalized conversation transcript entry before office-side filtering and redaction.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptEntry {
    /// Stable per-parse entry identifier.
    pub id: String,
    /// 1-based turn index associated with this entry.
    pub turn_index: u32,
    /// Normalized entry kind.
    pub kind: TranscriptEntryKind,
    /// RFC 3339 timestamp string from the runtime log, if present.
    pub timestamp: Option<String>,
    /// Role label (`"user"`, `"assistant"`, `"tool"`, `"system"`), if applicable.
    pub role: Option<String>,
    /// Primary text body (message, thinking, notice, or raw line).
    pub text: Option<String>,
    /// Invoked tool name, if applicable.
    pub tool_name: Option<String>,
    /// Correlation ID linking a tool call and its result or sub-agent.
    pub tool_use_id: Option<String>,
    /// Structured tool input payload, if applicable.
    pub tool_input: Option<Value>,
    /// Truncated tool output text, if applicable.
    pub tool_output: Option<String>,
    /// Whether the tool result reported an error.
    pub is_error: bool,
    /// Target path for a [`TranscriptEntryKind::FileEvent`].
    pub file_path: Option<String>,
    /// Operation kind for a [`TranscriptEntryKind::FileEvent`].
    pub file_action: Option<TranscriptFileEventKind>,
    /// Truncated content or diff preview for a [`TranscriptEntryKind::FileEvent`].
    pub diff_preview: Option<String>,
    /// Usage metrics for a [`TranscriptEntryKind::Usage`] or [`TranscriptEntryKind::SubagentRun`].
    pub usage: Option<TranscriptUsage>,
    /// Original vendor frame tag when `kind == TranscriptEntryKind::Raw`.
    pub raw_type: Option<String>,
    /// Nested sub-agent transcript attached to this entry (`SUB-6`).
    pub subagent: Option<SubagentTranscript>,
}

/// Parsed conversation transcript returned by [`TranscriptParser::parse`].
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTranscript {
    /// Runtime format identifier (`"claude"`, `"codex"`, `"agy"`, ...).
    pub format_id: String,
    /// Normalized entries in log order.
    pub entries: Vec<TranscriptEntry>,
    /// Aggregated file touches across `entries`.
    pub files: Vec<TranscriptFileTouch>,
    /// Aggregated token usage across the session (and attached sub-agents).
    pub usage: TranscriptUsage,
}

/// Reference to a sibling sub-agent conversation discovered inside a parent transcript body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentTranscriptRef {
    /// Sub-agent conversation or agent identifier (validated by the office before path joins).
    pub agent_id: String,
    /// Optional human-readable label or role.
    pub label: Option<String>,
    /// Optional parent tool-call identifier that launched this sub-agent.
    pub parent_tool_use_id: Option<String>,
}

/// A sub-agent transcript read safely by the office and parsed by the adapter, ready to nest.
#[derive(Debug, Clone)]
pub struct LoadedSubagentTranscript {
    /// Sub-agent identifier.
    pub agent_id: String,
    /// Optional human-readable label or role.
    pub label: Option<String>,
    /// Optional parent tool-call identifier.
    pub parent_tool_use_id: Option<String>,
    /// Parsed sub-agent transcript.
    pub parsed: ParsedTranscript,
}

/// Pure filesystem layout descriptor telling the office where a runtime writes transcript files
/// inside an agent's private home directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TranscriptDiscoveryLayout {
    /// Recursively walk `search_roots` under the agent's private home for `.jsonl` files,
    /// skipping files whose immediate parent directory matches `skip_parent_dir`.
    RecursiveJsonl {
        /// Home-relative root directories to walk (e.g. `&[".claude/projects"]`).
        search_roots: &'static [&'static str],
        /// Immediate parent directory name skipped during top-level session discovery.
        skip_parent_dir: Option<&'static str>,
        /// Optional relative directory under `<session_parent>/<session_stem>/` containing
        /// sibling sub-agent `.jsonl` and `.meta.json` files.
        subagents_subdir: Option<&'static str>,
        /// Optional filename prefix stripped from sub-agent stems to obtain `agent_id`.
        subagent_stem_prefix: Option<&'static str>,
    },
    /// Direct child directories `<session_id>` under `brain_root`, each containing a fixed
    /// relative transcript file `transcript_rel_path`. Sub-agents are sibling conversation
    /// directories referenced by [`TranscriptParser::extract_subagent_refs`].
    ConversationDir {
        /// Home-relative root directory containing one folder per conversation.
        brain_root: &'static str,
        /// Relative path to the transcript file inside each conversation folder.
        transcript_rel_path: &'static str,
    },
}

/// Sans-I/O conversation transcript parser and metadata extractor owned by a runtime adapter.
pub trait TranscriptParser: Send + Sync {
    /// Stable format identifier matching the runtime ID (e.g. `"claude"`, `"codex"`, `"agy"`).
    fn format_id(&self) -> &'static str;

    /// Pure filesystem layout descriptor the office uses to discover session files safely.
    fn discovery_layout(&self) -> TranscriptDiscoveryLayout;

    /// Extracts an authoritative session ID from the prefix of a transcript file when the
    /// runtime records it inside the log (e.g. Codex `session_meta`), or `None` to use the
    /// file/directory stem.
    fn extract_session_id_from_prefix(&self, prefix: &str) -> Option<String> {
        let _ = prefix;
        None
    }

    /// Parses optional sibling metadata JSON and/or transcript prefix for a discovered
    /// sub-agent file, returning `(label, parent_tool_use_id)`.
    fn parse_subagent_meta(
        &self,
        meta_json: Option<&str>,
        transcript_prefix: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        let _ = (meta_json, transcript_prefix);
        (None, None)
    }

    /// Extracts deduplicated sibling sub-agent references from a parent transcript body/prefix.
    fn extract_subagent_refs(
        &self,
        raw: &str,
        parent_session_id: &str,
    ) -> Vec<SubagentTranscriptRef> {
        let _ = (raw, parent_session_id);
        Vec::new()
    }

    /// Parses a single JSONL transcript (main or sub-agent) into normalized entries, file
    /// touches, and token usage. Unrecognized or malformed lines become [`TranscriptEntryKind::Raw`].
    fn parse(&self, raw: &str) -> ParsedTranscript;

    /// Nests loaded sub-agent transcripts onto their launching parent entries (`SUB-6`),
    /// merging sub-agent token usage into `parent.usage`.
    fn attach_subagents(
        &self,
        parent: &mut ParsedTranscript,
        subagents: Vec<LoadedSubagentTranscript>,
    ) {
        let _ = (parent, subagents);
    }
}

/// Constructs a blank [`TranscriptEntry`] populated with its core identifying fields.
pub fn base_entry(
    id: String,
    turn_index: u32,
    kind: TranscriptEntryKind,
    timestamp: Option<String>,
    role: Option<&str>,
    text: Option<String>,
) -> TranscriptEntry {
    TranscriptEntry {
        id,
        turn_index,
        kind,
        timestamp,
        role: role.map(ToOwned::to_owned),
        text,
        tool_name: None,
        tool_use_id: None,
        tool_input: None,
        tool_output: None,
        is_error: false,
        file_path: None,
        file_action: None,
        diff_preview: None,
        usage: None,
        raw_type: None,
        subagent: None,
    }
}

/// Truncates `text` to at most [`MAX_TOOL_OUTPUT_CHARS`] Unicode scalar values, appending a
/// truncation notice when clipped.
pub fn truncate_output(text: String) -> String {
    if text.chars().count() <= MAX_TOOL_OUTPUT_CHARS {
        text
    } else {
        let mut s: String = text.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
        s.push_str("\n… [truncated]");
        s
    }
}

/// Accumulates token counts from `src` into `dst`, keeping the first observed model name.
pub fn merge_usage(dst: &mut TranscriptUsage, src: &TranscriptUsage) {
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

/// Infers a file touch event `(path, action, diff_preview)` from a generic file tool call.
pub fn extract_file_event_from_tool(
    tool_name: &str,
    input: &Value,
) -> Option<(String, TranscriptFileEventKind, Option<String>)> {
    let lower = tool_name.to_ascii_lowercase();
    let path = input
        .get("file_path")
        .or_else(|| input.get("path"))
        .or_else(|| input.get("Path"))
        .or_else(|| input.get("target_file"))
        .and_then(Value::as_str)
        .filter(|p| !p.trim().is_empty())?;
    let (action, diff) = match lower.as_str() {
        "read" | "read_file" | "view_file" | "glob" | "grep" => {
            (TranscriptFileEventKind::Read, None)
        }
        "write" | "write_file" | "write_to_file" => (
            TranscriptFileEventKind::Write,
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
            (TranscriptFileEventKind::Edit, preview)
        }
        "delete" | "delete_file" => (TranscriptFileEventKind::Delete, None),
        _ => return None,
    };
    Some((path.to_owned(), action, diff))
}

/// Aggregates all [`TranscriptEntryKind::FileEvent`] entries into sorted per-path summaries.
pub fn aggregate_file_touches(entries: &[TranscriptEntry]) -> Vec<TranscriptFileTouch> {
    let mut map: BTreeMap<String, (BTreeSet<TranscriptFileEventKind>, u32, u32)> = BTreeMap::new();
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
        .map(
            |(path, (ops, count, last_turn_index))| TranscriptFileTouch {
                path,
                operations: ops.into_iter().collect(),
                count,
                last_turn_index,
            },
        )
        .collect()
}
