//! `read_blob_lines` against a real bare cache: exact 1-based slices with a deterministic
//! snippet hash, clamping at the end of the file, and binary files, symbolic links,
//! submodules, directories, missing paths, traversal and oversized files refused.

use super::fixture::Fixture;
use crate::inspect::{
    anchor_snippet_sha256, read_blob_lines, BlobLines, BlobLinesRequest, MAX_BLOB_BYTES,
    MAX_BLOB_LINES,
};
use crate::GitError;

/// `sha256("l3\nl4\nl5\nl6\n")`, computed independently.
const L3_TO_L6_SHA256: &str = "050276e596256d403e5eccd121ee403e170ec5b6e5e4a7baf482d3a2cbfc43b3";

fn ten_lines() -> String {
    (1..=10).map(|n| format!("l{n}\n")).collect()
}

fn read(
    fx: &Fixture,
    commit_oid: &str,
    path: &str,
    start_line: u32,
    line_count: u32,
) -> Result<BlobLines, GitError> {
    read_blob_lines(
        &fx.cache(),
        &BlobLinesRequest {
            commit_oid,
            path,
            start_line,
            line_count,
        },
    )
}

#[test]
fn an_exact_slice_carries_a_deterministic_snippet_hash() {
    let fx = Fixture::new();
    let head = fx.commit(&[("src/ten.txt", ten_lines().as_bytes())], "Ten lines");
    let slice = read(&fx, &head, "src/ten.txt", 3, 4).unwrap();
    assert_eq!(slice.lines, vec!["l3", "l4", "l5", "l6"]);
    assert_eq!(
        (slice.start_line, slice.end_line, slice.total_lines),
        (3, 6, 10)
    );
    assert!(!slice.truncated);
    assert_eq!(slice.anchor_snippet_sha256, L3_TO_L6_SHA256);
    assert_eq!(anchor_snippet_sha256(&slice.lines), L3_TO_L6_SHA256);
    assert_eq!(
        fx.git(&["rev-parse", &format!("{head}:src/ten.txt")]),
        slice.blob_oid
    );
    // The same lines with CRLF endings, at another commit, fingerprint the same.
    let crlf = ten_lines().replace('\n', "\r\n");
    let next = fx.commit(&[("src/ten.txt", crlf.as_bytes())], "CRLF");
    let again = read(&fx, &next, "src/ten.txt", 3, 4).unwrap();
    assert_eq!(again.anchor_snippet_sha256, L3_TO_L6_SHA256);
    assert_ne!(again.blob_oid, slice.blob_oid);
    // An edited anchored line changes the fingerprint.
    let edited = ten_lines().replace("l4\n", "l4 changed\n");
    let edited_head = fx.commit(&[("src/ten.txt", edited.as_bytes())], "Edit");
    let moved = read(&fx, &edited_head, "src/ten.txt", 3, 4).unwrap();
    assert_ne!(moved.anchor_snippet_sha256, L3_TO_L6_SHA256);
}

#[test]
fn a_slice_past_the_end_stops_at_the_last_line() {
    let fx = Fixture::new();
    let head = fx.commit(&[("ten.txt", ten_lines().as_bytes())], "Ten lines");
    let tail = read(&fx, &head, "ten.txt", 9, 5).unwrap();
    assert_eq!(tail.lines, vec!["l9", "l10"]);
    assert_eq!((tail.end_line, tail.total_lines), (10, 10));
    assert!(!tail.truncated);
    let last = fx.commit(&[("no-newline.txt", b"one\ntwo")], "No final newline");
    let both = read(&fx, &last, "no-newline.txt", 1, MAX_BLOB_LINES).unwrap();
    assert_eq!(both.lines, vec!["one", "two"]);
    assert_eq!(both.total_lines, 2);
}

