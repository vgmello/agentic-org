//! Isolated per-task worktrees, branch naming, non-force office push, closure cleanup,
//! checkpoint bundles, and sandbox allow/deny rules (`doc-35`, `FC-103`, `FC-140`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cache::{fetch_cache, verify_cache_integrity};
use crate::runner::{AllowedProtocol, GitCredential, GitRunner, RemoteSpec};
use crate::GitError;

/// Result of creating a per-task worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCreated {
    pub branch: String,
    pub base_commit: String,
    pub worktree_path: PathBuf,
    pub admin_dir_name: String,
}

/// Result of a verified office push of a task branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushResult {
    pub branch: String,
    pub commit: String,
}

/// Outcome of post-closure worktree cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupOutcome {
    pub tag_name: String,
    pub head_commit: String,
    pub merged: bool,
}

/// Derived sandbox writable roots and fixed write denies for one task worktree (`doc-35`, `FC-103`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxRules {
    pub writable_roots: Vec<PathBuf>,
    pub fixed_write_denies: Vec<PathBuf>,
    pub readable_cache: PathBuf,
}

/// Produces a lowercase ASCII branch slug from a task title.
pub fn slugify_branch_suffix(title: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 36 {
            break;
        }
    }
    let trimmed = out.trim_matches('-').to_owned();
    if trimmed.is_empty() {
        "work".to_owned()
    } else {
        trimmed
    }
}

/// Validates a 40-char (SHA-1) or 64-char (SHA-256) lowercase hex commit OID.
pub fn validate_commit_oid(oid: &str) -> Result<(), GitError> {
    let valid_len = oid.len() == 40 || oid.len() == 64;
    if !valid_len || !oid.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(GitError::InvalidInput(
            "commit OID must be 40 or 64 lowercase hex characters".into(),
        ));
    }
    Ok(())
}

/// Validates a full or branch ref name via `git check-ref-format`.
pub fn check_ref_format(runner: &GitRunner, full_ref: &str) -> Result<(), GitError> {
    if full_ref.is_empty() || full_ref.starts_with('-') {
        return Err(GitError::InvalidInput(format!("invalid ref `{full_ref}`")));
    }
    let out = runner.run(
        None,
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &["check-ref-format", full_ref],
    )?;
    if out.status != 0 {
        return Err(GitError::InvalidInput(format!(
            "ref `{full_ref}` failed git check-ref-format"
        )));
    }
    Ok(())
}

/// Selects a unique `task/<key>-<slug>` branch name not used by local/remote branches,
/// `merged/`/`archive/` tags, or `taken_branches` (`doc-35`).
pub fn choose_unique_task_branch(
    runner: &GitRunner,
    cache_dir: &Path,
    task_key: &str,
    title: &str,
    taken_branches: &BTreeSet<String>,
) -> Result<String, GitError> {
    let existing = list_cache_refs(runner, cache_dir)?;
    let slug = slugify_branch_suffix(title);
    let base = format!("task/{task_key}-{slug}");
    for suffix in 1u32..1000 {
        let candidate = if suffix == 1 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        let full_head = format!("refs/heads/{candidate}");
        check_ref_format(runner, &full_head)?;
        let collides = taken_branches.contains(&candidate)
            || existing.contains(&full_head)
            || existing.contains(&format!("refs/remotes/origin/{candidate}"))
            || existing.contains(&format!("refs/tags/merged/{candidate}"))
            || existing.contains(&format!("refs/tags/archive/{candidate}"));
        if !collides {
            return Ok(candidate);
        }
    }
    Err(GitError::Conflict(
        "could not find unique task branch name".into(),
    ))
}

fn list_cache_refs(runner: &GitRunner, cache_dir: &Path) -> Result<BTreeSet<String>, GitError> {
    let out = runner.run(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &["for-each-ref", "--format=%(refname)"],
    )?;
    if out.status != 0 {
        return Ok(BTreeSet::new());
    }
    Ok(out.stdout.lines().map(str::to_owned).collect())
}

/// Resolves the commit OID of `refs/remotes/origin/<branch>` in `cache_dir`.
pub fn resolve_remote_branch_commit(
    runner: &GitRunner,
    cache_dir: &Path,
    branch: &str,
) -> Result<String, GitError> {
    let ref_name = format!("refs/remotes/origin/{branch}");
    check_ref_format(runner, &ref_name)?;
    let out = runner.run(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{ref_name}^{{commit}}"),
        ],
    )?;
    if out.status != 0 {
        return Err(GitError::RemoteUnavailable(format!(
            "integration_branch_missing: `{branch}` not found in cache"
        )));
    }
    Ok(out.stdout.trim().to_owned())
}

