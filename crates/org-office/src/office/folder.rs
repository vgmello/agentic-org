//! The office folder (`<home>/offices/<office-id>/`): its name, its lock, the link and
//! existence checks every way into it makes, and what an office of an older build left
//! there.

use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::intent;
use crate::error::OfficeError;
use crate::private_fs::{absolute, private_file};

/// How long `provision_hq`, `open` and `discard` wait for the folder's lock: a slow-exiting
/// old server and its replacement never overlap.
pub const LOCK_WAIT: Duration = Duration::from_secs(10);

/// The office lock, taken before anything in the folder is read.
pub(crate) const LOCK_FILE: &str = "office.lock";

/// An older build's organization database.
const ORG_DB: &str = "org.db";
/// An older build's office database.
const OFFICE_DB: &str = "office.db";

/// What an office of the old databases leaves in its folder, besides `office.json` and the
/// creation intent: its databases, any of their logs, its workspace and its unfinished
/// `office.json`. A fresh create finds none of it; an opening refuses the databases.
const OLDER_BUILD_ENTRIES: [&str; 10] = [
    "office.json.tmp",
    "workspace",
    ORG_DB,
    "org.db-wal",
    "org.db-shm",
    "org.db-journal",
    OFFICE_DB,
    "office.db-wal",
    "office.db-shm",
    "office.db-journal",
];

/// Whether `name`, an entry of an office folder, is one an older build's office left.
pub(crate) fn is_older_builds(name: &str) -> bool {
    OLDER_BUILD_ENTRIES.contains(&name)
}

/// Refuses an office folder the old databases made.
pub(crate) fn refuse_older_build(folder: &Path) -> Result<(), OfficeError> {
    for database in [ORG_DB, OFFICE_DB] {
        if exists(&folder.join(database))? {
            return Err(intent::older_build());
        }
    }
    Ok(())
}

/// The office folder must be named by the office id, so an id never points outside `offices/`.
pub(crate) fn check_named_by(folder: &Path, office_id: &str) -> Result<(), OfficeError> {
    if folder.file_name().and_then(|name| name.to_str()) == Some(office_id) {
        Ok(())
    } else {
        Err(OfficeError::Invalid(
            "the folder is not named by the office id",
        ))
    }
}

/// Takes the folder's exclusive lock, waiting up to `wait` for another process to let go.
/// The lock file is close-on-exec (Rust opens every file that way), so a child never keeps
/// the lock after the server dies.
pub(crate) fn lock_folder(folder: &Path, wait: Duration) -> Result<File, OfficeError> {
    lock_folder_with(folder, wait, &|| {})
}

/// [`lock_folder`], calling `between` after the lock file is opened and before it is locked
/// (a test seam for the window in which another process can unlink the file).
fn lock_folder_with(
    folder: &Path,
    wait: Duration,
    between: &dyn Fn(),
) -> Result<File, OfficeError> {
    let path = folder.join(LOCK_FILE);
    let give_up_at = Instant::now() + wait;
    loop {
        let lock = match private_file(&path) {
            Ok(lock) => lock,
            // The folder was removed under us (a discard): there is nothing to lock.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(OfficeError::NotFound);
            }
            Err(e) => return Err(OfficeError::storage(e)),
        };
        between();
        match lock.try_lock() {
            // A lock on a file that was unlinked while we waited (a discard removed the
            // folder) locks nothing: only a lock on the file that is still there counts.
            Ok(()) if is_the_file_at(&lock, &path) => return Ok(lock),
            Ok(()) => {}
            Err(TryLockError::WouldBlock) if Instant::now() < give_up_at => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(TryLockError::WouldBlock) => return Err(OfficeError::InUse),
            Err(TryLockError::Error(e)) => return Err(OfficeError::storage(e)),
        }
        if Instant::now() >= give_up_at {
            return Err(OfficeError::InUse);
        }
    }
}

/// Whether `file` is the file now at `path` (same device and inode).
fn is_the_file_at(file: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), fs::metadata(path)) {
        (Ok(open), Ok(named)) => open.dev() == named.dev() && open.ino() == named.ino(),
        _ => false,
    }
}

/// Removes the lock file and then the folder, which must be empty by now. Not recursive: if a
/// create of the same id slipped in while the lock file was unlinked, the folder is no longer
/// empty and is left to it.
pub(crate) fn remove_the_empty_folder(folder: &Path) -> Result<(), OfficeError> {
    match fs::remove_file(folder.join(LOCK_FILE)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(OfficeError::storage(e)),
    }
    match fs::remove_dir(folder) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        // Something started a create here; it is not ours to remove.
        Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(()),
        Err(e) => Err(OfficeError::storage(e)),
    }
}

