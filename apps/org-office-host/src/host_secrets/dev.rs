//! Debug-only plaintext files. A lifetime lock owns crash-temp cleanup.

#[cfg(target_os = "linux")]
use libc::__errno_location as errno;
#[cfg(target_os = "macos")]
use libc::__error as errno;
use std::ffi::{CStr, CString, OsString};
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use super::{Error, HostSecrets, Name, Value, MAX_BYTES};

pub(crate) struct DevSecrets {
    directory: File,
    path: PathBuf,
    gate: Mutex<File>,
}

impl Drop for DevSecrets {
    fn drop(&mut self) {
        // Forked descriptors may outlive this owner; close alone cannot release the lock.
        let file = self.gate.get_mut().unwrap_or_else(|e| e.into_inner());
        // Drop cannot report unlock errors; the File still closes afterward.
        let _ = file.unlock();
    }
}

impl DevSecrets {
    /// `base` already exists, is owner-only, and is the shared development-key base.
    #[cfg_attr(not(test), expect(dead_code, reason = "hostdb staging"))]
    pub(crate) fn open(base: &Path, home: &Path, instance: &str) -> Result<Self, Error> {
        if !base.is_absolute()
            || base.components().any(|c| matches!(c, Component::ParentDir))
            || org_proto::ulid_time_ms(instance).is_none()
        {
            return Err(Error::Unsafe);
        }
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(base.components().collect::<PathBuf>())?;
        owned(&directory, true)?;
        let base = base.canonicalize()?;
        if base.starts_with(home.canonicalize()?) {
            return Err(Error::Unsafe);
        }
        let directory = child(&child(&directory, "host-secrets")?, instance)?;
        let lock = at(
            &directory,
            "lock",
            libc::O_RDWR | libc::O_CREAT | libc::O_NONBLOCK,
        )?;
        owned(&lock, false)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(Error::InUse),
            Err(fs::TryLockError::Error(e)) => return Err(e.into()),
        }
        let store = Self {
            directory,
            path: base.join("host-secrets").join(instance),
            gate: Mutex::new(lock),
        };
        for raw in store.names()? {
            if let Some(name) = raw.to_str().filter(|n| n.starts_with(".put.")) {
                store.item(name)?;
                store.remove(name)?;
            }
        }
        Ok(store)
    }
    fn verify(&self) -> Result<(), Error> {
        let path = fs::symlink_metadata(&self.path)?;
        let held = self.directory.metadata()?;
        if !path.is_dir() || path.dev() != held.dev() || path.ino() != held.ino() {
            return Err(Error::Unsafe);
        }
        owned(&self.directory, true)
    }
    fn names(&self) -> Result<Vec<OsString>, Error> {
        self.verify()?;
        let file = at(&self.directory, ".", libc::O_RDONLY | libc::O_DIRECTORY)?;
        // SAFETY: fdopendir takes ownership on success; otherwise File closes the fd.
        let stream = unsafe { libc::fdopendir(file.as_raw_fd()) };
        if stream.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        std::mem::forget(file);
        let stream = Entries(stream);
        let mut names = Vec::new();
        loop {
            // SAFETY: errno is thread-local; stream owns a live directory descriptor.
            let entry = unsafe {
                *errno() = 0;
                libc::readdir(stream.0)
            };
            if entry.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(0) {
                    return Err(error.into());
                }
                break;
            }
            // SAFETY: readdir's NUL-terminated name remains valid until the next call.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name != b"." && name != b".." {
                names.push(OsString::from_vec(name.to_vec()));
            }
        }
        self.verify()?;
        Ok(names)
    }
    fn item(&self, name: &str) -> Result<Option<File>, Error> {
        self.verify()?;
        if self.names()?.iter().any(|raw| {
            raw.to_str()
                .is_some_and(|n| n != name && n.eq_ignore_ascii_case(name))
        }) {
            return Err(Error::CaseAlias);
        }
        let file = match at(&self.directory, name, libc::O_RDONLY | libc::O_NONBLOCK) {
            Ok(file) => file,
            Err(Error::Io(io::ErrorKind::NotFound)) => return Ok(None),
            Err(e) => return Err(e),
        };
        owned(&file, false)?;
        Ok(Some(file))
    }
    fn remove(&self, name: &str) -> Result<(), Error> {
        let name = cstring(name)?;
        // SAFETY: retained directory fd and checked NUL-free single component.
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::NotFound {
                return Err(e.into());
            }
        }
        self.directory.sync_all()?;
        Ok(())
    }
}

