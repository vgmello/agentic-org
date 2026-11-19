//! Tests of the directory store, which need a real adapter (macOS or GNU/Linux). They
//! go through the contract ([`credentials`]), as the host does.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::testing as hooks;
use super::{
    credentials, CredentialError, CredentialLocation, Credentials, DirectoryStore, ItemName,
    Sharing, WriteError, MAX_ITEM_BYTES,
};

/// The store's directory below the anchor.
const BELOW: [&str; 2] = ["namespace", "store"];

struct Fixture {
    _temp: tempfile::TempDir,
    /// A private directory, resolved.
    anchor: PathBuf,
    /// A directory next to the anchor that the store has no business in.
    outside: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let (anchor, outside) = (root.join("anchor"), root.join("outside"));
        for dir in [&anchor, &outside] {
            fs::create_dir(dir).unwrap();
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self {
            _temp: temp,
            anchor,
            outside,
        }
    }

    fn location(&self) -> DirectoryStore<'_> {
        DirectoryStore {
            anchor: &self.anchor,
            below: &BELOW,
            create: true,
            sharing: Sharing::Exclusive,
            suffix: "",
        }
    }

    fn open(&self) -> Result<Credentials, CredentialError> {
        credentials(CredentialLocation::Directory(self.location()))
    }

    fn shared(&self) -> Result<Credentials, CredentialError> {
        credentials(CredentialLocation::Directory(DirectoryStore {
            sharing: Sharing::Shared,
            ..self.location()
        }))
    }

    fn dir(&self) -> PathBuf {
        self.anchor.join(BELOW[0]).join(BELOW[1])
    }
}

fn name(raw: &str) -> ItemName {
    ItemName::new(raw).unwrap()
}

fn loaded(store: &Credentials, name: &ItemName) -> Option<Vec<u8>> {
    store
        .load(name)
        .unwrap()
        .map(|secret| secret.expose().to_vec())
}

fn names(store: &Credentials) -> Vec<String> {
    store
        .list(|_| true)
        .unwrap()
        .names
        .iter()
        .map(|name| name.as_str().to_owned())
        .collect()
}

/// Writes an owner-only file, as an item must be.
fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().mode() & 0o7777
}

fn temporaries(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".put.")
        })
        .collect()
}

fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

