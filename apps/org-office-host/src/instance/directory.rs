//! Descriptor-relative control-file operations. Never follow an instance-owned symlink.

use std::ffi::{CStr, CString};
use std::fs::{File, TryLockError};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use super::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    fn of(file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

pub(super) struct Directory {
    file: File,
    path: PathBuf,
    identity: Identity,
    locked: bool,
}

impl Drop for Directory {
    fn drop(&mut self) {
        if self.locked {
            // An inherited/duplicated fd may outlive this owner; closing alone is insufficient.
            let _ = self.file.unlock();
        }
    }
}

pub(super) struct FileLock(File);

impl std::ops::Deref for FileLock {
    type Target = File;
    fn deref(&self) -> &File {
        &self.0
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Normalize spelling only; reject traversal rather than resolving it through symlinks.
pub(super) fn absolute(path: &Path) -> Result<PathBuf, Error> {
    if !path.is_absolute() || path.as_os_str().len() > 4096 || path.to_str().is_none() {
        return Err(Error::Invalid(
            "root must be an absolute UTF-8 path of at most 4096 bytes",
        ));
    }
    let mut result = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => result.push(name),
            _ => return Err(Error::Invalid("root must not contain parent traversal")),
        }
    }
    if result == Path::new("/") {
        return Err(Error::Invalid(
            "the filesystem root cannot be an instance root",
        ));
    }
    Ok(result)
}

fn cstring(value: &std::ffi::OsStr) -> Result<CString, Error> {
    CString::new(value.as_bytes()).map_err(|_| Error::Invalid("path contains NUL"))
}

fn leaf(name: &str) -> Result<CString, Error> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(part)) if part == std::ffi::OsStr::new(name))
        || components.next().is_some()
    {
        return Err(Error::Invalid(
            "control name must be one plain path component",
        ));
    }
    cstring(std::ffi::OsStr::new(name))
}

