//! Read-only inspection of office bare caches and task worktrees (`TASK-92.1`, `doc-35`): a
//! worktree's exact head commit and clean or dirty state, bounded diffs between two commits
//! (a first round against its base, or one round against the previous round's head), and
//! bounded line slices of one file at one commit with a tamper-evident snippet hash.
//!
//! Every helper goes through [`ReadOnlyCache`]: the cache's [`crate::RepoLocks`] lock, the
//! canonical cache integrity check, then Git with no transport and no credential, no optional
//! locks, literal pathspecs, no hooks, no fsmonitor, no external diff and no textconv, with
//! capped output and a timeout. Commit OIDs pass [`crate::validate_commit_oid`] and must name
//! commits in the cache; paths pass [`validate_repo_path`]. Knows only paths and commit OIDs.

mod blob_lines;
mod cache_reader;
mod diff;
mod repo_path;
mod sha256;
#[cfg(test)]
mod tests;
mod worktree_head;

pub use blob_lines::{
    anchor_snippet_sha256, read_blob_lines, BlobLines, BlobLinesRequest, MAX_BLOB_BYTES,
    MAX_BLOB_LINES, MAX_BLOB_SLICE_BYTES,
};
pub use cache_reader::ReadOnlyCache;
pub use diff::{
    diff_commits, CommitDiff, DiffCommitsRequest, DiffFileStat, DiffFileStatus,
    DEFAULT_DIFF_CONTEXT_LINES, MAX_DIFF_BYTES, MAX_DIFF_CONTEXT_LINES, MAX_DIFF_FILES,
    MAX_DIFF_SUMMARY_BYTES,
};
pub use repo_path::{validate_repo_path, MAX_REPO_PATH_BYTES};
pub use worktree_head::{resolve_worktree_head, WorktreeChanges, WorktreeHead, MAX_STATUS_BYTES};