/// Runs this test binary again for one test, with `marker` set.
fn child(test: &str, marker: &str, value: &Path) -> std::process::Output {
    use std::process::{Command, Stdio};
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env(marker, value)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("credential store test child timed out: {output:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn operations_reopen_and_permissions_follow_the_contract() {
    let f = Fixture::new();
    let store = f.open().unwrap();
    let (registry, recovery) = (name("registry"), name("recovery.Abc123"));
    assert!(store.load(&registry).unwrap().is_none());
    store.delete(&registry).unwrap();
    assert!(names(&store).is_empty());
    store.replace(&registry, &[1; 32]).unwrap();
    assert_eq!(loaded(&store, &registry).unwrap(), [1; 32]);
    store.replace(&registry, &vec![9; MAX_ITEM_BYTES]).unwrap();
    assert_eq!(loaded(&store, &registry).unwrap(), vec![9; MAX_ITEM_BYTES]);
    store.add(&recovery, &[]).unwrap();
    assert_eq!(names(&store), ["recovery.Abc123", "registry"]);
    assert!(loaded(&store, &recovery).unwrap().is_empty());
    store.delete(&registry).unwrap();
    store.delete(&registry).unwrap();
    assert!(store.load(&registry).unwrap().is_none());
    assert_eq!(names(&store), ["recovery.Abc123"]);
    store.delete(&recovery).unwrap();

    store.replace(&registry, &[17; 32]).unwrap();
    for dir in [&f.anchor, &f.anchor.join(BELOW[0]), &f.dir()] {
        assert_eq!(mode(dir), 0o700);
    }
    for file in ["registry", ".lock"] {
        let meta = fs::metadata(f.dir().join(file)).unwrap();
        assert_eq!((meta.mode() & 0o7777, meta.nlink()), (0o600, 1), "{file}");
    }
    // The item is its value and nothing else; the lock is empty.
    assert_eq!(fs::read(f.dir().join("registry")).unwrap(), [17; 32]);
    assert_eq!(fs::metadata(f.dir().join(".lock")).unwrap().len(), 0);
    assert!(temporaries(&f.dir()).is_empty());
    assert_eq!(f.open().unwrap_err(), CredentialError::InUse);
    drop(store);
    let reopened = f.open().unwrap();
    assert_eq!(loaded(&reopened, &registry).unwrap(), [17; 32]);
}

#[test]
fn an_add_never_replaces_and_a_value_too_large_changes_nothing() {
    let f = Fixture::new();
    let store = f.open().unwrap();
    let n = name("tls.aa");
    store.add(&n, b"first").unwrap();
    assert_eq!(
        store.add(&n, b"second"),
        Err(WriteError::from(CredentialError::Exists))
    );
    assert_eq!(loaded(&store, &n).unwrap(), b"first");
    drop(store);
    let store = f.open().unwrap();
    assert_eq!(
        store.add(&n, b"third").unwrap_err().kind,
        CredentialError::Exists
    );
    assert_eq!(loaded(&store, &n).unwrap(), b"first");
    assert_eq!(names(&store), ["tls.aa"]);
    // After an explicit delete the name is free again.
    store.delete(&n).unwrap();
    store.add(&n, b"after delete").unwrap();
    assert_eq!(loaded(&store, &n).unwrap(), b"after delete");

    let large = vec![1; MAX_ITEM_BYTES + 1];
    for write in [Credentials::add, Credentials::replace] {
        for target in [&n, &name("new")] {
            assert_eq!(
                write(&store, target, &large),
                Err(WriteError::from(CredentialError::TooLarge))
            );
        }
    }
    assert_eq!(loaded(&store, &n).unwrap(), b"after delete");
    assert_eq!(names(&store), ["tls.aa"]);
    assert!(temporaries(&f.dir()).is_empty());
}

#[test]
fn a_location_is_checked_before_anything_is_made() {
    let f = Fixture::new();
    let open = |location: DirectoryStore<'_>| {
        credentials(CredentialLocation::Directory(location)).unwrap_err()
    };
    let relative = DirectoryStore {
        anchor: Path::new("relative"),
        ..f.location()
    };
    assert_eq!(open(relative), CredentialError::Unsafe);
    let dotted = f.anchor.join("..").join("anchor");
    let parent = DirectoryStore {
        anchor: &dotted,
        ..f.location()
    };
    assert_eq!(open(parent), CredentialError::Unsafe);
    let missing = f.anchor.join("missing");
    let absent = DirectoryStore {
        anchor: &missing,
        ..f.location()
    };
    assert_eq!(open(absent), CredentialError::Io(ErrorKind::NotFound));
    for below in [&[""][..], &[".."], &["a/b"], &[".hidden"], &["store", "."]] {
        let location = DirectoryStore {
            below,
            ..f.location()
        };
        assert_eq!(open(location), CredentialError::InvalidName, "{below:?}");
    }
    for suffix in ["/x", "a b", ".0123456789abcdef0"] {
        let location = DirectoryStore {
            suffix,
            ..f.location()
        };
        assert_eq!(open(location), CredentialError::InvalidName, "{suffix:?}");
    }
    // A symbolic link is not the anchor, however it is spelled.
    let alias = f.anchor.with_file_name("alias");
    symlink(&f.anchor, &alias).unwrap();
    for spelling in [
        alias.clone(),
        PathBuf::from(format!("{}/", alias.display())),
        alias.join("."),
    ] {
        let location = DirectoryStore {
            anchor: &spelling,
            ..f.location()
        };
        assert!(
            credentials(CredentialLocation::Directory(location)).is_err(),
            "{spelling:?}"
        );
    }
    // A file is not the anchor.
    let file = f.outside.join("file");
    write(&file, b"");
    let location = DirectoryStore {
        anchor: &file,
        ..f.location()
    };
    assert!(credentials(CredentialLocation::Directory(location)).is_err());
    assert_eq!(fs::read_dir(&f.anchor).unwrap().count(), 0);
    // A link in the way of a directory below the anchor is not followed.
    symlink(&f.outside, f.anchor.join(BELOW[0])).unwrap();
    assert!(f.open().is_err());
    assert_eq!(fs::read_dir(&f.outside).unwrap().count(), 1);
}

#[test]
fn without_create_a_missing_directory_is_not_found_and_nothing_is_made() {
    let f = Fixture::new();
    let reading = DirectoryStore {
        create: false,
        ..f.location()
    };
    let open = || credentials(CredentialLocation::Directory(reading));
    assert_eq!(
        open().unwrap_err(),
        CredentialError::Io(ErrorKind::NotFound)
    );
    assert_eq!(fs::read_dir(&f.anchor).unwrap().count(), 0);
    fs::create_dir(f.anchor.join(BELOW[0])).unwrap();
    fs::set_permissions(f.anchor.join(BELOW[0]), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        open().unwrap_err(),
        CredentialError::Io(ErrorKind::NotFound)
    );
    assert_eq!(fs::read_dir(f.anchor.join(BELOW[0])).unwrap().count(), 0);
    let n = name("registry");
    f.open().unwrap().replace(&n, b"made").unwrap();
    assert_eq!(loaded(&open().unwrap(), &n).unwrap(), b"made");
    // The anchor alone can be the store.
    let flat = DirectoryStore {
        anchor: &f.outside,
        below: &[],
        create: false,
        sharing: Sharing::Exclusive,
        suffix: "",
    };
    let store = credentials(CredentialLocation::Directory(flat)).unwrap();
    store.add(&n, b"flat").unwrap();
    assert_eq!(fs::read(f.outside.join("registry")).unwrap(), b"flat");
}

