//! Private release-store output primitives. Only authenticated names reach these
//! methods; every mutation is relative to a retained no-follow directory.
use super::*;
use std::fs::{Permissions, TryLockError};
use std::os::unix::fs::PermissionsExt;

#[cfg(test)]
thread_local! {static FLUSH_COUNTS:std::cell::RefCell<[usize;3]>=const {std::cell::RefCell::new([0;3])};}
#[cfg(test)]
pub(in crate::release) fn take_flush_counts() -> [usize; 3] {
    FLUSH_COUNTS.with(|v| v.replace([0; 3]))
}

pub(in crate::release) struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        // Closing alone does not release a lock inherited across fork.
        let _ = self.0.unlock();
    }
}
impl Lock {
    pub(in crate::release) fn verify(&self, directory: &Directory) -> Result<(), Error> {
        if identity(&self.0)? != identity(&directory.file("store.lock")?)? {
            return Err(Error::UnsafeInput);
        }
        Ok(())
    }
}

pub(in crate::release) fn flush(file: &File) -> Result<(), Error> {
    file.sync_all()?;
    #[cfg(test)]
    FLUSH_COUNTS.with(|v| v.borrow_mut()[0] += 1);
    #[cfg(target_os = "macos")]
    // SAFETY: full device flush of our retained regular-file descriptor.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    #[cfg(all(test, target_os = "macos"))]
    FLUSH_COUNTS.with(|v| v.borrow_mut()[1] += 1);
    Ok(())
}

