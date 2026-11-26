//! Writing a file all at once: an owner-only (`0600`) temporary file beside the destination,
//! fully flushed, then renamed into place without replacing anything, then the folder
//! flushed. A failure or a drop before [`AtomicFile::commit`] removes the temporary file.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use ring::rand::{SecureRandom, SystemRandom};

use crate::error::BackupError;

pub(crate) struct AtomicFile {
    destination: PathBuf,
    temporary: PathBuf,
    file: Option<File>,
}

fn c_path(path: &Path) -> Result<CString, BackupError> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| BackupError::Usage(format!("{} holds a NUL byte", path.display())))
}

/// Flushes the entries of folder `dir` to the device.
pub(crate) fn sync_dir(dir: &Path) -> Result<(), BackupError> {
    let context = || format!("flushing {}", dir.display());
    let handle = File::open(dir).map_err(BackupError::io(context()))?;
    org_platform::fs::full_sync(&handle).map_err(BackupError::io(context()))
}

/// A random suffix for a temporary name.
pub(crate) fn random_suffix() -> Result<String, BackupError> {
    let mut bytes = [0u8; 8];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| BackupError::Refused("no system randomness".into()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

impl AtomicFile {
    /// Starts writing `destination`, which must not exist and whose folder must.
    pub(crate) fn create(destination: &Path) -> Result<Self, BackupError> {
        let destination = std::path::absolute(destination).map_err(BackupError::io(format!(
            "resolving {}",
            destination.display()
        )))?;
        let (Some(folder), Some(name)) = (destination.parent(), destination.file_name()) else {
            return Err(BackupError::Usage(format!(
                "{} is not a file path",
                destination.display()
            )));
        };
        match std::fs::symlink_metadata(&destination) {
            Ok(_) => {
                return Err(BackupError::Usage(format!(
                    "{} already exists; choose a new --out path",
                    destination.display()
                )))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(BackupError::io(format!(
                    "checking {}",
                    destination.display()
                ))(e))
            }
        }
        if !folder.is_dir() {
            return Err(BackupError::Usage(format!(
                "the folder {} does not exist",
                folder.display()
            )));
        }
        let mut temporary_name = std::ffi::OsString::from(".");
        temporary_name.push(name);
        temporary_name.push(format!(".part-{}", random_suffix()?));
        let temporary = folder.join(temporary_name);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(org_platform::fs::O_NOFOLLOW | org_platform::fs::O_CLOEXEC)
            .open(&temporary)
            .map_err(BackupError::io(format!("creating {}", temporary.display())))?;
        Ok(Self {
            destination,
            temporary,
            file: Some(file),
        })
    }

    pub(crate) fn destination(&self) -> &Path {
        &self.destination
    }

    /// Takes the file to wrap it in writers; it comes back through [`AtomicFile::commit`].
    pub(crate) fn take(&mut self) -> Result<File, BackupError> {
        self.file
            .take()
            .ok_or_else(|| BackupError::Refused("the output was already taken".into()))
    }

    /// Flushes `file` (the one [`AtomicFile::take`] gave), renames it into place without
    /// replacing anything, and flushes the folder.
    pub(crate) fn commit(mut self, file: File) -> Result<PathBuf, BackupError> {
        org_platform::fs::full_sync(&file).map_err(BackupError::io("flushing the archive"))?;
        drop(file);
        let renamed = org_platform::fs::rename_noreplace(
            &c_path(&self.temporary)?,
            &c_path(&self.destination)?,
        );
        renamed.map_err(BackupError::io(format!(
            "moving the archive into place at {}",
            self.destination.display()
        )))?;
        // Renamed: nothing for the drop to remove.
        self.temporary = PathBuf::new();
        if let Some(folder) = self.destination.parent() {
            sync_dir(folder)?;
        }
        Ok(std::mem::take(&mut self.destination))
    }
}

impl Drop for AtomicFile {
    fn drop(&mut self) {
        if !self.temporary.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.temporary);
        }
    }
}
