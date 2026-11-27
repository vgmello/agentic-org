//! Bounded, symlink-safe discovery of runtime session and sub-agent transcript files.
//!
//! Filesystem traversal, symlink refusal, and component-wise `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`
//! opens (`org_platform::fs::open_regular_beneath`) live exclusively here in the office; runtime
//! adapters supply only pure layout descriptors and metadata extractors via [`TranscriptParser`].

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use org_agent_runtime::transcript::{TranscriptDiscoveryLayout, TranscriptParser};

use crate::runtime_registry::RuntimeRegistry;

use super::{
    is_safe_session_id, DiscoveredSession, SubagentFile, MAX_META_READ_BYTES,
    MAX_SUBAGENTS_PER_TRANSCRIPT, MAX_TRANSCRIPT_READ_BYTES, MAX_WALK_DEPTH,
};

pub(super) fn discover_sessions(
    canon_home: &Path,
    runtimes: &RuntimeRegistry,
) -> Vec<DiscoveredSession> {
    let mut out = Vec::new();
    for adapter in runtimes.adapters() {
        let Some(parser) = adapter.transcript_parser() else {
            continue;
        };
        match parser.discovery_layout() {
            TranscriptDiscoveryLayout::RecursiveJsonl {
                search_roots,
                skip_parent_dir,
                subagents_subdir,
                subagent_stem_prefix,
            } => {
                for rel in search_roots {
                    discover_recursive_jsonl(
                        &canon_home.join(rel),
                        canon_home,
                        parser,
                        skip_parent_dir,
                        subagents_subdir,
                        subagent_stem_prefix,
                        &mut out,
                    );
                }
            }
            TranscriptDiscoveryLayout::ConversationDir {
                brain_root,
                transcript_rel_path,
            } => {
                discover_conversation_dirs(
                    &canon_home.join(brain_root),
                    transcript_rel_path,
                    canon_home,
                    parser,
                    &mut out,
                );
            }
            _ => {}
        }
    }
    out
}

