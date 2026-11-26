//! `refresh_cache` against real `git`: the before/after `origin/*` summary, the origin `HEAD`,
//! untouched `refs/heads/*` and worktrees, unreachable origins, and parser edge cases.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{list_origin_branches, parse_origin_refs, parse_symref_head, refresh_cache};
use crate::{create_worktree, GitError, GitRunner, RemoteSpec};

fn git(dir: &Path, args: &[&str]) -> String {
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
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A bare origin with `main` and `old`, plus the clone used to push to it.
fn seed(tmp: &Path) -> (PathBuf, PathBuf) {
    let origin = tmp.join("origin.git");
    let seed = tmp.join("seed");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&seed).unwrap();
    git(&origin, &["init", "--bare", "-b", "main"]);
    git(&seed, &["init", "-b", "main"]);
    fs::write(seed.join("README.md"), "# Seed\n").unwrap();
    git(&seed, &["add", "README.md"]);
    git(&seed, &["commit", "-m", "initial"]);
    let url = origin.to_str().unwrap();
    git(
        &seed,
        &["push", url, "HEAD:refs/heads/main", "HEAD:refs/heads/old"],
    );
    (origin, seed)
}

fn heads(cache: &Path) -> String {
    git(
        cache,
        &[
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            "refs/heads",
        ],
    )
}

#[test]
fn refresh_reports_added_updated_and_pruned_origin_branches_and_leaves_heads_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let (origin, seed_dir) = seed(tmp.path());
    let runner = GitRunner::new(tmp.path().join("clean-home")).unwrap();
    let remote = RemoteSpec::local(&origin).unwrap();
    let cache = tmp.path().join("cache.git");

    // First refresh initializes the cache: everything is new.
    let first = refresh_cache(&runner, &cache, &remote, false).unwrap();
    assert_eq!(first.added, vec!["main".to_owned(), "old".to_owned()]);
    assert!(first.updated.is_empty() && first.pruned.is_empty());
    assert_eq!(first.default_branch.as_deref(), Some("main"));
    let main_before = git(&seed_dir, &["rev-parse", "HEAD"]);
    assert_eq!(first.head_oid(), Some(main_before.as_str()));

    // A task worktree with an uncommitted edit, on its own `refs/heads/task/*` branch.
    let wt = tmp.path().join("wt");
    create_worktree(&runner, &cache, &wt, "task/T-A1-x", &main_before, false).unwrap();
    fs::write(wt.join("README.md"), "# Local edit\n").unwrap();
    let heads_before = heads(&cache);

    // The origin gains `feature/new`, moves `main` and drops `old`.
    let url = origin.to_str().unwrap();
    fs::write(seed_dir.join("more.txt"), "more\n").unwrap();
    git(&seed_dir, &["add", "more.txt"]);
    git(&seed_dir, &["commit", "-m", "more"]);
    git(
        &seed_dir,
        &[
            "push",
            url,
            "HEAD:refs/heads/main",
            "HEAD:refs/heads/feature/new",
        ],
    );
    git(&seed_dir, &["push", url, ":refs/heads/old"]);
    let main_after = git(&seed_dir, &["rev-parse", "HEAD"]);

    let second = refresh_cache(&runner, &cache, &remote, false).unwrap();
    assert_eq!(second.added, vec!["feature/new".to_owned()]);
    assert_eq!(second.updated, vec!["main".to_owned()]);
    assert_eq!(second.pruned, vec!["old".to_owned()]);
    assert_eq!(second.head_oid(), Some(main_after.as_str()));
    assert_eq!(
        second.branches.keys().cloned().collect::<Vec<_>>(),
        vec!["feature/new".to_owned(), "main".to_owned()]
    );

    // Nothing under `refs/heads/*` moved; the worktree keeps its checkout and its edit.
    assert_eq!(heads(&cache), heads_before);
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), main_before);
    assert_eq!(git(&wt, &["status", "--porcelain"]), "M README.md");
    assert_eq!(
        fs::read_to_string(wt.join("README.md")).unwrap(),
        "# Local edit\n"
    );
    assert!(git(&wt, &["branch", "-r"]).contains("origin/feature/new"));

    // A refresh with nothing new reports no change.
    let third = refresh_cache(&runner, &cache, &remote, false).unwrap();
    assert!(third.added.is_empty() && third.updated.is_empty() && third.pruned.is_empty());
}

#[test]
fn unreachable_origin_fails_as_remote_unavailable_and_keeps_cached_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let (origin, _seed) = seed(tmp.path());
    let runner = GitRunner::new(tmp.path().join("clean-home")).unwrap();
    let remote = RemoteSpec::local(&origin).unwrap();
    let cache = tmp.path().join("cache.git");
    refresh_cache(&runner, &cache, &remote, false).unwrap();
    let cached = list_origin_branches(&runner, &cache).unwrap();

    fs::remove_dir_all(&origin).unwrap();
    let err = refresh_cache(&runner, &cache, &remote, false).unwrap_err();
    assert!(matches!(err, GitError::RemoteUnavailable(_)), "{err:?}");
    assert_eq!(list_origin_branches(&runner, &cache).unwrap(), cached);

    // A cache that does not exist yet lists no branches rather than failing.
    let missing = tmp.path().join("missing.git");
    assert!(list_origin_branches(&runner, &missing).unwrap().is_empty());
}

#[test]
fn hostile_cache_config_is_refused_before_any_ref_is_read() {
    let tmp = tempfile::tempdir().unwrap();
    let (origin, _seed) = seed(tmp.path());
    let runner = GitRunner::new(tmp.path().join("clean-home")).unwrap();
    let remote = RemoteSpec::local(&origin).unwrap();
    let cache = tmp.path().join("cache.git");
    refresh_cache(&runner, &cache, &remote, false).unwrap();
    fs::write(
        cache.join("config"),
        "[core]\n\tbare = true\n\tfsmonitor = /tmp/evil\n",
    )
    .unwrap();
    assert!(matches!(
        refresh_cache(&runner, &cache, &remote, false),
        Err(GitError::SecurityViolation(_))
    ));
}

#[test]
fn parsers_ignore_foreign_lines_symbolic_heads_and_non_branch_targets() {
    let refs = parse_origin_refs(
        "aaaa refs/remotes/origin/main\nbbbb refs/remotes/origin/HEAD\ncccc refs/remotes/upstream/x\n\
         garbage\ndddd refs/remotes/origin/\neeee refs/remotes/origin/feat/ü",
    );
    let expected: BTreeMap<String, String> = [("feat/ü", "eeee"), ("main", "aaaa")]
        .into_iter()
        .map(|(b, o)| (b.to_owned(), o.to_owned()))
        .collect();
    assert_eq!(refs, expected);

    assert_eq!(
        parse_symref_head("ref: refs/heads/trunk\tHEAD\n0123\tHEAD\n").as_deref(),
        Some("trunk")
    );
    for odd in [
        "",
        "0123\tHEAD\n",
        "ref: refs/tags/v1\tHEAD\n",
        "ref: refs/heads/\tHEAD\n",
        "ref: refs/heads/main\trefs/heads/other\n",
        "ref: refs/heads/main HEAD\n",
    ] {
        assert_eq!(parse_symref_head(odd), None, "{odd:?}");
    }
}
