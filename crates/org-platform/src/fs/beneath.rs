//! Component-wise `O_NOFOLLOW` open and atomic `0600` regular-file writes strictly beneath a
//! canonical root directory (shared by the macOS and GNU/Linux adapters).

use std::ffi::{CStr, CString, OsStr};
use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

use super::shared::{entry_kind_at, entry_name, normalize_lstat_path, unlink_at, EntryKind};

static TEMP_FILE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Descends one plain directory segment relative to `dir` without following symlinks or
/// blocking on a FIFO (`O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK`).
fn open_dir_component_at(dir: &File, name: &OsStr) -> io::Result<File> {
    let cname = entry_name(name)?;
    // SAFETY: `dir` is an open directory descriptor owned by the caller and `cname` is a live
    // NUL-terminated single path component; `openat` returns a fresh descriptor or -1.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            cname.as_ptr(),
            libc::O_RDONLY
                | libc::O_DIRECTORY
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        let err = io::Error::last_os_error();
        // Linux returns `ENOTDIR` (rather than `ELOOP`) when `O_DIRECTORY | O_NOFOLLOW` hits a
        // symlink; normalize symlinks to `ELOOP` so callers see a uniform symlink refusal across
        // Linux and macOS.
        if err.raw_os_error() == Some(libc::ENOTDIR)
            && matches!(entry_kind_at(dir, &cname), Ok(EntryKind::Symlink))
        {
            return Err(io::Error::from_raw_os_error(libc::ELOOP));
        }
        return Err(err);
    }
    // SAFETY: `fd` was just opened by this function and nothing else owns it.
    let child = unsafe { File::from_raw_fd(fd) };
    if !child.metadata()?.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the opened component is not a directory",
        ));
    }
    Ok(child)
}

/// Walks every intermediate component of `relative` beneath `root` without following any
/// symbolic link, returning the open parent directory descriptor and the validated leaf name.
fn walk_beneath(root: &Path, relative: &Path) -> io::Result<(File, CString)> {
    let raw = relative.as_os_str().as_bytes();
    if raw.is_empty() || raw.ends_with(b"/") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "relative file path must name a file without a trailing slash",
        ));
    }
    let mut segments: Vec<&OsStr> = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(seg) => segments.push(seg),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "relative path beneath root must contain plain segments only",
                ));
            }
        }
    }
    let Some((leaf, parents)) = segments.split_last() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "relative file path is empty",
        ));
    };
    let mut dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(normalize_lstat_path(root))?;
    for seg in parents {
        dir = open_dir_component_at(&dir, seg)?;
    }
    Ok((dir, entry_name(leaf)?))
}

/// Opens a regular file at `relative` beneath canonical directory `root`, walking every
/// intermediate directory and the leaf with `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC` and verifying
/// `S_ISREG` on the opened descriptor so symlinks, FIFOs, devices, sockets and directories are
/// refused without TOCTOU.
pub(super) fn open_regular_beneath(root: &Path, relative: &Path) -> io::Result<File> {
    let (dir, leaf) = walk_beneath(root, relative)?;
    // SAFETY: `dir` is an open directory descriptor we own and `leaf` is a live NUL-terminated
    // single component; `openat` returns a fresh descriptor or -1.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            leaf.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` was just opened by this function and nothing else owns it.
    let file = unsafe { File::from_raw_fd(fd) };
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "target is not a regular file",
        ));
    }
    Ok(file)
}

/// RAII cleanup guard that unlinks a sibling temporary file inside `dir` unless disarmed after
/// atomic publish succeeds.
struct BeneathTempGuard<'a> {
    dir: &'a File,
    name: Option<CString>,
}

impl<'a> BeneathTempGuard<'a> {
    fn new(dir: &'a File, name: CString) -> Self {
        Self {
            dir,
            name: Some(name),
        }
    }

    fn disarm(&mut self) {
        self.name = None;
    }
}