#[test]
fn unsafe_directories_and_lock_files_are_refused_and_never_repaired() {
    for (level, open) in [("anchor", 0o750), ("namespace", 0o777), ("store", 0o755)] {
        let f = Fixture::new();
        drop(f.open().unwrap());
        let path = match level {
            "anchor" => f.anchor.clone(),
            "namespace" => f.anchor.join(BELOW[0]),
            _ => f.dir(),
        };
        fs::set_permissions(&path, fs::Permissions::from_mode(open)).unwrap();
        assert_eq!(f.open().unwrap_err(), CredentialError::Unsafe, "{level}");
        assert_eq!(f.shared().unwrap_err(), CredentialError::Unsafe, "{level}");
        assert_eq!(mode(&path), open, "{level}");
    }
    for kind in ["symlink", "directory", "permissions", "second link"] {
        let f = Fixture::new();
        drop(f.open().unwrap());
        let lock = f.dir().join(".lock");
        if kind != "second link" {
            fs::remove_file(&lock).unwrap();
        }
        match kind {
            "symlink" => symlink(&f.anchor, &lock).unwrap(),
            "directory" => fs::create_dir(&lock).unwrap(),
            "permissions" => {
                write(&lock, b"");
                fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
            }
            _ => fs::hard_link(&lock, f.outside.join("lock")).unwrap(),
        }
        assert!(f.open().is_err(), "{kind}");
    }
    let f = Fixture::new();
    drop(f.open().unwrap());
    fs::remove_dir_all(f.dir()).unwrap();
    symlink(&f.outside, f.dir()).unwrap();
    assert!(f.open().is_err());
    assert!(!f.outside.join(".lock").exists());
    // Root's extra topology probe can exercise uid independently of mode permissions.
    if is_root() {
        for level in ["anchor", "namespace", "store", "lock", "item"] {
            let f = Fixture::new();
            let store = f.open().unwrap();
            let n = name("registry");
            store.replace(&n, &[1]).unwrap();
            drop(store);
            let path = match level {
                "anchor" => f.anchor.clone(),
                "namespace" => f.anchor.join(BELOW[0]),
                "store" => f.dir(),
                "lock" => f.dir().join(".lock"),
                _ => f.dir().join("registry"),
            };
            std::os::unix::fs::chown(&path, Some(1), None).unwrap();
            if level == "item" {
                assert_eq!(
                    f.open().unwrap().load(&n).unwrap_err(),
                    CredentialError::Unsafe
                );
            } else {
                assert_eq!(f.open().unwrap_err(), CredentialError::Unsafe, "{level}");
            }
        }
    }
}

