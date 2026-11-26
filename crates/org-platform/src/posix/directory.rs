//! Opening, making, inspecting, renaming and removing names relative to a held directory
//! descriptor (the `*at` calls), and reading a directory's entries.
//!
//! The consumer keeps its own policy (which flags, which modes, which owner it accepts);
//! these only make each call safe to write: the directory is borrowed from its owner, a
//! name is a NUL-terminated [`CStr`], a new descriptor comes back owned, and a failed call
//! is the system's [`io::Error`].

use std::ffi::{CStr, CString};
use std::io;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, IntoRawFd, OwnedFd};

use super::check;

/// Open `path` (absolute, or relative to the working directory) with `flags` (`open`),
/// creating it with `mode` when `flags` has `O_CREAT`. The new descriptor is owned by the
/// caller; add `O_CLOEXEC` to keep it from a program this process starts.
pub fn open(path: &CStr, flags: libc::c_int, mode: libc::mode_t) -> io::Result<OwnedFd> {
    // SAFETY: `path` is NUL-terminated and live for the call; the mode is passed as the
    // promoted unsigned int the variadic argument expects, and read only with O_CREAT.
    let fd = check(unsafe { libc::open(path.as_ptr(), flags, libc::c_uint::from(mode)) })?;
    // SAFETY: a successful open returns a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Open `name` relative to the directory `dir` with `flags` (`openat`), creating it with
/// `mode` when `flags` has `O_CREAT`. `O_NOFOLLOW` refuses a symbolic link in the last
/// component; `O_DIRECTORY` refuses anything but a directory. The new descriptor is owned
/// by the caller.
pub fn open_at(
    dir: impl AsFd,
    name: &CStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> io::Result<OwnedFd> {
    // SAFETY: `dir` is borrowed from its owner and `name` is NUL-terminated, both live for
    // the call; the mode is the promoted unsigned int, read only with O_CREAT.
    let fd = check(unsafe {
        libc::openat(
            dir.as_fd().as_raw_fd(),
            name.as_ptr(),
            flags,
            libc::c_uint::from(mode),
        )
    })?;
    // SAFETY: a successful openat returns a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Make the directory `name` in `dir` with `mode`, less the process's mask (`mkdirat`).
/// An existing name is [`io::ErrorKind::AlreadyExists`].
pub fn make_directory_at(dir: impl AsFd, name: &CStr, mode: libc::mode_t) -> io::Result<()> {
    // SAFETY: `dir` is borrowed from its owner and `name` is NUL-terminated for the call.
    check(unsafe { libc::mkdirat(dir.as_fd().as_raw_fd(), name.as_ptr(), mode) }).map(drop)
}

/// The status of the open descriptor `fd` (`fstat`).
pub fn status(fd: impl AsFd) -> io::Result<libc::stat> {
    // SAFETY: stat is plain data for which all-zero bytes are a valid value.
    let mut found: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `fd` is borrowed from its owner; `found` is a live, exclusively borrowed
    // stat record of the size fstat writes.
    check(unsafe { libc::fstat(fd.as_fd().as_raw_fd(), &mut found) })?;
    Ok(found)
}

/// The status of a descriptor number (`fstat`), for a check that looks over numbers it
/// does not own (which of them name a file). A number that is not open is an error.
pub fn status_of_number(fd: std::os::fd::RawFd) -> io::Result<libc::stat> {
    // SAFETY: stat is plain data for which all-zero bytes are a valid value.
    let mut found: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fstat only reads the descriptor and writes the live, exclusively borrowed
    // stat record; any number is a valid argument.
    check(unsafe { libc::fstat(fd, &mut found) })?;
    Ok(found)
}

/// The status of `name` in `dir` (`fstatat`). With `AT_SYMLINK_NOFOLLOW` in `flags` a
/// symbolic link is described, not followed.
pub fn status_at(dir: impl AsFd, name: &CStr, flags: libc::c_int) -> io::Result<libc::stat> {
    // SAFETY: stat is plain data for which all-zero bytes are a valid value.
    let mut found: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `dir` is borrowed from its owner, `name` is NUL-terminated and `found` is a
    // live, exclusively borrowed stat record of the size fstatat writes.
    check(unsafe { libc::fstatat(dir.as_fd().as_raw_fd(), name.as_ptr(), &mut found, flags) })?;
    Ok(found)
}

/// Remove `name` from `dir` (`unlinkat`): a file, or with `AT_REMOVEDIR` in `flags` an
/// empty directory. A symbolic link is removed, never followed.
pub fn remove_at(dir: impl AsFd, name: &CStr, flags: libc::c_int) -> io::Result<()> {
    // SAFETY: `dir` is borrowed from its owner and `name` is NUL-terminated for the call.
    check(unsafe { libc::unlinkat(dir.as_fd().as_raw_fd(), name.as_ptr(), flags) }).map(drop)
}

/// Rename `from` in `from_dir` to `to` in `to_dir` (`renameat`), replacing what `to`
/// named. Neither name's symbolic link is followed. For a rename that never replaces,
/// see [`crate::fs::rename_noreplace_at`].
pub fn rename_at(from_dir: impl AsFd, from: &CStr, to_dir: impl AsFd, to: &CStr) -> io::Result<()> {
    // SAFETY: both directories are borrowed from their owners and both names are
    // NUL-terminated, all live for the call.
    check(unsafe {
        libc::renameat(
            from_dir.as_fd().as_raw_fd(),
            from.as_ptr(),
            to_dir.as_fd().as_raw_fd(),
            to.as_ptr(),
        )
    })
    .map(drop)
}

/// Make a FIFO (named pipe) at `path` with `mode`, less the process's mask (`mkfifo`).
pub fn make_fifo(path: &CStr, mode: libc::mode_t) -> io::Result<()> {
    // SAFETY: `path` is NUL-terminated and live for the call.
    check(unsafe { libc::mkfifo(path.as_ptr(), mode) }).map(drop)
}

/// The entries of a directory, read from a new open description of it (so its offset is
/// its own: a copy made with `dup` would share and exhaust the original's). `.` and `..`
/// are left out; each other name comes back as it is stored, which may not be UTF-8.
///
/// The stream is closed when this is dropped.
#[derive(Debug)]
pub struct Entries {
    stream: std::ptr::NonNull<libc::DIR>,
}

impl Entries {
    /// Opens a new description of the directory `dir` (`openat(".")` without following a
    /// link) and a stream over it (`fdopendir`).
    pub fn of(dir: impl AsFd) -> io::Result<Self> {
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        let own = open_at(dir, c".", flags, 0)?;
        let fd = own.into_raw_fd();
        // SAFETY: `fd` is an open directory descriptor this function owns; fdopendir takes
        // ownership of it only when it succeeds.
        let stream = unsafe { libc::fdopendir(fd) };
        match std::ptr::NonNull::new(stream) {
            Some(stream) => Ok(Self { stream }),
            None => {
                let error = io::Error::last_os_error();
                // SAFETY: fdopendir failed, so `fd` is still owned here; this takes it back
                // once, to close it.
                drop(unsafe { OwnedFd::from_raw_fd(fd) });
                Err(error)
            }
        }
    }
}

impl Iterator for Entries {
    type Item = io::Result<CString>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // readdir reports the end and a failure the same way; errno tells them apart.
            if let Err(error) = crate::fs::set_errno(0) {
                return Some(Err(error));
            }
            // SAFETY: `stream` is the live stream this value owns, read only through
            // `&mut self`, so no other call uses it meanwhile.
            let entry = unsafe { libc::readdir(self.stream.as_ptr()) };
            if entry.is_null() {
                let error = io::Error::last_os_error();
                return match error.raw_os_error() {
                    Some(0) | None => None,
                    Some(_) => Some(Err(error)),
                };
            }
            // SAFETY: a non-null readdir result points at an entry whose `d_name` is a
            // NUL-terminated string, valid until the next readdir or closedir on this
            // stream; it is copied out before either.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if matches!(name.to_bytes(), b"." | b"..") {
                continue;
            }
            return Some(Ok(name.to_owned()));
        }
    }
}

impl Drop for Entries {
    fn drop(&mut self) {
        // SAFETY: this value owns the stream returned by fdopendir and closes it once.
        unsafe { libc::closedir(self.stream.as_ptr()) };
    }
}
