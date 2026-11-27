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
    // Directory planted at `worktrees/<admin>/config.worktree` is removed cleanly (not EISDIR).
    let wt_cfg_dir = cache_dir
        .join("worktrees")
        .join(&created.admin_dir_name)
        .join("config.worktree");
    fs::create_dir_all(&wt_cfg_dir).unwrap();
    fs::write(wt_cfg_dir.join("evil"), "x").unwrap();
    crate::sanitize_worktree_links(&cache_dir, &created.admin_dir_name, &wt_dir).unwrap();
    assert!(
        !wt_cfg_dir.exists(),
        "directory config.worktree must be removed"
    );
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
    // TASK-155: the cache and the worktree's folders are owner-only whatever the mask.
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&cache_dir), 0o700);
        for folder in [
            "teams/t1/agents/a1/tasks/k1",
            "teams/t1/agents/a1/tasks/k1/repositories",
        ] {
            assert_eq!(
                mode(&tmp.path().join("runtime/office-1").join(folder)),
                0o700
            );
        }
    }
    let rules = derive_sandbox_rules(&cache_dir, &created.admin_dir_name, &wt_dir);
    assert!(rules.writable_roots.contains(&wt_dir));
    assert!(rules.fixed_write_denies.contains(&cache_dir.join("config")));

    // 6. Agent commits in worktree; office pushes by commit OID.
    fs::write(wt_dir.join("feature.rs"), "pub fn ok() -> bool { true }\n").unwrap();
    fs::write(wt_dir.join(".gitignore"), "/target/\n").unwrap();
    git_raw(&wt_dir, &["add", "feature.rs", ".gitignore"]);
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
    // An agent's Cargo build output inside its worktree is ignored, so it neither blocks the
    // cleanup nor outlives it: tearing the worktree down reclaims its `target/` too.
    let build = wt_dir.join("target/debug/deps");
    fs::create_dir_all(&build).unwrap();
    fs::write(build.join("libfeature.rlib"), vec![0_u8; 64 * 1024]).unwrap();
    fs::write(
        wt_dir.join("target/CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )
    .unwrap();
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
    assert!(
        !wt_dir.join("target").exists(),
        "teardown leaves no Cargo target"
    );
    assert!(!wt_dir.exists());
}

/// TASK-155: the folders the office makes for Git are owner-only (`0700`) whatever the
/// process's file-creation mask (a user-private-group account's `0002`); folders that exist
/// are left as they are.
#[test]
fn git_folders_are_created_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let tmp = tempfile::tempdir().unwrap();
    let existing = tmp.path().join("existing");
    fs::create_dir(&existing).unwrap();
    fs::set_permissions(&existing, fs::Permissions::from_mode(0o755)).unwrap();
    let nested = existing.join("teams/t1/agents/a1/tasks/k1/repositories");
    crate::create_git_dirs(&nested, false).unwrap();
    let mut at = existing.clone();
    for part in ["teams", "t1", "agents", "a1", "tasks", "k1", "repositories"] {
        at.push(part);
        assert_eq!(mode(&at), 0o700, "{}", at.display());
    }
    assert_eq!(mode(&existing), 0o755, "an existing folder is not changed");
    crate::create_git_dirs(&nested, false).unwrap();
    // A file in the way is an error naming the folder, never replaced.
    fs::write(tmp.path().join("file"), b"x").unwrap();
    assert!(matches!(
        crate::create_git_dirs(&tmp.path().join("file/below"), false),
        Err(GitError::Io { .. })
    ));
    assert_eq!(fs::read(tmp.path().join("file")).unwrap(), b"x");
}

