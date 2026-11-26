//! The unit directory held open: a definition is read, published and removed relative to
//! the descriptor, never again by its path.

use std::{
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
};

use crate::service::ServiceError;

/// The placement's directory, held open by [`Systemd::directory`], which judged it: a
/// directory of this user that other users cannot write to. A definition is read, published
/// and removed relative to this descriptor and never again by its path, so an entry renamed
/// or replaced meanwhile is never the one changed, and the mode is set on the open file
/// (X5 review, fourth round, P2-5).
///
/// [`Systemd::directory`]: super::systemd::Systemd::directory
pub(super) struct UnitDirectory {
    pub(super) file: fs::File,
    pub(super) path: PathBuf,
}

impl UnitDirectory {
    fn entry(&self, name: &str) -> Result<std::ffi::CString, ServiceError> {
        std::ffi::CString::new(name)
            .map_err(|_| ServiceError::Invalid("a unit name holds a NUL byte"))
    }

    /// The bytes of the regular file `name`, opened without following a link; `None` when
    /// there is none.
    pub(super) fn read(&self, name: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        use std::io::Read;
        use std::os::fd::{AsRawFd, FromRawFd};
        let shown = self.path.join(name);
        let entry = self.entry(name)?;
        // SAFETY: the held directory and a NUL-terminated single name.
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                entry.as_ptr(),
                libc::O_RDONLY
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | libc::O_NOCTTY
                    | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let error = io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(libc::ENOENT) => Ok(None),
                Some(libc::ELOOP) => Err(ServiceError::Conflict(format!(
                    "{} is not a regular file",
                    shown.display()
                ))),
                _ => Err(ServiceError::System(format!(
                    "cannot read {}: {error}",
                    shown.display()
                ))),
            };
        }
        // SAFETY: a new descriptor owned only here.
        let file = unsafe { fs::File::from_raw_fd(fd) };
        let system =
            |e: io::Error| ServiceError::System(format!("cannot read {}: {e}", shown.display()));
        if !file.metadata().map_err(system)?.is_file() {
            return Err(ServiceError::Conflict(format!(
                "{} is not a regular file",
                shown.display()
            )));
        }
        let mut bytes = Vec::new();
        file.take(MAX_DEFINITION + 1)
            .read_to_end(&mut bytes)
            .map_err(system)?;
        if bytes.len() as u64 > MAX_DEFINITION {
            return Err(ServiceError::Conflict(format!(
                "{} is not a definition this adapter wrote",
                shown.display()
            )));
        }
        Ok(Some(bytes))
    }

    /// Publishes `bytes` as `name`: a complete temporary file (mode 0644, set on the open
    /// file whatever the process's mask is), flushed, then a hard link, which never
    /// replaces a definition another writer placed meanwhile; or, to `replace` the earlier
    /// rendering register found, a rename over it. The directory is flushed afterwards, so
    /// the definition is still there after a power loss.
    pub(super) fn publish(
        &self,
        name: &str,
        bytes: &[u8],
        replace: bool,
    ) -> Result<(), ServiceError> {
        use std::os::fd::{AsRawFd, FromRawFd};
        let shown = self.path.join(name);
        let system =
            |e: io::Error| ServiceError::System(format!("cannot write {}: {e}", shown.display()));
        let entry = self.entry(name)?;
        let temporary = self.entry(&format!("{name}.{}.tmp", std::process::id()))?;
        let directory = self.file.as_raw_fd();
        // SAFETY: `directory` is our held directory fd and `temporary` is a NUL-terminated single name.
        unsafe { libc::unlinkat(directory, temporary.as_ptr(), 0) };
        // SAFETY: `directory` is our held directory fd and `temporary` is a NUL-terminated single name.
        let fd = unsafe {
            libc::openat(
                directory,
                temporary.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o644 as libc::c_uint,
            )
        };
        if fd < 0 {
            return Err(system(io::Error::last_os_error()));
        }
        // SAFETY: a new descriptor owned only here.
        let mut file = unsafe { fs::File::from_raw_fd(fd) };
        let written = file
            .write_all(bytes)
            .and_then(|()| file.set_permissions(fs::Permissions::from_mode(0o644)))
            .and_then(|()| file.sync_all());
        drop(file);
        let published = written.map_err(system).and_then(|()| {
            let status = if replace {
                // SAFETY: `directory` is our held directory fd; both names are NUL-terminated single entries.
                unsafe { libc::renameat(directory, temporary.as_ptr(), directory, entry.as_ptr()) }
            } else {
                // SAFETY: `directory` is our held directory fd; both names are NUL-terminated single entries.
                unsafe { libc::linkat(directory, temporary.as_ptr(), directory, entry.as_ptr(), 0) }
            };
            if status == 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            Err(if error.kind() == io::ErrorKind::AlreadyExists {
                ServiceError::Conflict(format!(
                    "{} appeared while registering; it is left untouched",
                    shown.display()
                ))
            } else {
                system(error)
            })
        });
        // SAFETY: `directory` is our held directory fd and `temporary` is a NUL-terminated single name.
        unsafe { libc::unlinkat(directory, temporary.as_ptr(), 0) };
        published?;
        self.file.sync_all().map_err(system)
    }

    /// Removes the entry `name` (never a directory).
    pub(super) fn remove(&self, name: &str) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        let entry = std::ffi::CString::new(name).map_err(io::Error::other)?;
        // SAFETY: the held directory and a NUL-terminated single name.
        if unsafe { libc::unlinkat(self.file.as_raw_fd(), entry.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        self.file.sync_all()
    }
}

/// The longest definition read back: a rendering is a few hundred bytes.
const MAX_DEFINITION: u64 = 1 << 20;
