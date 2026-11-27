//! Bounded diffs between two commits of a bare cache (`TASK-92.1`): a per-file summary
//! (status, added and deleted line counts, binary flag) and a unified diff capped at
//! [`MAX_DIFF_BYTES`], cut on a character boundary. A first round diffs its base against its
//! head; a later round diffs the previous round's head against its own.
//!
//! Git compares the two trees directly (`diff-tree`): no rename detection, no external diff,
//! no textconv, no color, and binary files appear as `Binary files ... differ` only.

use std::collections::BTreeMap;

use super::cache_reader::{require_success, ReadOnlyCache};
use super::repo_path::validate_repo_path;
use crate::bounded::{decode_bounded, BoundedOutput};
use crate::worktree::validate_commit_oid;
use crate::GitError;

/// Largest unified diff text returned (256 KiB); longer diffs are cut and marked `truncated`.
pub const MAX_DIFF_BYTES: usize = 256 * 1024;
/// Most context lines around each change.
pub const MAX_DIFF_CONTEXT_LINES: u32 = 20;
/// Context lines when the caller has no preference (Git's default).
pub const DEFAULT_DIFF_CONTEXT_LINES: u32 = 3;
/// Most per-file summary entries returned; the rest are counted in `total_files` only.
pub const MAX_DIFF_FILES: usize = 1000;
/// Largest per-file summary listing read; a diff touching more files is refused.
pub const MAX_DIFF_SUMMARY_BYTES: usize = 8 * 1024 * 1024;

/// The options common to both diff listings.
const DIFF_TREE_ARGS: [&str; 7] = [
    "diff-tree",
    "-r",
    "--no-renames",
    "--no-ext-diff",
    "--no-textconv",
    "--no-color",
    "--submodule=short",
];

/// One diff between two commits of the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffCommitsRequest<'a> {
    /// The older side: the review's base, or the previous round's head.
    pub base_oid: &'a str,
    /// The newer side: the round's head.
    pub head_oid: &'a str,
    /// Only this file or directory (a normalized repository-relative path).
    pub path_filter: Option<&'a str>,
    /// Only the per-file summary, no unified diff.
    pub stat_only: bool,
    /// Context lines around each change, at most [`MAX_DIFF_CONTEXT_LINES`].
    pub context_lines: u32,
}

impl<'a> DiffCommitsRequest<'a> {
    /// A full diff of `base_oid..head_oid` with default context.
    pub fn new(base_oid: &'a str, head_oid: &'a str) -> Self {
        Self {
            base_oid,
            head_oid,
            path_filter: None,
            stat_only: false,
            context_lines: DEFAULT_DIFF_CONTEXT_LINES,
        }
    }
}

/// How one file changed between the two commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffFileStatus {
    Added,
    Modified,
    Deleted,
    /// The entry changed kind (for example a regular file became a symbolic link).
    TypeChanged,
}

impl DiffFileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::TypeChanged => "type_changed",
        }
    }
}

/// One changed file's summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFileStat {
    /// Repository-relative path (non-UTF-8 bytes shown as U+FFFD).
    pub path: String,
    pub status: DiffFileStatus,
    /// Lines added; `0` for a binary file.
    pub added: u64,
    /// Lines deleted; `0` for a binary file.
    pub deleted: u64,
    /// Git treats the file as binary: no line counts and no text in the unified diff.
    pub binary: bool,
}

/// A bounded diff between two commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDiff {
    pub base_oid: String,
    pub head_oid: String,
    /// Changed files in path order, at most [`MAX_DIFF_FILES`].
    pub files: Vec<DiffFileStat>,
    /// Every changed file, including those past [`MAX_DIFF_FILES`].
    pub total_files: u64,
    /// `files` stops short of `total_files`.
    pub files_truncated: bool,
    /// Unified diff text, at most [`MAX_DIFF_BYTES`]; empty for a summary-only request.
    pub unified_diff: String,
    /// `unified_diff` was cut at [`MAX_DIFF_BYTES`].
    pub truncated: bool,
}

