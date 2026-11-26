//! On-demand origin refresh of a bare cache, summarizing how `refs/remotes/origin/*` moved
//! (`TASK-173`). Read-only against the remote. Writes only objects, `refs/remotes/origin/*` and
//! `refs/tags/*` in the cache: never `refs/heads/*`, so no worktree's checked-out branch,
//! index or working tree changes.

use std::collections::BTreeMap;
use std::path::Path;

use crate::cache::{fetch_cache, verify_cache_integrity};
use crate::runner::{AllowedProtocol, GitCredential, GitRunner, RemoteSpec};
use crate::GitError;

const ORIGIN_REFS: &str = "refs/remotes/origin/";

/// How one origin refresh changed the cache's `origin/*` branches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheRefresh {
    /// Every `refs/remotes/origin/<branch>` after the fetch, as `branch -> commit OID`.
    pub branches: BTreeMap<String, String>,
    /// Branches the cache did not have before the fetch (sorted).
    pub added: Vec<String>,
    /// Branches whose commit moved (sorted).
    pub updated: Vec<String>,
    /// Branches the origin no longer has, pruned from the cache (sorted).
    pub pruned: Vec<String>,
    /// The origin's `HEAD` branch, kept only when it is one of `branches` (best effort).
    pub default_branch: Option<String>,
}

impl CacheRefresh {
    /// Commit OID of `origin/<default_branch>` after the fetch.
    pub fn head_oid(&self) -> Option<&str> {
        self.default_branch
            .as_deref()
            .and_then(|branch| self.branches.get(branch))
            .map(String::as_str)
    }
}

/// Lists `refs/remotes/origin/*` in `cache_dir` as `branch -> commit OID` (empty while the
/// cache does not exist yet). The caller holds the repository lock.
pub fn list_origin_branches(
    runner: &GitRunner,
    cache_dir: &Path,
) -> Result<BTreeMap<String, String>, GitError> {
    if !cache_dir.join("HEAD").is_file() {
        return Ok(BTreeMap::new());
    }
    let out = runner.run_checked(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &[
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            ORIGIN_REFS,
        ],
    )?;
    Ok(parse_origin_refs(&out))
}

fn parse_origin_refs(stdout: &str) -> BTreeMap<String, String> {
    stdout
        .lines()
        .filter_map(|line| {
            let (oid, refname) = line.split_once(' ')?;
            let branch = refname.strip_prefix(ORIGIN_REFS)?;
            (!branch.is_empty() && branch != "HEAD").then(|| (branch.to_owned(), oid.to_owned()))
        })
        .collect()
}

/// Reads the origin's `HEAD` branch with a hardened `git ls-remote --symref`; `None` when the
/// origin advertises no symbolic `HEAD`.
pub fn remote_default_branch(
    runner: &GitRunner,
    remote: &RemoteSpec,
) -> Result<Option<String>, GitError> {
    let out = runner.run(
        None,
        None,
        remote.protocol,
        &remote.credential,
        &["ls-remote", "--symref", "--", &remote.url, "HEAD"],
    )?;
    if out.status != 0 {
        return Err(GitError::RemoteUnavailable(out.stderr.trim().to_owned()));
    }
    Ok(parse_symref_head(&out.stdout))
}

fn parse_symref_head(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let (target, name) = line.strip_prefix("ref: ")?.split_once('\t')?;
        if name.trim() != "HEAD" {
            return None;
        }
        target
            .trim()
            .strip_prefix("refs/heads/")
            .filter(|branch| !branch.is_empty())
            .map(str::to_owned)
    })
}

/// Fetches the origin into `cache_dir` (initializing it when missing) and reports which
/// `origin/*` branches were added, moved or pruned. The caller holds the repository lock.
pub fn refresh_cache(
    runner: &GitRunner,
    cache_dir: &Path,
    remote: &RemoteSpec,
    shared_group: bool,
) -> Result<CacheRefresh, GitError> {
    if cache_dir.join("HEAD").is_file() {
        verify_cache_integrity(cache_dir, shared_group)?;
    }
    let before = list_origin_branches(runner, cache_dir)?;
    fetch_cache(runner, cache_dir, remote, shared_group)?;
    let after = list_origin_branches(runner, cache_dir)?;
    // Best effort: the fetch already succeeded, so a failed `HEAD` lookup only omits it.
    let default_branch = remote_default_branch(runner, remote)
        .ok()
        .flatten()
        .filter(|branch| after.contains_key(branch));
    Ok(summarize(&before, after, default_branch))
}

fn summarize(
    before: &BTreeMap<String, String>,
    after: BTreeMap<String, String>,
    default_branch: Option<String>,
) -> CacheRefresh {
    let mut added = Vec::new();
    let mut updated = Vec::new();
    for (branch, oid) in &after {
        match before.get(branch) {
            None => added.push(branch.clone()),
            Some(old) if old != oid => updated.push(branch.clone()),
            Some(_) => {}
        }
    }
    let pruned = before
        .keys()
        .filter(|branch| !after.contains_key(*branch))
        .cloned()
        .collect();
    CacheRefresh {
        branches: after,
        added,
        updated,
        pruned,
        default_branch,
    }
}

#[cfg(test)]
#[path = "refresh_tests.rs"]
mod tests;