/// The folder as an absolute path with no trailing separator or `.` components, so the link
/// check looks at the folder itself: `link/` would resolve the link and look like a folder.
pub(crate) fn plain_folder(folder: PathBuf) -> Result<PathBuf, OfficeError> {
    let absolute = absolute(folder).map_err(OfficeError::storage)?;
    Ok(absolute.components().collect())
}

/// Refuses a folder path that is a link (or anything but a directory): an office folder is
/// never reached through one, so nothing is created, locked or removed through a planted link.
pub(crate) fn check_not_a_link(folder: &Path) -> Result<(), OfficeError> {
    match fs::symlink_metadata(folder) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(OfficeError::Invalid(
            "the office folder is not a plain folder",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(OfficeError::storage(e)),
    }
}

/// Whether anything is at `path` (a dangling link counts). Anything but "not found" is an
/// error, so a create never mistakes an unreadable entry for a missing one.
pub(crate) fn exists(path: &Path) -> Result<bool, OfficeError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(OfficeError::storage(e)),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const SHORT: Duration = Duration::from_millis(100);

    fn folder() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("f");
        fs::create_dir_all(&folder).unwrap();
        (dir, folder)
    }

    #[test]
    fn a_lock_taken_on_a_file_that_was_unlinked_does_not_count() {
        let (_dir, folder) = folder();
        let first = lock_folder(&folder, SHORT).unwrap();
        // A waiter opened the lock file; then the owner removed it and made a new one.
        let stale = private_file(&folder.join(LOCK_FILE)).unwrap();
        fs::remove_file(folder.join(LOCK_FILE)).unwrap();
        drop(first);
        stale.try_lock().unwrap();
        assert!(!is_the_file_at(&stale, &folder.join(LOCK_FILE)));
        let fresh = lock_folder(&folder, SHORT).unwrap();
        assert!(is_the_file_at(&fresh, &folder.join(LOCK_FILE)));
    }

    #[test]
    fn a_lock_whose_file_is_replaced_before_it_is_taken_is_not_returned() {
        let (_dir, folder) = folder();
        let path = folder.join(LOCK_FILE);
        let calls = Cell::new(0);
        // On the first go, between opening the file and locking it, the file is replaced (as
        // a discard followed by a create would).
        let swap = || {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                fs::remove_file(&path).unwrap();
                fs::write(&path, b"").unwrap();
            }
        };

        let lock = lock_folder_with(&folder, SHORT, &swap).unwrap();

        assert!(
            is_the_file_at(&lock, &path),
            "it locked a file nobody else can see"
        );
        assert_eq!(calls.get(), 2, "it did not try again");
    }

    #[test]
    fn a_held_lock_is_waited_for_and_then_given_up() {
        let (_dir, folder) = folder();
        let _held = lock_folder(&folder, SHORT).unwrap();
        assert!(matches!(
            lock_folder(&folder, SHORT),
            Err(OfficeError::InUse)
        ));
    }

    #[test]
    fn a_folder_that_is_gone_is_not_found_not_a_storage_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            lock_folder(&dir.path().join("gone"), SHORT),
            Err(OfficeError::NotFound)
        ));
    }

    #[test]
    fn the_last_step_never_removes_a_folder_a_create_has_started_filling() {
        let (_dir, folder) = folder();
        fs::write(folder.join(LOCK_FILE), b"").unwrap();
        fs::write(folder.join("hq-creation.v1.json"), b"a new create's").unwrap();

        remove_the_empty_folder(&folder).unwrap();

        assert_eq!(
            fs::read(folder.join("hq-creation.v1.json")).unwrap(),
            b"a new create's"
        );
    }

    #[test]
    fn a_trailing_slash_does_not_hide_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        fs::create_dir_all(&target).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let slashed = PathBuf::from(format!("{}/", link.display()));

        let plain = plain_folder(slashed).unwrap();
        assert_eq!(plain, link);
        assert!(matches!(
            check_not_a_link(&plain),
            Err(OfficeError::Invalid(_))
        ));
        // A real folder with a trailing slash is the folder.
        let real = dir.path().join("real");
        fs::create_dir_all(&real).unwrap();
        let plain = plain_folder(PathBuf::from(format!("{}/", real.display()))).unwrap();
        assert_eq!(plain, real);
        check_not_a_link(&plain).unwrap();
    }

    #[test]
    fn the_folder_is_named_by_its_office_id() {
        let folder = Path::new("/offices/0190a5d2-7c3e-7a01-8b2c-3d4e5f607182");
        check_named_by(folder, "0190a5d2-7c3e-7a01-8b2c-3d4e5f607182").unwrap();
        for id in ["other", "", "0190a5d2-7c3e-7a01-8b2c-3d4e5f607183"] {
            assert!(
                matches!(check_named_by(folder, id), Err(OfficeError::Invalid(_))),
                "{id:?}"
            );
        }
    }
}
