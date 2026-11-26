use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::testkit::faults::{DiskOp, Faults, Target};

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    faults: Arc<Faults>,
    files: Files,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    // macOS temporary directories sit behind a link; the root must be the real path.
    let root = dir.path().canonicalize().unwrap().join("durable");
    let faults = Faults::new();
    let files = faults.files(&root).unwrap();
    Fixture {
        _dir: dir,
        root,
        faults,
        files,
    }
}

fn path(text: &str) -> DurablePath {
    DurablePath::parse(text).unwrap()
}

fn version<'a>(
    path: &'a DurablePath,
    op: FileOp,
    bytes: Option<&'a [u8]>,
    previous: Option<&[u8]>,
) -> FileVersion<'a> {
    FileVersion {
        path,
        op,
        bytes,
        sha256: bytes.map(Sha256::of),
        previous_sha256: previous.map(Sha256::of),
    }
}

fn on_disk(f: &Fixture, path: &str) -> Option<Vec<u8>> {
    fs::read(f.root.join(path)).ok()
}

#[test]
fn a_relative_root_is_refused() {
    assert!(Files::open(Path::new("durable")).is_err());
    assert!(Faults::new().files(Path::new("durable")).is_err());
}

#[test]
fn a_round_flushes_the_whole_directory_chain_and_reads_flush_nothing() {
    let f = fixture();
    // Visible directories do not prove an earlier creator flushed their links.
    fs::create_dir_all(f.root.join("state/teams")).unwrap();
    let p = path("state/teams/a.yaml");
    f.files
        .materialize(&[version(&p, FileOp::Put, Some(b"one"), None)])
        .unwrap();
    assert_eq!(
        f.faults.paths(DiskOp::SyncDir),
        [
            f.root.parent().unwrap().to_path_buf(),
            f.root.clone(),
            f.root.join("state"),
            f.root.join("state/teams")
        ]
    );
    // The file's data is flushed before the rename, and once more after the directories.
    assert_eq!(f.faults.paths(DiskOp::Flush).len(), 2);
    f.faults.clear_log();
    assert_eq!(&*f.files.read(&p).unwrap().unwrap(), b"one");
    assert_eq!(f.files.read(&path("state/none/x")).unwrap(), None);
    assert!(f.faults.log().is_empty());
}

#[test]
fn a_failed_directory_flush_fails_the_round_for_new_and_identical_bytes_and_is_not_retried() {
    for existing in [false, true] {
        let f = fixture();
        let p = path("state/a.yaml");
        if existing {
            f.files
                .materialize(&[version(&p, FileOp::Create, Some(b"same"), None)])
                .unwrap();
            f.faults.clear_log();
        }
        f.faults.fail(DiskOp::SyncDir, Target::Directory, 2);
        let error = f
            .files
            .materialize(&[version(&p, FileOp::Create, Some(b"same"), None)])
            .unwrap_err();
        assert!(
            matches!(&error, FilesError::Io { flush: true, path, .. } if *path == f.root.display().to_string()),
            "{error:?}"
        );
        // Two calls: the one before it and the failed one. Nothing after it.
        assert_eq!(f.faults.paths(DiskOp::SyncDir).len(), 2);
    }
}

#[test]
fn files_and_directories_are_private_to_the_owner() {
    let f = fixture();
    let p = path("state/teams/t/team.yaml");
    f.files
        .materialize(&[version(&p, FileOp::Put, Some(b"x"), None)])
        .unwrap();
    let mode = |path: PathBuf| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(f.root.join("state/teams/t/team.yaml")), 0o600);
    for directory in ["", "state", "state/teams", "state/teams/t"] {
        assert_eq!(mode(f.root.join(directory)), 0o700, "{directory}");
    }
}

