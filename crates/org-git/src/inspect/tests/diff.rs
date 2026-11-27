//! `diff_commits` against a real bare cache: first-round and inter-round diffs, summary-only,
//! a path filter, binary files, the 256 KiB cap on a character boundary, the file-count cap,
//! and argument injection, revision expressions and traversal refused before Git runs.

use super::fixture::Fixture;
use crate::inspect::{
    diff_commits, CommitDiff, DiffCommitsRequest, DiffFileStat, DiffFileStatus, MAX_DIFF_BYTES,
    MAX_DIFF_FILES,
};
use crate::{AllowedProtocol, GitCredential, GitError};

fn diff(fx: &Fixture, req: &DiffCommitsRequest<'_>) -> CommitDiff {
    diff_commits(&fx.cache(), req).unwrap()
}

fn stat(path: &str, status: DiffFileStatus, added: u64, deleted: u64) -> DiffFileStat {
    DiffFileStat {
        path: path.into(),
        status,
        added,
        deleted,
        binary: false,
    }
}

/// Round 1 adds `src/lib.rs` and edits the README; round 2 edits `src/lib.rs` again and
/// deletes nothing else.
fn two_rounds(fx: &Fixture) -> (String, String) {
    let c1 = fx.commit(
        &[
            ("src/lib.rs", b"pub fn one() {}\n"),
            ("README.md", b"# Seed\n\nfirst\nchanged\n"),
        ],
        "Round 1",
    );
    let c2 = fx.commit(
        &[("src/lib.rs", b"pub fn one() {}\npub fn two() {}\n")],
        "Round 2",
    );
    (c1, c2)
}

#[test]
fn a_first_round_diff_covers_every_change_since_the_base() {
    let fx = Fixture::new();
    let (c1, _) = two_rounds(&fx);
    let out = diff(&fx, &DiffCommitsRequest::new(&fx.base, &c1));
    assert_eq!(out.base_oid, fx.base);
    assert_eq!(out.head_oid, c1);
    assert_eq!(
        out.files,
        vec![
            stat("README.md", DiffFileStatus::Modified, 1, 1),
            stat("src/lib.rs", DiffFileStatus::Added, 1, 0),
        ]
    );
    assert_eq!(out.total_files, 2);
    assert!(!out.files_truncated && !out.truncated);
    assert!(out
        .unified_diff
        .contains("diff --git a/README.md b/README.md"));
    assert!(out.unified_diff.contains("-second\n+changed\n"));
    assert!(out.unified_diff.contains("+pub fn one() {}\n"));
}

#[test]
fn an_inter_round_diff_shows_only_the_later_round() {
    let fx = Fixture::new();
    let (c1, c2) = two_rounds(&fx);
    let out = diff(&fx, &DiffCommitsRequest::new(&c1, &c2));
    assert_eq!(
        out.files,
        vec![stat("src/lib.rs", DiffFileStatus::Modified, 1, 0)]
    );
    assert!(out.unified_diff.contains("+pub fn two() {}\n"));
    assert!(!out.unified_diff.contains("README.md"));
}

#[test]
fn a_summary_only_diff_has_counts_but_no_text() {
    let fx = Fixture::new();
    let (_, c2) = two_rounds(&fx);
    let req = DiffCommitsRequest {
        stat_only: true,
        ..DiffCommitsRequest::new(&fx.base, &c2)
    };
    let out = diff(&fx, &req);
    assert_eq!(out.files.len(), 2);
    assert_eq!(
        out.files[1],
        stat("src/lib.rs", DiffFileStatus::Added, 2, 0)
    );
    assert!(out.unified_diff.is_empty());
    assert!(!out.truncated);
}

#[test]
fn a_path_filter_limits_both_the_summary_and_the_text() {
    let fx = Fixture::new();
    let (_, c2) = two_rounds(&fx);
    for filter in ["src/lib.rs", "src"] {
        let req = DiffCommitsRequest {
            path_filter: Some(filter),
            ..DiffCommitsRequest::new(&fx.base, &c2)
        };
        let out = diff(&fx, &req);
        assert_eq!(
            out.files,
            vec![stat("src/lib.rs", DiffFileStatus::Added, 2, 0)]
        );
        assert!(!out.unified_diff.contains("README.md"), "{filter}");
    }
    // Globs are literal: they match nothing.
    let req = DiffCommitsRequest {
        path_filter: Some("*.md"),
        ..DiffCommitsRequest::new(&fx.base, &c2)
    };
    assert!(diff(&fx, &req).files.is_empty());
    // Pathspec magic is refused before Git runs.
    for magic in [
        ":(glob)*",
        ":(icase)README.md",
        ":(attr:text)*",
        ":!src",
        ":/README.md",
        ":colon",
    ] {
        let req = DiffCommitsRequest {
            path_filter: Some(magic),
            ..DiffCommitsRequest::new(&fx.base, &c2)
        };
        assert!(
            matches!(
                diff_commits(&fx.cache(), &req),
                Err(GitError::InvalidInput(_))
            ),
            "{magic}"
        );
    }
}

