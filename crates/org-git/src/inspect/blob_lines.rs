//! Bounded line slices of one regular file at one commit of a bare cache (`TASK-92.1`), with
//! the SHA-256 of the exact anchored lines so a finding's anchor stays tamper-evident across
//! rounds. Binary files, symbolic links, submodules, directories and missing paths are
//! refused; nothing outside the commit's tree is ever read.
//!
//! The snippet hash covers each line without its terminator (`\n` or `\r\n`), followed by
//! one `\n`, in raw bytes; [`anchor_snippet_sha256`] recomputes it for any lines.

use super::cache_reader::{require_success, ReadOnlyCache};
use super::repo_path::validate_repo_path;
use super::sha256::Sha256;
use crate::worktree::validate_commit_oid;
use crate::GitError;

/// Most lines one slice may ask for.
pub const MAX_BLOB_LINES: u32 = 500;
/// Largest slice text returned (256 KiB, one `\n` counted per line).
pub const MAX_BLOB_SLICE_BYTES: usize = 256 * 1024;
/// Largest file read (8 MiB); larger files are refused.
pub const MAX_BLOB_BYTES: u64 = 8 * 1024 * 1024;

/// Largest `ls-tree` answer read for one path.
const MAX_TREE_ENTRY_BYTES: usize = 64 * 1024;

/// One slice of one file at one commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobLinesRequest<'a> {
    pub commit_oid: &'a str,
    /// Normalized repository-relative path of a regular file.
    pub path: &'a str,
    /// First line, counted from 1.
    pub start_line: u32,
    /// How many lines, 1 to [`MAX_BLOB_LINES`]; a slice past the end stops at the last line.
    pub line_count: u32,
}

/// A slice of a file's lines and its anchor fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobLines {
    /// The file's blob OID at the commit.
    pub blob_oid: String,
    pub start_line: u32,
    /// Last line returned (inclusive).
    pub end_line: u32,
    /// Lines in the whole file.
    pub total_lines: u32,
    /// The lines without terminators (non-UTF-8 bytes shown as U+FFFD).
    pub lines: Vec<String>,
    /// Fewer lines than asked for fit in [`MAX_BLOB_SLICE_BYTES`].
    pub truncated: bool,
    /// Lowercase hex SHA-256 of the returned lines ([`anchor_snippet_sha256`]).
    pub anchor_snippet_sha256: String,
}

/// Reads `req.line_count` lines from `req.start_line` of the file at `req.path` in commit
/// `req.commit_oid`.
pub fn read_blob_lines(
    cache: &ReadOnlyCache<'_>,
    req: &BlobLinesRequest<'_>,
) -> Result<BlobLines, GitError> {
    validate_commit_oid(req.commit_oid)?;
    let path = validate_repo_path(req.path)?;
    if req.start_line == 0 {
        return Err(GitError::InvalidInput("start_line counts from 1".into()));
    }
    if req.line_count == 0 || req.line_count > MAX_BLOB_LINES {
        return Err(GitError::InvalidInput(format!(
            "line_count must be 1 to {MAX_BLOB_LINES}"
        )));
    }
    cache.locked(|| {
        cache.verify_commit(req.commit_oid)?;
        let blob_oid = regular_file_blob(cache, req.commit_oid, path)?;
        // `MAX_BLOB_BYTES` (8 MiB) always fits `usize`.
        let max = usize::try_from(MAX_BLOB_BYTES).unwrap_or(usize::MAX);
        let out = cache.git(&["cat-file", "blob", &blob_oid], max)?;
        if out.stdout_truncated {
            return Err(too_large(path));
        }
        require_success(&out)?;
        if out.stdout.contains(&0) {
            return Err(GitError::InvalidInput(format!("`{path}` is a binary file")));
        }
        let slice = slice_lines(&out.stdout, req.start_line, req.line_count)?;
        Ok(BlobLines {
            blob_oid,
            start_line: req.start_line,
            end_line: slice.end_line,
            total_lines: slice.total_lines,
            lines: slice.lines,
            truncated: slice.truncated,
            anchor_snippet_sha256: slice.sha256,
        })
    })
}

/// Lowercase hex SHA-256 of `lines`: each line without a trailing `\n` or `\r\n`, followed by
/// one `\n`.
pub fn anchor_snippet_sha256<L: AsRef<[u8]>>(lines: &[L]) -> String {
    let mut hasher = Sha256::new();
    for line in lines {
        hasher.update(strip_terminator(line.as_ref()));
        hasher.update(b"\n");
    }
    hasher.finish_hex()
}

