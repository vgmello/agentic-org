//! POSIX filesystem helpers shared by the macOS and GNU/Linux adapters.

use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Component, Path, PathBuf};

/// Strips trailing `/` and `/.` so POSIX `lstat(2)` inspects the final path component itself
/// rather than dereferencing a symlinked directory (`TASK-167`, `TASK-168`).
fn normalize_lstat_path(path: &Path) -> PathBuf {
    path.components().collect()
}

pub(super) fn set_owner_only_dir(path: &Path) -> io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

pub(super) fn create_owner_only_dir(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(path)
}

fn create_one_owner_only_dir(path: &Path) -> io::Result<()> {
    let norm = normalize_lstat_path(path);
    match DirBuilder::new().mode(0o700).create(&norm) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            let meta = std::fs::symlink_metadata(&norm)?;
            if meta.file_type().is_dir() {
                return Ok(());
            }
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} exists and is not a real directory", norm.display()),
            ))
        }
        Err(err) => Err(err),
    }
}

pub(super) fn create_owner_only_dir_all(path: &Path) -> io::Result<()> {
    let norm = normalize_lstat_path(path);
    match std::fs::symlink_metadata(&norm) {
        Ok(meta) if meta.file_type().is_dir() => return Ok(()),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} exists and is not a real directory", norm.display()),
            ));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let mut missing = Vec::new();
    let mut cursor = norm.as_path();
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
    create_one_owner_only_dir(&norm)
}

pub(super) fn write_owner_only_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(bytes)
}

pub(super) fn open_nofollow_ro(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    Ok(file)
}

pub(super) fn create_symlink(original: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

pub(super) fn effective_uid() -> io::Result<u32> {
    // SAFETY: `geteuid(2)` takes no pointers, has no preconditions, and always succeeds.
    Ok(unsafe { libc::geteuid() })
}

pub(super) fn verify_owner_only_dir(path: &Path, expected_uid: u32) -> io::Result<()> {
    let norm = normalize_lstat_path(path);
    let meta = std::fs::symlink_metadata(&norm)?;
    if !meta.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be a real directory (never a symlink)",
                norm.display()
            ),
        ));
    }
    if meta.uid() != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not owned by the expected account", norm.display()),
        ));
    }
    if (meta.permissions().mode() & 0o7077) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} permissions are too permissive (require owner-only 0700 without special bits)",
                norm.display()
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
    if stop_at
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} must not contain `..` components", stop_at.display()),
        ));
    }
    let norm_stop = normalize_lstat_path(stop_at);
    let norm_path = normalize_lstat_path(path);
    let rel = norm_path.strip_prefix(&norm_stop).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is not under {}",
                norm_path.display(),
                norm_stop.display()
            ),
        )
    })?;
    verify_owner_only_dir(&norm_stop, expected_uid)?;
    let mut current = norm_stop;
    for component in rel.components() {
        let Component::Normal(seg) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{} contains disallowed relative component under {}",
                    norm_path.display(),
                    stop_at.display()
                ),
            ));
        };
        current.push(seg);
        verify_owner_only_dir(&current, expected_uid)?;
    }
    Ok(())
}

pub(super) fn verify_owner_only_file(
    path: &Path,
    expected_uid: u32,
    require_exec: bool,
) -> io::Result<PathBuf> {
    let norm = normalize_lstat_path(path);
    let meta = std::fs::symlink_metadata(&norm)?;
    if !meta.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be a regular file (never a symlink)",
                norm.display()
            ),
        ));
    }
    if meta.uid() != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not owned by the expected account", norm.display()),
        ));
    }
    let mode = meta.permissions().mode();
    if (mode & 0o7077) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} permissions are too permissive (require owner-only without special bits)",
                norm.display()
            ),
        ));
    }
    if require_exec && (mode & 0o100) == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} must be executable by owner", norm.display()),
        ));
    }
    norm.canonicalize()
}
