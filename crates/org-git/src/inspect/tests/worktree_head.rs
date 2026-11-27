//! `resolve_worktree_head` against a real task worktree: clean, modified, staged, untracked,
//! detached, no index rewrite, canonical links restored, hostile cache config refused.

use std::fs;
use std::time::{Duration, SystemTime};

use super::fixture::{git, Fixture, BRANCH};
use crate::inspect::worktree_head::parse_status;
use crate::inspect::{resolve_worktree_head, WorktreeChanges};
use crate::GitError;

fn head(fx: &Fixture) -> crate::inspect::WorktreeHead {
    resolve_worktree_head(&fx.cache(), &fx.admin, &fx.worktree).unwrap()
}

#[test]
fn a_clean_worktree_reports_its_exact_head_and_branch() {
    let fx = Fixture::new();
    let head = head(&fx);
    assert_eq!(head.head_oid, fx.base);
    assert_eq!(head.branch.as_deref(), Some(BRANCH));
    assert!(!head.dirty);
    assert!(head.changes.is_clean());

    let next = fx.commit(&[("src/lib.rs", b"pub fn a() {}\n")], "Add lib");
    let after = resolve_worktree_head(&fx.cache(), &fx.admin, &fx.worktree).unwrap();
    assert_eq!(after.head_oid, next);
    assert!(!after.dirty);
}

#[test]
fn a_modified_tracked_file_is_an_unstaged_change() {
    let fx = Fixture::new();
    fs::write(fx.worktree.join("README.md"), "# Changed\n").unwrap();
    let head = head(&fx);
    assert!(head.dirty);
    assert_eq!(
        head.changes,
        WorktreeChanges {
            unstaged: 1,
            ..WorktreeChanges::default()
        }
    );
    assert_eq!(head.head_oid, fx.base);
}

#[test]
fn a_deleted_tracked_file_is_an_unstaged_change() {
    let fx = Fixture::new();
    fs::remove_file(fx.worktree.join("README.md")).unwrap();
    let head = head(&fx);
    assert!(head.dirty);
    assert_eq!(head.changes.unstaged, 1);
}

#[test]
fn a_staged_file_is_a_staged_change() {
    let fx = Fixture::new();
    fs::write(fx.worktree.join("new.txt"), "staged\n").unwrap();
    fx.git(&["add", "new.txt"]);
    let head = head(&fx);
    assert!(head.dirty);
    assert_eq!(
        head.changes,
        WorktreeChanges {
            staged: 1,
            ..WorktreeChanges::default()
        }
    );
}

#[test]
fn a_file_staged_then_edited_again_counts_both_ways() {
    let fx = Fixture::new();
    fs::write(fx.worktree.join("README.md"), "# Staged\n").unwrap();
    fx.git(&["add", "README.md"]);
    fs::write(fx.worktree.join("README.md"), "# Edited after staging\n").unwrap();
    let changes = head(&fx).changes;
    assert_eq!((changes.staged, changes.unstaged), (1, 1));
}

#[test]
fn untracked_files_are_counted_one_by_one_even_in_new_folders() {
    let fx = Fixture::new();
    fs::create_dir_all(fx.worktree.join("notes/deep")).unwrap();
    fs::write(fx.worktree.join("notes/a.md"), "a\n").unwrap();
    fs::write(fx.worktree.join("notes/deep/b.md"), "b\n").unwrap();
    let head = head(&fx);
    assert!(head.dirty);
    assert_eq!(
        head.changes,
        WorktreeChanges {
            untracked: 2,
            ..WorktreeChanges::default()
        }
    );
}

#[test]
fn ignored_files_leave_the_worktree_clean() {
    let fx = Fixture::new();
    fx.commit(&[(".gitignore", b"target/\n")], "Ignore target");
    fs::create_dir_all(fx.worktree.join("target")).unwrap();
    fs::write(fx.worktree.join("target/out.bin"), "x").unwrap();
    assert!(!head(&fx).dirty);
}

#[test]
fn a_detached_head_has_no_branch() {
    let fx = Fixture::new();
    fx.git(&["checkout", "-q", "--detach"]);
    let head = head(&fx);
    assert_eq!(head.branch, None);
    assert_eq!(head.head_oid, fx.base);
    assert!(!head.dirty);
}

#[test]
fn reading_status_never_rewrites_the_index() {
    let fx = Fixture::new();
    // A newer mtime with the same content makes a locking `git status` refresh the index.
    let readme = fs::File::options()
        .write(true)
        .open(fx.worktree.join("README.md"))
        .unwrap();
    readme
        .set_modified(SystemTime::now() + Duration::from_secs(120))
        .unwrap();
    drop(readme);
    let index = fx.admin_dir().join("index");
    let before = fs::read(&index).unwrap();
    let head = head(&fx);
    assert!(!head.dirty, "same content is not a change");
    assert_eq!(fs::read(&index).unwrap(), before);
    assert!(!fx.admin_dir().join("index.lock").exists());
}

#[test]
fn tampered_worktree_links_are_restored_before_status_runs() {
    let fx = Fixture::new();
    let decoy = fx.tmp.path().join("decoy.git");
    fs::create_dir_all(&decoy).unwrap();
    fs::write(
        fx.admin_dir().join("commondir"),
        format!("{}\n", decoy.display()),
    )
    .unwrap();
    let head = head(&fx);
    assert_eq!(head.head_oid, fx.base);
    assert_eq!(
        fs::read_to_string(fx.admin_dir().join("commondir")).unwrap(),
        "../..\n"
    );
}

