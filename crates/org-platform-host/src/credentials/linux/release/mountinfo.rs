//! The kernel's mount table of this process, and where a held directory is in it.

use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, BorrowedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use super::descriptors::identity;
use crate::credentials::mounts::{self, Mount, Place, Table};
use crate::credentials::MountDoubt;

/// The kernel's table of this process's mounts.
const MOUNT_TABLE: &str = "/proc/self/mountinfo";

/// The bytes of the kernel's mount table of this process, or the doubt that not
/// having them leaves.
pub(super) fn mount_table() -> Result<Vec<u8>, MountDoubt> {
    let unreadable = |error: io::Error| MountDoubt::Unreadable(error.kind());
    let mut table = Vec::new();
    File::open(MOUNT_TABLE)
        .map_err(unreadable)?
        .take(mounts::LARGEST_TABLE as u64 + 1)
        .read_to_end(&mut table)
        .map_err(unreadable)?;
    Ok(table)
}

/// Where the open directory `fd` is inside its filesystem, the mount it is in, and its
/// path from this process's root: the mount the kernel says it is in (`statx`), found in
/// the mount table `table` by its id, and the directory's path (its descriptor's entry
/// in `/proc/self/fd`), placed below that mount.
///
/// The path is only believed if it names the same directory the descriptor holds: a
/// directory that was deleted or moved has a path that does not.
pub(super) fn place(
    fd: BorrowedFd<'_>,
    table: &Table<'_>,
) -> Result<(Place, Mount, PathBuf), MountDoubt> {
    let unreadable = |error: io::Error| MountDoubt::Unreadable(error.kind());
    // SAFETY: an all-zero `statx` is a valid value for the call to overwrite.
    let mut stat: libc::statx = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor, an empty NUL-terminated path with `AT_EMPTY_PATH`
    // (the descriptor itself is examined), and a valid out-pointer.
    let status = unsafe {
        libc::statx(
            fd.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_MNT_ID,
            &mut stat,
        )
    };
    if status != 0 {
        return Err(unreadable(io::Error::last_os_error()));
    }
    if stat.stx_mask & libc::STATX_MNT_ID == 0 {
        return Err(MountDoubt::NoMountId);
    }
    // The mount first: where its root was deleted, no path names the directory.
    let mount = table.mount(stat.stx_mnt_id)?;
    let path =
        std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())).map_err(unreadable)?;
    let held = File::from(fd.try_clone_to_owned().map_err(unreadable)?)
        .metadata()
        .map_err(unreadable)?;
    let named = std::fs::metadata(&path).map_err(|_| MountDoubt::PathOutsideMount)?;
    if identity(&named) != identity(&held) {
        return Err(MountDoubt::PathOutsideMount);
    }
    let place = mounts::place(&mount, path.as_os_str().as_bytes())?;
    Ok((place, mount, path))
}
