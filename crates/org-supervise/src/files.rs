//! Directory-relative, no-follow planned file installation. Never writes through a leaf.
use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

use crate::invalid;

fn c(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| invalid("NUL in planned path"))
}

fn directory(parent: &OwnedFd, name: &OsStr, create: bool) -> io::Result<OwnedFd> {
    let name = c(name)?;
    if create {
        // SAFETY: valid directory descriptor and literal child name; no symlink traversal.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::AlreadyExists {
                return Err(e);
            }
        }
    }
    // SAFETY: openat is confined to this component; O_NOFOLLOW rejects a symlink.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: newly opened descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn root(path: &Path) -> io::Result<OwnedFd> {
    if !path.is_absolute() {
        return Err(invalid("planned-file root must be absolute"));
    }
    // SAFETY: constant C string names root, returning a newly owned directory fd.
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: ownership from open.
    let mut current = unsafe { OwnedFd::from_raw_fd(fd) };
    for part in path.components() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => current = directory(&current, name, false)?,
            _ => return Err(invalid("planned-file root must be normalized")),
        }
    }
    Ok(current)
}

fn components(relative: &Path) -> io::Result<Vec<&OsStr>> {
    let parts = relative
        .components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name),
            _ => Err(invalid("planned path must be relative and normalized")),
        })
        .collect::<io::Result<Vec<_>>>()?;
    if parts.is_empty() {
        return Err(invalid("empty planned path"));
    }
    Ok(parts)
}

pub(crate) fn create_dir(base: &Path, relative: &Path) -> io::Result<()> {
    let mut current = root(base)?;
    for name in components(relative)? {
        current = directory(&current, name, true)?;
    }
    Ok(())
}

pub(crate) fn write_file(base: &Path, relative: &Path, contents: &[u8]) -> io::Result<()> {
    let mut parent = root(base)?;
    let mut parts = components(relative)?;
    let leaf = c(parts.pop().unwrap())?;
    for name in parts {
        parent = directory(&parent, name, false)?;
    }
    let mut random = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    let temp = CString::new(format!(
        ".org-supervise-{:032x}",
        u128::from_ne_bytes(random)
    ))
    .unwrap();
    // SAFETY: parent is held open, name is literal, O_EXCL prevents writing another file.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            temp.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: new descriptor transferred exactly once to File.
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = (|| {
        // SAFETY: adjust only the newly created inode held by our descriptor. A restrictive
        // process umask must not silently change the planned file's specified mode.
        if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
            return Err(io::Error::last_os_error());
        }
        file.write_all(contents)?;
        // SAFETY: both names relative to the same owned directory. Replace the leaf, never
        // follow its symlink or truncate a file with additional hardlinks.
        if unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                temp.as_ptr(),
                parent.as_raw_fd(),
                leaf.as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() {
        // SAFETY: only the temporary file created above, in the still-owned parent.
        unsafe { libc::unlinkat(parent.as_raw_fd(), temp.as_ptr(), 0) };
    }
    result
}