#[test]
fn create_is_exclusive_identical_bytes_are_kept_and_other_bytes_stop() {
    let f = fixture();
    let p = path("messaging/m/1.json");
    let create = [version(&p, FileOp::Create, Some(b"event"), None)];
    f.files.materialize(&create).unwrap();
    assert_eq!(on_disk(&f, "messaging/m/1.json").unwrap(), b"event");
    f.faults.clear_log();
    f.files.materialize(&create).unwrap();
    assert!(f.faults.paths(DiskOp::Link).is_empty());

    fs::write(f.root.join("messaging/m/1.json"), b"other").unwrap();
    assert_eq!(
        f.files.materialize(&create),
        Err(FilesError::Foreign { path: p.clone() })
    );
    assert_eq!(on_disk(&f, "messaging/m/1.json").unwrap(), b"other");
    // No temporary file is left where a walk would see it.
    assert_eq!(
        f.files.walk(&path("messaging")).unwrap(),
        std::slice::from_ref(&p)
    );
}

#[test]
fn put_replaces_only_a_version_this_office_wrote() {
    let f = fixture();
    let p = path("state/t.yaml");
    f.files
        .materialize(&[version(&p, FileOp::Put, Some(b"v1"), None)])
        .unwrap();
    f.files
        .materialize(&[version(&p, FileOp::Put, Some(b"v2"), Some(b"v1"))])
        .unwrap();
    assert_eq!(on_disk(&f, "state/t.yaml").unwrap(), b"v2");

    // Foreign bytes: stop and name the path, replacing nothing.
    fs::write(f.root.join("state/t.yaml"), b"edited by hand").unwrap();
    let put = [version(&p, FileOp::Put, Some(b"v3"), Some(b"v2"))];
    assert_eq!(
        f.files.materialize(&put),
        Err(FilesError::Foreign { path: p.clone() })
    );
    assert_eq!(on_disk(&f, "state/t.yaml").unwrap(), b"edited by hand");

    // A missing file is recreated from the journal.
    fs::remove_file(f.root.join("state/t.yaml")).unwrap();
    f.files.materialize(&put).unwrap();
    assert_eq!(on_disk(&f, "state/t.yaml").unwrap(), b"v3");
}

#[test]
fn a_round_writes_the_final_state_and_accepts_any_retained_version_after_a_crash() {
    let f = fixture();
    let (a, b) = (path("state/a"), path("state/b"));
    let round = [
        version(&a, FileOp::Put, Some(b"a1"), None),
        version(&b, FileOp::Create, Some(b"b1"), None),
        version(&a, FileOp::Put, Some(b"a2"), Some(b"a1")),
        version(&b, FileOp::Remove, None, Some(b"b1")),
        version(&a, FileOp::Put, Some(b"a3"), Some(b"a2")),
    ];
    f.files.materialize(&round).unwrap();
    assert_eq!(on_disk(&f, "state/a").unwrap(), b"a3");
    assert_eq!(on_disk(&f, "state/b"), None);
    // Only the final version of `a` was ever written.
    assert_eq!(f.faults.paths(DiskOp::Rename).len(), 1);

    // A crash left an intermediate or the final version: the round runs again.
    for left in [&b"a2"[..], b"a3"] {
        fs::write(f.root.join("state/a"), left).unwrap();
        f.files.materialize(&round).unwrap();
        assert_eq!(on_disk(&f, "state/a").unwrap(), b"a3");
    }
}

#[test]
fn remove_unlinks_only_a_version_this_office_wrote() {
    let f = fixture();
    let p = path("state/r");
    f.files
        .materialize(&[version(&p, FileOp::Put, Some(b"v1"), None)])
        .unwrap();
    let remove = [version(&p, FileOp::Remove, None, Some(b"v1"))];
    fs::write(f.root.join("state/r"), b"foreign").unwrap();
    assert_eq!(
        f.files.materialize(&remove),
        Err(FilesError::Foreign { path: p.clone() })
    );
    fs::write(f.root.join("state/r"), b"v1").unwrap();
    f.files.materialize(&remove).unwrap();
    assert_eq!(on_disk(&f, "state/r"), None);
    f.files.materialize(&remove).unwrap();
}

