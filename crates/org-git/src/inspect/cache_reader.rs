//! The read-only view of one office bare cache that every inspection helper runs through: the
//! repository lock, the canonical integrity check, local-only bounded Git that takes no
//! optional locks and reads pathspecs literally, and strict commit verification.

use std::path::Path;
use std::time::Duration;

use crate::bounded::{BoundedOutput, OutputLimits};
use crate::cache::{verify_cache_integrity, RepoLocks};
use crate::runner::GitRunner;
use crate::worktree::validate_commit_oid;
use crate::GitError;

/// Wall-clock limit of one inspection command.
const INSPECT_TIMEOUT: Duration = Duration::from_secs(60);

/// Global options every inspection command starts with: never take optional locks (so no
/// index refresh is ever written) and read every pathspec literally (no `:(magic)`, no globs).
const READ_ONLY_GLOBALS: [&str; 2] = ["--no-optional-locks", "--literal-pathspecs"];

/// Largest `rev-parse` answer read back (one OID and a newline).
const MAX_OID_LINE_BYTES: usize = 128;

/// One office bare cache opened for read-only inspection.
///
/// Its helpers take the cache's [`RepoLocks`] lock themselves: never call them while this
/// thread already holds the lock for the same cache.
#[derive(Clone, Copy)]
pub struct ReadOnlyCache<'a> {
    runner: &'a GitRunner,
    locks: &'a RepoLocks,
    cache_dir: &'a Path,
    shared_group: bool,
}

impl<'a> ReadOnlyCache<'a> {
    /// `shared_group` is the cache's `core.sharedRepository = group` mode, which selects the
    /// canonical config the integrity check expects.
    pub fn new(
        runner: &'a GitRunner,
        locks: &'a RepoLocks,
        cache_dir: &'a Path,
        shared_group: bool,
    ) -> Self {
        Self {
            runner,
            locks,
            cache_dir,
            shared_group,
        }
    }

    pub fn cache_dir(&self) -> &'a Path {
        self.cache_dir
    }

    /// Runs `f` under the cache's repository lock, after the canonical integrity check.
    pub(crate) fn locked<T>(&self, f: impl FnOnce() -> Result<T, GitError>) -> Result<T, GitError> {
        self.locks.with_lock(self.cache_dir, || {
            verify_cache_integrity(self.cache_dir, self.shared_group)?;
            f()
        })
    }

    /// Runs one bounded, local-only, read-only Git command against the bare cache.
    pub(crate) fn git(&self, args: &[&str], max_stdout: usize) -> Result<BoundedOutput, GitError> {
        self.git_in(Some(self.cache_dir), None, args, max_stdout)
    }

    /// Runs one bounded, local-only, read-only Git command with an explicit `GIT_DIR` and
    /// optional work tree (a task worktree's administrative directory and checkout).
    pub(crate) fn git_in(
        &self,
        git_dir: Option<&Path>,
        work_tree: Option<&Path>,
        args: &[&str],
        max_stdout: usize,
    ) -> Result<BoundedOutput, GitError> {
        let mut full = Vec::with_capacity(READ_ONLY_GLOBALS.len() + args.len());
        full.extend_from_slice(&READ_ONLY_GLOBALS);
        full.extend_from_slice(args);
        let limits = OutputLimits {
            max_stdout,
            timeout: INSPECT_TIMEOUT,
        };
        self.runner
            .run_local_bounded(git_dir, work_tree, &full, limits)
    }

    /// Checks that `oid` is a full commit OID naming a commit object in the cache (never a
    /// tag, tree or blob, and never a revision expression). Call it under [`Self::locked`].
    pub(crate) fn verify_commit(&self, oid: &str) -> Result<(), GitError> {
        validate_commit_oid(oid)?;
        let spec = format!("{oid}^{{commit}}");
        let out = self.git(
            &["rev-parse", "--verify", "--quiet", &spec],
            MAX_OID_LINE_BYTES,
        )?;
        let resolved = String::from_utf8_lossy(&out.stdout);
        if out.status != 0 || out.stdout_truncated || resolved.trim() != oid {
            return Err(GitError::InvalidInput(format!(
                "`{oid}` is not a commit in the repository cache"
            )));
        }
        Ok(())
    }
}

/// Maps a failed, complete bounded command to [`GitError::CommandFailed`].
pub(crate) fn require_success(out: &BoundedOutput) -> Result<(), GitError> {
    if out.status == 0 {
        return Ok(());
    }
    Err(GitError::CommandFailed {
        status: out.status,
        stderr: out.stderr.trim().to_owned(),
    })
}
