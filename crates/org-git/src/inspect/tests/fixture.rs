//! A bare office cache fetched from a seeded origin, with one task worktree on
//! `task/T-A1-review`, all inside a temporary directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::inspect::ReadOnlyCache;
use crate::{create_worktree, fetch_cache, GitRunner, RemoteSpec, RepoLocks};

pub(super) const BRANCH: &str = "task/T-A1-review";

pub(super) struct Fixture {
    pub(super) tmp: tempfile::TempDir,
    pub(super) runner: GitRunner,
    pub(super) locks: RepoLocks,
    pub(super) cache_dir: PathBuf,
    pub(super) worktree: PathBuf,
    pub(super) admin: String,
    /// The seeded `main` commit the worktree starts from.
    pub(super) base: String,
}

/// Runs the test's own `git` (not the hardened runner) with an isolated configuration.
pub(super) fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Reviewer")
        .env("GIT_AUTHOR_EMAIL", "reviewer@org.local")
        .env("GIT_COMMITTER_NAME", "Reviewer")
        .env("GIT_COMMITTER_EMAIL", "reviewer@org.local")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

impl Fixture {
    pub(super) fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin.git");
        let seed = tmp.path().join("seed");
        fs::create_dir_all(&origin).unwrap();
        fs::create_dir_all(&seed).unwrap();
        git(&origin, &["init", "--bare", "-b", "main"]);
        git(&seed, &["init", "-b", "main"]);
        fs::write(seed.join("README.md"), "# Seed\n\nfirst\nsecond\n").unwrap();
        git(&seed, &["add", "README.md"]);
        git(&seed, &["commit", "-m", "Initial commit"]);
        let base = git(&seed, &["rev-parse", "HEAD"]);
        git(
            &seed,
            &["push", origin.to_str().unwrap(), "HEAD:refs/heads/main"],
        );

        let runner = GitRunner::new(tmp.path().join("clean-home")).unwrap();
        let remote = RemoteSpec::local(&origin).unwrap();
        let cache_dir = tmp.path().join("repositories/repo-1.git");
        let locks = RepoLocks::new();
        locks
            .with_lock(&cache_dir, || {
                fetch_cache(&runner, &cache_dir, &remote, false)
            })
            .unwrap();
        let worktree = tmp.path().join("tasks/k1/repositories/repo-1");
        let created =
            create_worktree(&runner, &cache_dir, &worktree, BRANCH, &base, false).unwrap();
        Self {
            tmp,
            runner,
            locks,
            cache_dir,
            worktree,
            admin: created.admin_dir_name,
            base,
        }
    }

    pub(super) fn cache(&self) -> ReadOnlyCache<'_> {
        ReadOnlyCache::new(&self.runner, &self.locks, &self.cache_dir, false)
    }

    /// Runs `git` inside the task worktree, as the agent working there would.
    pub(super) fn git(&self, args: &[&str]) -> String {
        git(&self.worktree, args)
    }

    /// Writes `files` into the worktree and commits everything, answering the new head.
    pub(super) fn commit(&self, files: &[(&str, &[u8])], message: &str) -> String {
        for (path, content) in files {
            let full = self.worktree.join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(full, content).unwrap();
        }
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    pub(super) fn admin_dir(&self) -> PathBuf {
        self.cache_dir.join("worktrees").join(&self.admin)
    }
}