impl Directory {
    fn sync_directory(&self) -> Result<(), Error> {
        self.0.sync_all()?;
        #[cfg(test)]
        FLUSH_COUNTS.with(|v| v.borrow_mut()[2] += 1);
        Ok(())
    }
    pub(in crate::release) fn duplicate_output(&self) -> Result<Self, Error> {
        // SAFETY: module-owned '.' addresses this held directory only.
        Ok(Self(owned(unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?))
    }
    pub(in crate::release) fn create_store(path: &Path) -> Result<Self, Error> {
        let parent = Self::open(path.parent().ok_or(Error::UnsafeInput)?)?;
        let name = path.file_name().ok_or(Error::UnsafeInput)?;
        let cname = component(name)?;
        // SAFETY: checked single component under held parent, private fixed mode.
        if unsafe { libc::mkdirat(parent.0.as_raw_fd(), cname.as_ptr(), 0o700) } != 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error.into());
            }
        } else {
            parent.sync_directory()?;
        }
        let root = Self(open_at(&parent.0, name, true)?);
        root.require_mode(0o700)?;
        Ok(root)
    }

    pub(in crate::release) fn require_mode(&self, mode: u32) -> Result<(), Error> {
        let m = self.0.metadata()?;
        if !m.is_dir() || m.uid() != crate::home::euid() || m.mode() & 0o7777 != mode {
            return Err(Error::UnsafeInput);
        }
        Ok(())
    }

    pub(in crate::release) fn same_directory(&self, other: &Self) -> Result<bool, Error> {
        let a = self.0.metadata()?;
        let b = other.0.metadata()?;
        Ok(a.dev() == b.dev() && a.ino() == b.ino())
    }

    pub(in crate::release) fn lock(&self) -> Result<Lock, Error> {
        let file = match self.new_file("store.lock") {
            Err(Error::Io(io::ErrorKind::AlreadyExists)) => self.file("store.lock")?,
            result => result?,
        };
        if file.metadata()?.mode() & 0o7777 != 0o600 {
            return Err(Error::UnsafeInput);
        }
        match file.try_lock() {
            Ok(()) => Ok(Lock(file)),
            Err(TryLockError::WouldBlock) => Err(Error::Busy),
            Err(TryLockError::Error(e)) => Err(e.into()),
        }
    }

    pub(in crate::release) fn output_child(&self, name: &str, create: bool) -> Result<Self, Error> {
        if create {
            let cname = component(OsStr::new(name))?;
            // SAFETY: create only a checked component under a retained directory.
            if unsafe { libc::mkdirat(self.0.as_raw_fd(), cname.as_ptr(), 0o700) } != 0 {
                let e = io::Error::last_os_error();
                if e.kind() != io::ErrorKind::AlreadyExists {
                    return Err(e.into());
                }
            } else {
                self.sync_directory()?;
            }
        }
        let result = Self(open_at(&self.0, OsStr::new(name), true)?);
        let m = result.0.metadata()?;
        if m.dev() != self.0.metadata()?.dev() {
            return Err(Error::UnsafeInput);
        }
        result.require_mode(if create { 0o700 } else { m.mode() & 0o7777 })?;
        if !matches!(m.mode() & 0o7777, 0o700 | 0o500) {
            return Err(Error::UnsafeInput);
        }
        Ok(result)
    }

    pub(in crate::release) fn claim_output(&self, name: &str) -> Result<Self, Error> {
        let name = component(OsStr::new(name))?;
        // SAFETY: exclusive claim of a checked, module-generated component.
        if unsafe { libc::mkdirat(self.0.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        self.sync_directory()?;
        self.output_child(name.to_str().map_err(|_| Error::UnsafeInput)?, false)
    }

    pub(in crate::release) fn new_file(&self, name: &str) -> Result<File, Error> {
        let name = component(OsStr::new(name))?;
        // SAFETY: retained directory, checked single name, create-new no-follow.
        let file = owned(unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        identity(&file)?;
        Ok(file)
    }

    pub(in crate::release) fn seal_file(&self, file: &File, executable: bool) -> Result<(), Error> {
        file.set_permissions(Permissions::from_mode(if executable {
            0o500
        } else {
            0o400
        }))?;
        flush(file)?;
        self.sync_directory()?;
        Ok(())
    }

    pub(in crate::release) fn seal_directory(&self) -> Result<(), Error> {
        self.0.set_permissions(Permissions::from_mode(0o500))?;
        self.sync_directory()?;
        Ok(())
    }

    pub(in crate::release) fn publish_directory(
        &self,
        name: &str,
        target: &Self,
        to: &str,
    ) -> Result<(), Error> {
        let from = component(OsStr::new(name))?;
        let to = component(OsStr::new(to))?;
        if self.0.metadata()?.dev() != target.0.metadata()?.dev() {
            return Err(Error::UnsafeInput);
        }
        // SAFETY: retained same-device dirs, validated names; never replace a tree.
        let rc = unsafe { rename_exclusive(self.0.as_raw_fd(), &from, target.0.as_raw_fd(), &to) };
        if rc != 0 {
            return Err(io::Error::last_os_error().into());
        }
        target.sync_directory()?;
        self.sync_directory()?;
        Ok(())
    }

    pub(in crate::release) fn publish_control(
        &self,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), Error> {
        let temp = format!(".pending-{}", crate::ids::generate()?);
        let mut file = self.new_file(&temp)?;
        file.write_all(bytes)?;
        self.seal_file(&file, false)?;
        // Same no-replace primitive works for immutable regular control files.
        self.publish_directory(&temp, self, name)
    }

    /// Iterate without an accumulated-state memory cap. Each name is bounded by
    /// the filesystem, validated by the caller, and never used as a parsed path.
    pub(in crate::release) fn visit_names(
        &self,
        mut visit: impl FnMut(&str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let dot = CString::new(".").map_err(|_| Error::UnsafeInput)?;
        // SAFETY: reopen '.' for an independent directory offset (dup shares it).
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                dot.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        let file = owned(fd)?;
        use std::os::fd::IntoRawFd;
        let fd = file.into_raw_fd();
        // SAFETY: fdopendir takes this newly owned directory descriptor.
        let dir = unsafe { libc::fdopendir(fd) };
        if dir.is_null() {
            // SAFETY: failed fdopendir did not take ownership.
            unsafe {
                libc::close(fd);
            }
            return Err(io::Error::last_os_error().into());
        }
        struct Entries(*mut libc::DIR);
        impl Drop for Entries {
            fn drop(&mut self) {
                unsafe {
                    libc::closedir(self.0);
                }
            }
        }
        let _entries = Entries(dir);
        loop {
            set_errno(0);
            // SAFETY: this is a live directory stream; entry remains valid until next call.
            let entry = unsafe { libc::readdir(dir) };
            if entry.is_null() {
                let e = io::Error::last_os_error();
                return if e.raw_os_error() == Some(0) {
                    Ok(())
                } else {
                    Err(e.into())
                };
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if matches!(name.to_bytes(), b"." | b"..") {
                continue;
            }
            visit(name.to_str().map_err(|_| Error::UnsafeInput)?)?;
        }
    }
}

#[cfg(target_os = "macos")]
unsafe fn rename_exclusive(from: i32, name: &CStr, to: i32, target: &CStr) -> i32 {
    // SAFETY: callers retain both dirfds and valid single-component C strings.
    unsafe { libc::renameatx_np(from, name.as_ptr(), to, target.as_ptr(), libc::RENAME_EXCL) }
}
#[cfg(target_os = "linux")]
unsafe fn rename_exclusive(from: i32, name: &CStr, to: i32, target: &CStr) -> i32 {
    // SAFETY: same descriptor-relative constraints; RENAME_NOREPLACE is flag 1.
    unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            from,
            name.as_ptr(),
            to,
            target.as_ptr(),
            1,
        ) as i32
    }
}
