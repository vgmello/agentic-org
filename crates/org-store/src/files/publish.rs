//! Materializing accepted versions into the canonical tree: publication, temporary names and
//! directories.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::{fs, io};

use super::{read_file, FileOp, FileVersion, Files, Plan};
use crate::change::Sha256;
use crate::disk::{write_all, DiskFile};
use crate::error::FilesError;
use crate::path::{DurablePath, TEMP_PREFIX};

impl Files {
    /// Writes the final state of each path over `versions` (in acceptance order): each file
    /// wholly old or wholly new, its data flushed before it is visible, then every touched
    /// directory, then one full flush. Any error leaves the round unfinished; the caller
    /// must not record it as done, and must not retry after a flush error.
    pub fn materialize(&self, versions: &[FileVersion<'_>]) -> Result<(), FilesError> {
        let mut plans: BTreeMap<&DurablePath, Plan<'_>> = BTreeMap::new();
        for version in versions {
            match plans.get_mut(version.path) {
                Some(plan) => {
                    plan.versions.extend(version.sha256);
                    plan.last = *version;
                    plan.immutable = false;
                }
                None => {
                    plans.insert(
                        version.path,
                        Plan {
                            first_previous: version.previous_sha256,
                            versions: version.sha256.into_iter().collect(),
                            last: *version,
                            immutable: version.op == FileOp::Create,
                        },
                    );
                }
            }
        }
        if plans.is_empty() {
            return Ok(());
        }
        let mut directories = BTreeSet::new();
        let mut written: Option<(PathBuf, Arc<dyn DiskFile>)> = None;
        for (path, plan) in &plans {
            self.publish(path, plan, &mut directories, &mut written)?;
        }
        for directory in &directories {
            self.disk
                .sync_dir(directory)
                .map_err(|e| FilesError::flush(directory, "flush directory", &e))?;
        }
        // Directory entries are now issued to the device. A full flush of any file on the
        // same file system empties the drive's cache behind them.
        match written {
            Some((path, file)) => file
                .flush_full()
                .map_err(|e| FilesError::flush(&path, "flush", &e)),
            None => {
                let (path, file) = self.temporary(&self.root)?;
                file.flush_full()
                    .map_err(|e| FilesError::flush(&path, "flush", &e))?;
                self.disk
                    .unlink(&path)
                    .map_err(|e| FilesError::io(&path, "remove", &e))
            }
        }
    }

    pub(super) fn publish(
        &self,
        path: &DurablePath,
        plan: &Plan<'_>,
        directories: &mut BTreeSet<PathBuf>,
        written: &mut Option<(PathBuf, Arc<dyn DiskFile>)>,
    ) -> Result<(), FilesError> {
        let full = self.root.join(path.as_str());
        let parent = full.parent().unwrap_or(&self.root).to_path_buf();
        if !path.names_a_file() {
            return Err(FilesError::io(
                &full,
                "publish",
                &io::Error::new(io::ErrorKind::InvalidInput, "a tree is not a file"),
            ));
        }
        self.directories(&parent, true, directories)?;
        let current = read_file(&full)?.map(|bytes| Sha256::of(&bytes));
        if let Some(current) = current {
            if plan.first_previous != Some(current) && !plan.versions.contains(&current) {
                return Err(FilesError::Foreign { path: path.clone() });
            }
        }
        let Some(bytes) = plan.last.bytes else {
            if current.is_some() {
                self.disk
                    .unlink(&full)
                    .map_err(|e| FilesError::io(&full, "remove", &e))?;
            }
            return Ok(());
        };
        if current.is_some() && current == plan.last.sha256 {
            return Ok(());
        }
        let (temp, file) = self.temporary(&parent)?;
        write_all(file.as_ref(), bytes).map_err(|e| FilesError::io(&temp, "write", &e))?;
        file.flush_full()
            .map_err(|e| FilesError::flush(&temp, "flush", &e))?;
        if plan.immutable {
            match self.disk.link(&temp, &full) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    // Something appeared since the check above. Only our own bytes may stay.
                    if read_file(&full)?.as_deref() != Some(bytes) {
                        return Err(FilesError::Foreign { path: path.clone() });
                    }
                }
                Err(e) => return Err(FilesError::io(&full, "link", &e)),
            }
            self.disk
                .unlink(&temp)
                .map_err(|e| FilesError::io(&temp, "remove", &e))?;
        } else {
            self.disk
                .rename(&temp, &full)
                .map_err(|e| FilesError::io(&full, "rename", &e))?;
        }
        *written = Some((full, file));
        Ok(())
    }

    /// A new, empty temporary file in `directory`. A name left by a dead process is ours
    /// by its prefix and is replaced.
    pub(super) fn temporary(
        &self,
        directory: &Path,
    ) -> Result<(PathBuf, Arc<dyn DiskFile>), FilesError> {
        let name = format!(
            "{TEMP_PREFIX}{}-{}",
            std::process::id(),
            self.temp.fetch_add(1, Ordering::Relaxed)
        );
        let path = directory.join(name);
        let file = match self.disk.create_new(&path) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                self.disk
                    .unlink(&path)
                    .map_err(|e| FilesError::io(&path, "remove", &e))?;
                self.disk.create_new(&path)
            }
            other => other,
        }
        .map_err(|e| FilesError::io(&path, "create", &e))?;
        Ok((path, file))
    }

    /// Checks that every directory from the root down to `directory` is a real directory
    /// (never a link). With `create`, missing ones are made `0700`, and every link of the
    /// chain, the root's own included, joins `touched`: a directory that is visible does
    /// not prove an earlier process flushed its entry.
    pub(super) fn directories(
        &self,
        directory: &Path,
        create: bool,
        touched: &mut BTreeSet<PathBuf>,
    ) -> Result<bool, FilesError> {
        let below = directory.strip_prefix(&self.root).map_err(|_| {
            FilesError::io(
                directory,
                "open",
                &io::Error::new(io::ErrorKind::InvalidInput, "outside the durable root"),
            )
        })?;
        let mut current = self.root.clone();
        let mut components = below.components();
        loop {
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    return Err(FilesError::io(
                        &current,
                        "open",
                        &io::Error::new(io::ErrorKind::InvalidInput, "not a directory"),
                    ))
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    if !create {
                        return Ok(false);
                    }
                    self.disk
                        .create_dir(&current)
                        .map_err(|e| FilesError::io(&current, "create directory", &e))?;
                }
                Err(e) => return Err(FilesError::io(&current, "open", &e)),
            }
            if create {
                if let Some(parent) = current.parent() {
                    touched.insert(parent.to_path_buf());
                }
                touched.insert(current.clone());
            }
            match components.next() {
                Some(component) => current.push(component),
                None => return Ok(true),
            }
        }
    }
}