/// Creates a per-task worktree on a new branch (`-b`, never `-B`) from `base_commit`, reads back
/// the real Git administrative directory name, and rolls back on failure (`doc-35`).
pub fn create_worktree(
    runner: &GitRunner,
    cache_dir: &Path,
    worktree_dir: &Path,
    branch: &str,
    base_commit: &str,
    shared_group: bool,
) -> Result<WorktreeCreated, GitError> {
    if !branch.starts_with("task/") {
        return Err(GitError::InvalidInput(
            "task worktree branch must start with `task/`".into(),
        ));
    }
    validate_commit_oid(base_commit)?;
    check_ref_format(runner, &format!("refs/heads/{branch}"))?;
    verify_cache_integrity(cache_dir, shared_group)?;
    if fs::symlink_metadata(worktree_dir).is_ok() {
        return Err(GitError::Conflict("worktree path already exists".into()));
    }
    if let Some(parent) = worktree_dir.parent() {
        fs::create_dir_all(parent).map_err(|e| GitError::io(parent, e))?;
    }
    let wt_str = worktree_dir
        .to_str()
        .ok_or_else(|| GitError::InvalidInput("worktree path is not valid UTF-8".into()))?;
    if let Err(err) = runner.run_checked(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &["worktree", "add", "-b", branch, "--", wt_str, base_commit],
    ) {
        let _ = fs::remove_dir_all(worktree_dir);
        let _ = rollback_branch_ref(runner, cache_dir, branch);
        return Err(err);
    }

    let admin_dir_name = match find_worktree_admin_dir(cache_dir, worktree_dir) {
        Ok(name) => name,
        Err(err) => {
            rollback_created_worktree(runner, cache_dir, worktree_dir, branch);
            return Err(err);
        }
    };
    if let Err(err) = sanitize_worktree_links(cache_dir, &admin_dir_name, worktree_dir) {
        rollback_created_worktree(runner, cache_dir, worktree_dir, branch);
        return Err(err);
    }

    Ok(WorktreeCreated {
        branch: branch.to_owned(),
        base_commit: base_commit.to_owned(),
        worktree_path: worktree_dir.to_path_buf(),
        admin_dir_name,
    })
}

/// Rolls back a freshly created worktree directory, admin entry, and `refs/heads/<branch>`.
pub fn rollback_created_worktree(
    runner: &GitRunner,
    cache_dir: &Path,
    worktree_dir: &Path,
    branch: &str,
) {
    if let Some(wt_str) = worktree_dir.to_str() {
        let _ = runner.run(
            Some(cache_dir),
            None,
            AllowedProtocol::None,
            &GitCredential::None,
            &["worktree", "remove", "--force", "--", wt_str],
        );
    }
    let _ = fs::remove_dir_all(worktree_dir);
    let _ = runner.run(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &["worktree", "prune"],
    );
    let _ = rollback_branch_ref(runner, cache_dir, branch);
}

fn rollback_branch_ref(runner: &GitRunner, cache_dir: &Path, branch: &str) -> Result<(), GitError> {
    let _ = runner.run(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &["update-ref", "-d", &format!("refs/heads/{branch}")],
    )?;
    Ok(())
}

fn find_worktree_admin_dir(cache_dir: &Path, worktree_dir: &Path) -> Result<String, GitError> {
    let target_git = fs::canonicalize(worktree_dir)
        .unwrap_or_else(|_| worktree_dir.to_path_buf())
        .join(".git");
    let wt_root = cache_dir.join("worktrees");
    for entry in fs::read_dir(&wt_root).map_err(|e| GitError::io(&wt_root, e))? {
        let entry = entry.map_err(|e| GitError::io(&wt_root, e))?;
        let gitdir_file = entry.path().join("gitdir");
        if let Ok(content) = fs::read_to_string(&gitdir_file) {
            let trimmed = PathBuf::from(content.trim());
            let canonical_parent = trimmed
                .parent()
                .and_then(|p| fs::canonicalize(p).ok())
                .map(|p| p.join(".git"));
            if trimmed == target_git || canonical_parent.as_ref() == Some(&target_git) {
                if let Some(name) = entry.file_name().to_str() {
                    return Ok(name.to_owned());
                }
            }
        }
    }
    Err(GitError::InvalidInput(
        "could not locate Git administrative worktree directory".into(),
    ))
}

