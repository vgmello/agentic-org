//! The verified image: a sealed, execute-only copy of the pinned executable's bytes in
//! a memory file, and its SHA-256 (design section 3.1, steps 1 and 2).
//!
//! A child is started from this copy and from nothing else, and the digest that is
//! judged is read back from the copy after it was sealed, so what was hashed is what
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

/// What is known of the file at the pinned path.
#[derive(Clone, Copy, Debug)]
pub(super) struct PinnedFile {
    /// The file's mode, with its type bits.
    pub(super) mode: libc::mode_t,
    /// The file's owner.
    pub(super) owner: libc::uid_t,
    /// The file's group.
    pub(super) group: libc::gid_t,
}

/// Whether `file` is one the host, running as `service_user`, may copy an image from:
/// a regular file that only the service user or root can have written.
///
/// It is owned by the service user or by root, and is never writable by others. It is
/// writable by its group in one case only: the service user's own file whose group is
/// that user's private group (`crate::account`), as everything a default Ubuntu
/// account builds is (its file-creation mask is `0002`). Nobody else is in that group,
/// so the file is as closed as one without the bit. A group-writable file of root is
/// refused whatever its group is, and so is one of the service user when that user is
/// root: root's files are not built under a user's mask.
///
/// `private_group` gives the service user's private group, if it has one. It is asked
/// only when the answer decides.
pub(super) fn fit(
    file: PinnedFile,
    service_user: libc::uid_t,
    private_group: impl FnOnce() -> Option<libc::gid_t>,
) -> Result<(), ImageRefusal> {
    let PinnedFile { mode, owner, group } = file;
    if mode & libc::S_IFMT != libc::S_IFREG {
        return Err(ImageRefusal::UnsafeFile("is not a regular file"));
    }
    if owner != service_user && owner != 0 {
        return Err(ImageRefusal::UnsafeFile("is owned by another user"));
    }
    if mode & libc::S_IWOTH != 0 {
        return Err(ImageRefusal::UnsafeFile("is writable by others"));
    }
    if mode & libc::S_IWGRP != 0
        && (owner == 0 || owner != service_user || private_group() != Some(group))
    {
        return Err(ImageRefusal::UnsafeFile(
            "is writable by a group that is not its owner's private group",
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
        // Before the memory file exists: until it is sealed, another process of this
        // user could otherwise open it through `/proc/<pid>/fd/<n>` and write to it.
        sys::close_to_other_processes()?;
        let source = open_pinned(&expected.program)?;
        let stat = sys::status(source.as_fd())?;
        let service_user = sys::effective_uid();
        fit(
            PinnedFile {
                mode: stat.st_mode,
                owner: stat.st_uid,
                group: stat.st_gid,
            },
            service_user,
            // An account database that cannot be read vouches for no group.
            || {
                crate::account::private_group_of(service_user)
                    .ok()
                    .flatten()
            },
        )?;
        let file =
            sys::above_the_childs_descriptors(sys::memory_file(&name_of(&expected.program))?)?;
        let (read, length) = copy(source, file.as_fd())?;
        // SAFETY: a valid descriptor of a memory file made with `MFD_ALLOW_SEALING`.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, SEALS) } != 0 {
            return Err(last("fcntl(F_ADD_SEALS)").into());
        }
        // SAFETY: a valid descriptor; only the file's mode changes.
        if unsafe { libc::fchmod(file.as_raw_fd(), EXECUTE_ONLY) } != 0 {
            return Err(last("fchmod").into());
        }
        let sealed = sys::status(file.as_fd())?;
        // What is judged is the digest of the sealed object, read back through the
        // descriptor that a child executes: no write before the seals can have put
        // other bytes there than the ones judged. (This descriptor was opened for
        // reading and writing; the mode `0111` governs later opens only, and the
        // seals refuse the writes.)
        let sha256 = sealed_digest(file.as_fd())?;
        if sha256 != read || u64::try_from(sealed.st_size).ok() != Some(length) {
            return Err(io::Error::other(
                "the sealed image is not the bytes that were read from the pinned file",
            )
            .into());
        }
        judge(expected, &sha256)?;
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
/// SHA-256 and the number of exactly the bytes read and written.
fn copy(mut source: File, target: BorrowedFd<'_>) -> io::Result<([u8; 32], u64)> {
    let mut hash = Sha256::new();
    let mut length = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let count = match source.read(&mut buffer) {
            Ok(0) => return Ok((hash.finalize().into(), length)),
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(named("read", error)),
        };
        hash.update(&buffer[..count]);
        write_all(target, &buffer[..count])?;
        length += count as u64;
    }
}

/// The SHA-256 of everything the memory file `sealed` holds, read from its start
/// through this descriptor without moving its position.
fn sealed_digest(sealed: BorrowedFd<'_>) -> io::Result<[u8; 32]> {
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut offset: libc::off_t = 0;
    loop {
        // SAFETY: a valid descriptor, and a buffer of exactly the length passed.
        let count = unsafe {
            libc::pread(
                sealed.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                offset,
            )
        };
        match usize::try_from(count) {
            Ok(0) => return Ok(hash.finalize().into()),
            Ok(count) => {
                let count = count.min(buffer.len());
                hash.update(&buffer[..count]);
                offset += count as libc::off_t;
            }
            Err(_) => {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(named("pread", error));
                }
            }
        }
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