#[test]
fn symlinks_nonregular_items_second_links_and_oversize_files_fail_closed() {
    let f = Fixture::new();
    let s = f.open().unwrap();
    let n = name("registry");
    let path = f.dir().join(n.as_str());
    let external = f.outside.join("untouched");
    write(&external, b"keep");
    symlink(&external, &path).unwrap();
    assert!(s.load(&n).is_err());
    assert!(s.replace(&n, &[1]).is_err());
    assert!(s.add(&n, &[1]).is_err());
    assert!(s.delete(&n).is_err());
    assert!(s.list(|_| true).is_err());
    // What the consumer's rule leaves out is not opened at all.
    assert_eq!(s.list(|_| false).unwrap().ignored, 1);
    assert_eq!(fs::read(&external).unwrap(), b"keep");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert_eq!(s.load(&n).unwrap_err(), CredentialError::Unsafe);
    assert!(s.replace(&n, &[1]).is_err());
    assert_eq!(s.add(&n, &[1]).unwrap_err().kind, CredentialError::Unsafe);
    assert!(s.delete(&n).is_err());
    fs::remove_dir(&path).unwrap();
    write(&path, &vec![7; MAX_ITEM_BYTES + 1]);
    assert_eq!(s.load(&n).unwrap_err(), CredentialError::Corrupt);
    write(&path, b"ok");
    for open in [0o644, 0o640, 0o400, 0o700] {
        fs::set_permissions(&path, fs::Permissions::from_mode(open)).unwrap();
        assert_eq!(s.load(&n).unwrap_err(), CredentialError::Unsafe, "{open:o}");
        assert_eq!(mode(&path), open);
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(loaded(&s, &n).unwrap(), b"ok");
    // One file under two names is not an item under either.
    fs::hard_link(&path, f.outside.join("second-name")).unwrap();
    assert_eq!(s.load(&n).unwrap_err(), CredentialError::Unsafe);
    assert_eq!(s.delete(&n), Err(CredentialError::Unsafe));
    fs::remove_file(&path).unwrap();
    // Never blocked by a FIFO: only a regular file is an item.
    use std::os::unix::ffi::OsStrExt;
    let raw = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: live, NUL-terminated temporary fixture path.
    assert_eq!(unsafe { libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
    assert_eq!(s.load(&n).unwrap_err(), CredentialError::Unsafe);
    assert!(s.replace(&n, &[]).is_err());
    assert!(s.delete(&n).is_err());
}

#[test]
fn crash_temporaries_are_removed_at_open_and_other_files_are_left_alone() {
    let f = Fixture::new();
    let s = f.open().unwrap();
    let n = name("registry");
    s.replace(&n, b"old").unwrap();
    drop(s);
    let pending = f.dir().join(".put.01BBBBBBBBBBBBBBBBBBBBBBBB");
    write(&pending, b"unpublished");
    write(&f.dir().join("unrelated"), b"preserve");
    fs::write(f.dir().join(".other"), b"not this store's").unwrap();
    let s = f.open().unwrap();
    assert!(!pending.exists());
    assert_eq!(loaded(&s, &n).unwrap(), b"old");
    let listing = s.list(|name| name == "registry").unwrap();
    assert_eq!(listing.names, std::slice::from_ref(&n));
    assert_eq!(listing.ignored, 2);
    s.replace(&n, b"new").unwrap();
    assert!(temporaries(&f.dir()).is_empty());
    drop(s);
    assert_eq!(loaded(&f.open().unwrap(), &n).unwrap(), b"new");
    assert_eq!(fs::read(f.dir().join("unrelated")).unwrap(), b"preserve");
    assert_eq!(
        fs::read(f.dir().join(".other")).unwrap(),
        b"not this store's"
    );
}

#[test]
fn stale_temporaries_that_are_not_plain_files_refuse_the_open_and_are_not_followed() {
    let f = Fixture::new();
    drop(f.open().unwrap());
    let target = f.outside.join("victim");
    write(&target, b"keep");
    let stale = f.dir().join(".put.01BBBBBBBBBBBBBBBBBBBBBBBB");
    symlink(&target, &stale).unwrap();
    assert!(f.open().is_err());
    assert!(f.shared().is_err());
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    assert!(fs::symlink_metadata(&stale).is_ok());
}

#[test]
fn a_changed_directory_is_refused_before_any_operation() {
    let f = Fixture::new();
    let s = f.open().unwrap();
    let n = name("registry");
    s.replace(&n, b"old").unwrap();
    let moved = f.dir().with_file_name("moved");
    fs::rename(f.dir(), &moved).unwrap();
    fs::create_dir(f.dir()).unwrap();
    fs::set_permissions(f.dir(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(s.load(&n).unwrap_err(), CredentialError::Unsafe);
    assert_eq!(s.delete(&n), Err(CredentialError::Unsafe));
    assert_eq!(
        s.replace(&n, &[]).unwrap_err().kind,
        CredentialError::Unsafe
    );
    assert_eq!(
        s.add(&name("other"), &[]).unwrap_err().kind,
        CredentialError::Unsafe
    );
    assert_eq!(s.list(|_| true), Err(CredentialError::Unsafe));
    assert_eq!(fs::read(moved.join("registry")).unwrap(), b"old");
    assert_eq!(fs::read_dir(f.dir()).unwrap().count(), 0);
    // A directory that others can enter is refused as well, even if it is the same one.
    fs::remove_dir(f.dir()).unwrap();
    fs::rename(&moved, f.dir()).unwrap();
    assert_eq!(loaded(&s, &n).unwrap(), b"old");
    fs::set_permissions(f.dir(), fs::Permissions::from_mode(0o750)).unwrap();
    assert_eq!(s.load(&n).unwrap_err(), CredentialError::Unsafe);
}

#[test]
fn case_aliases_fail_closed_in_both_orders() {
    for (first, alias) in [("recovery.A", "recovery.a"), ("recovery.a", "recovery.A")] {
        let f = Fixture::new();
        let store = f.open().unwrap();
        let (first, alias) = (name(first), name(alias));
        store.replace(&first, b"keep").unwrap();
        assert_eq!(
            store.replace(&alias, b"wrong").unwrap_err().kind,
            CredentialError::CaseAlias
        );
        assert_eq!(
            store.add(&alias, b"wrong").unwrap_err().kind,
            CredentialError::CaseAlias
        );
        assert_eq!(store.load(&alias).unwrap_err(), CredentialError::CaseAlias);
        assert_eq!(store.delete(&alias), Err(CredentialError::CaseAlias));
        assert_eq!(loaded(&store, &first).unwrap(), b"keep");
        assert_eq!(
            store.list(|_| true).unwrap().names,
            std::slice::from_ref(&first)
        );
        // Where names are bytes, a second spelling made by someone else is two entries, and
        // neither is an item then. Where the filesystem folds case, it is the item itself.
        write(&f.dir().join(alias.as_str()), b"keep");
        let spellings = fs::read_dir(f.dir())
            .unwrap()
            .filter(|entry| {
                let name = entry.as_ref().unwrap().file_name();
                name.to_string_lossy().eq_ignore_ascii_case(first.as_str())
            })
            .count();
        if spellings == 2 {
            assert_eq!(store.list(|_| true), Err(CredentialError::CaseAlias));
            assert_eq!(store.load(&first).unwrap_err(), CredentialError::CaseAlias);
        } else {
            assert_eq!(loaded(&store, &first).unwrap(), b"keep");
        }
    }
}

#[test]
fn concurrent_case_alias_publications_have_one_winner() {
    for _ in 0..8 {
        let f = Fixture::new();
        let store = f.open().unwrap();
        let barrier = std::sync::Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                barrier.wait();
                store.replace(&name("recovery.A"), &[1])
            });
            let b = scope.spawn(|| {
                barrier.wait();
                store.add(&name("recovery.a"), &[2])
            });
            (a.join().unwrap(), b.join().unwrap())
        });
        let refused = |result: &Result<(), WriteError>| matches!(result, Err(failure) if failure.kind == CredentialError::CaseAlias);
        assert!(
            (results.0.is_ok() && refused(&results.1))
                || (refused(&results.0) && results.1.is_ok()),
            "{results:?}"
        );
        let listed = names(&store);
        assert_eq!(listed.len(), 1);
        let expected = if listed[0] == "recovery.A" { 1 } else { 2 };
        assert_eq!(loaded(&store, &name(&listed[0])).unwrap(), [expected]);
    }
}

#[test]
fn replacements_are_atomic_to_readers() {
    let f = Fixture::new();
    let s = Arc::new(f.open().unwrap());
    let n = name("registry");
    s.replace(&n, &vec![1; MAX_ITEM_BYTES]).unwrap();
    std::thread::scope(|scope| {
        let writer = Arc::clone(&s);
        let key = n.clone();
        scope.spawn(move || {
            for i in 0..40 {
                writer
                    .replace(&key, &vec![1 + i % 2; MAX_ITEM_BYTES])
                    .unwrap();
            }
        });
        for _ in 0..80 {
            let value = loaded(&s, &n).unwrap();
            assert_eq!(value.len(), MAX_ITEM_BYTES);
            assert!(value.iter().all(|b| *b == value[0]));
        }
    });
}

#[test]
fn the_lock_is_exclusive_across_processes_and_released_on_drop() {
    const MARKER: &str = "ORG_CREDENTIALS_LOCK_CHILD";
    const TEST: &str =
        "credentials::adapter_tests::the_lock_is_exclusive_across_processes_and_released_on_drop";
    if let Some(anchor) = std::env::var_os(MARKER) {
        let anchor = PathBuf::from(anchor);
        for sharing in [Sharing::Exclusive, Sharing::Shared] {
            let location = DirectoryStore {
                anchor: &anchor,
                below: &BELOW,
                create: true,
                sharing,
                suffix: "",
            };
            assert_eq!(
                credentials(CredentialLocation::Directory(location)).unwrap_err(),
                CredentialError::InUse
            );
        }
        return;
    }
    let f = Fixture::new();
    let store = f.open().unwrap();
    let temporary = f.dir().join(".put.01BBBBBBBBBBBBBBBBBBBBBBBB");
    write(&temporary, b"active owner");
    let output = child(TEST, MARKER, &f.anchor);
    assert!(output.status.success(), "{output:?}");
    assert!(
        temporary.exists(),
        "a contender must not clean the active owner's temporary file"
    );
    drop(store);
    assert!(f.open().is_ok());
    assert!(!temporary.exists());
}

#[test]
fn drop_unlocks_even_while_a_forked_child_holds_the_descriptor() {
    use std::os::fd::AsRawFd;
    let f = Fixture::new();
    let store = f.open().unwrap();
    // Standard-library sockets retain CLOEXEC; a byte releases the child even if
    // another fork briefly inherits a socket, so the test never depends on EOF.
    let (mut release, wait) = std::os::unix::net::UnixStream::pair().unwrap();
    let (release_fd, wait_fd) = (release.as_raw_fd(), wait.as_raw_fd());
    // SAFETY: the child only uses async-signal-safe close/read/_exit before exiting.
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        // SAFETY: close the parent's socket, then retain the inherited store descriptors
        // until the parent has attempted its post-drop reopen and sends one byte.
        unsafe {
            libc::close(release_fd);
            let mut byte = 0u8;
            let read = libc::read(wait_fd, (&mut byte as *mut u8).cast(), 1);
            libc::_exit(if read == 1 && byte == 1 { 0 } else { 42 });
        }
    }
    assert!(pid > 0, "fork failed");
    drop(wait);
    drop(store);
    let reopened = f.open();
    let released = std::io::Write::write_all(&mut release, &[1]);
    drop(release);
    // Reap before assertions so the regression failure never leaves a child behind.
    let mut status = 0;
    // SAFETY: wait only for the child this test created.
    let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
    released.unwrap();
    assert_eq!(waited, pid);
    assert_eq!(status, 0);
    assert!(
        reopened.is_ok(),
        "parent drop must release the store's lock even with an inherited fd: {:?}",
        reopened.err()
    );
}

