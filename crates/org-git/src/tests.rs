//! Real-`git` security and lifecycle tests for `org-git` (`doc-35`, `TASK-51`).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::{
    canonical_cache_config, cleanup_worktree, create_checkpoint_bundle, create_worktree,
    derive_sandbox_rules, fetch_cache, init_bare_cache, ls_remote_heads, push_task_branch,
    validate_remote_url, AllowedProtocol, GitError, GitRunner, RemoteSpec, RepoLocks,
};

fn git_raw(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Owner")
        .env("GIT_AUTHOR_EMAIL", "owner@org.local")
        .env("GIT_COMMITTER_NAME", "Owner")
        .env("GIT_COMMITTER_EMAIL", "owner@org.local")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn seed_bare_remote(tmp: &Path) -> (std::path::PathBuf, String) {
    let remote = tmp.join("remote.git");
    let seed = tmp.join("seed");
    fs::create_dir_all(&remote).unwrap();
    fs::create_dir_all(&seed).unwrap();
    git_raw(&remote, &["init", "--bare", "-b", "main"]);
    git_raw(&seed, &["init", "-b", "main"]);
    fs::write(seed.join("README.md"), "# Seed\n").unwrap();
    git_raw(&seed, &["add", "README.md"]);
    git_raw(&seed, &["commit", "-m", "Initial commit"]);
    let sha = git_raw(&seed, &["rev-parse", "HEAD"]);
    git_raw(
        &seed,
        &["push", remote.to_str().unwrap(), "HEAD:refs/heads/main"],
    );
    (remote, sha)
}

#[test]
fn url_validation_enforces_https_and_ssh_only() {
    assert_eq!(
        validate_remote_url("https://github.com/acme/app.git").unwrap(),
        AllowedProtocol::Https
    );
    assert_eq!(
        validate_remote_url("ssh://git@github.com/acme/app.git").unwrap(),
        AllowedProtocol::Ssh
    );
    assert_eq!(
        validate_remote_url("git@github.com:acme/app.git").unwrap(),
        AllowedProtocol::Ssh
    );
    for bad in [
        "http://github.com/acme/app.git",
        "git://github.com/acme/app.git",
        "ext::sh -c evil",
        "fd::0/repo",
        "file:///tmp/repo.git",
        "https://user:secret@github.com/acme/app.git",
        "-u evil",
        "ssh://-oProxyCommand=evil/repo",
        "ssh://git@-oProxyCommand=evil/repo",
        "git@-oProxyCommand=evil:org/repo.git",
        "foo/bar@host:repo",
        "/etc/passwd@x:y",
        "ssh://host",
    ] {
        assert!(validate_remote_url(bad).is_err(), "must reject {bad}");
    }
}

#[cfg(unix)]
#[test]
fn symlink_tampering_on_cache_config_and_worktree_dot_git_is_refused() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let (remote_dir, base_sha) = seed_bare_remote(tmp.path());
    let runner = GitRunner::new(tmp.path().join("clean-home")).unwrap();
    let remote = RemoteSpec::local(&remote_dir).unwrap();
    let cache_dir = tmp.path().join("cache.git");
    fetch_cache(&runner, &cache_dir, &remote, false).unwrap();

    // Symlink at `<cache>/config` is unlinked, restored, and refused without touching target.
    let target = tmp.path().join("victim.txt");
    fs::write(&target, "SAFE\n").unwrap();
    fs::remove_file(cache_dir.join("config")).unwrap();
    symlink(&target, cache_dir.join("config")).unwrap();
    assert!(matches!(
        crate::verify_cache_integrity(&cache_dir, false),
        Err(GitError::SecurityViolation(_))
    ));
    assert_eq!(fs::read_to_string(&target).unwrap(), "SAFE\n");

    // Broken symlink at `objects/info/http-alternates` is refused.
    fs::create_dir_all(cache_dir.join("objects/info")).unwrap();
    symlink(
        tmp.path().join("missing"),
        cache_dir.join("objects/info/http-alternates"),
    )
    .unwrap();
    assert!(matches!(
        crate::verify_cache_integrity(&cache_dir, false),
        Err(GitError::SecurityViolation(_))
    ));
    fs::remove_file(cache_dir.join("objects/info/http-alternates")).unwrap();

    // Symlink at `<worktree>/.git` is refused and never overwrites target.
    let wt_dir = tmp.path().join("wt-sym");
    let created = create_worktree(
        &runner,
        &cache_dir,
        &wt_dir,
        "task/T-A9-sym",
        &base_sha,
        false,
    )
    .unwrap();
    fs::remove_file(wt_dir.join(".git")).unwrap();
    symlink(&target, wt_dir.join(".git")).unwrap();
    assert!(matches!(
        crate::sanitize_worktree_links(&cache_dir, &created.admin_dir_name, &wt_dir),
        Err(GitError::SecurityViolation(_))
    ));
    assert_eq!(fs::read_to_string(&target).unwrap(), "SAFE\n");

    // Invalid commit OID rejected before invoking git merge-base.
    assert!(matches!(
        push_task_branch(
            &runner,
            &cache_dir,
            &remote,
            &created.branch,
            "--all",
            &BTreeSet::new(),
            false
        ),
        Err(GitError::InvalidInput(_))
    ));
}

