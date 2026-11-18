//! The macOS filesystem adapter.

use std::ffi::CStr;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Component, Path};

use super::FilesystemRefusal;
use crate::Refused;

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

pub(super) fn paths_may_alias(a: &Path, b: &Path) -> Result<bool, Refused> {
    // Compare components, not string prefixes: APFS/HFS+ also equate canonical
    // Unicode spellings. No localized, diacritic-ignoring or width-ignoring flags.
    // Existing registered targets retain their proven distinct inode ownership
    // on case-sensitive volumes; this conservative comparison is for new claims.
    Ok(a.components()
        .zip(b.components())
        .all(|(left, right)| match (left, right) {
            (Component::Normal(left), Component::Normal(right)) => {
                use core_foundation::base::{CFComparisonResult, TCFType};
                use core_foundation::string::{
                    kCFCompareCaseInsensitive, kCFCompareNonliteral, CFString, CFStringCompare,
                };
                let left = CFString::new(&left.to_string_lossy());
                let right = CFString::new(&right.to_string_lossy());
                // SAFETY: both owned strings remain live throughout this comparison.
                unsafe {
                    CFStringCompare(
                        left.as_concrete_TypeRef(),
                        right.as_concrete_TypeRef(),
                        kCFCompareCaseInsensitive | kCFCompareNonliteral,
                    ) == CFComparisonResult::EqualTo
                }
            }
            (left, right) => left == right,
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_path_comparison_preserves_component_and_accent_boundaries() {
        assert!(paths_may_alias(
            Path::new("/data/CAF\u{c9}"),
            Path::new("/data/cafe\u{301}/child"),
        )
        .unwrap());
        assert!(!paths_may_alias(Path::new("/data/caf\u{e9}"), Path::new("/data/cafe")).unwrap());
        assert!(
            !paths_may_alias(Path::new("/data/caf\u{e9}"), Path::new("/data/caf\u{e9}2"),).unwrap()
        );
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
