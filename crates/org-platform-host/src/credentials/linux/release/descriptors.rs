//! Raw descriptor probes of the release directory walk: opening directories without
//! following a link, `fstatfs`/`statx`/`fstatat` facts, the chain of directories up to
//! `/`, and reading a directory's names through a stream of its own.

use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::credentials::rules::Identity;

/// The most directories a walk up to `/` passes before it gives up: more than any
/// path the kernel resolves can have.
const DEEPEST: usize = 4096;

pub(super) fn owned(fd: libc::c_int) -> io::Result<File> {
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new descriptor that nothing else owns.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| io::ErrorKind::InvalidInput.into())
}

/// Opens the directory at the absolute path `path` for passing through. Only the
/// start of a walk is opened by path.
pub(super) fn open_absolute(path: &Path) -> io::Result<File> {
    let path = c_name(path.as_os_str())?;
    let flags = libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: a NUL-terminated path that outlives the call, which keeps nothing.
    owned(unsafe { libc::open(path.as_ptr(), flags) })
}

/// Opens the directory `name` in `parent`, never following a link: `access` is
/// `O_PATH` to pass through it or `O_RDONLY` to read it.
pub(super) fn open_at(
    parent: BorrowedFd<'_>,
    name: &OsStr,
    access: libc::c_int,
) -> io::Result<File> {
    let name = c_name(name)?;
    let flags = access | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: a valid directory descriptor and a NUL-terminated name that outlives
    // the call, which keeps nothing.
    owned(unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) })
}

/// The type number of the filesystem `fd` is on, as `fstatfs` gives it.
pub(super) fn filesystem_kind(fd: BorrowedFd<'_>) -> io::Result<u64> {
    // SAFETY: an all-zero `statfs` is a valid value for the call to overwrite.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor and a valid out-pointer.
    if unsafe { libc::fstatfs(fd.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // The number is 32 bits wide; where the field is a signed 32-bit word, a number
    // with the top bit set arrives sign-extended.
    Ok(stat.f_type as u64 & 0xffff_ffff)
}

pub(super) fn identity(found: &std::fs::Metadata) -> Identity {
    (found.dev(), found.ino())
}

/// The directory `fd` and every directory above it up to `/`, each identified through
/// a held descriptor: `..` is opened relative to the one before, until a directory is
/// its own parent.
pub(super) fn chain(fd: BorrowedFd<'_>) -> io::Result<Vec<Identity>> {
    // A descriptor of this function's own for the same open directory.
    let first = File::from(fd.try_clone_to_owned()?).metadata()?;
    if !first.is_dir() {
        return Err(io::ErrorKind::NotADirectory.into());
    }
    let mut chain = vec![identity(&first)];
    let mut above = open_at(fd, OsStr::new(".."), libc::O_PATH)?;
    loop {
        let next = identity(&above.metadata()?);
        if chain.last() == Some(&next) {
            return Ok(chain);
        }
        if chain.len() >= DEEPEST {
            return Err(io::ErrorKind::InvalidData.into());
        }
        chain.push(next);
        above = open_at(above.as_fd(), OsStr::new(".."), libc::O_PATH)?;
    }
}

/// What `statx` says of the directory `fd`: its attributes, and the mask of the
/// attributes this kernel reports.
pub(super) fn mount_attributes(fd: BorrowedFd<'_>) -> io::Result<(u64, u64)> {
    // SAFETY: an all-zero `statx` is a valid value for the call to overwrite.
    let mut stat: libc::statx = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor, an empty NUL-terminated path with `AT_EMPTY_PATH`
    // (the descriptor itself is examined), and a valid out-pointer.
    let status = unsafe {
        libc::statx(
            fd.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_TYPE,
            &mut stat,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((stat.stx_attributes, stat.stx_attributes_mask))
}

/// Whether the entry `name` of `directory` is a regular file. A link is not followed;
/// an entry that is gone again is no file.
pub(super) fn is_regular(directory: BorrowedFd<'_>, name: &OsStr) -> io::Result<bool> {
    let name = c_name(name)?;
    // SAFETY: an all-zero `stat` is a valid value for the call to overwrite.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a valid directory descriptor, a NUL-terminated name that outlives the
    // call, and a valid out-pointer.
    let status = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if status != 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(error)
        };
    }
    Ok(stat.st_mode & libc::S_IFMT == libc::S_IFREG)
}

/// Every name in `directory` but `.` and `..`, read through a descriptor of its own,
/// so that the held one's position is never moved.
pub(super) fn entries(directory: BorrowedFd<'_>) -> io::Result<Vec<OsString>> {
    let fresh = open_at(directory, OsStr::new("."), libc::O_RDONLY)?;
    // SAFETY: a valid directory descriptor. On success the stream owns it; on failure
    // `fresh` still does and closes it.
    let stream = unsafe { libc::fdopendir(fresh.as_raw_fd()) };
    if stream.is_null() {
        return Err(io::Error::last_os_error());
    }
    std::mem::forget(fresh);
    let stream = Stream(stream);
    let mut names = Vec::new();
    loop {
        // `readdir` reports the end of the directory and a failure the same way.
        org_platform::fs::set_errno(0)?;
        // SAFETY: `stream` owns a live directory stream that no other thread reads.
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(0) {
                return Err(error);
            }
            return Ok(names);
        }
        // SAFETY: `readdir`'s NUL-terminated name stays valid until the next call.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name != b"." && name != b".." {
            names.push(OsString::from_vec(name.to_vec()));
        }
    }
}

struct Stream(*mut libc::DIR);

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns the successful `fdopendir` result.
        unsafe {
            libc::closedir(self.0);
        }
    }
}
