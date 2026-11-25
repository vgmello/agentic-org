//! Durable flushes, exclusive renames and filesystem checks.
//!
//! Linux host design section 4, `fs`. The consumers are `org-host` (the instance
//! registry, the release store, the host database, the writer marker, the recovery
//! package export), `org-office` (durable publication), and `org-client` (the agent spool).
//! Each keeps its policy: what is flushed and when, which
//! names are renamed, what a refusal means to a person. This module only asks the
//! system, and takes plain data (a descriptor, C strings, a path to name in a refusal).
//!
//! # Systems
//!
//! - macOS: `F_FULLFSYNC`, `renamex_np` and `renameatx_np` with `RENAME_EXCL`,
//!   CoreFoundation's name comparison. An instance root has no filesystem rule.
//! - GNU/Linux: `fsync`, `renameat2` with `RENAME_NOREPLACE`, and the instance-root
//!   rules: no case-folding directory on the path, and a root on ext4, XFS without
//!   case-insensitive names, Btrfs, F2FS or overlayfs.
//! - Every other Unix target: each function returns the `fs` refusal
//!   ([`crate::Refused`], as an [`io::ErrorKind::Unsupported`] error where the function
//!   returns `io::Result`).
//!
//! The `open(2)` flags and error numbers below come from `libc` and exist on every
//! Unix target, so they have no adapter.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(
        dead_code,
        reason = "refusing fallback module is used on unsupported targets and in unit tests"
    )
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

#[cfg(all(
    test,
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))
))]
mod adapter_tests;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;

use crate::Refused;
use std::ffi::CStr;
use std::fmt;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// `open(2)`: fail instead of blocking (on a FIFO or a device).
pub const O_NONBLOCK: i32 = libc::O_NONBLOCK;
/// `open(2)`: fail with [`ELOOP`] when the last path component is a symbolic link.
pub const O_NOFOLLOW: i32 = libc::O_NOFOLLOW;
/// `open(2)`: close the descriptor in a program this process starts.
pub const O_CLOEXEC: i32 = libc::O_CLOEXEC;
/// The error number of an [`O_NOFOLLOW`] open that met a symbolic link.
pub const ELOOP: i32 = libc::ELOOP;
/// The error number of a call that would block.
pub const EAGAIN: i32 = libc::EAGAIN;

/// Flush `file`'s data and metadata to the storage device itself: `File::sync_all`,
/// and on macOS then `F_FULLFSYNC`, which also empties the drive's write cache. A
/// failure of either step is returned; a failed full flush is never downgraded. (On
/// macOS `sync_all` is itself a device flush, so the device is flushed twice per call,
/// as the host's and the office's own copies of this code did before they moved here.)
pub fn full_sync(file: &File) -> io::Result<()> {
    native::full_sync(file)
}

/// Rename the entry `from` of the directory `from_dir` to `to` in `to_dir`, never
/// replacing an entry that is already there (`EEXIST`, as
/// [`io::ErrorKind::AlreadyExists`]). A filesystem that cannot rename exclusively
/// refuses: the system's error is returned, and nothing is linked and unlinked instead.
pub fn rename_noreplace_at(
    from_dir: &File,
    from: &CStr,
    to_dir: &File,
    to: &CStr,
) -> io::Result<()> {
    native::rename_noreplace_at(from_dir, from, to_dir, to)
}

/// [`rename_noreplace_at`] for two paths, each absolute or relative to the working
/// directory.
pub fn rename_noreplace(from: &CStr, to: &CStr) -> io::Result<()> {
    native::rename_noreplace(from, to)
}

/// Set this thread's `errno`. `readdir` reports the end of a directory and a failure
/// the same way, so its callers clear `errno` before each call.
pub fn set_errno(value: i32) -> io::Result<()> {
    native::set_errno(value)
}

/// The device and inode numbers of a `stat` result, each as the `u64` that
/// `std::os::unix::fs::MetadataExt` reports for the same file (`st_dev` is narrower and
/// signed on macOS).
pub fn device_and_inode(stat: &libc::stat) -> io::Result<(u64, u64)> {
    native::device_and_inode(stat)
}

/// Why a directory cannot hold what the consumer wants to keep in it.
#[derive(Debug)]
pub enum FilesystemRefusal {
    /// This build target has no adapter; nothing was probed.
    Unsupported(Refused),
    /// The directory folds the case of the names in it.
    CaseInsensitive(PathBuf),
    /// The directory is on a filesystem outside the supported set. `kind` is the
    /// system's number for the filesystem type.
    UnsupportedFilesystem { path: PathBuf, kind: u64 },
    /// The directory is on a memory filesystem, which does not persist.
    MemoryBacked { path: PathBuf, kind: u64 },
    /// A probe of the directory failed, so nothing is known about it.
    Probe {
        path: PathBuf,
        probe: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for FilesystemRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(refused) => refused.fmt(f),
            Self::CaseInsensitive(path) => {
                write!(f, "{} is a case-insensitive directory", path.display())
            }
            Self::UnsupportedFilesystem { path, kind } => write!(
                f,
                "{} is on an unsupported filesystem (type {kind:#x})",
                path.display()
            ),
            Self::MemoryBacked { path, kind } => write!(
                f,
                "{} is on a memory filesystem (type {kind:#x})",
                path.display()
            ),
            Self::Probe {
                path,
                probe,
                source,
            } => write!(f, "cannot probe {} with {probe}: {source}", path.display()),
        }
    }
}

impl std::error::Error for FilesystemRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unsupported(refused) => Some(refused),
            Self::Probe { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Check one directory on the way to an instance root. The consumer walks from `/` to
/// the root and calls this for each directory it opens: `dir` is the open directory,
/// `path` names it in a refusal, and `root` is true for the root itself, or for the
/// existing parent a new root is about to be created in.
///
/// GNU/Linux refuses a case-folding directory anywhere on the path
/// (`FS_IOC_GETFLAGS`), and for the root a filesystem other than ext4, XFS, Btrfs, F2FS
/// or overlayfs (`fstatfs`) and an XFS with case-insensitive names
/// (`XFS_IOC_FSGEOMETRY`); a probe that fails refuses. macOS has no rule here: its
/// volumes may fold case, and the consumer compares names with
/// `org_platform_host::names::paths_may_alias`.
pub fn check_instance_root(dir: &File, path: &Path, root: bool) -> Result<(), FilesystemRefusal> {
    native::check_instance_root(dir, path, root)
}

/// Require that `dir` is on a filesystem that persists. GNU/Linux refuses `tmpfs`,
/// `ramfs` and `hugetlbfs`. macOS has no such check and accepts every directory.
pub fn require_disk(dir: &File, path: &Path) -> Result<(), FilesystemRefusal> {
    native::require_disk(dir, path)
}

/// Restrict an existing directory at `path` so only its owner can read, write, or enter it
/// (mode `0700` on macOS and GNU/Linux). Every unsupported target fails closed with the
/// `fs` refusal rather than silently leaving directory permissions unhardened.
pub fn set_owner_only_dir(path: &Path) -> io::Result<()> {
    native::set_owner_only_dir(path)
}

/// Atomically create a new directory at `path` accessible only to its owner (mode `0700` on
/// macOS and GNU/Linux, with no `0755` window). Every unsupported target fails closed.
pub fn create_owner_only_dir(path: &Path) -> io::Result<()> {
    native::create_owner_only_dir(path)
}