#[test]
fn a_link_in_the_path_is_refused_and_nothing_is_written_through_it() {
    let f = fixture();
    let outside = f.root.parent().unwrap().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(f.root.join("state")).unwrap();
    std::os::unix::fs::symlink(&outside, f.root.join("state/escape")).unwrap();
    let p = path("state/escape/x");
    let error = f
        .files
        .materialize(&[version(&p, FileOp::Put, Some(b"x"), None)])
        .unwrap_err();
    assert!(matches!(error, FilesError::Io { flush: false, .. }));
    assert!(fs::read_dir(&outside).unwrap().next().is_none());
    assert!(f.files.read(&p).is_err());
    assert!(f.files.walk(&path("state")).is_err());

    // A link where the file itself would be is not followed either.
    fs::remove_file(f.root.join("state/escape")).unwrap();
    fs::write(outside.join("target"), b"keep").unwrap();
    std::os::unix::fs::symlink(outside.join("target"), f.root.join("state/link")).unwrap();
    let p = path("state/link");
    assert!(f.files.read(&p).is_err());
    assert!(f
        .files
        .materialize(&[version(&p, FileOp::Put, Some(b"x"), None)])
        .is_err());
    assert_eq!(fs::read(outside.join("target")).unwrap(), b"keep");
}

#[test]
fn a_file_is_wholly_old_or_wholly_new_at_every_injected_failure() {
    for crash in [false, true] {
        let mut nth = 1;
        loop {
            let f = fixture();
            let p = path("state/deep/t.yaml");
            f.files
                .materialize(&[version(&p, FileOp::Put, Some(b"old bytes"), None)])
                .unwrap();
            if crash {
                f.faults.crash_before_call(nth);
            } else {
                f.faults.fail_call(nth);
            }
            let result = f.files.materialize(&[version(
                &p,
                FileOp::Put,
                Some(b"new bytes, longer"),
                Some(b"old bytes"),
            )]);
            let now = on_disk(&f, "state/deep/t.yaml").unwrap();
            assert!(
                now == b"old bytes" || now == b"new bytes, longer",
                "call {nth} (crash {crash}) left {now:?}"
            );
            if !f.faults.fired() {
                result.unwrap();
                assert_eq!(now, b"new bytes, longer");
                assert!(nth > 5, "the loop covered {nth} calls");
                break;
            }
            assert!(result.is_err(), "call {nth} failed silently");
            nth += 1;
        }
    }
}

#[test]
fn a_failed_file_flush_is_returned_and_never_retried() {
    let f = fixture();
    let p = path("state/t.yaml");
    f.faults.fail(DiskOp::Flush, Target::File, 1);
    let error = f
        .files
        .materialize(&[version(&p, FileOp::Put, Some(b"x"), None)])
        .unwrap_err();
    assert!(matches!(error, FilesError::Io { flush: true, .. }));
    assert!(error.path().contains(TEMP_PREFIX));
    assert_eq!(f.faults.paths(DiskOp::Flush).len(), 1);
    assert!(f.faults.paths(DiskOp::Rename).is_empty());
    assert_eq!(on_disk(&f, "state/t.yaml"), None);
}

#[test]
fn a_walk_is_sorted_by_path_bytes_and_skips_unfinished_publications() {
    let f = fixture();
    let names = ["state/b", "state/a/z", "state/a-", "state/a/y/1", "local/x"];
    let paths: Vec<_> = names.iter().map(|n| path(n)).collect();
    let versions: Vec<_> = paths
        .iter()
        .map(|p| version(p, FileOp::Put, Some(b"x"), None))
        .collect();
    f.files.materialize(&versions).unwrap();
    fs::write(f.root.join(format!("state/a/{TEMP_PREFIX}9-9")), b"partial").unwrap();
    let walked = f.files.walk(&path("state")).unwrap();
    let texts: Vec<_> = walked.iter().map(DurablePath::as_str).collect();
    assert_eq!(texts, ["state/a-", "state/a/y/1", "state/a/z", "state/b"]);
    assert_eq!(
        f.files.walk(&path("state/a/z")).unwrap(),
        [path("state/a/z")]
    );
    assert!(f.files.walk(&path("budgets")).unwrap().is_empty());
    assert!(f
        .files
        .walk(&path("state/a/none/deeper"))
        .unwrap()
        .is_empty());
}