/// The blob OID of the regular file at `path` in `commit_oid`, refusing anything else.
fn regular_file_blob(
    cache: &ReadOnlyCache<'_>,
    commit_oid: &str,
    path: &str,
) -> Result<String, GitError> {
    let out = cache.git(
        &["ls-tree", "-z", "-l", commit_oid, "--", path],
        MAX_TREE_ENTRY_BYTES,
    )?;
    if out.stdout_truncated {
        return Err(GitError::InvalidInput(
            "`git ls-tree` answer is too long".into(),
        ));
    }
    require_success(&out)?;
    for record in out.stdout.split(|b| *b == 0) {
        let Some(tab) = record.iter().position(|b| *b == b'\t') else {
            continue;
        };
        if &record[tab + 1..] != path.as_bytes() {
            continue;
        }
        let meta = String::from_utf8_lossy(&record[..tab]);
        let fields: Vec<&str> = meta.split_whitespace().collect();
        return match fields.as_slice() {
            ["100644" | "100755", "blob", oid, size] => {
                validate_commit_oid(oid)?;
                let size: u64 = size
                    .parse()
                    .map_err(|_| GitError::InvalidInput("malformed `git ls-tree` size".into()))?;
                if size > MAX_BLOB_BYTES {
                    return Err(too_large(path));
                }
                Ok((*oid).to_owned())
            }
            ["120000", ..] => Err(not_a_file(path, "a symbolic link")),
            ["160000", ..] => Err(not_a_file(path, "a submodule")),
            [_, "tree", ..] => Err(not_a_file(path, "a directory")),
            _ => Err(not_a_file(path, "not a regular file")),
        };
    }
    Err(GitError::InvalidInput(format!(
        "`{path}` does not exist at commit {commit_oid}"
    )))
}

fn not_a_file(path: &str, what: &str) -> GitError {
    GitError::InvalidInput(format!("`{path}` is {what}, not a regular file"))
}

fn too_large(path: &str) -> GitError {
    GitError::InvalidInput(format!("`{path}` is larger than {MAX_BLOB_BYTES} bytes"))
}

/// The lines of one slice.
pub(crate) struct LineSlice {
    pub(crate) end_line: u32,
    pub(crate) total_lines: u32,
    pub(crate) lines: Vec<String>,
    pub(crate) truncated: bool,
    pub(crate) sha256: String,
}

/// Slices `count` lines from 1-based `start` of `blob`, stopping at the file's end or before
/// a line that would take the slice past [`MAX_BLOB_SLICE_BYTES`].
pub(crate) fn slice_lines(blob: &[u8], start: u32, count: u32) -> Result<LineSlice, GitError> {
    let first = start.saturating_sub(1);
    let last = first.saturating_add(count);
    let mut total_lines: u32 = 0;
    let mut raw: Vec<&[u8]> = Vec::new();
    let mut lines = Vec::new();
    let mut used = 0usize;
    let mut truncated = false;
    for segment in blob.split_inclusive(|b| *b == b'\n') {
        let index = total_lines;
        total_lines = total_lines.saturating_add(1);
        if index < first || index >= last || truncated {
            continue;
        }
        let line = strip_terminator(segment);
        let text = String::from_utf8_lossy(line);
        let cost = text.len() + 1;
        if used + cost > MAX_BLOB_SLICE_BYTES {
            if lines.is_empty() {
                return Err(GitError::InvalidInput(format!(
                    "line {start} is longer than {MAX_BLOB_SLICE_BYTES} bytes"
                )));
            }
            truncated = true;
            continue;
        }
        used += cost;
        raw.push(line);
        lines.push(text.into_owned());
    }
    if start > total_lines {
        return Err(GitError::InvalidInput(format!(
            "start_line {start} is past the end of the file ({total_lines} lines)"
        )));
    }
    let returned = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    Ok(LineSlice {
        end_line: start.saturating_add(returned).saturating_sub(1),
        total_lines,
        truncated,
        sha256: anchor_snippet_sha256(&raw),
        lines,
    })
}

/// `line` without one trailing `\n` (and the `\r` before it).
fn strip_terminator(line: &[u8]) -> &[u8] {
    match line.strip_suffix(b"\n") {
        Some(line) => line.strip_suffix(b"\r").unwrap_or(line),
        None => line,
    }
}