#[test]
fn deletions_and_type_changes_are_reported() {
    let fx = Fixture::new();
    let c1 = fx.commit(&[("a.txt", b"a\n"), ("b.txt", b"b\n")], "Add");
    fx.git(&["rm", "-q", "a.txt"]);
    std::fs::remove_file(fx.worktree.join("b.txt")).unwrap();
    std::os::unix::fs::symlink("README.md", fx.worktree.join("b.txt")).unwrap();
    let c2 = fx.commit(&[], "Delete and retype");
    let out = diff(&fx, &DiffCommitsRequest::new(&c1, &c2));
    let statuses: Vec<_> = out
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.status))
        .collect();
    assert_eq!(
        statuses,
        vec![
            ("a.txt", DiffFileStatus::Deleted),
            ("b.txt", DiffFileStatus::TypeChanged)
        ]
    );
    assert_eq!(DiffFileStatus::TypeChanged.as_str(), "type_changed");
}

#[test]
fn a_binary_file_is_flagged_and_its_bytes_never_appear() {
    let fx = Fixture::new();
    let mut blob = b"\x00\x01\x02BINARY-PAYLOAD\xff\xfe".to_vec();
    blob.extend(std::iter::repeat_n(0u8, 64));
    let c1 = fx.commit(
        &[("assets/logo.bin", &blob), ("notes.txt", b"text\n")],
        "Add binary",
    );
    let out = diff(&fx, &DiffCommitsRequest::new(&fx.base, &c1));
    let binary = out
        .files
        .iter()
        .find(|f| f.path == "assets/logo.bin")
        .unwrap();
    assert!(binary.binary);
    assert_eq!((binary.added, binary.deleted), (0, 0));
    assert!(
        !out.files
            .iter()
            .find(|f| f.path == "notes.txt")
            .unwrap()
            .binary
    );
    assert!(out
        .unified_diff
        .contains("Binary files /dev/null and b/assets/logo.bin differ"));
    assert!(!out.unified_diff.contains("BINARY-PAYLOAD"));
    assert!(!out.unified_diff.contains('\0'));
}

#[test]
fn context_lines_are_honoured_and_bounded() {
    let fx = Fixture::new();
    let body: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    let c1 = fx.commit(&[("long.txt", body.as_bytes())], "Long");
    let edited = body.replace("line 15\n", "line fifteen\n");
    let c2 = fx.commit(&[("long.txt", edited.as_bytes())], "Edit");
    let with = |context_lines| DiffCommitsRequest {
        context_lines,
        ..DiffCommitsRequest::new(&c1, &c2)
    };
    let none = diff(&fx, &with(0)).unified_diff;
    assert!(none.contains("@@ -15 +15 @@"));
    assert!(!none.contains("\n line 14\n"));
    let wide = diff(&fx, &with(20)).unified_diff;
    assert!(wide.contains(" line 1\n") && wide.contains(" line 30\n"));
    assert!(matches!(
        diff_commits(&fx.cache(), &with(21)),
        Err(GitError::InvalidInput(_))
    ));
}

#[test]
fn a_diff_over_256_kib_is_cut_on_a_character_boundary() {
    let fx = Fixture::new();
    let full_patch = |base: &str, head: &str| -> String {
        fx.runner
            .run_checked(
                Some(&fx.cache_dir),
                None,
                AllowedProtocol::None,
                &GitCredential::None,
                &[
                    "diff-tree",
                    "-r",
                    "-p",
                    "--no-renames",
                    "--no-color",
                    base,
                    head,
                    "--",
                ],
            )
            .unwrap()
    };
    // A single line of 4-byte characters; shifting it by 0..3 ASCII bytes guarantees one
    // variant puts the 256 KiB cut inside a character.
    let mut chosen = None;
    for pad in 0..4 {
        let content = format!("{}{}\n", "x".repeat(pad), "😀".repeat(80 * 1024));
        let head = fx.commit(&[("emoji.txt", content.as_bytes())], &format!("Pad {pad}"));
        let full = full_patch(&fx.base, &head);
        assert!(full.len() > MAX_DIFF_BYTES);
        if !full.is_char_boundary(MAX_DIFF_BYTES) {
            chosen = Some((head, full));
            break;
        }
    }
    let (head, full) = chosen.expect("one padding cuts inside a character");
    let out = diff(&fx, &DiffCommitsRequest::new(&fx.base, &head));
    assert!(out.truncated);
    assert!(out.unified_diff.len() < MAX_DIFF_BYTES);
    assert!(out.unified_diff.len() > MAX_DIFF_BYTES - 4);
    assert!(full.starts_with(&out.unified_diff));
    assert!(out.unified_diff.ends_with('😀'));
    assert_eq!(
        out.files,
        vec![stat("emoji.txt", DiffFileStatus::Added, 1, 0)]
    );
}

