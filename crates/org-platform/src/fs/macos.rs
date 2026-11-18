//! The macOS filesystem adapter.

use std::ffi::CStr;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

use super::FilesystemRefusal;

pub(super) fn full_sync(file: &File) -> io::Result<()> {
    file.sync_all()?;
    #[cfg(any(test, feature = "test-support"))]
    super::testing::COUNTS.with(|counts| {
        counts.update(|mut now| {
            now.fsync += 1;
            now
        })
    });
    // SAFETY: a live descriptor and a command that takes no pointer argument. Failure
    // is returned, never downgraded to the ordinary flush above.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    #[cfg(any(test, feature = "test-support"))]
    super::testing::COUNTS.with(|counts| {
        counts.update(|mut now| {
            now.device_flush += 1;
            now
        })
    });
    Ok(())
}

/// One `F_FULLFSYNC` after every `fsync`.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn device_flushes_per_full_sync() -> Result<u64, crate::Refused> {
    Ok(1)
}

pub(super) fn rename_noreplace_at(
    from_dir: &File,
    from: &CStr,
    to_dir: &File,
    to: &CStr,
) -> io::Result<()> {
    // SAFETY: both descriptors are live directories the caller retains, and both names
    // are NUL-terminated strings that live across the call. The exclusive rename never
    // overwrites an existing entry.
    let rc = unsafe {
        libc::renameatx_np(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn rename_noreplace(from: &CStr, to: &CStr) -> io::Result<()> {
    // SAFETY: both paths are live NUL-terminated strings; exclusive rename never overwrites.
    let rc = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn set_errno(value: i32) -> io::Result<()> {
    // SAFETY: libc returns this thread's errno pointer; no shared memory access.
    unsafe {
        *libc::__error() = value;
    }
    Ok(())
}

pub(super) fn device_and_inode(stat: &libc::stat) -> io::Result<(u64, u64)> {
    Ok((stat.st_dev as u64, stat.st_ino))
}

pub(super) fn check_instance_root(
    _dir: &File,
    _path: &Path,
    _root: bool,
) -> Result<(), FilesystemRefusal> {
    Ok(())
}

pub(super) fn require_disk(_dir: &File, _path: &Path) -> Result<(), FilesystemRefusal> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_full_sync_ends_with_one_device_flush() {
        let directory = tempfile::tempdir().unwrap();
        let file = File::create(directory.path().join("file")).unwrap();
        let before = crate::fs::testing::full_sync_counts();
        full_sync(&file).unwrap();
        let after = crate::fs::testing::full_sync_counts();
        assert_eq!(after.fsync - before.fsync, 1);
        assert_eq!(after.device_flush - before.device_flush, 1);
    }

    #[test]
    fn an_instance_root_and_a_temp_base_have_no_filesystem_rule() {
        let directory = File::open("/").unwrap();
        for root in [false, true] {
            check_instance_root(&directory, Path::new("/"), root).unwrap();
        }
        require_disk(&directory, Path::new("/")).unwrap();
    }
}
