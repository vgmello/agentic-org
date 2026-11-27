//! The exact head commit and clean or dirty state of one task worktree (`TASK-92.1`), read with
//! `git status --porcelain=v2` without optional locks, so the index is never rewritten. A
//! review pins only a clean worktree's head; any staged, unstaged, untracked or unmerged path
//! makes it dirty, and so does a status listing too long to read whole (fail closed).
//!
//! Submodules are read with `--ignore-submodules=dirty`: a moved submodule commit counts as a
//! change, but Git never runs inside a submodule, whose own configuration the worktree's
//! writer controls.

use std::path::Path;

use super::cache_reader::{require_success, ReadOnlyCache};
use crate::worktree::{sanitize_worktree_links, validate_commit_oid};
use crate::GitError;

/// Largest `git status` listing read; a longer one marks the worktree dirty.
pub const MAX_STATUS_BYTES: usize = 1024 * 1024;

const STATUS_ARGS: [&str; 7] = [
    "status",
    "--porcelain=v2",
    "--branch",
    "-z",
    "--untracked-files=all",
    "--ignore-submodules=dirty",
    "--no-renames",
];

/// How a worktree differs from its head commit, counted by path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorktreeChanges {
    /// Paths whose index entry differs from the head commit (staged).
    pub staged: u64,
    /// Tracked paths whose working file differs from the index (modified, deleted, type changed).
    pub unstaged: u64,
    /// Paths Git neither tracks nor ignores.
    pub untracked: u64,
    /// Paths with an unresolved merge conflict.
    pub unmerged: u64,
    /// The listing exceeded [`MAX_STATUS_BYTES`]: the counts cover only what was read.
    pub truncated: bool,
}

impl WorktreeChanges {
    /// No change of any kind, and the whole listing was read.
    pub fn is_clean(&self) -> bool {
        *self == Self::default()
    }
}

/// A worktree's exact head commit and whether it holds uncommitted changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeHead {
    /// Full lowercase hex commit OID of `HEAD`.
    pub head_oid: String,
    /// Checked-out branch name (`task/...`); `None` when `HEAD` is detached.
    pub branch: Option<String>,
    /// Anything staged, unstaged, untracked or unmerged (or a listing too long to read).
    pub dirty: bool,
    pub changes: WorktreeChanges,
}

/// Resolves the head commit and clean or dirty state of the task worktree at `worktree_dir`,
/// whose administrative directory is `<cache>/worktrees/<admin_dir_name>`. Rewrites the
/// worktree's Git links to their canonical values first (`doc-35` checks before any command).
pub fn resolve_worktree_head(
    cache: &ReadOnlyCache<'_>,
    admin_dir_name: &str,
    worktree_dir: &Path,
) -> Result<WorktreeHead, GitError> {
    cache.locked(|| {
        sanitize_worktree_links(cache.cache_dir(), admin_dir_name, worktree_dir)?;
        let admin_dir = cache.cache_dir().join("worktrees").join(admin_dir_name);
        let out = cache.git_in(
            Some(&admin_dir),
            Some(worktree_dir),
            &STATUS_ARGS,
            MAX_STATUS_BYTES,
        )?;
        if !out.stdout_truncated {
            require_success(&out)?;
        }
        parse_status(&out.stdout, out.stdout_truncated)
    })
}

/// Parses `git status --porcelain=v2 --branch -z` output. Only complete NUL-terminated
/// records count; a cut-off last record is dropped, and `truncated` marks the worktree dirty.
pub(crate) fn parse_status(stdout: &[u8], truncated: bool) -> Result<WorktreeHead, GitError> {
    let complete = stdout
        .iter()
        .rposition(|b| *b == 0)
        .map_or(&stdout[..0], |end| &stdout[..end]);
    let mut head_oid = None;
    let mut branch = None;
    let mut changes = WorktreeChanges {
        truncated,
        ..WorktreeChanges::default()
    };
    let mut records = complete.split(|b| *b == 0);
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix(b"# ") {
            let header = String::from_utf8_lossy(header);
            if let Some(oid) = header.strip_prefix("branch.oid ") {
                head_oid = Some(oid.to_owned());
            } else if let Some(name) = header.strip_prefix("branch.head ") {
                branch = (name != "(detached)").then(|| name.to_owned());
            }
            continue;
        }
        match record.first() {
            None | Some(b'!') => {}
            Some(b'?') => changes.untracked += 1,
            Some(b'u') => changes.unmerged += 1,
            Some(kind @ (b'1' | b'2')) => {
                let (staged, unstaged) = entry_xy(record)?;
                changes.staged += u64::from(staged);
                changes.unstaged += u64::from(unstaged);
                if *kind == b'2' {
                    // A rename record is followed by its original path.
                    records.next();
                }
            }
            Some(_) => {
                return Err(GitError::InvalidInput(
                    "unexpected `git status` record".into(),
                ))
            }
        }
    }
    let head_oid = head_oid.ok_or_else(|| {
        GitError::InvalidInput("`git status` did not report the worktree head".into())
    })?;
    if head_oid == "(initial)" {
        return Err(GitError::Conflict("worktree has no commit yet".into()));
    }
    validate_commit_oid(&head_oid)?;
    Ok(WorktreeHead {
        head_oid,
        branch,
        dirty: !changes.is_clean(),
        changes,
    })
}

/// Whether a `1`/`2` record's path is changed in the index (`X`) and in the work tree (`Y`).
fn entry_xy(record: &[u8]) -> Result<(bool, bool), GitError> {
    match record {
        [_, b' ', x, y, b' ', ..] => Ok((*x != b'.', *y != b'.')),
        _ => Err(GitError::InvalidInput(
            "malformed `git status` change record".into(),
        )),
    }
}