fn write_regular_file_no_symlink(path: &Path, content: &str, label: &str) -> Result<(), GitError> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            let _ = fs::remove_file(path);
            return Err(GitError::SecurityViolation(format!(
                "hostile symlink detected at `{label}`"
            )));
        }
        if !meta.file_type().is_file() {
            return Err(GitError::SecurityViolation(format!(
                "non-regular file detected at `{label}`"
            )));
        }
    }
    fs::write(path, content).map_err(|e| GitError::io(path, e))
}

/// Removes any `config.worktree` and rewrites `commondir`, `gitdir`, and `<worktree>/.git`
/// to exact canonical values before office commands (`doc-35` §Checks before any command),
/// refusing symlinks to prevent arbitrary-file overwrite.
pub fn sanitize_worktree_links(
    cache_dir: &Path,
    admin_dir_name: &str,
    worktree_dir: &Path,
) -> Result<(), GitError> {
    if admin_dir_name.is_empty()
        || admin_dir_name.contains(['/', '\\'])
        || admin_dir_name == "."
        || admin_dir_name == ".."
    {
        return Err(GitError::SecurityViolation(
            "invalid worktree admin_dir_name".into(),
        ));
    }
    let admin_dir = cache_dir.join("worktrees").join(admin_dir_name);
    let admin_meta = fs::symlink_metadata(&admin_dir).map_err(|e| GitError::io(&admin_dir, e))?;
    if admin_meta.file_type().is_symlink() || !admin_meta.file_type().is_dir() {
        return Err(GitError::SecurityViolation(
            "worktree admin directory is missing or a symlink".into(),
        ));
    }
    let wt_cfg = admin_dir.join("config.worktree");
    if fs::symlink_metadata(&wt_cfg).is_ok() {
        fs::remove_file(&wt_cfg).map_err(|e| GitError::io(&wt_cfg, e))?;
    }
    let commondir = admin_dir.join("commondir");
    write_regular_file_no_symlink(&commondir, "../..\n", "worktree/commondir")?;
    let canonical_wt = fs::canonicalize(worktree_dir).map_err(|e| GitError::io(worktree_dir, e))?;
    let canonical_admin = fs::canonicalize(&admin_dir).map_err(|e| GitError::io(&admin_dir, e))?;
    let gitdir = canonical_admin.join("gitdir");
    write_regular_file_no_symlink(
        &gitdir,
        &format!("{}\n", canonical_wt.join(".git").display()),
        "worktree/gitdir",
    )?;
    let dot_git = canonical_wt.join(".git");
    write_regular_file_no_symlink(
        &dot_git,
        &format!("gitdir: {}\n", canonical_admin.display()),
        "worktree/.git",
    )?;
    Ok(())
}

/// Pushes a recorded task branch by commit OID (never force-push, never default/integration
/// branch, verifies `base_commit` ancestry, updates `refs/remotes/origin/<branch>`) (`doc-35`).
pub fn push_task_branch(
    runner: &GitRunner,
    cache_dir: &Path,
    remote: &RemoteSpec,
    branch: &str,
    base_commit: &str,
    forbidden_branches: &BTreeSet<String>,
    shared_group: bool,
) -> Result<PushResult, GitError> {
    if !branch.starts_with("task/") {
        return Err(GitError::SecurityViolation(format!(
            "refusing to push non-task branch `{branch}`"
        )));
    }
    if forbidden_branches.contains(branch) {
        return Err(GitError::SecurityViolation(format!(
            "refusing to push protected/integration branch `{branch}`"
        )));
    }
    validate_commit_oid(base_commit)?;
    let full_ref = format!("refs/heads/{branch}");
    check_ref_format(runner, &full_ref)?;
    verify_cache_integrity(cache_dir, shared_group)?;

    let head_oid = runner
        .run_checked(
            Some(cache_dir),
            None,
            AllowedProtocol::None,
            &GitCredential::None,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{full_ref}^{{commit}}"),
            ],
        )?
        .trim()
        .to_owned();

    // Verify head descends from recorded base_commit (`doc-35` §Pushing step 3).
    let anc = runner.run(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &["merge-base", "--is-ancestor", base_commit, &head_oid],
    )?;
    if anc.status != 0 {
        return Err(GitError::Conflict(format!(
            "branch `{branch}` head `{head_oid}` does not descend from recorded base `{base_commit}`"
        )));
    }

    // Push by commit ID, never `+`, never `--force` (`doc-35` §Pushing step 4).
    let refspec = format!("{head_oid}:refs/heads/{branch}");
    let out = runner.run(
        Some(cache_dir),
        None,
        remote.protocol,
        &remote.credential,
        &["push", "--", &remote.url, &refspec],
    )?;
    if out.status != 0 {
        let stderr = out.stderr.trim().to_owned();
        if stderr.contains("non-fast-forward") || stderr.contains("rejected") {
            return Err(GitError::Conflict(format!(
                "non-fast-forward push rejected: {stderr}"
            )));
        }
        return Err(GitError::RemoteUnavailable(stderr));
    }

    // Record pushed head under `refs/remotes/origin/<branch>`.
    runner.run_checked(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &[
            "update-ref",
            &format!("refs/remotes/origin/{branch}"),
            &head_oid,
        ],
    )?;

    Ok(PushResult {
        branch: branch.to_owned(),
        commit: head_oid,
    })
}