#[test]
fn a_hostile_cache_config_is_refused_before_status_runs() {
    let fx = Fixture::new();
    let marker = fx.tmp.path().join("filter-ran");
    fs::write(
        fx.cache_dir.join("config"),
        format!(
            "[core]\n\tbare = true\n[filter \"x\"]\n\tclean = touch {}\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::write(fx.worktree.join(".gitattributes"), "* filter=x\n").unwrap();
    let err = resolve_worktree_head(&fx.cache(), &fx.admin, &fx.worktree).unwrap_err();
    assert!(matches!(err, GitError::SecurityViolation(_)), "{err:?}");
    assert!(!marker.exists());
}

/// A submodule checkout inside the worktree whose own config holds a filter driver that
/// would leave `marker` behind if Git ever ran inside it.
fn hostile_submodule(fx: &Fixture) -> (std::path::PathBuf, std::path::PathBuf) {
    let sub = fx.worktree.join("sub");
    fs::create_dir_all(&sub).unwrap();
    git(&sub, &["init", "-q", "-b", "main"]);
    fs::write(sub.join("s.txt"), "s\n").unwrap();
    git(&sub, &["add", "s.txt"]);
    git(&sub, &["commit", "-q", "-m", "Submodule"]);
    fx.git(&["add", "sub"]);
    fx.git(&["commit", "-q", "-m", "Add submodule"]);
    let marker = fx.tmp.path().join("filter-ran");
    let clean = format!("touch {}; cat", marker.display());
    git(&sub, &["config", "filter.x.clean", &clean]);
    fs::write(sub.join(".gitattributes"), "* filter=x\n").unwrap();
    // A newer mtime makes a `git status` inside the submodule re-read the file through it.
    fs::File::options()
        .write(true)
        .open(sub.join("s.txt"))
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(120))
        .unwrap();
    (sub, marker)
}

#[test]
fn git_never_runs_inside_a_submodule_the_worktree_controls() {
    let fx = Fixture::new();
    let (_, marker) = hostile_submodule(&fx);
    let head = head(&fx);
    assert!(!marker.exists(), "a submodule's filter driver ran");
    // Uncommitted work inside a submodule is not part of the pinned commit.
    assert!(!head.dirty, "{head:?}");
}

#[test]
fn a_moved_submodule_commit_makes_the_worktree_dirty() {
    let fx = Fixture::new();
    let (sub, marker) = hostile_submodule(&fx);
    // The test's own Git in the submodule overrides the hostile filter while setting up.
    git(&sub, &["-c", "filter.x.clean=cat", "add", "-A"]);
    git(
        &sub,
        &["-c", "filter.x.clean=cat", "commit", "-q", "-m", "Moved"],
    );
    let _ = fs::remove_file(&marker);
    let head = head(&fx);
    assert!(head.dirty);
    assert_eq!(head.changes.unstaged, 1);
    assert!(!marker.exists());
}

#[test]
fn an_invalid_admin_directory_name_is_refused() {
    let fx = Fixture::new();
    for bad in ["", "..", "a/b"] {
        assert!(
            resolve_worktree_head(&fx.cache(), bad, &fx.worktree).is_err(),
            "{bad:?}"
        );
    }
}

const OID: &str = "0123456789abcdef0123456789abcdef01234567";

fn listing(records: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        out.extend_from_slice(record.as_bytes());
        out.push(0);
    }
    out
}

#[test]
fn status_parsing_counts_every_record_kind() {
    let oid_header = format!("# branch.oid {OID}");
    let out = listing(&[
        &oid_header,
        "# branch.head task/T-A1-x",
        "1 M. N... 100644 100644 100644 a b staged.rs",
        "1 .M N... 100644 100644 100644 a b unstaged.rs",
        "1 MD N... 100644 100644 000000 a b both.rs",
        "2 R. N... 100644 100644 100644 a b R100 new.rs",
        "old.rs",
        "u UU N... 100644 100644 100644 100644 a b c conflict.rs",
        "? untracked.rs",
        "! ignored.rs",
    ]);
    let head = parse_status(&out, false).unwrap();
    assert_eq!(head.head_oid, OID);
    assert_eq!(head.branch.as_deref(), Some("task/T-A1-x"));
    assert_eq!(
        head.changes,
        WorktreeChanges {
            staged: 3,
            unstaged: 2,
            untracked: 1,
            unmerged: 1,
            truncated: false,
        }
    );
}

#[test]
fn a_truncated_listing_is_dirty_and_drops_its_partial_record() {
    let oid_header = format!("# branch.oid {OID}");
    let mut out = listing(&[&oid_header, "# branch.head main"]);
    out.extend_from_slice(b"1 .M N... 100644 100644 1006");
    let head = parse_status(&out, true).unwrap();
    assert!(head.dirty);
    assert!(head.changes.truncated);
    assert_eq!(head.changes.unstaged, 0);
}

#[test]
fn status_without_a_head_commit_is_refused() {
    let initial = listing(&["# branch.oid (initial)", "# branch.head main"]);
    assert!(matches!(
        parse_status(&initial, false),
        Err(GitError::Conflict(_))
    ));
    assert!(parse_status(&listing(&["# branch.head main"]), false).is_err());
    assert!(parse_status(b"", false).is_err());
    let bad_oid = listing(&["# branch.oid HEAD~1"]);
    assert!(matches!(
        parse_status(&bad_oid, false),
        Err(GitError::InvalidInput(_))
    ));
}

#[test]
fn unknown_or_malformed_status_records_fail_closed() {
    let oid_header = format!("# branch.oid {OID}");
    assert!(parse_status(&listing(&[&oid_header, "Z what"]), false).is_err());
    assert!(parse_status(&listing(&[&oid_header, "1 M"]), false).is_err());
}