fn from_fd(fd: libc::c_int) -> io::Result<File> {
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: the successful open/dup transferred this new descriptor to us.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

fn effective_user() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

impl Directory {
    pub(super) fn open(path: &Path, create: bool) -> Result<Self, Error> {
        let path = absolute(path)?;
        let file = Self::walk(&path, create)?;
        let metadata = file.metadata()?;
        if metadata.uid() != effective_user() || metadata.mode() & 0o7777 != 0o700 {
            return Err(Error::Unsafe(
                "directory must be owned by this user with mode 0700",
            ));
        }
        let identity = Identity::of(&file)?;
        Ok(Self {
            file,
            path,
            identity,
            locked: false,
        })
    }

    fn walk(path: &Path, create_final: bool) -> Result<File, Error> {
        let mut directory = File::open("/")?;
        let parts: Vec<_> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(name) => Some(name),
                _ => None,
            })
            .collect();
        for (index, name) in parts.iter().enumerate() {
            let name = cstring(name)?;
            if create_final && index + 1 == parts.len() {
                // SAFETY: the parent fd and NUL-terminated component are valid.
                let result = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
                if result != 0 && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists
                {
                    return Err(io::Error::last_os_error().into());
                }
                if result == 0 {
                    directory.sync_all()?;
                }
            }
            // SAFETY: each component is opened relative to a retained parent fd.
            directory = from_fd(unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            })?;
        }
        Ok(directory)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn identity(&self) -> Identity {
        self.identity
    }

    /// Claim a fresh directory. Existing names, including symlinks, are never adopted.
    pub(super) fn claim_child(&self, name: &str) -> Result<Option<Self>, Error> {
        self.verify()?;
        let component = leaf(name)?;
        // SAFETY: the retained parent fd and module-owned component are valid.
        if unsafe { libc::mkdirat(self.file.as_raw_fd(), component.as_ptr(), 0o700) } != 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::AlreadyExists {
                Ok(None)
            } else {
                Err(error.into())
            };
        }
        self.file.sync_all()?;
        Ok(Some(Self::open(&self.path.join(name), false)?))
    }

    #[cfg(target_os = "linux")]
    pub(super) fn require_disk(&self) -> Result<(), Error> {
        // Linux UAPI include/uapi/linux/magic.h defines ramfs/tmpfs/hugetlbfs.
        // Fail closed on known memory filesystems; setup supplies the explicit disk base.
        let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: fstatfs initializes stats on success for our valid directory fd.
        if unsafe { libc::fstatfs(self.file.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        // SAFETY: fstatfs succeeded.
        let kind = unsafe { stats.assume_init() }.f_type as u64;
        if matches!(kind, 0x01021994 | 0x858458f6 | 0x958458f6) {
            return Err(Error::Unsafe("user temp base must be disk-backed"));
        }
        Ok(())
    }

    pub(super) fn verify(&self) -> Result<(), Error> {
        let current = Self::open(&self.path, false)?;
        if current.identity != self.identity {
            return Err(Error::Unsafe("registered directory was replaced or moved"));
        }
        Ok(())
    }

    pub(super) fn lock_root(&mut self) -> Result<(), Error> {
        lock(&self.file, &self.path)?;
        self.locked = true;
        Ok(())
    }

    pub(super) fn lock_file(&self, name: &str) -> Result<FileLock, Error> {
        let file = self.open_file(name, libc::O_RDWR | libc::O_CREAT)?;
        lock(&file, &self.path.join(name))?;
        let held = FileLock(file);
        self.verify_file(name, &held)?;
        Ok(held)
    }

    pub(super) fn verify_file(&self, name: &str, held: &File) -> Result<(), Error> {
        let current = self.open_file(name, libc::O_RDONLY)?;
        if Identity::of(&current)? != Identity::of(held)? {
            return Err(Error::Unsafe("control file was replaced"));
        }
        self.verify()
    }

    fn open_file(&self, name: &str, flags: i32) -> Result<File, Error> {
        let name = leaf(name)?;
        // SAFETY: the fd is retained and the name is a single, module-owned component.
        let file = from_fd(unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != effective_user()
            || metadata.mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(Error::Unsafe(
                "control file must be regular, private, owned and singly linked",
            ));
        }
        Ok(file)
    }

    pub(super) fn read(&self, name: &str, limit: usize) -> Result<Option<Vec<u8>>, Error> {
        let mut file = match self.open_file(name, libc::O_RDONLY) {
            Err(Error::Io(error)) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            other => other?,
        };
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(Error::Unsafe("control file exceeds its size limit"));
        }
        self.verify()?;
        Ok(Some(bytes))
    }

    pub(super) fn publish(&self, name: &str, bytes: &[u8], replace: bool) -> Result<(), Error> {
        let to = leaf(name)?;
        self.verify()?;
        let temporary = format!(".instance-{}.tmp", crate::ids::generate()?);
        let mut file = self.open_file(&temporary, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            self.verify()?;
            self.verify_file(&temporary, &file)?;
            let from = leaf(&temporary)?;
            let fd = self.file.as_raw_fd();
            // SAFETY: both names are module-owned components relative to our retained fd.
            let rc = unsafe {
                if replace {
                    libc::renameat(fd, from.as_ptr(), fd, to.as_ptr())
                } else {
                    rename_exclusive(fd, &from, &to)
                }
            };
            if rc != 0 {
                return Err(io::Error::last_os_error().into());
            }
            self.file.sync_all()?;
            self.verify_file(name, &file)?;
            self.verify()
        })();
        let temporary = cstring(std::ffi::OsStr::new(&temporary))?;
        // SAFETY: removes only our own temporary name; after publication it is absent.
        unsafe { libc::unlinkat(self.file.as_raw_fd(), temporary.as_ptr(), 0) };
        result
    }

    pub(super) fn is_empty(&self) -> Result<bool, Error> {
        // A separately opened description avoids sharing a directory iteration offset.
        let dot = c".";
        // SAFETY: the retained directory fd and static component are valid.
        let descriptor = from_fd(unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                dot.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        })?;
        use std::os::fd::IntoRawFd;
        let fd = descriptor.into_raw_fd();
        // SAFETY: fdopendir takes ownership on success.
        let stream = unsafe { libc::fdopendir(fd) };
        if stream.is_null() {
            let error = io::Error::last_os_error();
            // SAFETY: fdopendir failed, so ownership remains ours.
            unsafe { libc::close(fd) };
            return Err(error.into());
        }
        struct Stream(*mut libc::DIR);
        impl Drop for Stream {
            fn drop(&mut self) {
                // SAFETY: this guard uniquely owns the successful fdopendir result.
                unsafe { libc::closedir(self.0) };
            }
        }
        let stream = Stream(stream);
        loop {
            set_errno(0);
            // SAFETY: stream is live and exclusively used by this loop.
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(0) {
                    return Err(error.into());
                }
                return Ok(true);
            }
            // SAFETY: the name is NUL-terminated and valid until the next readdir.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name != b"." && name != b".." {
                return Ok(false);
            }
        }
    }

    /// Compare ancestor inodes, including case aliases and mount aliases.
    pub(super) fn contains_path(&self, path: &Path) -> Result<bool, Error> {
        for ancestor in path.ancestors() {
            match Self::walk(ancestor, false) {
                Ok(file) if Identity::of(&file)? == self.identity => return Ok(true),
                Ok(_) => {}
                Err(Error::Io(error)) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(false)
    }
}

fn lock(file: &File, path: &Path) -> Result<(), Error> {
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(Error::Busy(path.to_owned())),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

#[cfg(target_os = "macos")]
unsafe fn rename_exclusive(fd: i32, from: &CStr, to: &CStr) -> i32 {
    // SAFETY: forwarded validated relative names and retained parent fd.
    unsafe { libc::renameatx_np(fd, from.as_ptr(), fd, to.as_ptr(), libc::RENAME_EXCL) }
}

#[cfg(target_os = "linux")]
unsafe fn rename_exclusive(fd: i32, from: &CStr, to: &CStr) -> i32 {
    // SAFETY: forwarded validated relative names and retained parent fd.
    unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            fd,
            from.as_ptr(),
            fd,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        ) as i32
    }
}

fn set_errno(value: i32) {
    // SAFETY: libc returns this thread's errno pointer.
    unsafe {
        #[cfg(target_os = "macos")]
        {
            *libc::__error() = value;
        }
        #[cfg(target_os = "linux")]
        {
            *libc::__errno_location() = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn dropping_lock_owners_unlocks_even_while_a_duplicate_fd_survives() {
        let temporary = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = temporary.path().canonicalize().unwrap();
        let mut root = Directory::open(&path, false).unwrap();
        let held = root.lock_file("registry.lock").unwrap();
        let inherited_file = held.try_clone().unwrap();
        drop(held);
        root.lock_file("registry.lock").unwrap();
        drop(inherited_file);
        root.lock_root().unwrap();
        let inherited_root = root.file.try_clone().unwrap();
        drop(root);
        Directory::open(&path, false).unwrap().lock_root().unwrap();
        drop(inherited_root);
    }

    #[test]
    fn relative_file_capabilities_cannot_escape_the_opened_directory() {
        let temporary = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let parent = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "/escape",
            "nested/file",
            "trailing/",
            "a\0b",
        ] {
            assert!(parent.claim_child(name).is_err(), "{name:?}");
            assert!(parent.read(name, 10).is_err(), "{name:?}");
        }
        assert!(parent.is_empty().unwrap());
    }
}