/// TASK-155 review B1: the folders `RepoLocks::with_lock` makes above a cache (the office's
/// `repositories` folder and the office's runtime folder) are owner-only (`0700`) like the
/// cache itself, and its lock file is owner-only (`0600`), whatever the mask; an existing
/// lock file is locked again as it is.
#[test]
fn repository_locks_make_owner_only_folders_and_lock_files() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let tmp = tempfile::tempdir().unwrap();
    let runner = GitRunner::new(tmp.path().join("clean-home")).unwrap();
    let cache_dir = tmp.path().join("runtime/office-1/repositories/repo-1.git");
    let lock_file = tmp
        .path()
        .join("runtime/office-1/repositories/repo-1.git.lock");
    let locks = RepoLocks::new();
    locks
        .with_lock(&cache_dir, || init_bare_cache(&runner, &cache_dir, false))
        .unwrap();
    for folder in [
        cache_dir.clone(),
        tmp.path().join("runtime/office-1/repositories"),
        tmp.path().join("runtime/office-1"),
        tmp.path().join("runtime"),
    ] {
        assert_eq!(mode(&folder), 0o700, "{}", folder.display());
    }
    assert!(fs::symlink_metadata(&lock_file).unwrap().is_file());
    assert_eq!(mode(&lock_file), 0o600);
    // Taken again: the same lock file, not truncated or replaced.
    fs::write(&lock_file, b"kept").unwrap();
    locks.with_lock(&cache_dir, || Ok(())).unwrap();
    assert_eq!(fs::read(&lock_file).unwrap(), b"kept");
    assert_eq!(mode(&lock_file), 0o600);
}

#[test]
fn task135_https_token_scopes_to_exact_repo_url_never_writes_config_and_redacts_debug() {
    use base64::Engine as _;
    use zeroize::Zeroizing;

    let tmp = tempfile::tempdir().unwrap();
    let runner = GitRunner::new(tmp.path().join("clean-home")).unwrap();
    let secret = "ghp_TASK135_SCOPED_TOKEN_987654321";
    let cred = crate::GitCredential::HttpsToken {
        username: "x-access-token".to_owned(),
        token: Zeroizing::new(secret.to_owned()),
        scope_url: "https://github.com/acme/app".to_owned(),
    };
    let debug = format!("{cred:?}");
    assert!(
        !debug.contains(secret),
        "GitCredential Debug must redact token: {debug}"
    );
    assert!(debug.contains("<redacted>"), "{debug}");

    let expected_header = format!(
        "Authorization: Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{secret}"))
    );
    let urlmatch = |target: &str| {
        runner.run_checked(
            None,
            None,
            AllowedProtocol::Https,
            &cred,
            &["config", "--get-urlmatch", "http.extraHeader", target],
        )
    };
    // Exact repo URL and its `.git` variant both receive the scoped header.
    assert_eq!(
        urlmatch("https://github.com/acme/app").unwrap().trim(),
        expected_header
    );
    assert_eq!(
        urlmatch("https://github.com/acme/app.git").unwrap().trim(),
        expected_header
    );
    // Sibling repo on the same host and unrelated host MUST NOT receive the header.
    for other in [
        "https://github.com/acme/other.git",
        "https://github.com/acme/app-2.git",
        "https://evil.example/acme/app.git",
    ] {
        let res = urlmatch(other);
        assert!(
            res.is_err() || res.unwrap().trim().is_empty(),
            "scoped token must NOT match unrelated URL `{other}`"
        );
    }

    // Invalid token/username/scope values fail closed before invoking git.
    for bad in [
        crate::GitCredential::HttpsToken {
            username: "bad:user".to_owned(),
            token: Zeroizing::new("tok".to_owned()),
            scope_url: "https://github.com/acme/app.git".to_owned(),
        },
        crate::GitCredential::HttpsToken {
            username: "x-access-token".to_owned(),
            token: Zeroizing::new("tok\ninjected".to_owned()),
            scope_url: "https://github.com/acme/app.git".to_owned(),
        },
        crate::GitCredential::HttpsToken {
            username: "x-access-token".to_owned(),
            token: Zeroizing::new("tok".to_owned()),
            scope_url: "http://github.com/acme/app.git".to_owned(),
        },
        crate::GitCredential::HttpsToken {
            username: "x-access-token".to_owned(),
            token: Zeroizing::new("tok".to_owned()),
            scope_url: "https://user@github.com/acme/app.git".to_owned(),
        },
    ] {
        assert!(matches!(
            runner.run_checked(None, None, AllowedProtocol::Https, &bad, &["--version"]),
            Err(GitError::InvalidInput(_))
        ));
    }

    // Bare cache config and worktree `.git` contain zero tokens or extraHeader lines.
    let (remote_dir, base_sha) = seed_bare_remote(tmp.path());
    let cache_dir = tmp.path().join("cache.git");
    fetch_cache(
        &runner,
        &cache_dir,
        &RemoteSpec::local(&remote_dir).unwrap(),
        false,
    )
    .unwrap();
    let wt_dir = tmp.path().join("wt");
    create_worktree(
        &runner,
        &cache_dir,
        &wt_dir,
        "task/T-135-check",
        &base_sha,
        false,
    )
    .unwrap();
    let cache_cfg = fs::read_to_string(cache_dir.join("config")).unwrap();
    let wt_git = fs::read_to_string(wt_dir.join(".git")).unwrap();
    assert!(!cache_cfg.contains("extraHeader") && !cache_cfg.contains("credential"));
    assert!(!wt_git.contains("extraHeader") && !wt_git.contains(secret));
}

