//! Private release-store output primitives. Only authenticated names reach these
//! methods; every mutation is relative to a retained no-follow directory.

use std::ffi::OsStr;
use std::fs::File;
use std::fs::{Permissions, TryLockError};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use org_platform::posix;

use super::{component, identity, open_at, owned_at, Directory};
use crate::Error;

#[cfg(test)]
thread_local! {static FLUSH_COUNTS: std::cell::RefCell<[usize; 3]> = const { std::cell::RefCell::new([0; 3]) };}

#[cfg(test)]
pub(crate) fn take_flush_counts() -> [usize; 3] {
    FLUSH_COUNTS.with(|v| v.replace([0; 3]))
}

pub(crate) struct Lock(File);

impl Drop for Lock {
    fn drop(&mut self) {
        // Closing alone does not release a lock inherited across fork.
        let _ = self.0.unlock();
    }
}

impl Lock {
    pub(crate) fn verify(&self, directory: &Directory) -> Result<(), Error> {
        if identity(&self.0)? != identity(&directory.file("store.lock")?)? {
            return Err(Error::UnsafeInput);
        }
        Ok(())
    }
}

pub(crate) fn flush(file: &File) -> Result<(), Error> {
    #[cfg(test)]
    let before = org_platform::fs::testing::full_sync_counts();
    // Full device flush of our retained regular-file descriptor.
    let flushed = org_platform::fs::full_sync(file);
    #[cfg(test)]
    FLUSH_COUNTS.with(|v| {
        // The platform counts each step that succeeded, also when a later one failed.
        let after = org_platform::fs::testing::full_sync_counts();
        let mut counts = v.borrow_mut();
        counts[0] += (after.fsync - before.fsync) as usize;
        counts[1] += (after.device_flush - before.device_flush) as usize;
    });
    Ok(flushed?)
}

impl Directory {
    fn sync_directory(&self) -> Result<(), Error> {
        self.0.sync_all()?;
        #[cfg(test)]
        FLUSH_COUNTS.with(|v| v.borrow_mut()[2] += 1);
        Ok(())
    }

    pub(crate) fn duplicate_output(&self) -> Result<Self, Error> {
        // The module-owned '.' addresses this held directory only.
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        Ok(Self(owned_at(&self.0, c".", flags, 0)?))
    }

    pub(crate) fn create_store(path: &Path) -> Result<Self, Error> {
        let parent = Self::open(path.parent().ok_or(Error::UnsafeInput)?)?;
        let name = path.file_name().ok_or(Error::UnsafeInput)?;
        let cname = component(name)?;
        // A checked single component under the held parent, private fixed mode.
        match posix::make_directory_at(&parent.0, &cname, 0o700) {
            Ok(()) => parent.sync_directory()?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let root = Self(open_at(&parent.0, name, true)?);
        root.require_mode(0o700)?;
        Ok(root)
    }

    pub(crate) fn require_mode(&self, mode: u32) -> Result<(), Error> {
        let m = self.0.metadata()?;
        if !m.is_dir()
            || m.uid() != org_platform::posix::effective_uid()
            || m.mode() & 0o7777 != mode
        {
            return Err(Error::UnsafeInput);
        }
        Ok(())
    }

    pub(crate) fn same_directory(&self, other: &Self) -> Result<bool, Error> {
        let a = self.0.metadata()?;
        let b = other.0.metadata()?;
        Ok(a.dev() == b.dev() && a.ino() == b.ino())
    }

    pub(crate) fn lock(&self) -> Result<Lock, Error> {
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

    pub(crate) fn output_child(&self, name: &str, create: bool) -> Result<Self, Error> {
        if create {
            let cname = component(OsStr::new(name))?;
            // Create only a checked component under a retained directory.
            match posix::make_directory_at(&self.0, &cname, 0o700) {
                Ok(()) => self.sync_directory()?,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
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

    pub(crate) fn claim_output(&self, name: &str) -> Result<Self, Error> {
        let name = component(OsStr::new(name))?;
        // An exclusive claim of a checked, module-generated component.
        posix::make_directory_at(&self.0, &name, 0o700)?;
        self.sync_directory()?;
        self.output_child(name.to_str().map_err(|_| Error::UnsafeInput)?, false)
    }

    pub(crate) fn new_file(&self, name: &str) -> Result<File, Error> {
        let name = component(OsStr::new(name))?;
        // A retained directory, a checked single name, create-new no-follow.
        let flags =
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        let file = owned_at(&self.0, &name, flags, 0o600)?;
        identity(&file)?;
        Ok(file)
    }

    pub(crate) fn seal_file(&self, file: &File, executable: bool) -> Result<(), Error> {
        file.set_permissions(Permissions::from_mode(if executable {
            0o500
        } else {
            0o400
        }))?;
        flush(file)?;
        self.sync_directory()?;
        Ok(())
    }

    pub(crate) fn seal_directory(&self) -> Result<(), Error> {
        self.0.set_permissions(Permissions::from_mode(0o500))?;
        self.sync_directory()?;
        Ok(())
    }

    pub(crate) fn publish_directory(
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
        // Retained same-device dirs, validated names; never replace a tree.
        org_platform::fs::rename_noreplace_at(&self.0, &from, &target.0, &to)?;
        target.sync_directory()?;
        self.sync_directory()?;
        Ok(())
    }

    pub(crate) fn publish_control(&self, name: &str, bytes: &[u8]) -> Result<(), Error> {
        let temp = format!(".pending-{}", crate::ids::generate()?);
        let mut file = self.new_file(&temp)?;
        file.write_all(bytes)?;
        self.seal_file(&file, false)?;
        // Same no-replace primitive works for immutable regular control files.
        self.publish_directory(&temp, self, name)
    }

    /// Iterate without an accumulated-state memory cap. Each name is bounded by
    /// the filesystem, validated by the caller, and never used as a parsed path.
    pub(crate) fn visit_names(
        &self,
        mut visit: impl FnMut(&str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        // A new open description of '.', for an independent directory offset (dup shares it).
        for name in posix::Entries::of(&self.0)? {
            visit(name?.to_str().map_err(|_| Error::UnsafeInput)?)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flush_counts_the_steps_the_platform_took() {
        let directory = tempfile::tempdir().unwrap();
        let file = File::create(directory.path().join("file")).unwrap();
        take_flush_counts();
        let before = org_platform::fs::testing::full_sync_counts();
        flush(&file).unwrap();
        flush(&file).unwrap();
        let after = org_platform::fs::testing::full_sync_counts();
        let counts = take_flush_counts();
        assert_eq!(counts[0], 2);
        assert_eq!(counts[1] as u64, after.device_flush - before.device_flush);
        let per_flush = org_platform::fs::testing::device_flushes_per_full_sync().unwrap();
        assert_eq!(counts[1] as u64, 2 * per_flush);
        assert_eq!(counts[2], 0);
        assert_eq!(take_flush_counts(), [0; 3]);
    }
}