impl HostSecrets for DevSecrets {
    fn put(&self, name: &Name, value: &Value) -> Result<(), Error> {
        if value.expose().len() > MAX_BYTES {
            return Err(Error::TooLarge);
        }
        let _guard = self.gate.lock().map_err(|_| Error::Unsafe)?;
        let add_only = name.is_add_only();
        if self.item(name.as_str())?.is_some() && add_only {
            return Ok(());
        }
        let temporary = format!(".put.{}", crate::ids::generate()?);
        // O_EXCL is defence in depth against a random temporary-name collision.
        let mut file = at(
            &self.directory,
            &temporary,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        let result = (|| {
            file.set_permissions(Permissions::from_mode(0o600))?;
            owned(&file, false)?;
            file.write_all(value.expose())?;
            file.sync_all()?;
            #[cfg(test)]
            super::tests::before_publish(&self.path);
            let (from, to) = (cstring(&temporary)?, cstring(name.as_str())?);
            let dir = self.directory.as_raw_fd();
            // SAFETY: checked leaves and retained fd; TLS link is exclusive and never replaces.
            let result = unsafe {
                if add_only {
                    libc::linkat(dir, from.as_ptr(), dir, to.as_ptr(), 0)
                } else {
                    libc::renameat(dir, from.as_ptr(), dir, to.as_ptr())
                }
            };
            if result != 0 {
                let error = io::Error::last_os_error();
                if !add_only || error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error.into());
                }
                #[cfg(test)]
                super::tests::after_collision(&self.path);
                self.item(name.as_str())?.ok_or(Error::Unsafe)?;
            }
            if add_only {
                self.remove(&temporary)?;
            }
            self.directory.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = self.remove(&temporary);
        }
        result
    }
    fn load(&self, name: &Name) -> Result<Option<Value>, Error> {
        let guard = self.gate.lock().map_err(|_| Error::Unsafe)?;
        let Some(file) = self.item(name.as_str())? else {
            return Ok(None);
        };
        drop(guard); // The opened inode stays stable across a concurrent replacement.
                     // Reserve the bound before reading so no reallocation leaves a stale secret buffer.
        let mut value = Value(Vec::with_capacity(MAX_BYTES + 1));
        file.take(MAX_BYTES as u64 + 1).read_to_end(&mut value.0)?;
        if value.0.len() > MAX_BYTES {
            return Err(Error::Corrupt);
        }
        Ok(Some(value))
    }
    fn delete(&self, name: &Name) -> Result<(), Error> {
        let _guard = self.gate.lock().map_err(|_| Error::Unsafe)?;
        if self.item(name.as_str())?.is_some() {
            self.remove(name.as_str())?;
        }
        Ok(())
    }
    fn list(&self) -> Result<Vec<Name>, Error> {
        let _guard = self.gate.lock().map_err(|_| Error::Unsafe)?;
        let mut names = Vec::new();
        for raw in self.names()? {
            if raw == "lock" {
                continue;
            }
            let Some(name) = raw.to_str().and_then(|raw| Name::parse(raw).ok()) else {
                crate::host_log::log("ignored an invalid host-secret item name");
                continue;
            };
            if self.item(name.as_str())?.is_some() {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }
}

fn cstring(name: &str) -> Result<CString, Error> {
    if name.contains('/') {
        return Err(Error::InvalidName);
    }
    CString::new(name).map_err(|_| Error::InvalidName)
}
fn owned(file: &File, directory: bool) -> Result<(), Error> {
    let m = file.metadata()?;
    if m.uid() != crate::home::euid()
        || m.is_dir() != directory
        || (!directory && !m.is_file())
        || m.mode() & 0o7777 != if directory { 0o700 } else { 0o600 }
    {
        return Err(Error::Unsafe);
    }
    Ok(())
}
fn at(directory: &File, name: &str, flags: i32) -> Result<File, Error> {
    let name = cstring(name)?;
    // SAFETY: live directory fd and checked leaf; successful open transfers a new fd to File.
    let flags = flags | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: fd is fresh and uniquely owned.
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn child(parent: &File, name: &str) -> Result<File, Error> {
    let name_c = cstring(name)?;
    // SAFETY: retained parent fd and checked leaf, mode excludes other users.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name_c.as_ptr(), 0o700) } != 0 {
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::AlreadyExists {
            return Err(e.into());
        }
    } else {
        parent.sync_all()?;
    }
    let child = at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    owned(&child, true)?;
    Ok(child)
}

struct Entries(*mut libc::DIR);
impl Drop for Entries {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns the successful fdopendir result.
        unsafe {
            libc::closedir(self.0);
        }
    }
}