fn discover_recursive_jsonl(
    root: &Path,
    canon_home: &Path,
    parser: &dyn TranscriptParser,
    skip_parent_dir: Option<&str>,
    subagents_subdir: Option<&str>,
    subagent_stem_prefix: Option<&str>,
    out: &mut Vec<DiscoveredSession>,
) {
    let mut files = Vec::new();
    walk_regular_files(root, canon_home, 0, &mut files);
    for path in files {
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if let Some(skip) = skip_parent_dir {
            if path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                == Some(skip)
            {
                continue;
            }
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let session_id = read_prefix_utf8(&path, canon_home, 4096)
            .ok()
            .and_then(|prefix| parser.extract_session_id_from_prefix(&prefix))
            .filter(|id| is_safe_session_id(id))
            .unwrap_or_else(|| stem.to_owned());
        if !is_safe_session_id(&session_id) {
            continue;
        }
        let Ok(file) = open_safe_regular_file(&path, canon_home) else {
            continue;
        };
        let (modified_ms, size_bytes) = file_meta_of(&file);
        let (subagent_files, subagents_capped) = match subagents_subdir {
            Some(sub_rel) => {
                discover_nested_subagents(&path, canon_home, parser, sub_rel, subagent_stem_prefix)
            }
            None => (Vec::new(), false),
        };
        out.push(DiscoveredSession {
            session_id,
            format_id: parser.format_id(),
            path,
            modified_ms,
            size_bytes,
            subagent_files,
            subagents_capped,
        });
    }
}

fn discover_nested_subagents(
    session_file: &Path,
    canon_home: &Path,
    parser: &dyn TranscriptParser,
    subagents_subdir: &str,
    subagent_stem_prefix: Option<&str>,
) -> (Vec<SubagentFile>, bool) {
    let Some(stem) = session_file.file_stem().and_then(|s| s.to_str()) else {
        return (Vec::new(), false);
    };
    if !is_safe_session_id(stem) {
        return (Vec::new(), false);
    }
    let Some(parent) = session_file.parent() else {
        return (Vec::new(), false);
    };
    let sub_dir = parent.join(stem).join(subagents_subdir);
    let mut files = Vec::new();
    walk_regular_files(&sub_dir, canon_home, 0, &mut files);
    files.sort();
    let mut out = Vec::new();
    let mut capped = false;
    for path in files {
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(raw_name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let agent_id = subagent_stem_prefix
            .and_then(|prefix| raw_name.strip_prefix(prefix))
            .unwrap_or(raw_name)
            .to_owned();
        if !is_safe_session_id(&agent_id) {
            continue;
        }
        if out.len() >= MAX_SUBAGENTS_PER_TRANSCRIPT {
            capped = true;
            break;
        }
        let meta_path = path.with_extension("meta.json");
        let meta_raw = read_prefix_utf8(&meta_path, canon_home, MAX_META_READ_BYTES).ok();
        let prefix = read_prefix_utf8(&path, canon_home, 8192).ok();
        let (label, parent_tool_use_id) =
            parser.parse_subagent_meta(meta_raw.as_deref(), prefix.as_deref());
        out.push(SubagentFile {
            agent_id,
            label,
            parent_tool_use_id,
            path,
        });
    }
    out.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    (out, capped)
}

fn discover_conversation_dirs(
    root: &Path,
    transcript_rel_path: &str,
    canon_home: &Path,
    parser: &dyn TranscriptParser,
    out: &mut Vec<DiscoveredSession>,
) {
    if !dir_has_no_symlinks_beneath(root, canon_home) {
        return;
    }
    let Ok(rd) = fs::read_dir(root) else {
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
        let transcript = root.join(&conv_id).join(transcript_rel_path);
        let Ok(file) = open_safe_regular_file(&transcript, canon_home) else {
            continue;
        };
        let (modified_ms, size_bytes) = file_meta_of(&file);
        let (subagent_files, subagents_capped) = discover_sibling_subagent_files(
            &transcript,
            root,
            transcript_rel_path,
            canon_home,
            parser,
            &conv_id,
        );
        out.push(DiscoveredSession {
            session_id: conv_id,
            format_id: parser.format_id(),
            path: transcript,
            modified_ms,
            size_bytes,
            subagent_files,
            subagents_capped,
        });
    }
}

fn discover_sibling_subagent_files(
    transcript: &Path,
    brain_root: &Path,
    transcript_rel_path: &str,
    canon_home: &Path,
    parser: &dyn TranscriptParser,
    parent_id: &str,
) -> (Vec<SubagentFile>, bool) {
    let Ok(prefix) = read_prefix_utf8(transcript, canon_home, MAX_META_READ_BYTES) else {
        return (Vec::new(), false);
    };
    let mut out = Vec::new();
    let mut capped = false;
    for r in parser.extract_subagent_refs(&prefix, parent_id) {
        if !is_safe_session_id(&r.agent_id) {
            continue;
        }
        if out.len() >= MAX_SUBAGENTS_PER_TRANSCRIPT {
            capped = true;
            break;
        }
        let sub_path = brain_root.join(&r.agent_id).join(transcript_rel_path);
        if is_safe_regular_file(&sub_path, canon_home) {
            out.push(SubagentFile {
                agent_id: r.agent_id,
                label: r.label,
                parent_tool_use_id: r.parent_tool_use_id,
                path: sub_path,
            });
        }
    }
    (out, capped)
}

fn dir_has_no_symlinks_beneath(dir: &Path, canon_home: &Path) -> bool {
    let Ok(rel) = dir.strip_prefix(canon_home) else {
        return false;
    };
    let Ok(home_meta) = fs::symlink_metadata(canon_home) else {
        return false;
    };
    if !home_meta.is_dir() || home_meta.file_type().is_symlink() {
        return false;
    }
    let mut cur = canon_home.to_path_buf();
    for component in rel.components() {
        let Component::Normal(seg) = component else {
            return false;
        };
        cur.push(seg);
        let Ok(meta) = fs::symlink_metadata(&cur) else {
            return false;
        };
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return false;
        }
    }
    true
}

pub(super) fn walk_regular_files(
    dir: &Path,
    canon_home: &Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
) {
    if depth > MAX_WALK_DEPTH || out.len() >= 512 {
        return;
    }
    if depth == 0 {
        if !dir_has_no_symlinks_beneath(dir, canon_home) {
            return;
        }
    } else if dir.strip_prefix(canon_home).is_err()
        || !fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
    {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
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
        let path = dir.join(entry.file_name());
        if ft.is_dir() {
            walk_regular_files(&path, canon_home, depth + 1, out);
        } else if ft.is_file() && is_safe_regular_file(&path, canon_home) {
            out.push(path);
        }
    }
}

pub(super) fn is_safe_regular_file(path: &Path, canon_home: &Path) -> bool {
    open_safe_regular_file(path, canon_home).is_ok()
}

/// Opens a regular file strictly beneath `canon_home` via component-wise
/// `openat(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)` and verifies `S_ISREG` on the
/// opened handle (`org_platform::fs::open_regular_beneath`), closing parent-directory and leaf
/// symlink/FIFO TOCTOU races.
pub(super) fn open_safe_regular_file(path: &Path, canon_home: &Path) -> io::Result<File> {
    let rel = path.strip_prefix(canon_home).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "transcript path escapes agent private home",
        )
    })?;
    org_platform::fs::open_regular_beneath(canon_home, rel)
}

pub(super) fn read_bounded_utf8_with_limit(
    path: &Path,
    canon_home: &Path,
    max_bytes: u64,
) -> io::Result<(String, bool, usize)> {
    let file = open_safe_regular_file(path, canon_home)?;
    let cap = max_bytes.min(MAX_TRANSCRIPT_READ_BYTES);
    let mut buf = Vec::new();
    file.take(cap.saturating_add(1)).read_to_end(&mut buf)?;
    let truncated = buf.len() as u64 > cap;
    let consumed = (buf.len() as u64).min(cap) as usize;
    if truncated {
        buf.truncate(cap as usize);
        if let Some(last_nl) = buf.iter().rposition(|&b| b == b'\n') {
            buf.truncate(last_nl);
        }
    }
    Ok((
        String::from_utf8_lossy(&buf).into_owned(),
        truncated,
        consumed,
    ))
}

pub(super) fn read_prefix_utf8(
    path: &Path,
    canon_home: &Path,
    max_bytes: u64,
) -> io::Result<String> {
    let file = open_safe_regular_file(path, canon_home)?;
    let mut buf = Vec::new();
    file.take(max_bytes).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn file_meta_of(file: &File) -> (u64, u64) {
    let Ok(meta) = file.metadata() else {
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
