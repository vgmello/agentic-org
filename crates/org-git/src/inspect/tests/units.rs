//! Pure pieces of inspection: the SHA-256 digest against NIST and padding-edge vectors, the
//! repository path validator, the line slicer's bounds and the diff summary parser.

use crate::inspect::blob_lines::slice_lines;
use crate::inspect::diff::parse_summary;
use crate::inspect::sha256::Sha256;
use crate::inspect::{
    anchor_snippet_sha256, validate_repo_path, DiffFileStatus, MAX_BLOB_SLICE_BYTES,
    MAX_REPO_PATH_BYTES,
};
use crate::GitError;

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finish_hex()
}

#[test]
fn sha256_matches_the_nist_vectors() {
    for (message, expected) in [
        (&b""[..], "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        (b"abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
        (
            b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
        ),
    ] {
        assert_eq!(sha256_hex(message), expected);
    }
    let million = vec![b'a'; 1_000_000];
    assert_eq!(
        sha256_hex(&million),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn sha256_pads_every_length_around_a_block_edge() {
    for (len, expected) in [
        (
            55,
            "463eb28e72f82e0a96c0a4cc53690c571281131f672aa229e0d45ae59b598b59",
        ),
        (
            56,
            "da2ae4d6b36748f2a318f23e7ab1dfdf45acdc9d049bd80e59de82a60895f562",
        ),
        (
            63,
            "29af2686fd53374a36b0846694cc342177e428d1647515f078784d69cdb9e488",
        ),
        (
            64,
            "fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108",
        ),
        (
            65,
            "4bfd2c8b6f1eec7a2afeb48b934ee4b2694182027e6d0fc075074f2fabb31781",
        ),
        (
            119,
            "da18797ed7c3a777f0847f429724a2d8cd5138e6ed2895c3fa1a6d39d18f7ec6",
        ),
        (
            120,
            "f52b23db1fbb6ded89ef42a23ce0c8922c45f25c50b568a93bf1c075420bbb7c",
        ),
        (
            128,
            "471fb943aa23c511f6f72f8d1652d9c880cfa392ad80503120547703e56a2be5",
        ),
    ] {
        let message: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        assert_eq!(sha256_hex(&message), expected, "length {len}");
        // Fed in uneven pieces, the digest is the same.
        let mut hasher = Sha256::new();
        for piece in message.chunks(7) {
            hasher.update(piece);
        }
        assert_eq!(hasher.finish_hex(), expected, "length {len} in pieces");
    }
}

#[test]
fn snippet_hashes_ignore_line_terminators_only() {
    let expected = sha256_hex(b"a\nb\n");
    assert_eq!(anchor_snippet_sha256(&["a", "b"]), expected);
    assert_eq!(anchor_snippet_sha256(&["a\n", "b\r\n"]), expected);
    assert_ne!(anchor_snippet_sha256(&["a ", "b"]), expected);
    assert_ne!(anchor_snippet_sha256(&["a", "b", ""]), expected);
    assert_eq!(anchor_snippet_sha256::<&str>(&[]), sha256_hex(b""));
}

#[test]
fn repo_paths_are_accepted_only_when_normalized_and_relative() {
    for good in [
        "README.md",
        "src/lib.rs",
        "a/b/c.d",
        "dir",
        ".github/x.yml",
        "é/ü.txt",
        "a/:colon",
    ] {
        assert_eq!(validate_repo_path(good).unwrap(), good);
    }
    let too_long = "a".repeat(MAX_REPO_PATH_BYTES + 1);
    for bad in [
        "",
        "/abs",
        "-flag",
        "--all",
        "../up",
        "a/../b",
        "a/..",
        "./a",
        "a/.",
        "a//b",
        "a/",
        ".git",
        "a/.Git/b",
        "a\0b",
        "a\tb",
        "a\nb",
        "a\u{7f}b",
        "a\\b",
        ":colon",
        ":(glob)*",
        ":(icase)README.md",
        ":(attr:text)*",
        ":!src",
        ":/README.md",
        too_long.as_str(),
    ] {
        assert!(
            matches!(validate_repo_path(bad), Err(GitError::InvalidInput(_))),
            "{bad:?}"
        );
    }
    assert!(validate_repo_path(&"a".repeat(MAX_REPO_PATH_BYTES)).is_ok());
}

#[test]
fn slices_stop_before_a_line_that_breaks_the_byte_bound() {
    let long = "x".repeat(MAX_BLOB_SLICE_BYTES / 2);
    let blob = format!("short\n{long}\n{long}\ntail\n");
    let slice = slice_lines(blob.as_bytes(), 1, 4).unwrap();
    assert!(slice.truncated);
    assert_eq!(slice.lines.len(), 2);
    assert_eq!(slice.end_line, 2);
    assert_eq!(slice.total_lines, 4);
    assert_eq!(slice.sha256, anchor_snippet_sha256(&slice.lines));
    // A first line alone over the bound is refused rather than cut.
    let huge = format!("{}\n", "y".repeat(MAX_BLOB_SLICE_BYTES));
    assert!(matches!(
        slice_lines(huge.as_bytes(), 1, 1),
        Err(GitError::InvalidInput(_))
    ));
}

#[test]
fn non_utf8_lines_are_shown_lossily_but_hashed_raw() {
    let blob = b"caf\xe9\nok\n";
    let slice = slice_lines(blob, 1, 2).unwrap();
    assert_eq!(slice.lines, vec!["caf\u{FFFD}", "ok"]);
    assert_eq!(slice.sha256, sha256_hex(b"caf\xe9\nok\n"));
    assert_ne!(slice.sha256, anchor_snippet_sha256(&slice.lines));
}

#[test]
fn slicing_an_empty_blob_or_past_its_end_is_refused() {
    assert!(slice_lines(b"", 1, 1).is_err());
    assert!(slice_lines(b"one\n", 2, 1).is_err());
    let only = slice_lines(b"one\n", 1, 500).unwrap();
    assert_eq!((only.end_line, only.total_lines), (1, 1));
}

#[test]
fn summary_parsing_joins_raw_and_numstat_records() {
    let out = b":000000 100644 0000000 1111111 A\0new.txt\0\
:100644 100644 2222222 3333333 M\0tab\tname.rs\0\
:100644 000000 4444444 0000000 D\0gone.bin\0\
3\t0\tnew.txt\0\
1\t2\ttab\tname.rs\0\
-\t-\tgone.bin\0";
    let files = parse_summary(out).unwrap();
    let rows: Vec<_> = files
        .iter()
        .map(|f| (f.path.as_str(), f.status, f.added, f.deleted, f.binary))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("new.txt", DiffFileStatus::Added, 3, 0, false),
            ("tab\tname.rs", DiffFileStatus::Modified, 1, 2, false),
            ("gone.bin", DiffFileStatus::Deleted, 0, 0, true),
        ]
    );
    assert!(parse_summary(b"").unwrap().is_empty());
}

#[test]
fn malformed_summaries_fail_closed() {
    for bad in [
        &b":000000 100644 0000000 1111111 R100\0a\0"[..],
        b":000000 100644 0000000 1111111 AM\0a\0",
        b":000000 100644 0000000 1111111 A\0",
        b"1\t2\tunknown.txt\0",
        b":000000 100644 0000000 1111111 A\0a\0x\t1\ta\0",
        b"just text\0",
    ] {
        assert!(
            parse_summary(bad).is_err(),
            "{:?}",
            String::from_utf8_lossy(bad)
        );
    }
}