impl Drop for BeneathTempGuard<'_> {
    fn drop(&mut self) {
        if let Some(name) = self.name.take() {
            let _ = unlink_at(self.dir, &name);
        }
    }
}

/// Creates a brand-new sibling temporary regular file inside `dir` with mode `0600` (`O_CREAT |
/// O_EXCL | O_NOFOLLOW | O_CLOEXEC`), retrying on `EEXIST`.
fn create_exclusive_temp_at(dir: &File) -> io::Result<(File, CString)> {
    let pid = std::process::id();
    for _ in 0..64 {
        let seq = TEMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed);
        let name = CString::new(format!(".org-tmp.{pid}.{seq}")).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid temporary file name")
        })?;
        // SAFETY: `dir` is an open directory descriptor we own and `name` is a live NUL-terminated
        // single component; `O_CREAT` passes a `c_uint` mode argument per POSIX variadic `openat`.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600 as libc::c_uint,
            )
        };
        if fd >= 0 {
            // SAFETY: `fd` was just opened exclusively by this function and nothing else owns it.
            return Ok((unsafe { File::from_raw_fd(fd) }, name));
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::AlreadyExists {
            return Err(err);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary file in destination directory",
    ))
}

/// Writes `bytes` into an exclusive `0600` sibling temporary file in `dir` and atomically publishes
/// it to `leaf` via `rename_noreplace_at` (when creating) or `renameat` (when replacing a regular
/// file, never following symlinks). Any error unlinks the temporary file before returning.
pub(super) fn stage_and_publish_at(
    dir: &File,
    leaf: &CStr,
    bytes: &[u8],
    replacing: bool,
) -> io::Result<()> {
    let (mut tmp_file, tmp_name) = create_exclusive_temp_at(dir)?;
    let mut guard = BeneathTempGuard::new(dir, tmp_name.clone());
    // SAFETY: `tmp_file` is an open descriptor owned by this function; `fchmod` sets mode `0600`
    // directly on the inode regardless of process umask.
    if unsafe { libc::fchmod(tmp_file.as_raw_fd(), libc::S_IRUSR | libc::S_IWUSR) } != 0 {
        return Err(io::Error::last_os_error());
    }
    tmp_file.write_all(bytes)?;
    drop(tmp_file);

    if replacing {
        match entry_kind_at(dir, leaf)? {
            EntryKind::Regular => {}
            EntryKind::Symlink => return Err(io::Error::from_raw_os_error(libc::ELOOP)),
            EntryKind::Missing | EntryKind::Directory | EntryKind::Other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "output target is not a regular file",
                ));
            }
        }
        // SAFETY: `dir` is an open directory descriptor we own and both names are live
        // NUL-terminated single components; POSIX `renameat` replaces the directory entry without
        // ever dereferencing a symlink at `leaf`.
        if unsafe {
            libc::renameat(
                dir.as_raw_fd(),
                tmp_name.as_ptr(),
                dir.as_raw_fd(),
                leaf.as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
    } else {
        super::rename_noreplace_at(dir, &tmp_name, dir, leaf)?;
    }
    guard.disarm();
    Ok(())
}

/// Atomically writes `bytes` to a regular file at `relative` beneath canonical directory `root`
/// with mode `0600`, walking `root` and every intermediate directory with `O_NOFOLLOW`, refusing
/// symlinks, directories, FIFOs, devices, and sockets, and leaving no temporary or partial file on
/// failure.
pub(super) fn write_regular_beneath(root: &Path, relative: &Path, bytes: &[u8]) -> io::Result<()> {
    let (dir, leaf) = walk_beneath(root, relative)?;
    let replacing = match entry_kind_at(&dir, &leaf)? {
        EntryKind::Missing => false,
        EntryKind::Regular => true,
        EntryKind::Symlink => return Err(io::Error::from_raw_os_error(libc::ELOOP)),
        EntryKind::Directory | EntryKind::Other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "output target is not a regular file",
            ));
        }
    };
    stage_and_publish_at(&dir, &leaf, bytes, replacing)
}