/// Diffs `req.base_oid..req.head_oid` in the cache, refusing anything but two full commit
/// OIDs of the same hash that name commits, a validated path filter and bounded context.
pub fn diff_commits(
    cache: &ReadOnlyCache<'_>,
    req: &DiffCommitsRequest<'_>,
) -> Result<CommitDiff, GitError> {
    validate_commit_oid(req.base_oid)?;
    validate_commit_oid(req.head_oid)?;
    if req.base_oid.len() != req.head_oid.len() {
        return Err(GitError::InvalidInput(
            "base and head commit OIDs must use the same hash".into(),
        ));
    }
    let path = req.path_filter.map(validate_repo_path).transpose()?;
    if req.context_lines > MAX_DIFF_CONTEXT_LINES {
        return Err(GitError::InvalidInput(format!(
            "diff context must be at most {MAX_DIFF_CONTEXT_LINES} lines"
        )));
    }
    cache.locked(|| {
        cache.verify_commit(req.base_oid)?;
        cache.verify_commit(req.head_oid)?;
        let summary = run_diff(
            cache,
            req,
            path,
            &["-z", "--raw", "--numstat"],
            MAX_DIFF_SUMMARY_BYTES,
        )?;
        if summary.stdout_truncated {
            return Err(GitError::InvalidInput(
                "the diff touches too many files to summarize; narrow it with a path filter".into(),
            ));
        }
        let mut files = parse_summary(&summary.stdout)?;
        let total_files = files.len() as u64;
        let files_truncated = files.len() > MAX_DIFF_FILES;
        files.truncate(MAX_DIFF_FILES);
        let (unified_diff, truncated) = if req.stat_only {
            (String::new(), false)
        } else {
            let context = format!("-U{}", req.context_lines);
            let patch = run_diff(cache, req, path, &["-p", &context], MAX_DIFF_BYTES)?;
            decode_bounded(&patch.stdout, MAX_DIFF_BYTES, patch.stdout_truncated)
        };
        Ok(CommitDiff {
            base_oid: req.base_oid.to_owned(),
            head_oid: req.head_oid.to_owned(),
            files,
            total_files,
            files_truncated,
            unified_diff,
            truncated,
        })
    })
}

/// Runs `git diff-tree` over the two commits with `format`, the path filter after `--`.
fn run_diff(
    cache: &ReadOnlyCache<'_>,
    req: &DiffCommitsRequest<'_>,
    path: Option<&str>,
    format: &[&str],
    max_stdout: usize,
) -> Result<BoundedOutput, GitError> {
    let mut args = Vec::with_capacity(DIFF_TREE_ARGS.len() + format.len() + 4);
    args.extend_from_slice(&DIFF_TREE_ARGS);
    args.extend_from_slice(format);
    args.extend([req.base_oid, req.head_oid, "--"]);
    args.extend(path);
    let out = cache.git(&args, max_stdout)?;
    if !out.stdout_truncated {
        require_success(&out)?;
    }
    Ok(out)
}

/// Parses `diff-tree -z --raw --numstat`: every raw record (`:<modes> <oids> <status>`, then
/// the path) comes first, then one numstat record (`<added>\t<deleted>\t<path>`) per file.
pub(crate) fn parse_summary(stdout: &[u8]) -> Result<Vec<DiffFileStat>, GitError> {
    let malformed = || GitError::InvalidInput("malformed `git diff-tree` summary".into());
    let mut files = Vec::new();
    let mut by_path: BTreeMap<&[u8], usize> = BTreeMap::new();
    let mut records = stdout.split(|b| *b == 0);
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if let Some(raw) = record.strip_prefix(b":") {
            let status = raw.rsplit(|b| *b == b' ').next().ok_or_else(malformed)?;
            let status = match status {
                b"A" => DiffFileStatus::Added,
                b"M" => DiffFileStatus::Modified,
                b"D" => DiffFileStatus::Deleted,
                b"T" => DiffFileStatus::TypeChanged,
                _ => return Err(malformed()),
            };
            let path = records
                .next()
                .filter(|p| !p.is_empty())
                .ok_or_else(malformed)?;
            by_path.insert(path, files.len());
            files.push(DiffFileStat {
                path: String::from_utf8_lossy(path).into_owned(),
                status,
                added: 0,
                deleted: 0,
                binary: false,
            });
            continue;
        }
        let mut fields = record.splitn(3, |b| *b == b'\t');
        let (Some(added), Some(deleted), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return Err(malformed());
        };
        let file = by_path
            .get(path)
            .and_then(|index| files.get_mut(*index))
            .ok_or_else(malformed)?;
        if added == b"-" && deleted == b"-" {
            file.binary = true;
        } else {
            file.added = parse_count(added).ok_or_else(malformed)?;
            file.deleted = parse_count(deleted).ok_or_else(malformed)?;
        }
    }
    Ok(files)
}

fn parse_count(raw: &[u8]) -> Option<u64> {
    std::str::from_utf8(raw).ok()?.parse().ok()
}
