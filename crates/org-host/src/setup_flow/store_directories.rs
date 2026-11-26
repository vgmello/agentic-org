use std::ffi::CString;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

/// The directories one call made, shallowest first, each with its held parent, and the
/// directory the whole path names, held.
#[derive(Debug)]
pub(super) struct Made {
    made: Vec<(OwnedFd, CString)>,
    directory: OwnedFd,
}

impl Made {
    /// Removes what the call made, deepest first, with the lock file the platform's
    /// check may have left in the store. Nothing that existed before is touched.
    pub(super) fn remove(self) {
        undo(self.made);
    }

    /// The directory the whole path names, held since the call that made it.
    pub(super) fn directory(&self) -> BorrowedFd<'_> {
        std::os::fd::AsFd::as_fd(&self.directory)
    }

    #[cfg(test)]
    pub(super) fn count(&self) -> usize {
        self.made.len()
    }
}

/// Removes the directories in `made` (shallowest first, each with its held parent),
/// deepest first, with the lock file the platform's check may have left in the deepest.
fn undo(made: Vec<(OwnedFd, CString)>) {
    {
        let mut deepest = true;
        for (parent, name) in made.into_iter().rev() {
            if deepest {
                // SAFETY: valid descriptor and NUL-terminated names; errors are ignored.
                unsafe {
                    let store = libc::openat(
                        parent.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    );
                    if store >= 0 {
                        libc::unlinkat(store, c".lock".as_ptr(), 0);
                        libc::close(store);
                    }
                }
                deepest = false;
            }
            // SAFETY: as above; a directory that is not empty stays.
            unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
        }
    }
}

/// Whether `directory` certainly has no entry `name` (of any kind, a link not
/// followed): only an answer that it does not exist says so.
pub(super) fn lacks(directory: BorrowedFd<'_>, name: &std::ffi::CStr) -> bool {
    // SAFETY: plain data that fstatat fills in.
    let mut found: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: an open directory, a NUL-terminated name and a writable stat.
    let status = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            &mut found,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    status != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT)
}

/// `name` as a directory, below `parent` or (without one) as a whole path: never a
/// symbolic link in its last component, and never anything but a directory, which is
/// refused at once (a FIFO is not opened and waited on).
pub(super) fn open_directory(parent: Option<&OwnedFd>, name: &CString) -> std::io::Result<OwnedFd> {
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: `name` is NUL-terminated and `parent`, when given, is an open directory.
    let fd = unsafe {
        match parent {
            Some(parent) => libc::openat(parent.as_raw_fd(), name.as_ptr(), flags),
            None => libc::open(name.as_ptr(), flags),
        }
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `fd` is a new descriptor owned only here.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn status(fd: BorrowedFd<'_>) -> std::io::Result<libc::stat> {
    // SAFETY: stat is plain data that fstat fills in; zeroed is a valid start.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: an open descriptor and a writable stat.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(stat)
}

/// Makes the directories of `path` that are missing, below ancestors judged first (see
/// the module). `path` is absolute and normalized, and `judged_from` is `path` or above
/// it. `data_root`, when given, is a directory the path must not pass through. A
/// refusal names the directory and the rule, and nothing was made.
pub(super) fn make(
    path: &Path,
    data_root: Option<BorrowedFd<'_>>,
    judged_from: &Path,
) -> Result<Made, String> {
    use std::os::fd::AsFd;
    let os = |at: &Path, e: std::io::Error| format!("{}: {e}", at.display());
    let rest = path
        .strip_prefix(judged_from)
        .map_err(|_| format!("{}: is not below {}", path.display(), judged_from.display()))?;
    let data = data_root
        .map(|root| status(root).map(|found| (found.st_dev, found.st_ino)))
        .transpose()
        .map_err(|e| format!("the data root: {e}"))?;
    // SAFETY: geteuid has no preconditions and cannot fail.
    let user = unsafe { libc::geteuid() };
    let judge = |at: &Path, fd: &OwnedFd| -> Result<(), String> {
        let found = status(fd.as_fd()).map_err(|e| os(at, e))?;
        let rule = if Some((found.st_dev, found.st_ino)) == data {
            "is inside the data root"
        } else if found.st_uid != 0 && found.st_uid != user {
            "is owned by another user"
        } else if found.st_mode & 0o002 != 0 {
            "is writable by others"
        } else {
            return Ok(());
        };
        Err(format!("{}: {rule}", at.display()))
    };
    let start = CString::new(judged_from.as_os_str().as_bytes())
        .map_err(|_| format!("{}: is not a path", judged_from.display()))?;
    let mut current = open_directory(None, &start).map_err(|e| os(judged_from, e))?;
    let mut at = judged_from.to_owned();
    judge(&at, &current)?;
    let mut made = Vec::new();
    for component in rest.components() {
        let Component::Normal(part) = component else {
            return Err(format!("{}: is not a normalized path", path.display()));
        };
        let name = CString::new(part.as_bytes())
            .map_err(|_| format!("{}: is not a path", path.display()))?;
        at.push(part);
        let next = match open_directory(Some(&current), &name) {
            Ok(next) if made.is_empty() => {
                judge(&at, &next)?;
                next
            }
            Ok(next) => next,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // SAFETY: an open, judged parent and a NUL-terminated single name.
                if unsafe { libc::mkdirat(current.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                    let error = std::io::Error::last_os_error();
                    undo(made);
                    return Err(os(&at, error));
                }
                let parent = current.try_clone().map_err(|e| os(&at, e));
                let opened = open_directory(Some(&current), &name).map_err(|e| os(&at, e));
                match (parent, opened) {
                    (Ok(parent), Ok(next)) => {
                        made.push((parent, name));
                        next
                    }
                    (parent, opened) => {
                        // SAFETY: as above; the directory this call just made.
                        unsafe {
                            libc::unlinkat(current.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR)
                        };
                        undo(made);
                        return Err(parent.err().or(opened.err()).unwrap_or_default());
                    }
                }
            }
            Err(e) => {
                undo(made);
                // A symbolic link, or something that is not a directory, on the way.
                return Err(format!(
                    "{}: {} is not a directory this user's store can be below ({e})",
                    path.display(),
                    at.display()
                ));
            }
        };
        current = next;
    }
    Ok(Made {
        made,
        directory: current,
    })
}
