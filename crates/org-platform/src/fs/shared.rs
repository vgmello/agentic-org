//! POSIX filesystem helpers shared by the macOS and GNU/Linux adapters.

use std::fs::{DirBuilder, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Path, PathBuf};

fn create_one_owner_only_dir(path: &Path) -> io::Result<()> {
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            let meta = std::fs::symlink_metadata(path)?;
            if meta.file_type().is_dir() {
                return Ok(());
            }
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} exists and is not a real directory", path.display()),
            ))
        }
        Err(err) => Err(err),
    }
}

pub(super) fn create_owner_only_dir_all(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => return Ok(()),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} exists and is not a real directory", path.display()),
            ));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let mut missing = Vec::new();
    let mut cursor = path;
    while let Some(parent) = cursor.parent() {
        if let Some(name) = cursor.file_name() {
            missing.push(name.to_os_string());
        }
        if parent.as_os_str().is_empty() || parent.exists() {
            let mut current = parent.to_path_buf();
            for seg in missing.into_iter().rev() {
                current.push(seg);
                create_one_owner_only_dir(&current)?;
            }
            return Ok(());
        }
        cursor = parent;
    }
    create_one_owner_only_dir(path)
}

pub(super) fn write_owner_only_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(bytes)
}

pub(super) fn open_nofollow_ro(path: &Path) -> io::Result<std::fs::File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
}

pub(super) fn create_symlink(original: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

pub(super) fn effective_uid() -> io::Result<u32> {
    // SAFETY: `geteuid(2)` takes no pointers, has no preconditions, and always succeeds.
    Ok(unsafe { libc::geteuid() })
}

pub(super) fn verify_owner_only_dir(path: &Path, expected_uid: u32) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be a real directory (never a symlink)",
                path.display()
            ),
        ));
    }
    if meta.uid() != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not owned by the expected account", path.display()),
        ));
    }
    if (meta.permissions().mode() & 0o7077) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} permissions are too permissive (require owner-only 0700 without special bits)",
                path.display()
            ),
        ));
    }
    Ok(())
}

pub(super) fn verify_owner_only_ancestors(
    path: &Path,
    stop_at: &Path,
    expected_uid: u32,
) -> io::Result<()> {
    let rel = path.strip_prefix(stop_at).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not under {}", path.display(), stop_at.display()),
        )
    })?;
    verify_owner_only_dir(stop_at, expected_uid)?;
    let mut current = stop_at.to_path_buf();
    for component in rel.components() {
        current.push(component);
        verify_owner_only_dir(&current, expected_uid)?;
    }
    Ok(())
}

pub(super) fn verify_owner_only_file(
    path: &Path,
    expected_uid: u32,
    require_exec: bool,
) -> io::Result<PathBuf> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be a regular file (never a symlink)",
                path.display()
            ),
        ));
    }
    if meta.uid() != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not owned by the expected account", path.display()),
        ));
    }
    let mode = meta.permissions().mode();
    if (mode & 0o7077) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} permissions are too permissive (require owner-only without special bits)",
                path.display()
            ),
        ));
    }
    if require_exec && (mode & 0o100) == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} must be executable by owner", path.display()),
        ));
    }
    path.canonicalize()
}