fn verify_cleanup_head_and_status(
    runner: &GitRunner,
    cache_dir: &Path,
    worktree_dir: &Path,
    admin_dir_name: &str,
    branch: &str,
    integration_branch: &str,
) -> Result<(String, bool, bool), GitError> {
    let status_out = runner.run_checked(
        Some(&cache_dir.join("worktrees").join(admin_dir_name)),
        Some(worktree_dir),
        AllowedProtocol::None,
        &GitCredential::None,
        &["status", "--porcelain"],
    )?;
    if !status_out.trim().is_empty() {
        return Err(GitError::Conflict(
            "worktree has uncommitted changes; refusing cleanup".into(),
        ));
    }

    let head_oid = runner
        .run_checked(
            Some(cache_dir),
            None,
            AllowedProtocol::None,
            &GitCredential::None,
            &[
                "rev-parse",
                "--verify",
                &format!("refs/heads/{branch}^{{commit}}"),
            ],
        )?
        .trim()
        .to_owned();
    let wt_head = runner
        .run_checked(
            Some(&cache_dir.join("worktrees").join(admin_dir_name)),
            Some(worktree_dir),
            AllowedProtocol::None,
            &GitCredential::None,
            &["rev-parse", "--verify", "HEAD^{commit}"],
        )?
        .trim()
        .to_owned();
    if wt_head != head_oid {
        return Err(GitError::Conflict(
            "worktree HEAD does not match recorded task branch; refusing cleanup".into(),
        ));
    }

    let is_ancestor = |target_ref: &str| -> Result<bool, GitError> {
        Ok(runner
            .run(
                Some(cache_dir),
                None,
                AllowedProtocol::None,
                &GitCredential::None,
                &["merge-base", "--is-ancestor", &head_oid, target_ref],
            )?
            .status
            == 0)
    };
    let is_merged = is_ancestor(&format!("refs/remotes/origin/{integration_branch}"))?;
    let on_remote = is_ancestor(&format!("refs/remotes/origin/{branch}"))?;
    if !is_merged && !on_remote {
        return Err(GitError::Conflict(
            "worktree has unpushed commits not on remote or integration branch; refusing cleanup"
                .into(),
        ));
    }
    Ok((head_oid, is_merged, on_remote))
}

