//! The one way the spool reads a message or metadata file: never follows a symlink, never blocks
//! on a FIFO or device, and never reads more than a cap, whatever another process (or a v1-old
//! CLI) left in a spool directory.

use std::fs::{self, OpenOptions};
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
