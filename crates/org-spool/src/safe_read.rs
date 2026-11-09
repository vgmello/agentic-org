//! The one way the spool reads a message or metadata file: never follows a symlink, never blocks
//! on a FIFO or device, and never reads more than a cap, whatever another process (or a v1-old
//! CLI) left in a spool directory.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

// `open(2)` flags, from the platform headers (no `libc` dependency). `O_RDONLY` is 0 everywhere
// and comes from `.read(true)`; std masks the access mode out of `custom_flags`.
#[cfg(target_vendor = "apple")]
mod flags {
    pub const O_NONBLOCK: i32 = 0x0004;
    pub const O_NOFOLLOW: i32 = 0x0100;
    pub const O_CLOEXEC: i32 = 0x0100_0000;
    pub const ELOOP: i32 = 62;
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "x86", target_arch = "riscv64")
))]
mod flags {
    pub const O_NONBLOCK: i32 = 0o4000;
    pub const O_NOFOLLOW: i32 = 0o400000;
    pub const O_CLOEXEC: i32 = 0o2000000;
    pub const ELOOP: i32 = 40;
}

#[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")))]
mod flags {
    pub const O_NONBLOCK: i32 = 0o4000;
    pub const O_NOFOLLOW: i32 = 0o100000;
    pub const O_CLOEXEC: i32 = 0o2000000;
    pub const ELOOP: i32 = 40;
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
compile_error!("org-spool: add this platform's O_NONBLOCK / O_NOFOLLOW / O_CLOEXEC / ELOOP values");

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
    read_checked(path, max, false, 0)
}

/// Office fallback files additionally require this euid, exactly 0600, one link, and
/// lstat before open. The descriptor checks repeat the policy after O_NOFOLLOW open.
pub(crate) fn read_office_spool_file(path: &Path, max: usize, uid: u32) -> io::Result<SpoolFile> {
    read_checked(path, max, true, uid)
}

pub(crate) fn office_file_policy(meta: &fs::Metadata, uid: u32) -> Result<(), &'static str> {
    if !meta.is_file() {
        return Err("not a regular file");
    }
    if meta.uid() != uid {
        return Err("wrong file owner");
    }
    if meta.mode() & 0o7777 != 0o600 {
        return Err("file mode is not 0600");
    }
    if meta.nlink() != 1 {
        return Err("file has multiple hard links");
    }
    Ok(())
}

fn read_checked(path: &Path, max: usize, office: bool, uid: u32) -> io::Result<SpoolFile> {
    let before = if office {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(SpoolFile::Missing),
            Err(e) => return Err(e),
        };
        if let Err(reason) = office_file_policy(&meta, uid) {
            return Ok(SpoolFile::Rejected(reason.into()));
        }
        Some(meta)
    } else {
        None
    };

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
    if office {
        if let Err(reason) = office_file_policy(&meta, uid) {
            return Ok(SpoolFile::Rejected(reason.into()));
        }
        if before
            .as_ref()
            .is_some_and(|before| before.dev() != meta.dev() || before.ino() != meta.ino())
        {
            return Err(io::Error::other("spool entry changed"));
        }
    }
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

/// Create `path` (mode 0600) and write `bytes`, with `O_CREAT | O_EXCL | O_NOFOLLOW`: fails
/// if anything (a file, a symlink, a FIFO) is already there, so a planted symlink can never
/// redirect the write.
pub(crate) fn create_new_nofollow(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(flags::O_NOFOLLOW | flags::O_CLOEXEC)
        .open(path)?;
    file.write_all(bytes)
}

/// Whether an error reading a spool file is about the file itself, so it will fail the same
/// way every time (dead-letter it): no permission (EACCES/EPERM) or a symlink (ELOOP).
/// Anything else (EMFILE, ENFILE, EIO, EINTR, EAGAIN, ENOMEM, ...) may pass: leave the file
/// and retry it later. (Not a regular file, over the cap and invalid content are not errors
/// here: [`SpoolFile::Rejected`] and the envelope check cover them.)
pub(crate) fn is_permanent_file_error(e: &io::Error) -> bool {
    e.kind() == ErrorKind::PermissionDenied || e.raw_os_error() == Some(flags::ELOOP)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_errors_about_the_file_itself_are_permanent() {
        const EPERM: i32 = 1;
        const EACCES: i32 = 13;
        // The platform's real ELOOP: an O_NOFOLLOW open of a symlink.
        let dir = std::env::temp_dir().join(format!("osr_eloop_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(dir.join("target"), dir.join("link")).unwrap();
        let eloop = OpenOptions::new()
            .read(true)
            .custom_flags(flags::O_NOFOLLOW)
            .open(dir.join("link"))
            .unwrap_err();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(eloop.raw_os_error(), Some(flags::ELOOP));
        assert!(is_permanent_file_error(&eloop));
        for code in [EPERM, EACCES, flags::ELOOP] {
            assert!(
                is_permanent_file_error(&io::Error::from_raw_os_error(code)),
                "errno {code}"
            );
        }
        // EINTR, EIO, ENOMEM, ENFILE, EMFILE, EAGAIN (macOS 35, Linux 11).
        let eagain = if cfg!(target_vendor = "apple") {
            35
        } else {
            11
        };
        for code in [4, 5, 12, 23, 24, eagain] {
            assert!(
                !is_permanent_file_error(&io::Error::from_raw_os_error(code)),
                "errno {code}"
            );
        }
        assert!(!is_permanent_file_error(&io::Error::other("transient")));
    }
}

#[cfg(test)]
mod office_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn wrong_owner_is_refused_even_with_safe_mode_and_type() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("file");
        fs::write(&path, b"private").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let meta = fs::symlink_metadata(&path).unwrap();
        assert_eq!(office_file_policy(&meta, meta.uid()), Ok(()));
        assert_eq!(
            office_file_policy(&meta, meta.uid().wrapping_add(1)),
            Err("wrong file owner")
        );
    }
}
