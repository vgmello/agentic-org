//! Per-office bare repository caches, per-repository locking, and canonical integrity checks
//! (`doc-35` §The repository cache).

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::runner::{AllowedProtocol, GitCredential, GitRunner, RemoteSpec};
use crate::GitError;

/// Exact canonical `<cache>/config` bytes (`doc-35`: no remote, no URL, no credential helper,
/// no proxy, no include, no alias, no filter, no hook path, no `extensions.worktreeConfig`).
pub fn canonical_cache_config(shared_group: bool) -> &'static str {
    if shared_group {
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n\tsharedRepository = group\n"
    } else {
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n"
    }
}

/// Serializes all office operations per bare cache path (both in-process and cross-process).
#[derive(Default)]
pub struct RepoLocks {
    inner: Mutex<BTreeMap<PathBuf, Arc<Mutex<()>>>>,
}

impl RepoLocks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `f` while holding both the in-process mutex and OS file lock for `cache_dir`.
    pub fn with_lock<T>(
        &self,
        cache_dir: &Path,
        f: impl FnOnce() -> Result<T, GitError>,
    ) -> Result<T, GitError> {
        let key = cache_dir.to_path_buf();
        let mutex = {
            let mut map = self
                .inner
                .lock()
                .map_err(|_| GitError::Conflict("repository lock table poisoned".into()))?;
            Arc::clone(map.entry(key).or_insert_with(|| Arc::new(Mutex::new(()))))
        };
        let _proc_guard = mutex
            .lock()
            .map_err(|_| GitError::Conflict("repository mutex poisoned".into()))?;
        if let Some(parent) = cache_dir.parent() {
            fs::create_dir_all(parent).map_err(|e| GitError::io(parent, e))?;
        }
        let mut lock_os = cache_dir.as_os_str().to_os_string();
        lock_os.push(".lock");
        let lock_path = PathBuf::from(lock_os);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| GitError::io(&lock_path, e))?;
        file.lock().map_err(|e| GitError::io(&lock_path, e))?;
        let res = f();
        drop(file);
        res
    }
}

/// Checks `<cache>/config` against exact canonical bytes (refusing symlinks) and refuses
/// `alternates`, `http-alternates`, `shallow`, `grafts`, `config.worktree`, and `refs/replace/`
/// (`doc-35` §Checks before any command on a cache).
/// If `<cache>/config` differs or is a symlink, restores canonical bytes and returns
/// [`GitError::SecurityViolation`].
pub fn verify_cache_integrity(cache_dir: &Path, shared_group: bool) -> Result<(), GitError> {
    let config_path = cache_dir.join("config");
    let expected = canonical_cache_config(shared_group);
    let meta = fs::symlink_metadata(&config_path).map_err(|e| GitError::io(&config_path, e))?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        let _ = fs::remove_file(&config_path);
        let _ = fs::write(&config_path, expected);
        return Err(GitError::SecurityViolation(
            "hostile symlink or non-regular cache config detected and restored".into(),
        ));
    }
    let actual = fs::read_to_string(&config_path).map_err(|e| GitError::io(&config_path, e))?;
    if actual != expected {
        let _ = fs::write(&config_path, expected);
        return Err(GitError::SecurityViolation(
            "hostile cache config detected and restored to canonical bytes".into(),
        ));
    }
    for forbidden in [
        "objects/info/alternates",
        "objects/info/http-alternates",
        "shallow",
        "info/grafts",
        "config.worktree",
    ] {
        let p = cache_dir.join(forbidden);
        if fs::symlink_metadata(&p).is_ok() {
            return Err(GitError::SecurityViolation(format!(
                "forbidden git metadata `{forbidden}` present in cache"
            )));
        }
    }
    let replace_dir = cache_dir.join("refs/replace");
    if let Ok(rmeta) = fs::symlink_metadata(&replace_dir) {
        if rmeta.file_type().is_symlink()
            || (rmeta.file_type().is_dir() && has_any_file(&replace_dir)?)
        {
            return Err(GitError::SecurityViolation(
                "forbidden `refs/replace/` entries present in cache".into(),
            ));
        }
    }
    let packed = cache_dir.join("packed-refs");
    if let Ok(pmeta) = fs::symlink_metadata(&packed) {
        if pmeta.file_type().is_symlink() {
            return Err(GitError::SecurityViolation(
                "forbidden symlink at `packed-refs` in cache".into(),
            ));
        }
        if pmeta.file_type().is_file() {
            let content = fs::read_to_string(&packed).map_err(|e| GitError::io(&packed, e))?;
            if content.lines().any(|l| {
                l.split_whitespace()
                    .nth(1)
                    .is_some_and(|r| r.starts_with("refs/replace/"))
            }) {
                return Err(GitError::SecurityViolation(
                    "forbidden `refs/replace/` entry in packed-refs".into(),
                ));
            }
        }
    }
    Ok(())
}

