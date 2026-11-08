//! The one way the spool reads a message or metadata file: never follows a symlink, never blocks
//! on a FIFO or device, and never reads more than a cap, whatever another process (or a v1-old
//! CLI) left in a spool directory.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

// `open(2)` flags, from the platform headers (no `libc` dependency). `O_RDONLY` is 0 everywhere
// and comes from `.read(true)`; std masks the access mode out of `custom_flags`.
#[cfg(target_vendor = "apple")]
mod flags {
    pub const O_NONBLOCK: i32 = 0x0004;
    pub const O_NOFOLLOW: i32 = 0x0100;
    pub const O_CLOEXEC: i32 = 0x0100_0000;
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "x86", target_arch = "riscv64")
))]
mod flags {
    pub const O_NONBLOCK: i32 = 0o4000;
    pub const O_NOFOLLOW: i32 = 0o400000;
    pub const O_CLOEXEC: i32 = 0o2000000;
}

#[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")))]
mod flags {
    pub const O_NONBLOCK: i32 = 0o4000;
    pub const O_NOFOLLOW: i32 = 0o100000;
    pub const O_CLOEXEC: i32 = 0o2000000;
}

#[cfg(not(any(
    target_vendor = "apple",
    all(
        target_os = "linux",
        any(
            target_arch = "x86_64",
            target_arch = "x86",
            target_arch = "riscv64",
            target_arch = "aarch64",
            target_arch = "arm"
        )
    )
)))]
compile_error!("org-spool: add this platform's O_NONBLOCK / O_NOFOLLOW / O_CLOEXEC values");

/// What [`read_spool_file`] found at a path.
#[derive(Debug)]
pub(crate) enum SpoolFile {
    /// Nothing there (any more).
    Missing,
    /// A regular file's contents, at most the cap.
    Bytes(Vec<u8>),
    /// A symlink, FIFO, socket, device or directory, or a file over the cap: never read, and
    /// handled like a corrupt file. The string says why.
    Rejected(String),
}

/// Open `path` with `O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, check on the open
/// descriptor (`fstat`) that it is a regular file of at most `max` bytes, and read at most
/// `max + 1` bytes (the file may grow after the check). An I/O error on a regular file (e.g.
/// permissions) is still an error.
///
/// `O_NOFOLLOW` covers only the last path component: a symlinked spool directory (e.g.
/// `inbox/<agent>` or `~/.agency` itself) is still followed. That's within the threat model
/// (the spool belongs to one user, mode 0700); the point is that a file someone drops into a
/// spool directory can't make us read elsewhere or block.
pub(crate) fn read_spool_file(path: &Path, max: usize) -> io::Result<SpoolFile> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(flags::O_NOFOLLOW | flags::O_NONBLOCK | flags::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(SpoolFile::Missing),
        // O_NOFOLLOW fails on a symlink (ELOOP); a socket can't be opened at all. Neither is a
        // spool file; anything else (a regular file we may not read) is a real I/O error.
        Err(e) => {
            return match fs::symlink_metadata(path) {
                Ok(meta) if !meta.file_type().is_file() => Ok(SpoolFile::Rejected(format!(
                    "not a regular file ({})",
                    describe(meta.file_type())
                ))),
                Err(missing) if missing.kind() == ErrorKind::NotFound => Ok(SpoolFile::Missing),
                _ => Err(e),
            };
        }
    };
    let meta = file.metadata()?;
    if !meta.file_type().is_file() {
        return Ok(SpoolFile::Rejected(format!(
            "not a regular file ({})",
            describe(meta.file_type())
        )));
    }
    let too_large = || SpoolFile::Rejected(format!("file larger than {max} bytes"));
    if meta.len() > max as u64 {
        return Ok(too_large());
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Ok(too_large());
    }
    Ok(SpoolFile::Bytes(bytes))
}

/// Open `path` with `options` plus `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC` and require a
/// regular file (checked on the descriptor), for files that are written as well as read
/// (`relay.lock`): a symlink planted there must never redirect a write or a truncation. Any
/// other kind of file is an `InvalidData` error naming it.
pub(crate) fn open_regular_nofollow(options: &mut OpenOptions, path: &Path) -> io::Result<File> {
    let not_regular = |kind: fs::FileType| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "{} is not a regular file ({}); remove it",
                path.display(),
                describe(kind)
            ),
        )
    };
    let file = match options
        .custom_flags(flags::O_NOFOLLOW | flags::O_NONBLOCK | flags::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(e) => {
            return Err(match fs::symlink_metadata(path) {
                Ok(meta) if !meta.file_type().is_file() => not_regular(meta.file_type()),
                _ => e,
            });
        }
    };
    let kind = file.metadata()?.file_type();
    if !kind.is_file() {
        return Err(not_regular(kind));
    }
    Ok(file)
}

fn describe(kind: fs::FileType) -> &'static str {
    use std::os::unix::fs::FileTypeExt;
    if kind.is_symlink() {
        "symlink"
    } else if kind.is_dir() {
        "directory"
    } else if kind.is_fifo() {
        "FIFO"
    } else if kind.is_socket() {
        "socket"
    } else if kind.is_block_device() || kind.is_char_device() {
        "device"
    } else {
        "special file"
    }
}
