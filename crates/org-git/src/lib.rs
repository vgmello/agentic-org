//! Hardened plain-Git engine for Agentic Org (`TASK-51`, `C7`, `doc-35`).
//!
//! Owns host `git` execution with clean environments, strict transport allowlists, canonical
//! bare cache integrity checks, per-repository locking, on-demand origin refresh with a branch
//! change summary, isolated per-task worktrees,
//! non-force commit-OID branch pushes, `merged/`/`archive/` cleanup tags, checkpoint bundles,
//! derived sandbox allow/deny rules (`FC-103`, `FC-140`), and read-only inspection of caches
//! and worktrees: exact worktree heads, bounded commit diffs and line slices (`TASK-92.1`).
//!
//! Knows only paths, URLs, references, and commit OIDs — never projects, tasks, or databases.

#![forbid(unsafe_code)]

mod bounded;
pub mod cache;
pub mod inspect;
pub mod refresh;
pub mod runner;
#[cfg(test)]
mod tests;
pub mod worktree;

// The modes of the folders and lock file the office makes for Git (`create_git_dirs`,
// `open_lock_file`): the one place this crate names them.
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

pub use bounded::{BoundedOutput, OutputLimits, MAX_BOUNDED_STDERR_BYTES};
pub use cache::{
    canonical_cache_config, fetch_cache, init_bare_cache, ls_remote_heads, verify_cache_integrity,
    RepoLocks,
};
pub use inspect::{
    anchor_snippet_sha256, diff_commits, read_blob_lines, resolve_worktree_head,
    validate_repo_path, BlobLines, BlobLinesRequest, CommitDiff, DiffCommitsRequest, DiffFileStat,
    DiffFileStatus, ReadOnlyCache, WorktreeChanges, WorktreeHead, DEFAULT_DIFF_CONTEXT_LINES,
    MAX_BLOB_BYTES, MAX_BLOB_LINES, MAX_BLOB_SLICE_BYTES, MAX_DIFF_BYTES, MAX_DIFF_CONTEXT_LINES,
    MAX_DIFF_FILES, MAX_DIFF_SUMMARY_BYTES, MAX_REPO_PATH_BYTES, MAX_STATUS_BYTES,
};
pub use refresh::{list_origin_branches, refresh_cache, remote_default_branch, CacheRefresh};
pub use runner::{
    canonical_https_repo_scope, validate_local_origin_path, validate_remote_url, AllowedProtocol,
    GitCommandOutput, GitCredential, GitRunner, RemoteSpec, MIN_GIT_VERSION,
};
pub use worktree::{
    check_ref_format, choose_unique_task_branch, cleanup_worktree, create_checkpoint_bundle,
    create_worktree, derive_sandbox_rules, push_task_branch, resolve_remote_branch_commit,
    rollback_created_worktree, sanitize_worktree_links, slugify_branch_suffix, validate_commit_oid,
    CleanupOutcome, PushResult, SandboxRules, WorktreeCreated,
};

/// Errors produced by `org-git` operations.
#[derive(Debug)]
pub enum GitError {
    UnsupportedGit(String),
    InvalidInput(String),
    SecurityViolation(String),
    RemoteUnavailable(String),
    Conflict(String),
    CommandFailed {
        status: i32,
        stderr: String,
    },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedGit(m) => write!(f, "git binary unavailable or unsupported: {m}"),
            Self::InvalidInput(m) => write!(f, "invalid git input: {m}"),
            Self::SecurityViolation(m) => write!(f, "git security violation: {m}"),
            Self::RemoteUnavailable(m) => write!(f, "git remote unreachable or failed: {m}"),
            Self::Conflict(m) => write!(f, "git conflict: {m}"),
            Self::CommandFailed { status, stderr } => {
                write!(f, "git command failed ({status}): {stderr}")
            }
            Self::Io { path, source } => {
                write!(f, "git filesystem error at {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl GitError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Creates `path` and every missing folder above it for Git. Owner-only (`0700`) whatever
/// the process's file-creation mask, so a cache, worktree or checkpoint folder is never
/// group- or world-accessible even under a `0002` mask (TASK-155); with `shared_group`
/// (`core.sharedRepository = group`), with the mask's modes, which the group must traverse.
/// Folders that exist are left as they are.
pub(crate) fn create_git_dirs(path: &Path, shared_group: bool) -> Result<(), GitError> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    if !shared_group {
        builder.mode(0o700);
    }
    builder.create(path).map_err(|e| GitError::io(path, e))
}

/// Opens the per-cache lock file at `path` for writing, creating it owner-only (`0600`)
/// whatever the process's file-creation mask when it is missing (TASK-155). Its contents
/// are never read or changed: it exists to be locked.
pub(crate) fn open_lock_file(path: &Path) -> Result<std::fs::File, GitError> {
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| GitError::io(path, e))
}