fn has_any_file(dir: &Path) -> Result<bool, GitError> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(curr) = stack.pop() {
        for entry in fs::read_dir(&curr).map_err(|e| GitError::io(&curr, e))? {
            let entry = entry.map_err(|e| GitError::io(&curr, e))?;
            let ft = entry.file_type().map_err(|e| GitError::io(&curr, e))?;
            if ft.is_symlink() {
                return Ok(true);
            }
            if ft.is_dir() {
                stack.push(entry.path());
            } else {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Initializes a bare repository cache with canonical configuration (never `git clone`).
pub fn init_bare_cache(
    runner: &GitRunner,
    cache_dir: &Path,
    shared_group: bool,
) -> Result<(), GitError> {
    if cache_dir.join("HEAD").is_file() {
        return verify_cache_integrity(cache_dir, shared_group);
    }
    fs::create_dir_all(cache_dir).map_err(|e| GitError::io(cache_dir, e))?;
    let target = cache_dir
        .to_str()
        .ok_or_else(|| GitError::InvalidInput("cache_dir is not valid UTF-8".into()))?;
    runner.run_checked(
        None,
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &["init", "--bare", "--", target],
    )?;
    // Remove default sample hooks so no hook files exist in the cache.
    let hooks_dir = cache_dir.join("hooks");
    if hooks_dir.is_dir() {
        let _ = fs::remove_dir_all(&hooks_dir);
    }
    let config_path = cache_dir.join("config");
    fs::write(&config_path, canonical_cache_config(shared_group))
        .map_err(|e| GitError::io(&config_path, e))?;
    verify_cache_integrity(cache_dir, shared_group)
}

/// Lists remote branch heads (`branch_name -> commit_oid`) using hardened `git ls-remote --heads`.
pub fn ls_remote_heads(
    runner: &GitRunner,
    remote: &RemoteSpec,
) -> Result<BTreeMap<String, String>, GitError> {
    let out = runner.run(
        None,
        None,
        remote.protocol,
        &remote.credential,
        &["ls-remote", "--heads", "--", &remote.url],
    )?;
    if out.status != 0 {
        return Err(GitError::RemoteUnavailable(out.stderr.trim().to_owned()));
    }
    let mut heads = BTreeMap::new();
    for line in out.stdout.lines() {
        let mut parts = line.split_whitespace();
        let (Some(oid), Some(refname)) = (parts.next(), parts.next()) else {
            continue;
        };
        if let Some(branch) = refname.strip_prefix("refs/heads/") {
            heads.insert(branch.to_owned(), oid.to_owned());
        }
    }
    Ok(heads)
}

/// Fetches all remote heads into `refs/remotes/origin/*` (with pruning) and tags without force.
pub fn fetch_cache(
    runner: &GitRunner,
    cache_dir: &Path,
    remote: &RemoteSpec,
    shared_group: bool,
) -> Result<(), GitError> {
    init_bare_cache(runner, cache_dir, shared_group)?;
    verify_cache_integrity(cache_dir, shared_group)?;
    let out = runner.run(
        Some(cache_dir),
        None,
        remote.protocol,
        &remote.credential,
        &[
            "fetch",
            "--prune",
            "--no-tags",
            "--",
            &remote.url,
            "+refs/heads/*:refs/remotes/origin/*",
            "refs/tags/*:refs/tags/*",
        ],
    )?;
    if out.status != 0 {
        return Err(GitError::RemoteUnavailable(out.stderr.trim().to_owned()));
    }
    Ok(())
}