#[test]
fn bare_cache_worktree_commit_push_hostile_config_hooks_and_cleanup() {
    let tmp = tempfile::tempdir().unwrap();
    let (remote_dir, base_sha) = seed_bare_remote(tmp.path());
    let runner = GitRunner::new(tmp.path().join("clean-home")).unwrap();
    let remote = RemoteSpec::local(&remote_dir).unwrap();
    let cache_dir = tmp.path().join("runtime/office-1/repositories/repo-1.git");
    let locks = RepoLocks::new();

    // 1. ls-remote sees `main`; init bare cache & fetch.
    let heads = ls_remote_heads(&runner, &remote).unwrap();
    assert_eq!(
        heads.get("main").map(String::as_str),
        Some(base_sha.as_str())
    );

    locks
        .with_lock(&cache_dir, || {
            init_bare_cache(&runner, &cache_dir, false)?;
            fetch_cache(&runner, &cache_dir, &remote, false)
        })
        .unwrap();
    assert_eq!(
        fs::read_to_string(cache_dir.join("config")).unwrap(),
        canonical_cache_config(false)
    );

    // 2. Hostile cache config is detected, restored, and blocks operation!
    fs::write(
        cache_dir.join("config"),
        "[core]\n\tbare = true\n[credential]\n\thelper = !echo PWNED\n",
    )
    .unwrap();
    let err = fetch_cache(&runner, &cache_dir, &remote, false).unwrap_err();
    assert!(matches!(err, GitError::SecurityViolation(_)));
    assert_eq!(
        fs::read_to_string(cache_dir.join("config")).unwrap(),
        canonical_cache_config(false)
    );

    // 3. Forbidden grafts/alternates/replace objects are refused!
    fs::create_dir_all(cache_dir.join("info")).unwrap();
    fs::write(cache_dir.join("info/grafts"), "bad\n").unwrap();
    assert!(matches!(
        fetch_cache(&runner, &cache_dir, &remote, false),
        Err(GitError::SecurityViolation(_))
    ));
    fs::remove_file(cache_dir.join("info/grafts")).unwrap();

    // 4. Planted hook in cache never executes during office Git commands.
    let hook_marker = tmp.path().join("hook-ran");
    fs::create_dir_all(cache_dir.join("hooks")).unwrap();
    fs::write(
        cache_dir.join("hooks/pre-push"),
        format!("#!/bin/sh\ntouch {}\n", hook_marker.display()),
    )
    .unwrap();

    // 5. Create isolated per-task worktree on task/T-A1-fix-login.
    let wt_dir = tmp
        .path()
        .join("runtime/office-1/teams/t1/agents/a1/tasks/k1/repositories/repo-1");
    let created = create_worktree(
        &runner,
        &cache_dir,
        &wt_dir,
        "task/T-A1-fix-login",
        &base_sha,
        false,
    )
    .unwrap();
    let rules = derive_sandbox_rules(&cache_dir, &created.admin_dir_name, &wt_dir);
    assert!(rules.writable_roots.contains(&wt_dir));
    assert!(rules.fixed_write_denies.contains(&cache_dir.join("config")));

    // 6. Agent commits in worktree; office pushes by commit OID.
    fs::write(wt_dir.join("feature.rs"), "pub fn ok() -> bool { true }\n").unwrap();
    git_raw(&wt_dir, &["add", "feature.rs"]);
    git_raw(&wt_dir, &["commit", "-m", "Implement fix"]);
    let head_sha = git_raw(&wt_dir, &["rev-parse", "HEAD"]);

    let forbidden = BTreeSet::from(["main".to_owned()]);
    assert!(matches!(
        push_task_branch(&runner, &cache_dir, &remote, "main", &base_sha, &forbidden, false),
        Err(GitError::SecurityViolation(_))
    ));
    let pushed = push_task_branch(
        &runner,
        &cache_dir,
        &remote,
        &created.branch,
        &base_sha,
        &forbidden,
        false,
    )
    .unwrap();
    assert_eq!(pushed.commit, head_sha);
    assert!(!hook_marker.exists(), "planted hook must never execute");

    let remote_heads = ls_remote_heads(&runner, &remote).unwrap();
    assert_eq!(
        remote_heads.get("task/T-A1-fix-login").map(String::as_str),
        Some(head_sha.as_str())
    );

    // 7. Checkpoint bundle works.
    let bundle_path = tmp.path().join("durable/checkpoints/k1/repo-1/head.bundle");
    create_checkpoint_bundle(&runner, &cache_dir, &bundle_path, &created.branch, false).unwrap();
    assert!(bundle_path.is_file());

    // 8. Cleanup refuses dirty worktree, then succeeds with archive/ tag + CAS delete when clean.
    fs::write(wt_dir.join("dirty.tmp"), "uncommitted").unwrap();
    assert!(matches!(
        cleanup_worktree(
            &runner,
            &cache_dir,
            &wt_dir,
            &created.admin_dir_name,
            &remote,
            &created.branch,
            "main",
            false,
        ),
        Err(GitError::Conflict(_))
    ));
    fs::remove_file(wt_dir.join("dirty.tmp")).unwrap();
    let cleaned = cleanup_worktree(
        &runner,
        &cache_dir,
        &wt_dir,
        &created.admin_dir_name,
        &remote,
        &created.branch,
        "main",
        false,
    )
    .unwrap();
    assert_eq!(cleaned.tag_name, "archive/task/T-A1-fix-login");
    assert!(!cleaned.merged);
    assert!(!wt_dir.exists());
}
