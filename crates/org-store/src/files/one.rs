//! Publishing and reading one file directly in a directory, outside the canonical tree.

use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, io};

use super::read_limited;
use crate::error::FilesError;
use crate::path::TEMP_PREFIX;

/// Temporary names of [`publish_one`] in this process: one each, never reused.
pub(super) static ONE_TEMP: AtomicU64 = AtomicU64::new(0);

/// Publishes one file, `name`, directly in the existing folder `directory`, outside any
/// store: a configuration or marker file that is not a source (an office's `office.json`
/// and creation intent). Its bytes go to a new temporary file in that folder (never
/// through a link, a name no other writer uses), which is fully flushed, then renamed into
/// place, and the folder is synced. So after a crash the file is wholly old or wholly new.
///
/// `exclusive`: nothing at `name` is ever replaced. The same bytes already there are
/// success; other bytes are refused and kept. Otherwise the file is replaced.
pub fn publish_one(
    directory: &Path,
    name: &str,
    bytes: &[u8],
    exclusive: bool,
) -> Result<(), FilesError> {
    let target = one_path(directory, name)?;
    let (temp, mut file) = one_temporary(directory)?;
    let written = std::io::Write::write_all(&mut file, bytes)
        .map_err(|e| FilesError::io(&temp, "write", &e))
        .and_then(|()| {
            org_platform::fs::full_sync(&file).map_err(|e| FilesError::flush(&temp, "flush", &e))
        })
        .and_then(|()| place_one(&temp, &target, bytes, exclusive));
    if written.is_err() {
        // Best effort: a temporary file left by a failure is never read or projected.
        let _ = fs::remove_file(&temp);
        return written;
    }
    sync_one_directory(directory)
}

/// The bytes of `name` in `directory`, at most `limit` of them: `None` when there is no
/// such file. Never follows a link; anything but a regular file within the limit is
/// refused.
pub fn read_one(directory: &Path, name: &str, limit: u64) -> Result<Option<Vec<u8>>, FilesError> {
    let target = one_path(directory, name)?;
    read_limited(&target, limit)
}

/// `directory/name`, once `directory` is an absolute, real folder and `name` one portable
/// path component that is no temporary name.
pub(super) fn one_path(directory: &Path, name: &str) -> Result<PathBuf, FilesError> {
    let refused = |why: &str| {
        let error = io::Error::new(io::ErrorKind::InvalidInput, why.to_owned());
        FilesError::io(directory, "publish", &error)
    };
    if !directory.is_absolute() {
        return Err(refused("the folder must be absolute"));
    }
    if !crate::path::component_ok(name) {
        return Err(refused("the name is not one portable path component"));
    }
    match fs::symlink_metadata(directory) {
        Ok(meta) if meta.is_dir() => Ok(directory.join(name)),
        Ok(_) => Err(refused("not a folder")),
        Err(e) => Err(FilesError::io(directory, "open", &e)),
    }
}

pub(super) fn one_temporary(directory: &Path) -> Result<(PathBuf, fs::File), FilesError> {
    loop {
        let n = ONE_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = directory.join(format!("{TEMP_PREFIX}one-{}-{n}", std::process::id()));
        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(org_platform::fs::O_NOFOLLOW)
            .open(&temp);
        match opened {
            Ok(file) => return Ok((temp, file)),
            // Another process's, or one left by a dead process with this pid: never ours.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(FilesError::io(&temp, "create", &e)),
        }
    }
}

pub(super) fn place_one(
    temp: &Path,
    target: &Path,
    bytes: &[u8],
    exclusive: bool,
) -> Result<(), FilesError> {
    if !exclusive {
        return fs::rename(temp, target).map_err(|e| FilesError::io(target, "rename", &e));
    }
    use std::os::unix::ffi::OsStrExt;
    let c = |path: &Path| {
        std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| {
            let error = io::Error::new(io::ErrorKind::InvalidInput, "a NUL in the path");
            FilesError::io(path, "rename", &error)
        })
    };
    match org_platform::fs::rename_noreplace(&c(temp)?, &c(target)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(temp);
            let limit = bytes.len() as u64;
            if read_limited(target, limit).ok().flatten().as_deref() == Some(bytes) {
                return Ok(());
            }
            let error = io::Error::new(io::ErrorKind::AlreadyExists, "other bytes are there");
            Err(FilesError::io(target, "publish", &error))
        }
        Err(e) => Err(FilesError::io(target, "rename", &e)),
    }
}

pub(super) fn sync_one_directory(directory: &Path) -> Result<(), FilesError> {
    let folder = fs::File::open(directory).map_err(|e| FilesError::io(directory, "open", &e))?;
    folder
        .sync_all()
        .map_err(|e| FilesError::flush(directory, "flush directory", &e))
}