#[test]
fn shared_handles_admit_each_other_and_only_a_sole_holder_removes_temporaries() {
    let f = Fixture::new();
    let one = f.shared().unwrap();
    let two = f.shared().unwrap();
    assert_eq!(f.open().unwrap_err(), CredentialError::InUse);
    let (a, b) = (name("a"), name("b"));
    one.add(&a, b"one").unwrap();
    two.add(&b, b"two").unwrap();
    assert_eq!(loaded(&two, &a).unwrap(), b"one");
    assert_eq!(loaded(&one, &b).unwrap(), b"two");
    assert_eq!(
        two.add(&a, b"again").unwrap_err().kind,
        CredentialError::Exists
    );
    assert_eq!(names(&one), ["a", "b"]);
    // A handle that is not alone leaves a temporary file to its writer.
    let temporary = f.dir().join(".put.01BBBBBBBBBBBBBBBBBBBBBBBB");
    write(&temporary, b"another holder's");
    let three = f.shared().unwrap();
    assert!(temporary.exists());
    drop((one, two, three));
    // The next handle is alone when it opens: it removes the file, and then shares.
    let sole = f.shared().unwrap();
    assert!(!temporary.exists());
    let joined = f.shared().unwrap();
    assert_eq!(loaded(&joined, &a).unwrap(), b"one");
    drop((sole, joined));
    // An exclusive handle admits no shared one.
    let exclusive = f.open().unwrap();
    assert_eq!(f.shared().unwrap_err(), CredentialError::InUse);
    drop(exclusive);
    assert!(f.shared().is_ok());
}

