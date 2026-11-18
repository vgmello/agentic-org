//! The verified image: a sealed, execute-only copy of the pinned executable's bytes in
//! a memory file, and its SHA-256 (design section 3.1, steps 1 and 2).
//!
//! A child is started from this copy and from nothing else, so what was hashed is what
//! runs. The pinned path is read once, through one descriptor; replacing the file
//! afterwards changes nothing, and nobody can change the copy:
//!
//! - the four seals refuse every write, growth and shrinkage and any change of the
//!   seals themselves, for every holder of the file, this process included;
//! - the mode `0111` leaves the file unreadable by path (`/proc/<pid>/fd/<n>`) to its
//!   owner. It is also what makes the kernel start a child of an unprivileged user
//!   non-dumpable: it cannot read the image it executes.

use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::sys::{self, last, named};
use crate::admission::{AdmissionError, ExpectedImage, ImageRefusal};

/// The seals of a verified image: no write, no growth, no shrinkage, and no change of
/// the seals.
pub(super) const SEALS: libc::c_int =
    libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;

/// The mode of a verified image: executable, and neither readable nor writable.
pub(super) const EXECUTE_ONLY: libc::mode_t = 0o111;

/// The longest name `memfd_create` takes, in bytes.
const MEMORY_FILE_NAME_MAX: usize = 249;

/// A sealed, execute-only copy of an executable's bytes, with their SHA-256.
pub(super) struct SealedImage {
    /// The memory file. Its number is at least [`sys::FIRST_FREE`] and it is
    /// close-on-exec, as a spawn's child requires.
    file: OwnedFd,
    sha256: [u8; 32],
    /// The memory file's device and inode, and its size, as sealed.
    object: (libc::dev_t, libc::ino_t, libc::off_t),
}

/// Whether the bytes with the digest `read` may be started as `expected`.
///
/// A named digest must be the digest read. An image that names none is admitted only
/// as a development image: its digest is recorded, and no publisher vouches for it.
pub(super) fn judge(expected: &ExpectedImage, read: &[u8; 32]) -> Result<(), ImageRefusal> {
    match &expected.sha256 {
        Some(pin) if pin == read => Ok(()),
        Some(_) => Err(ImageRefusal::DigestMismatch),
        None if expected.development => Ok(()),
        None => Err(ImageRefusal::Unpinned),
    }
}

/// Whether a file with this `mode` and `owner` is one the host, running as
/// `service_user`, may copy an image from: a regular file that only the service user
/// or root can have written.
pub(super) fn fit(
    mode: libc::mode_t,
    owner: libc::uid_t,
    service_user: libc::uid_t,
) -> Result<(), ImageRefusal> {
    if mode & libc::S_IFMT != libc::S_IFREG {
        return Err(ImageRefusal::UnsafeFile("is not a regular file"));
    }
    if owner != service_user && owner != 0 {
        return Err(ImageRefusal::UnsafeFile("is owned by another user"));
    }
    if mode & (libc::S_IWGRP | libc::S_IWOTH) != 0 {
        return Err(ImageRefusal::UnsafeFile(
            "is writable by its group or by others",
        ));
    }
    Ok(())
}