#[test]
fn more_files_than_the_cap_are_counted_but_not_listed() {
    let fx = Fixture::new();
    let names: Vec<String> = (0..=MAX_DIFF_FILES)
        .map(|n| format!("many/f{n:04}.txt"))
        .collect();
    let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x\n"[..])).collect();
    let head = fx.commit(&files, "Many files");
    let req = DiffCommitsRequest {
        stat_only: true,
        ..DiffCommitsRequest::new(&fx.base, &head)
    };
    let out = diff(&fx, &req);
    assert_eq!(out.files.len(), MAX_DIFF_FILES);
    assert_eq!(out.total_files, MAX_DIFF_FILES as u64 + 1);
    assert!(out.files_truncated);
}

#[test]
fn revision_expressions_and_option_injection_are_refused() {
    let fx = Fixture::new();
    let (c1, c2) = two_rounds(&fx);
    let range = format!("{c1}..{c2}");
    let parent = format!("{c2}~1");
    let upper = c2.to_uppercase();
    let with_nul = format!("{}\0", &c2[..39]);
    for bad in [
        "--all",
        "-p",
        "HEAD",
        "main",
        range.as_str(),
        parent.as_str(),
        "HEAD~1",
        upper.as_str(),
        &c2[..39],
        with_nul.as_str(),
        "",
    ] {
        for req in [
            DiffCommitsRequest::new(bad, &c2),
            DiffCommitsRequest::new(&c1, bad),
        ] {
            assert!(
                matches!(
                    diff_commits(&fx.cache(), &req),
                    Err(GitError::InvalidInput(_))
                ),
                "{bad:?}"
            );
        }
    }
}

#[test]
fn well_formed_oids_that_are_not_commits_are_refused() {
    let fx = Fixture::new();
    let (c1, c2) = two_rounds(&fx);
    let tree = fx.git(&["rev-parse", &format!("{c2}^{{tree}}")]);
    let blob = fx.git(&["rev-parse", &format!("{c2}:src/lib.rs")]);
    fx.git(&["tag", "-a", "-m", "annotated", "v1", &c2]);
    let tag = fx.git(&["rev-parse", "refs/tags/v1"]);
    let missing = "0".repeat(40);
    let sha256_len = "a".repeat(64);
    for bad in [&tree, &blob, &tag, &missing, &sha256_len] {
        assert!(
            matches!(
                diff_commits(&fx.cache(), &DiffCommitsRequest::new(&c1, bad)),
                Err(GitError::InvalidInput(_))
            ),
            "{bad}"
        );
    }
}

#[test]
fn traversal_and_option_shaped_path_filters_are_refused() {
    let fx = Fixture::new();
    let (c1, c2) = two_rounds(&fx);
    for bad in [
        "../etc/passwd",
        "src/../../etc/passwd",
        "/etc/passwd",
        "-p",
        "--output=/tmp/x",
        "src/",
        "src//lib.rs",
        "./src",
        ".git/config",
        "src/.GIT/x",
        "a\0b",
        "a\nb",
        "a\\b",
        "",
    ] {
        let req = DiffCommitsRequest {
            path_filter: Some(bad),
            ..DiffCommitsRequest::new(&c1, &c2)
        };
        assert!(
            matches!(
                diff_commits(&fx.cache(), &req),
                Err(GitError::InvalidInput(_))
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn a_hostile_cache_config_is_refused_before_diffing() {
    let fx = Fixture::new();
    let (c1, c2) = two_rounds(&fx);
    std::fs::write(
        fx.cache_dir.join("config"),
        "[core]\n\tbare = true\n[diff]\n\texternal = /bin/false\n",
    )
    .unwrap();
    assert!(matches!(
        diff_commits(&fx.cache(), &DiffCommitsRequest::new(&c1, &c2)),
        Err(GitError::SecurityViolation(_))
    ));
}