#[test]
fn out_of_range_slices_are_refused_cleanly() {
    let fx = Fixture::new();
    let head = fx.commit(
        &[("ten.txt", ten_lines().as_bytes()), ("empty.txt", b"")],
        "Files",
    );
    for (start, count) in [
        (11, 1),
        (u32::MAX, 1),
        (0, 1),
        (1, 0),
        (1, MAX_BLOB_LINES + 1),
    ] {
        assert!(
            matches!(
                read(&fx, &head, "ten.txt", start, count),
                Err(GitError::InvalidInput(_))
            ),
            "{start}+{count}"
        );
    }
    assert!(matches!(
        read(&fx, &head, "empty.txt", 1, 1),
        Err(GitError::InvalidInput(_))
    ));
}

#[test]
fn binary_files_are_refused() {
    let fx = Fixture::new();
    let head = fx.commit(&[("logo.bin", b"PNG\x00\x01\x02 data\n")], "Binary");
    let err = read(&fx, &head, "logo.bin", 1, 1).unwrap_err();
    assert!(
        matches!(&err, GitError::InvalidInput(m) if m.contains("binary")),
        "{err:?}"
    );
}

#[test]
fn symbolic_links_and_paths_through_them_are_refused() {
    let fx = Fixture::new();
    fx.commit(&[("src/real.txt", b"real\n")], "Real file");
    std::os::unix::fs::symlink("src/real.txt", fx.worktree.join("link.txt")).unwrap();
    std::os::unix::fs::symlink("src", fx.worktree.join("linkdir")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", fx.worktree.join("passwd")).unwrap();
    let head = fx.commit(&[], "Links");
    for (path, why) in [
        ("link.txt", "symbolic link"),
        ("passwd", "symbolic link"),
        ("linkdir/real.txt", "does not exist"),
        ("src", "directory"),
    ] {
        let err = read(&fx, &head, path, 1, 1).unwrap_err();
        assert!(
            matches!(&err, GitError::InvalidInput(m) if m.contains(why)),
            "{path}: {err:?}"
        );
    }
}

#[test]
fn submodule_entries_are_refused() {
    let fx = Fixture::new();
    let cacheinfo = format!("160000,{},vendor/lib", fx.base);
    fx.git(&["update-index", "--add", "--cacheinfo", &cacheinfo]);
    fx.git(&["commit", "-q", "-m", "Gitlink"]);
    let head = fx.git(&["rev-parse", "HEAD"]);
    let err = read(&fx, &head, "vendor/lib", 1, 1).unwrap_err();
    assert!(
        matches!(&err, GitError::InvalidInput(m) if m.contains("submodule")),
        "{err:?}"
    );
}

#[test]
fn missing_paths_and_traversal_are_refused() {
    let fx = Fixture::new();
    let head = fx.commit(&[("src/a.txt", b"a\n")], "A");
    for bad in [
        "missing.txt",
        "src/a.txt/deeper",
        "../etc/passwd",
        "src/../../etc/passwd",
        "/etc/passwd",
        "-p",
        ".git/config",
        "src/./a.txt",
        "src//a.txt",
        "src/a.txt\0",
        "*.txt",
        ":(glob)src/*",
    ] {
        assert!(
            matches!(read(&fx, &head, bad, 1, 1), Err(GitError::InvalidInput(_))),
            "{bad:?}"
        );
    }
    for bad_oid in ["HEAD", "--all", "main"] {
        assert!(matches!(
            read(&fx, bad_oid, "src/a.txt", 1, 1),
            Err(GitError::InvalidInput(_))
        ));
    }
    let tree = fx.git(&["rev-parse", &format!("{head}^{{tree}}")]);
    assert!(matches!(
        read(&fx, &tree, "src/a.txt", 1, 1),
        Err(GitError::InvalidInput(_))
    ));
}

#[test]
fn files_over_the_read_bound_are_refused() {
    let fx = Fixture::new();
    let size = usize::try_from(MAX_BLOB_BYTES).unwrap() + 1;
    let mut big = "y\n".repeat(size / 2 + 1).into_bytes();
    big.truncate(size);
    let head = fx.commit(&[("big.txt", &big)], "Big");
    let err = read(&fx, &head, "big.txt", 1, 1).unwrap_err();
    assert!(
        matches!(&err, GitError::InvalidInput(m) if m.contains("larger than")),
        "{err:?}"
    );
}