impl SealedImage {
    /// Reads the pinned file of `expected` once, and seals a copy of exactly the bytes
    /// read, if they may be started as `expected` ([`judge`]). Nothing is started.
    pub(super) fn of(expected: &ExpectedImage) -> Result<Self, AdmissionError> {
        if expected.sha256.is_none() && !expected.development {
            // Nothing to check the bytes against: nothing is read.
            return Err(ImageRefusal::Unpinned.into());
        }
        let source = open_pinned(&expected.program)?;
        let stat = sys::status(source.as_fd())?;
        fit(stat.st_mode, stat.st_uid, sys::effective_uid())?;
        let file =
            sys::above_the_childs_descriptors(sys::memory_file(&name_of(&expected.program))?)?;
        let sha256 = copy(source, file.as_fd())?;
        judge(expected, &sha256)?;
        // SAFETY: a valid descriptor of a memory file made with `MFD_ALLOW_SEALING`.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, SEALS) } != 0 {
            return Err(last("fcntl(F_ADD_SEALS)").into());
        }
        // SAFETY: a valid descriptor; only the file's mode changes.
        if unsafe { libc::fchmod(file.as_raw_fd(), EXECUTE_ONLY) } != 0 {
            return Err(last("fchmod").into());
        }
        let sealed = sys::status(file.as_fd())?;
        Ok(Self {
            file,
            sha256,
            object: (sealed.st_dev, sealed.st_ino, sealed.st_size),
        })
    }

    /// The memory file, for a spawn to execute.
    pub(super) fn file(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }

    /// The SHA-256 of the sealed bytes.
    pub(super) fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    /// Whether the held descriptor still names the object that was sealed: the same
    /// memory file, of the same size, with every seal and the execute-only mode.
    /// The fixed text says which of them does not hold.
    pub(super) fn intact(&self) -> Result<(), String> {
        let now = sys::status(self.file()).map_err(|error| error.to_string())?;
        if (now.st_dev, now.st_ino, now.st_size) != self.object {
            return Err("the sealed image is another file".into());
        }
        if now.st_mode != libc::S_IFREG | EXECUTE_ONLY {
            return Err("the sealed image is no longer execute-only".into());
        }
        // SAFETY: a valid descriptor; the call only reads the file's seals.
        if unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_GET_SEALS) } != SEALS {
            return Err("the sealed image has lost a seal".into());
        }
        Ok(())
    }
}

/// Opens the pinned executable for reading. A symbolic link at the path is refused
/// (`O_NOFOLLOW`, `ELOOP`). `O_NONBLOCK` is there for one case: a FIFO at the path
/// would otherwise block the host until someone opened it for writing. It changes
/// nothing about reading a regular file, and [`fit`] refuses everything else.
fn open_pinned(program: &Path) -> io::Result<File> {
    let path = CString::new(program.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "the path holds a NUL byte"))?;
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    // SAFETY: a NUL-terminated path that outlives the call, which keeps nothing.
    let fd = unsafe { libc::open(path.as_ptr(), flags) };
    if fd < 0 {
        return Err(last("open"));
    }
    // SAFETY: a new descriptor that nothing else owns.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// The memory file's name: the program's file name, so that the child's
/// `/proc/<pid>/exe` reads `/memfd:<file name> (deleted)`.
fn name_of(program: &Path) -> CString {
    let name = program
        .file_name()
        .map_or(&b"image"[..], OsStrExt::as_bytes);
    let name = &name[..name.len().min(MEMORY_FILE_NAME_MAX)];
    // A file name holds no NUL byte: the path was opened, so it holds none at all.
    CString::new(name).unwrap_or_else(|_| c"image".to_owned())
}

/// Copies everything `source` holds into the memory file `target`, and returns the
/// SHA-256 of exactly the bytes written.
fn copy(mut source: File, target: BorrowedFd<'_>) -> io::Result<[u8; 32]> {
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let count = match source.read(&mut buffer) {
            Ok(0) => return Ok(hash.finalize().into()),
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(named("read", error)),
        };
        hash.update(&buffer[..count]);
        write_all(target, &buffer[..count])?;
    }
}

/// Writes all of `bytes` at the file position of `fd`.
fn write_all(fd: BorrowedFd<'_>, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        // SAFETY: a valid descriptor, and a buffer of exactly the length passed.
        let written = unsafe { libc::write(fd.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        match usize::try_from(written) {
            Ok(0) => return Err(named("write", io::ErrorKind::WriteZero.into())),
            Ok(written) => bytes = &bytes[written.min(bytes.len())..],
            Err(_) => {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(named("write", error));
                }
            }
        }
    }
    Ok(())
}

/// The SHA-256 of everything `source` holds, read through this one descriptor.
pub(super) fn digest(mut source: File) -> io::Result<[u8; 32]> {
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        match source.read(&mut buffer) {
            Ok(0) => return Ok(hash.finalize().into()),
            Ok(count) => hash.update(&buffer[..count]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(named("read", error)),
        }
    }
}
