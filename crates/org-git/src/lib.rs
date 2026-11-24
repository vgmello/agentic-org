//! Hardened plain-Git engine for Agentic Org (`TASK-51`, `C7`, `doc-35`).
//!
//! Owns host `git` execution with clean environments, strict transport allowlists, canonical
//! bare cache integrity checks, per-repository locking, isolated per-task worktrees,
//! non-force commit-OID branch pushes, `merged/`/`archive/` cleanup tags, checkpoint bundles,
//! and derived sandbox allow/deny rules (`FC-103`, `FC-140`).
//!
//! Knows only paths, URLs, references, and commit OIDs — never projects, tasks, or databases.

#![deny(unsafe_code)]

pub mod cache;
pub mod runner;
#[cfg(test)]
mod tests;
pub mod worktree;

use std::path::PathBuf;

pub use cache::{
    canonical_cache_config, fetch_cache, init_bare_cache, ls_remote_heads, verify_cache_integrity,
    RepoLocks,
};
pub use runner::{
    validate_local_origin_path, validate_remote_url, AllowedProtocol, GitCommandOutput,
    GitCredential, GitRunner, RemoteSpec, MIN_GIT_VERSION,
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