#[test]
fn a_restrictive_umask_cannot_publish_an_unreadable_replacement() {
    const MARKER: &str = "ORG_CREDENTIALS_UMASK_CHILD";
    const TEST: &str =
        "credentials::adapter_tests::a_restrictive_umask_cannot_publish_an_unreadable_replacement";
    if std::env::var_os(MARKER).is_some() {
        let f = Fixture::new();
        let store = f.open().unwrap();
        let n = name("registry");
        store.replace(&n, b"old").unwrap();
        // SAFETY: this exact-test child is isolated from the parent's parallel tests.
        let prior = unsafe { libc::umask(0o400) };
        let result = store.replace(&n, b"new");
        // SAFETY: restore the process mask before assertions and fixture cleanup.
        unsafe {
            libc::umask(prior);
        }
        result.unwrap();
        assert_eq!(mode(&f.dir().join("registry")), 0o600);
        assert_eq!(loaded(&store, &n).unwrap(), b"new");
        return;
    }
    let output = child(TEST, MARKER, Path::new("1"));
    assert!(output.status.success(), "{output:?}");
}

/// A mask that takes the owner's own bits away (`0277`) makes a new file `0400` and a
/// new directory `0500`. What an open makes gets its mode set explicitly, so the store
/// it makes is one the next open accepts; what exists with such a mode was not made by
/// that open, and is refused and never repaired.
#[test]
fn a_restrictive_umask_cannot_make_a_store_that_the_next_open_refuses() {
    const MARKER: &str = "ORG_CREDENTIALS_UMASK_OPEN_CHILD";
    const TEST: &str =
        "credentials::adapter_tests::a_restrictive_umask_cannot_make_a_store_that_the_next_open_refuses";
    if let Some(anchor) = std::env::var_os(MARKER) {
        let anchor = PathBuf::from(anchor);
        let location = DirectoryStore {
            anchor: &anchor,
            below: &BELOW,
            create: true,
            sharing: Sharing::Exclusive,
            suffix: "",
        };
        // SAFETY: this exact-test child is isolated from the parent's parallel tests.
        unsafe { libc::umask(0o277) };
        let store = credentials(CredentialLocation::Directory(location)).unwrap();
        store.add(&name("registry"), b"value").unwrap();
        drop(store);
        let again = credentials(CredentialLocation::Directory(location)).unwrap();
        assert_eq!(loaded(&again, &name("registry")).unwrap(), b"value");
        return;
    }
    let f = Fixture::new();
    let output = child(TEST, MARKER, &f.anchor);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(mode(&f.anchor.join(BELOW[0])), 0o700);
    assert_eq!(mode(&f.dir()), 0o700);
    assert_eq!(mode(&f.dir().join(".lock")), 0o600);
    assert_eq!(mode(&f.dir().join("registry")), 0o600);
    // An existing lock or directory with such a mode is left as it is, and refused.
    fs::set_permissions(f.dir().join(".lock"), fs::Permissions::from_mode(0o400)).unwrap();
    assert!(f.open().is_err());
    assert_eq!(mode(&f.dir().join(".lock")), 0o400);
    fs::set_permissions(f.dir().join(".lock"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(f.dir(), fs::Permissions::from_mode(0o500)).unwrap();
    assert_eq!(f.open().unwrap_err(), CredentialError::Unsafe);
    assert_eq!(mode(&f.dir()), 0o500);
    fs::set_permissions(f.dir(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(f.open().is_ok());
}

#[test]
fn an_add_whose_winner_is_gone_before_it_is_read_again_is_refused() {
    let f = Fixture::new();
    let store = f.open().unwrap();
    let n = name("tls.aa");
    hooks::before_publish(|dir| write(&dir.join("tls.aa"), b"first"));
    hooks::after_collision(|dir| fs::remove_file(dir.join("tls.aa")).unwrap());
    assert_eq!(
        store.add(&n, b"second"),
        Err(WriteError::from(CredentialError::Unsafe))
    );
    assert!(store.load(&n).unwrap().is_none());
    assert!(temporaries(&f.dir()).is_empty());
}

#[test]
fn an_add_of_an_existing_item_returns_before_anything_is_written() {
    let f = Fixture::new();
    let store = f.open().unwrap();
    let n = name("tls.aa");
    store.add(&n, b"first").unwrap();
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&calls);
    hooks::before_publish(move |_| observed.set(observed.get() + 1));
    let result = store.add(&n, b"second");
    let unused = hooks::take_before_publish();
    assert_eq!(result.unwrap_err().kind, CredentialError::Exists);
    assert_eq!(calls.get(), 0);
    assert!(unused, "no temporary file was made, so the hook never ran");
    assert_eq!(loaded(&store, &n).unwrap(), b"first");
}

#[test]
fn an_add_that_loses_the_name_at_the_rename_keeps_the_winner_and_leaves_no_temporary() {
    for race in ["existing", "symlink", "directory", "missing-temporary"] {
        let f = Fixture::new();
        let store = f.open().unwrap();
        let n = name("tls.aa");
        let other = f.outside.join("untouched");
        write(&other, b"outside");
        let target = other.clone();
        hooks::before_publish(move |dir| {
            let leaf = dir.join("tls.aa");
            match race {
                "existing" => write(&leaf, b"raced first"),
                "symlink" => symlink(&target, &leaf).unwrap(),
                "directory" => fs::create_dir(&leaf).unwrap(),
                _ => {
                    for temporary in temporaries(dir) {
                        fs::remove_file(temporary).unwrap();
                    }
                }
            }
        });
        let result = store.add(&n, b"new");
        match race {
            "existing" => {
                assert_eq!(result, Err(WriteError::from(CredentialError::Exists)));
                assert_eq!(loaded(&store, &n).unwrap(), b"raced first");
            }
            "symlink" => {
                assert!(result.is_err());
                assert!(fs::symlink_metadata(f.dir().join(n.as_str()))
                    .unwrap()
                    .file_type()
                    .is_symlink());
            }
            "directory" => {
                assert_eq!(result, Err(WriteError::from(CredentialError::Unsafe)));
                assert!(f.dir().join(n.as_str()).is_dir());
            }
            _ => {
                assert_eq!(
                    result,
                    Err(WriteError::from(CredentialError::Io(ErrorKind::NotFound)))
                );
                assert!(store.load(&n).unwrap().is_none());
            }
        }
        assert_eq!(fs::read(&other).unwrap(), b"outside");
        assert!(temporaries(&f.dir()).is_empty(), "{race}");
    }
}

/// What a failed write leaves, for a replace of an existing item and for an add: the
/// error's kind, whether the temporary file could not be removed, which value the item
/// holds, and that the next open removes what was left.
#[test]
fn a_failed_write_reports_its_kind_and_whether_its_temporary_file_is_left() {
    let real_permission = !is_root();
    for fault in [
        "success",
        "missing",
        "directory",
        "post-sync",
        "permission",
        "missing-sync",
        "post-double",
    ] {
        for add in [false, true] {
            let case = format!("{fault}/{}", if add { "add" } else { "replace" });
            let f = Fixture::new();
            let store = f.open().unwrap();
            let n = name("item");
            let (old, new) = (b"old-secret".as_slice(), b"new-secret".as_slice());
            if !add && fault != "directory" {
                store.replace(&n, old).unwrap();
            }
            let temporary = std::rc::Rc::new(std::cell::RefCell::new(None));
            let observed = temporary.clone();
            hooks::before_publish(move |dir| {
                let temps = temporaries(dir);
                assert_eq!(temps.len(), 1);
                let path = temps.into_iter().next().unwrap();
                *observed.borrow_mut() = Some(path.clone());
                match fault {
                    "permission" => {
                        if real_permission {
                            fs::set_permissions(dir, fs::Permissions::from_mode(0o500)).unwrap();
                        } else {
                            // Root bypasses mode bits: exercise cleanup via the sync seam instead.
                            fs::remove_file(&path).unwrap();
                        }
                    }
                    "directory" => fs::create_dir(dir.join("item")).unwrap(),
                    "missing" | "missing-sync" => fs::remove_file(&path).unwrap(),
                    _ => {}
                }
                hooks::fail_directory_syncs(match fault {
                    "permission" if !real_permission => vec![Some(ErrorKind::Interrupted)],
                    "missing-sync" => vec![Some(ErrorKind::Interrupted)],
                    "post-sync" => vec![Some(ErrorKind::WriteZero)],
                    "post-double" => vec![Some(ErrorKind::WriteZero), Some(ErrorKind::Interrupted)],
                    // One directory flush per successful write; the second outcome stays.
                    "success" => vec![None, Some(ErrorKind::Interrupted)],
                    _ => Vec::new(),
                });
            });
            let result = if add {
                store.add(&n, new)
            } else {
                store.replace(&n, new)
            };
            // Restore access before inspecting state or unwinding a failed assertion.
            fs::set_permissions(f.dir(), fs::Permissions::from_mode(0o700)).unwrap();
            if fault == "directory" {
                fs::remove_dir(f.dir().join("item")).unwrap();
            }
            let left = matches!(fault, "permission" | "missing-sync" | "post-double");
            let kind = match fault {
                "success" => None,
                "directory" if add => Some(CredentialError::Unsafe),
                "directory" => Some(CredentialError::Io(ErrorKind::IsADirectory)),
                "permission" if real_permission => {
                    Some(CredentialError::Io(ErrorKind::PermissionDenied))
                }
                "permission" | "missing" | "missing-sync" => {
                    Some(CredentialError::Io(ErrorKind::NotFound))
                }
                _ => Some(CredentialError::Io(ErrorKind::WriteZero)),
            };
            let expected = match kind {
                None => Ok(()),
                Some(kind) => Err(WriteError {
                    kind,
                    temporary_left: left,
                }),
            };
            assert_eq!(result, expected, "{case}");
            let unused = hooks::take_directory_sync_faults();
            if fault == "success" {
                assert_eq!(unused, [Some(ErrorKind::Interrupted)], "{case}");
            } else {
                assert!(unused.is_empty(), "{case}: all injected faults must run");
            }
            let path = temporary.borrow_mut().take().unwrap();
            assert_eq!(
                path.exists(),
                fault == "permission" && real_permission,
                "{case}"
            );
            if path.exists() {
                assert_eq!(fs::read(&path).unwrap(), new);
                assert_eq!(mode(&path), 0o600);
            }
            // A failed directory flush follows publication: the value is visible.
            let published = matches!(fault, "success" | "post-sync" | "post-double");
            let holds = if published {
                Some(new)
            } else if add || fault == "directory" {
                None
            } else {
                Some(old)
            };
            assert_eq!(loaded(&store, &n).as_deref(), holds, "{case}");
            drop(store);
            let reopened = f.open().unwrap();
            assert!(
                !path.exists(),
                "{case}: reopen must remove any retained plaintext temporary"
            );
            assert_eq!(loaded(&reopened, &n).as_deref(), holds, "{case}");
        }
    }
}

#[test]
fn a_suffix_names_the_items_files_and_every_other_file_is_ignored() {
    let f = Fixture::new();
    let keys = DirectoryStore {
        below: &[],
        suffix: ".key",
        sharing: Sharing::Shared,
        ..f.location()
    };
    let plain = DirectoryStore {
        below: &[],
        sharing: Sharing::Shared,
        ..f.location()
    };
    let store = credentials(CredentialLocation::Directory(keys)).unwrap();
    let (a, b) = (name("01AAAA"), name("01BBBB"));
    store.add(&b, b"two").unwrap();
    store.add(&a, b"one").unwrap();
    assert_eq!(fs::read(f.anchor.join("01AAAA.key")).unwrap(), b"one");
    assert_eq!(mode(&f.anchor.join("01BBBB.key")), 0o600);
    // Files that are not this store's: no suffix, an empty name, a name that is no item
    // name, a name the consumer refuses, a name that is not text. None is opened: each is
    // open to others, which an item may not be.
    for other in ["notes.txt", ".key", "a b.key", "refused.key", "01AAAA"] {
        fs::write(f.anchor.join(other), b"x").unwrap();
    }
    use std::os::unix::ffi::OsStringExt;
    let unreadable = std::ffi::OsString::from_vec(vec![0xff, b'.', b'k', b'e', b'y']);
    let not_text = fs::write(f.anchor.join(unreadable), b"x").is_ok();
    let listing = store.list(|name| name != "refused").unwrap();
    assert_eq!(listing.names, [a.clone(), b.clone()]);
    assert_eq!(listing.ignored, 5 + usize::from(not_text));
    // The same directory without the suffix is another set of items.
    let other = credentials(CredentialLocation::Directory(plain)).unwrap();
    assert!(other.load(&a).is_err(), "a file open to others was read");
    assert!(other.load(&name("01AAAA.key")).unwrap().is_some());
    assert!(other.load(&b).unwrap().is_none());
    store.delete(&a).unwrap();
    assert!(!f.anchor.join("01AAAA.key").exists());
    assert!(f.anchor.join("01AAAA").exists());
}
