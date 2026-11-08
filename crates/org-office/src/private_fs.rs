//! Owner-only filesystem helpers shared by the office and its storage.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Opens `path` for writing, creating it owner-only (0600) if missing; never truncates.
pub(crate) fn private_file(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)?;
    // `mode` only applies on creation; a file left by an earlier run may be looser.
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

/// Creates `path` and any missing parents as owner-only (0700) directories.
pub(crate) fn private_dir(path: &Path) -> std::io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(path)
}

/// Makes an existing `path` owner-only (0700 for directories).
pub(crate) fn tighten_dir(path: &Path) -> std::io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

/// Makes `path` owner-only (0600) if it exists; a missing file is fine.
pub(crate) fn tighten_file_if_present(path: &Path) -> std::io::Result<()> {
    match fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Makes `path` absolute without touching the filesystem. Needed because the database path
/// goes to SQLite, whose bundled build reads any path starting with `file:` as a URI.
pub(crate) fn absolute(path: PathBuf) -> std::io::Result<PathBuf> {
    std::path::absolute(path)
}
