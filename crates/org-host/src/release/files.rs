//! Descriptor-relative bounded inventory/copy. Input names never address an
//! instance, custody store, installation target or arbitrary output path.
mod output;
use super::Error;
use org_release::VerifiedManifest;
#[cfg(test)]
pub(super) use output::take_flush_counts;
pub(super) use output::Lock;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString, OsStr},
    fs::File,
    io::{self, Read, Seek, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path},
};
#[derive(PartialEq, Eq)]
pub(super) struct Identity {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
    mode: u32,
}
pub(super) fn identity(file: &File) -> Result<Identity, Error> {
    let m = file.metadata()?;
    if !m.is_file() || m.uid() != crate::home::euid() || m.nlink() != 1 || m.mode() & 0o6022 != 0 {
        return Err(Error::UnsafeInput);
    }
    Ok(Identity {
        dev: m.dev(),
        ino: m.ino(),
        len: m.len(),
        mtime: (m.mtime(), m.mtime_nsec()),
        ctime: (m.ctime(), m.ctime_nsec()),
        mode: m.mode(),
    })
}
fn owned(fd: libc::c_int) -> io::Result<File> {
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: successful open/dup transfers this new owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
fn component(name: &OsStr) -> Result<CString, Error> {
    let p = Path::new(name);
    if p.components().count() != 1
        || !matches!(p.components().next(),Some(Component::Normal(n)) if n==name)
    {
        return Err(Error::UnsafeInput);
    }
    CString::new(name.as_bytes()).map_err(|_| Error::UnsafeInput)
}
fn open_at(parent: &File, name: &OsStr, directory: bool) -> Result<File, Error> {
    let name = component(name)?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_CLOEXEC
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: retained descriptor plus validated single NUL-terminated component.
    Ok(owned(unsafe {
        libc::openat(parent.as_raw_fd(), name.as_ptr(), flags)
    })?)
}
fn directory(file: &File, final_part: bool) -> Result<(), Error> {
    let m = file.metadata()?;
    if !m.is_dir()
        || (m.uid() != 0 && m.uid() != crate::home::euid())
        || (m.mode() & 0o022 != 0 && (final_part || m.mode() & 0o1000 == 0))
        || (final_part && (m.uid() != crate::home::euid() || m.mode() & 0o7000 != 0))
    {
        return Err(Error::UnsafeInput);
    }
    Ok(())
}
pub(super) struct Directory(File);
impl Directory {
    /// Freeze bounded compressed input into a caller's already-private prepared
    /// directory. Names are module-generated, never taken from archive members.
    /// Failed inputs remain private; no publication or activation is implied.
    pub(super) fn freeze_archive(&self, source: &Path, max: u64) -> Result<File, Error> {
        if self.0.metadata()?.mode() & 0o7777 != 0o700 {
            return Err(Error::UnsafeInput);
        }
        let parent = source.parent().ok_or(Error::UnsafeInput)?;
        let name = source.file_name().ok_or(Error::UnsafeInput)?;
        let mut input = Self::open(parent)?.file(name.to_str().ok_or(Error::UnsafeInput)?)?;
        let before = identity(&input)?;
        if before.len > max {
            return Err(Error::UnsafeInput);
        }
        let name = CString::new(format!("input-{}.gz", crate::ids::generate()?))
            .map_err(|_| Error::UnsafeInput)?;
        // SAFETY: retained private directory, module-generated single component,
        // create-new no-follow, no caller/archive-provided output authority.
        let mut frozen = owned(unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        let mut total = 0_u64;
        let mut bytes = [0_u8; 64 * 1024];
        loop {
            let n = input.read(&mut bytes)?;
            if n == 0 {
                break;
            }
            total = total
                .checked_add(n as u64)
                .filter(|n| *n <= max)
                .ok_or(Error::UnsafeInput)?;
            frozen.write_all(&bytes[..n])?;
        }
        if total != before.len || before != identity(&input)? {
            return Err(Error::UnsafeInput);
        }
        use std::os::unix::fs::PermissionsExt;
        frozen.set_permissions(std::fs::Permissions::from_mode(0o400))?;
        org_platform::fs::full_sync(&frozen)?;
        self.0.sync_all()?;
        frozen.rewind()?;
        Ok(frozen)
    }
    pub(super) fn open(path: &Path) -> Result<Self, Error> {
        if !path.is_absolute() || path.as_os_str().len() > 4096 || path.to_str().is_none() {
            return Err(Error::UnsafeInput);
        }
        let mut current = File::open("/")?;
        let parts = path.components().collect::<Vec<_>>();
        if parts.len() < 2 {
            return Err(Error::UnsafeInput);
        }
        for (i, part) in parts.iter().enumerate() {
            match part {
                Component::RootDir => directory(&current, false)?,
                Component::Normal(name) => {
                    current = open_at(&current, name, true)?;
                    directory(&current, i + 1 == parts.len())?;
                }
                _ => return Err(Error::UnsafeInput),
            }
        }
        Ok(Self(current))
    }
    fn child(&self, name: &str) -> Result<Self, Error> {
        let file = open_at(&self.0, OsStr::new(name), true)?;
        directory(&file, true)?;
        Ok(Self(file))
    }
    fn file(&self, name: &str) -> Result<File, Error> {
        let file = open_at(&self.0, OsStr::new(name), false)?;
        identity(&file)?;
        Ok(file)
    }
    pub(super) fn payload(&self, path: &str) -> Result<File, Error> {
        let mut parts = path.split('/').peekable();
        let mut current = Self(self.0.try_clone()?);
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                return current.file(part);
            }
            current = current.child(part)?;
        }
        Err(Error::UnsafeInput)
    }
    pub(super) fn read(&self, name: &str, max: usize) -> Result<Vec<u8>, Error> {
        let file = self.file(name)?;
        let before = identity(&file)?;
        if before.len > max as u64 {
            return Err(Error::UnsafeInput);
        }
        let mut bytes = Vec::new();
        let mut bounded = (&file).take(max as u64 + 1);
        bounded.read_to_end(&mut bytes)?;
        if bytes.len() > max || identity(&file)? != before {
            return Err(Error::UnsafeInput);
        }
        Ok(bytes)
    }
    fn names(&self, limit: usize) -> Result<BTreeSet<String>, Error> {
        // SAFETY: reopen the retained directory itself to get an independent
        // enumeration offset. dup would share it, making repeat verification
        // incorrectly see EOF. The module-owned dot names no untrusted path.
        let duplicate = owned(unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        let pointer = unsafe { libc::fdopendir(duplicate.as_raw_fd()) };
        if pointer.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        std::mem::forget(duplicate);
        struct Entries(*mut libc::DIR);
        impl Drop for Entries {
            fn drop(&mut self) {
                // SAFETY: sole owner of the successful fdopendir result.
                unsafe {
                    libc::closedir(self.0);
                }
            }
        }
        let entries = Entries(pointer);
        let mut names = BTreeSet::new();
        loop {
            org_platform::fs::set_errno(0)?;
            // SAFETY: retained live DIR, same thread and no competing iterator.
            let item = unsafe { libc::readdir(entries.0) };
            if item.is_null() {
                if io::Error::last_os_error().raw_os_error() != Some(0) {
                    return Err(io::Error::last_os_error().into());
                }
                break;
            }
            // SAFETY: readdir supplies a NUL-terminated name valid until next call.
            let name = unsafe { CStr::from_ptr((*item).d_name.as_ptr()) }
                .to_str()
                .map_err(|_| Error::UnsafeInput)?;
            if matches!(name, "." | "..") {
                continue;
            }
            if names.len() >= limit || !names.insert(name.to_owned()) {
                return Err(Error::UnsafeInput);
            }
        }
        Ok(names)
    }
    pub(super) fn verify_inventory(&self, manifest: &VerifiedManifest) -> Result<(), Error> {
        let mut children = BTreeMap::<String, BTreeSet<String>>::new();
        let mut payloads = BTreeSet::new();
        for path in manifest
            .manifest()
            .files()
            .iter()
            .map(|f| f.path())
            .chain(["release.v1.json", "release.v1.sig"])
        {
            payloads.insert(path.to_owned());
            let mut parent = String::new();
            let parts = path.split('/').collect::<Vec<_>>();
            for (i, name) in parts.iter().enumerate() {
                children
                    .entry(parent.clone())
                    .or_default()
                    .insert((*name).into());
                if i + 1 < parts.len() {
                    if !parent.is_empty() {
                        parent.push('/');
                    }
                    parent.push_str(name);
                }
            }
        }
        let mut pending = vec![(String::new(), Self(self.0.try_clone()?))];
        while let Some((prefix, current)) = pending.pop() {
            let expected = children.get(&prefix).ok_or(Error::UnsafeInput)?;
            if &current.names(expected.len())? != expected {
                return Err(Error::UnsafeInput);
            }
            for name in expected {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                if payloads.contains(&path) {
                    current.file(name)?;
                } else {
                    pending.push((path, current.child(name)?));
                }
            }
        }
        Ok(())
    }
}
