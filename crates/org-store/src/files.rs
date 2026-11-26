//! Publication of canonical files under `durable/`: atomic, flushed before it is visible,
//! and never over bytes this office did not write.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, OpenOptions},
    io::{self, Read},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use serde::{Deserialize, Serialize};

use crate::{
    change::{Bytes, DurablePath, Sha256, TEMP_PREFIX},
    disk::{write_all, Disk, DiskFile, RealDisk},
};

mod one;
mod publish;

#[cfg(test)]
use one::ONE_TEMP;
pub use one::{publish_one, read_one};

/// The largest file the store reads or writes: the journal's entry limit.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileOp {
    Create,
    Put,
    Remove,
}

/// One accepted version of one path, in acceptance order.
#[derive(Clone, Copy, Debug)]
pub struct FileVersion<'a> {
    pub path: &'a DurablePath,
    pub op: FileOp,
    /// `None` for a removal.
    pub bytes: Option<&'a [u8]>,
    /// The hash of `bytes`; `None` for a removal.
    pub sha256: Option<Sha256>,
    /// The hash of the version this one replaced; `None` when there was no file.
    pub previous_sha256: Option<Sha256>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FilesError {
    /// The file holds bytes this office did not write. Nothing was replaced.
    Foreign { path: DurablePath },
    Io {
        path: String,
        action: &'static str,
        message: String,
        /// A flush failed. It must not be retried in this process.
        flush: bool,
    },
}

impl FilesError {
    fn io(path: &Path, action: &'static str, error: &io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            action,
            message: error.to_string(),
            flush: false,
        }
    }

    fn flush(path: &Path, action: &'static str, error: &io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            action,
            message: error.to_string(),
            flush: true,
        }
    }

    /// The path the failure names.
    pub fn path(&self) -> String {
        match self {
            Self::Foreign { path } => path.to_string(),
            Self::Io { path, .. } => path.clone(),
        }
    }
}

impl fmt::Display for FilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Foreign { path } => {
                write!(f, "{path} holds bytes this office did not write")
            }
            Self::Io {
                path,
                action,
                message,
                ..
            } => write!(f, "{action} {path}: {message}"),
        }
    }
}

impl std::error::Error for FilesError {}

/// The canonical files of one organization: `<data-root>/<organization-id>/durable`.
pub struct Files {
    root: PathBuf,
    disk: Arc<dyn Disk>,
    temp: AtomicU64,
}

struct Plan<'a> {
    /// What the file held before the first pending change.
    first_previous: Option<Sha256>,
    /// Every pending version's hash: after a crash mid-round the file may hold any of them.
    versions: Vec<Sha256>,
    last: FileVersion<'a>,
    /// One `Create` and nothing else: published by exclusive link, never by replacement.
    immutable: bool,
}

impl Files {
    /// The files under `root`, an absolute path, on the real disk.
    pub fn open(root: &Path) -> Result<Self, FilesError> {
        Self::new(root, Arc::new(RealDisk))
    }