#[test]
fn task135_git_runner_isolates_host_gitconfig_and_never_inherits_host_credential_helpers() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = GitRunner::new(tmp.path().join("clean-home")).unwrap();

    // `git config --list --show-origin` under `GitRunner` must inherit ZERO global/system
    // config entries (no host `~/.gitconfig`, no host `gh auth git-credential` helper).
    let out = runner
        .run_checked(
            None,
            None,
            AllowedProtocol::Https,
            &crate::GitCredential::None,
            &["config", "--list", "--show-origin"],
        )
        .unwrap();
    assert!(
        !out.contains("gh auth git-credential")
            && !out.contains("osxkeychain")
            && !out.contains("libsecret"),
        "office git runner must never inherit host credential helpers: {out}"
    );
    assert!(
        !out.contains("file:"),
        "office git runner must disable host global/system gitconfig files: {out}"
    );
    // `credential.helper` must only contain the explicit empty reset (`credential.helper=`).
    let helpers = runner
        .run_checked(
            None,
            None,
            AllowedProtocol::Https,
            &crate::GitCredential::None,
            &["config", "--get-all", "credential.helper"],
        )
        .unwrap();
    assert!(
        helpers.lines().all(|line| line.trim().is_empty()),
        "credential.helper must stay empty when GitCredential::None is active: {helpers:?}"
    );
    let follow = runner
        .run_checked(
            None,
            None,
            AllowedProtocol::Https,
            &crate::GitCredential::None,
            &["config", "--get", "http.followRedirects"],
        )
        .unwrap();
    assert_eq!(
        follow.trim(),
        "false",
        "GitRunner must disable HTTP redirects to prevent credential replay"
    );
}

#[test]
fn task135_canonical_https_repo_scope_rejects_widening_traversal_and_bad_ports() {
    assert_eq!(
        crate::canonical_https_repo_scope("https://GitHub.COM:443/Acme/App.git/").unwrap(),
        "https://github.com:443/Acme/App.git"
    );
    for bad in [
        "https://github.com",
        "https://github.com/",
        "https://github.com/acme/..",
        "https://github.com/acme/repo/..",
        "https://github.com/acme/./repo",
        "https://github.com/acme//repo",
        "https://github.com/acme/%2e%2e/other",
        "https://github.com/acme/%2Fother",
        "https://github.com/acme/%5cother",
        "https://github.com:0/acme/repo",
        "https://github.com:99999/acme/repo",
        "https://github.com:abc/acme/repo",
        "https://user@github.com/acme/repo",
        "http://github.com/acme/repo",
    ] {
        assert!(
            crate::canonical_https_repo_scope(bad).is_err(),
            "expected `{bad}` to be rejected"
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    let runner = GitRunner::new(tmp.path().join("clean-home")).unwrap();
    let bad_cred = crate::GitCredential::HttpsToken {
        username: "user@host".into(),
        token: zeroize::Zeroizing::new("tok".into()),
        scope_url: "https://github.com/acme/repo.git".into(),
    };
    assert!(runner
        .run_checked(
            None,
            None,
            AllowedProtocol::Https,
            &bad_cred,
            &["--version"]
        )
        .is_err());
}
