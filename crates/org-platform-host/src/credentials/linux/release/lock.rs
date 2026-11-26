//! The release directory's lock file: opened (or made) without following a link,
//! judged by the lock rule, and taken exclusively.

use std::fs::{File, Permissions, TryLockError};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use super::descriptors::owned;
use super::{refused, LOCK, LOCK_NAME};
use crate::credentials::rules;
use crate::credentials::{CredentialError, RootRule};

/// Opens and takes the lock file of the release directory `directory` (at `path`), never
/// following a link. `O_NONBLOCK`: a FIFO at the name must not block the host; the
/// lock rule then refuses it.
///
/// A missing lock is made by this call alone (`O_EXCL`), and its mode is then set to
/// `0600` on the descriptor that made it: the process's file-creation mask may have
/// taken the owner's own bits away, and a lock made unusable would refuse the store
/// for good. An existing lock is opened as it is and never changed.
///
/// A lock that cannot be opened is refused by the lock's own rule where what stands
/// there visibly breaks it (another owner, another mode, not a regular file), and as
/// [`RootRule::LockUnavailable`] otherwise.
pub(super) fn open_lock(
    directory: &File,
    path: &Path,
    service_user: libc::uid_t,
    create: bool,
) -> Result<Option<File>, CredentialError> {
    let flags = libc::O_RDWR | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    let at = directory.as_raw_fd();
    let opened = if create {
        // SAFETY: `at` is a valid directory descriptor and `LOCK` is a NUL-terminated name;
        // mode `0o600` is passed because `O_CREAT` is set.
        let created = unsafe {
            libc::openat(
                at,
                LOCK.as_ptr(),
                flags | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )
        };
        match owned(created) {
            Ok(lock) => lock
                .set_permissions(Permissions::from_mode(0o600))
                .map(|()| lock),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // SAFETY: the same descriptor and name, without O_CREAT.
                owned(unsafe { libc::openat(at, LOCK.as_ptr(), flags) })
            }
            Err(error) => Err(error),
        }
    } else {
        // SAFETY: the same descriptor and name, without O_CREAT.
        match owned(unsafe { libc::openat(at, LOCK.as_ptr(), flags) }) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            opened => opened,
        }
    };
    let unavailable = |kind| refused(RootRule::LockUnavailable(kind), path);
    let lock = opened.map_err(|error| {
        let broken = match error.raw_os_error() {
            // A link, a directory or a device node at the lock's name is no lock file.
            Some(libc::ELOOP | libc::EISDIR | libc::ENXIO) => true,
            // What stands there, as far as it can be seen without opening it.
            _ => std::fs::symlink_metadata(path.join(LOCK_NAME)).is_ok_and(|found| {
                rules::lock_file(
                    found.is_file(),
                    found.uid(),
                    found.mode(),
                    found.nlink(),
                    service_user,
                )
                .is_err()
            }),
        };
        let rule = if broken {
            RootRule::LockFile
        } else {
            RootRule::LockUnavailable(error.kind())
        };
        refused(rule, path)
    })?;
    let found = lock.metadata().map_err(|error| unavailable(error.kind()))?;
    rules::lock_file(
        found.is_file(),
        found.uid(),
        found.mode(),
        found.nlink(),
        service_user,
    )
    .map_err(|rule| refused(rule, path))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(TryLockError::WouldBlock) => Err(refused(RootRule::InUse, path)),
        Err(TryLockError::Error(error)) => Err(unavailable(error.kind())),
    }
}