    pub(crate) fn new(root: &Path, disk: Arc<dyn Disk>) -> Result<Self, FilesError> {
        if !root.is_absolute() {
            return Err(FilesError::io(
                root,
                "open",
                &io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the durable root must be absolute",
                ),
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
            disk,
            temp: AtomicU64::new(0),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The current bytes of a file, or `None` when there is none. Never follows a link.
    pub fn read(&self, path: &DurablePath) -> Result<Option<Bytes>, FilesError> {
        let full = self.root.join(path.as_str());
        let parent = full.parent().unwrap_or(&self.root);
        if !self.directories(parent, false, &mut BTreeSet::new())? {
            return Ok(None);
        }
        Ok(read_file(&full)?.map(Bytes::from))
    }

    /// Whether a directory stands at `path`: something lies, or once lay, below it, and
    /// no file can be published there. Directories are never removed.
    pub fn is_directory(&self, path: &DurablePath) -> Result<bool, FilesError> {
        let full = self.root.join(path.as_str());
        match fs::symlink_metadata(&full) {
            Ok(meta) => Ok(meta.is_dir()),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                Ok(false)
            }
            Err(e) => Err(FilesError::io(&full, "open", &e)),
        }
    }

    /// Every file at or under `prefix`, sorted by path bytes. Temporary files of an
    /// unfinished publication are skipped; anything that is not a file or a directory is
    /// an error.
    pub fn walk(&self, prefix: &DurablePath) -> Result<Vec<DurablePath>, FilesError> {
        self.walk_excluding(prefix, &[])
    }

    /// Like [`Self::walk`], but skips any path matching or under `excluded` BEFORE descending
    /// into directories or reading files (`TASK-97`).
    pub fn walk_excluding(
        &self,
        prefix: &DurablePath,
        excluded: &[DurablePath],
    ) -> Result<Vec<DurablePath>, FilesError> {
        if is_excluded(prefix, excluded) {
            return Ok(Vec::new());
        }
        let start = self.root.join(prefix.as_str());
        let parent = start.parent().unwrap_or(&self.root);
        let mut found = Vec::new();
        if !self.directories(parent, false, &mut BTreeSet::new())? {
            return Ok(found);
        }
        match fs::symlink_metadata(&start) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(found),
            Err(e) => return Err(FilesError::io(&start, "open", &e)),
            Ok(meta) if meta.is_file() => found.push(prefix.clone()),
            Ok(meta) if meta.is_dir() => self.walk_into(&start, prefix, excluded, &mut found)?,
            Ok(_) => return Err(not_a_record(&start)),
        }
        found.sort();
        Ok(found)
    }

    fn walk_into(
        &self,
        directory: &Path,
        prefix: &DurablePath,
        excluded: &[DurablePath],
        found: &mut Vec<DurablePath>,
    ) -> Result<(), FilesError> {
        let entries = fs::read_dir(directory).map_err(|e| FilesError::io(directory, "list", &e))?;
        for entry in entries {
            let entry = entry.map_err(|e| FilesError::io(directory, "list", &e))?;
            let full = entry.path();
            let kind = entry
                .file_type()
                .map_err(|e| FilesError::io(&full, "open", &e))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err(not_a_record(&full));
            };
            if kind.is_file() && name.starts_with(TEMP_PREFIX) {
                continue;
            }
            let path = prefix.join(name).map_err(|_| not_a_record(&full))?;
            if is_excluded(&path, excluded) {
                continue;
            }
            if kind.is_dir() {
                self.walk_into(&full, &path, excluded, found)?;
            } else if kind.is_file() {
                found.push(path);
            } else {
                return Err(not_a_record(&full));
            }
        }
        Ok(())
    }
}

fn is_excluded(path: &DurablePath, excluded: &[DurablePath]) -> bool {
    excluded.iter().any(|ex| path == ex || path.is_under(ex))
}

fn not_a_record(path: &Path) -> FilesError {
    FilesError::io(
        path,
        "walk",
        &io::Error::new(
            io::ErrorKind::InvalidData,
            "not a file or directory the store could have written",
        ),
    )
}

fn read_file(full: &Path) -> Result<Option<Vec<u8>>, FilesError> {
    read_limited(full, MAX_FILE_BYTES)
}

/// A regular file of at most `limit` bytes, never through a link: `None` when absent.
fn read_limited(full: &Path, limit: u64) -> Result<Option<Vec<u8>>, FilesError> {
    let limit = limit.min(MAX_FILE_BYTES);
    let file = match OpenOptions::new()
        .read(true)
        // Never follows a link, and never waits on a FIFO someone put there.
        .custom_flags(org_platform::fs::O_NOFOLLOW | org_platform::fs::O_NONBLOCK)
        .open(full)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(FilesError::io(full, "open", &e)),
    };
    let meta = file
        .metadata()
        .map_err(|e| FilesError::io(full, "open", &e))?;
    if !meta.is_file() || meta.len() > limit {
        return Err(FilesError::io(
            full,
            "read",
            &io::Error::new(
                io::ErrorKind::InvalidData,
                "not a file within the size limit",
            ),
        ));
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| FilesError::io(full, "read", &e))?;
    if bytes.len() as u64 > limit {
        return Err(FilesError::io(
            full,
            "read",
            &io::Error::new(io::ErrorKind::InvalidData, "larger than the size limit"),
        ));
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod one_file_tests;
