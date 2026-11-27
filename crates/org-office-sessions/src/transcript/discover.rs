//! Bounded, symlink-safe discovery of runtime session and sub-agent transcript files.
//!
//! Filesystem traversal, symlink refusal, and `O_NOFOLLOW | O_NONBLOCK` + `(dev, ino)` opens live
//! exclusively here in the office; runtime adapters supply only pure layout descriptors and
//! metadata extractors via [`TranscriptParser`].

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use org_agent_runtime::transcript::{TranscriptDiscoveryLayout, TranscriptParser};

use crate::runtime_registry::RuntimeRegistry;

use super::{
    is_safe_session_id, DiscoveredSession, SubagentFile, MAX_META_READ_BYTES,
    MAX_TRANSCRIPT_READ_BYTES, MAX_WALK_DEPTH,
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
        let (modified_ms, size_bytes) = file_meta(&path);
        let subagent_files = match subagents_subdir {
            Some(sub_rel) => {
                discover_nested_subagents(&path, canon_home, parser, sub_rel, subagent_stem_prefix)
            }
            None => Vec::new(),
        };
        out.push(DiscoveredSession {
            session_id,
            format_id: parser.format_id(),
            path,
            modified_ms,
            size_bytes,
            subagent_files,
        });
    }
}

fn discover_nested_subagents(
    session_file: &Path,
    canon_home: &Path,
    parser: &dyn TranscriptParser,
    subagents_subdir: &str,
    subagent_stem_prefix: Option<&str>,
) -> Vec<SubagentFile> {
    let Some(stem) = session_file.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let Some(parent) = session_file.parent() else {
        return Vec::new();
    };
    let sub_dir = parent.join(stem).join(subagents_subdir);
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
        let agent_id = subagent_stem_prefix
            .and_then(|prefix| raw_name.strip_prefix(prefix))
            .unwrap_or(raw_name)
            .to_owned();
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
    out
}

fn discover_conversation_dirs(
    root: &Path,
    transcript_rel_path: &str,
    canon_home: &Path,
    parser: &dyn TranscriptParser,
    out: &mut Vec<DiscoveredSession>,
) {
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
        let transcript = entry.path().join(transcript_rel_path);
        if is_safe_regular_file(&transcript, canon_home) {
            let (modified_ms, size_bytes) = file_meta(&transcript);
            let subagent_files = discover_sibling_subagent_files(
                &transcript,
                &canon_root,
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
            });
        }
    }
}

fn discover_sibling_subagent_files(
    transcript: &Path,
    brain_root: &Path,
    transcript_rel_path: &str,
    canon_home: &Path,
    parser: &dyn TranscriptParser,
    parent_id: &str,
) -> Vec<SubagentFile> {
    let Ok(prefix) = read_prefix_utf8(transcript, canon_home, MAX_META_READ_BYTES) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for r in parser.extract_subagent_refs(&prefix, parent_id) {
        if !is_safe_session_id(&r.agent_id) {
            continue;
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
    out
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

pub(super) fn is_safe_regular_file(path: &Path, canon_home: &Path) -> bool {
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
pub(super) fn open_safe_regular_file(path: &Path, canon_home: &Path) -> io::Result<File> {
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

pub(super) fn read_bounded_utf8(path: &Path, canon_home: &Path) -> io::Result<(String, bool)> {
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

pub(super) fn file_meta(path: &Path) -> (u64, u64) {
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