/// Cleans up a closed task's worktree after confirming zero uncommitted changes and zero unpushed
/// commits, pushing a `merged/<branch>` or `archive/<branch>` tag and compare-and-swap deleting
/// remote and local refs (`doc-35` §When a task is done).
#[expect(
    clippy::too_many_arguments,
    reason = "public worktree cleanup API takes explicit runner, paths, remote, branches, and group mode"
)]
pub fn cleanup_worktree(
    runner: &GitRunner,
    cache_dir: &Path,
    worktree_dir: &Path,
    admin_dir_name: &str,
    remote: &RemoteSpec,
    branch: &str,
    integration_branch: &str,
    shared_group: bool,
) -> Result<CleanupOutcome, GitError> {
    if !branch.starts_with("task/") {
        return Err(GitError::SecurityViolation(
            "cleanup only operates on `task/*` branches".into(),
        ));
    }
    fetch_cache(runner, cache_dir, remote, shared_group)?;
    sanitize_worktree_links(cache_dir, admin_dir_name, worktree_dir)?;
    let (head_oid, is_merged, on_remote) = verify_cleanup_head_and_status(
        runner,
        cache_dir,
        worktree_dir,
        admin_dir_name,
        branch,
        integration_branch,
    )?;

    let tag_name = if is_merged {
        format!("merged/{branch}")
    } else {
        format!("archive/{branch}")
    };
    let tag_ref = format!("refs/tags/{tag_name}");
    runner.run_checked(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &["update-ref", &tag_ref, &head_oid],
    )?;
    runner.run_checked(
        Some(cache_dir),
        None,
        remote.protocol,
        &remote.credential,
        &["push", "--", &remote.url, &format!("{tag_ref}:{tag_ref}")],
    )?;

    if on_remote {
        let lease = format!("--force-with-lease=refs/heads/{branch}:{head_oid}");
        let del_spec = format!(":refs/heads/{branch}");
        runner.run_checked(
            Some(cache_dir),
            None,
            remote.protocol,
            &remote.credential,
            &["push", &lease, "--", &remote.url, &del_spec],
        )?;
    }

    let wt_str = worktree_dir
        .to_str()
        .ok_or_else(|| GitError::InvalidInput("worktree path not UTF-8".into()))?;
    runner.run_checked(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &["worktree", "remove", "--", wt_str],
    )?;
    runner.run_checked(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &[
            "update-ref",
            "-d",
            &format!("refs/heads/{branch}"),
            &head_oid,
        ],
    )?;

    Ok(CleanupOutcome {
        tag_name,
        head_commit: head_oid,
        merged: is_merged,
    })
}

/// Creates a Git checkpoint bundle (`durable/checkpoints/<task>/<repo>/...`) before cache
/// deletion or cross-office handoff (`doc-35` Open Question #9).
pub fn create_checkpoint_bundle(
    runner: &GitRunner,
    cache_dir: &Path,
    bundle_path: &Path,
    branch: &str,
    shared_group: bool,
) -> Result<(), GitError> {
    if !branch.starts_with("task/") {
        return Err(GitError::SecurityViolation(
            "checkpoint bundle only operates on `task/*` branches".into(),
        ));
    }
    check_ref_format(runner, &format!("refs/heads/{branch}"))?;
    verify_cache_integrity(cache_dir, shared_group)?;
    let out_str = bundle_path
        .to_str()
        .ok_or_else(|| GitError::InvalidInput("bundle path not UTF-8".into()))?;
    if out_str.is_empty() || out_str.starts_with('-') {
        return Err(GitError::InvalidInput(
            "bundle path must be non-empty and must not start with `-`".into(),
        ));
    }
    if let Some(parent) = bundle_path.parent() {
        fs::create_dir_all(parent).map_err(|e| GitError::io(parent, e))?;
    }
    runner.run_checked(
        Some(cache_dir),
        None,
        AllowedProtocol::None,
        &GitCredential::None,
        &["bundle", "create", out_str, &format!("refs/heads/{branch}")],
    )?;
    Ok(())
}

/// Derives the exact sandbox writable roots and fixed write denies for a task session worktree
/// (`doc-35` §Sandbox and deny rules for the selected paths, `FC-103`).
pub fn derive_sandbox_rules(
    cache_dir: &Path,
    admin_dir_name: &str,
    worktree_dir: &Path,
) -> SandboxRules {
    let admin = cache_dir.join("worktrees").join(admin_dir_name);
    let writable_roots = vec![
        worktree_dir.to_path_buf(),
        cache_dir.join("objects"),
        cache_dir.join("refs/heads/task"),
        cache_dir.join("logs/refs/heads/task"),
        admin.clone(),
    ];
    let fixed_write_denies = vec![
        worktree_dir.join(".git"),
        cache_dir.join("config"),
        cache_dir.join("config.worktree"),
        cache_dir.join("hooks"),
        cache_dir.join("info"),
        cache_dir.join("objects/info"),
        cache_dir.join("HEAD"),
        cache_dir.join("packed-refs"),
        cache_dir.join("description"),
        cache_dir.join("FETCH_HEAD"),
        cache_dir.join("shallow"),
        cache_dir.join("refs/remotes"),
        cache_dir.join("refs/tags"),
        cache_dir.join("refs/office"),
        cache_dir.join("refs/replace"),
        admin.join("commondir"),
        admin.join("gitdir"),
        admin.join("config.worktree"),
    ];
    SandboxRules {
        writable_roots,
        fixed_write_denies,
        readable_cache: cache_dir.to_path_buf(),
    }
}
