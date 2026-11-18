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

    fn publish(
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
    fn temporary(&self, directory: &Path) -> Result<(PathBuf, Arc<dyn DiskFile>), FilesError> {
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
    fn directories(
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
            Ok(meta) if meta.is_dir() => self.walk_into(&start, prefix, &mut found)?,
            Ok(_) => return Err(not_a_record(&start)),
        }
        found.sort();
        Ok(found)
    }

    fn walk_into(
        &self,
        directory: &Path,
        prefix: &DurablePath,
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
            if kind.is_dir() {
                self.walk_into(&full, &path, found)?;
            } else if kind.is_file() {
                found.push(path);
            } else {
                return Err(not_a_record(&full));
            }
        }
        Ok(())
    }
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
    let file = match OpenOptions::new()
        .read(true)
        // Never follows a link, and never waits on a FIFO someone put there.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(full)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(FilesError::io(full, "open", &e)),
    };
    let meta = file
        .metadata()
        .map_err(|e| FilesError::io(full, "open", &e))?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
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
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| FilesError::io(full, "read", &e))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(FilesError::io(
            full,
            "read",
            &io::Error::new(io::ErrorKind::InvalidData, "larger than the size limit"),
        ));
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::testkit::faults::{DiskOp, Faults, Target};

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        faults: Arc<Faults>,
        files: Files,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        // macOS temporary directories sit behind a link; the root must be the real path.
        let root = dir.path().canonicalize().unwrap().join("durable");
        let faults = Faults::new();
        let files = faults.files(&root).unwrap();
        Fixture {
            _dir: dir,
            root,
            faults,
            files,
        }
    }

    fn path(text: &str) -> DurablePath {
        DurablePath::parse(text).unwrap()
    }

    fn version<'a>(
        path: &'a DurablePath,
        op: FileOp,
        bytes: Option<&'a [u8]>,
        previous: Option<&[u8]>,
    ) -> FileVersion<'a> {
        FileVersion {
            path,
            op,
            bytes,
            sha256: bytes.map(Sha256::of),
            previous_sha256: previous.map(Sha256::of),
        }
    }

    fn on_disk(f: &Fixture, path: &str) -> Option<Vec<u8>> {
        fs::read(f.root.join(path)).ok()
    }

    #[test]
    fn a_relative_root_is_refused() {
        assert!(Files::open(Path::new("durable")).is_err());
        assert!(Faults::new().files(Path::new("durable")).is_err());
    }

    #[test]
    fn a_round_flushes_the_whole_directory_chain_and_reads_flush_nothing() {
        let f = fixture();
        // Visible directories do not prove an earlier creator flushed their links.
        fs::create_dir_all(f.root.join("state/teams")).unwrap();
        let p = path("state/teams/a.yaml");
        f.files
            .materialize(&[version(&p, FileOp::Put, Some(b"one"), None)])
            .unwrap();
        assert_eq!(
            f.faults.paths(DiskOp::SyncDir),
            [
                f.root.parent().unwrap().to_path_buf(),
                f.root.clone(),
                f.root.join("state"),
                f.root.join("state/teams")
            ]
        );
        // The file's data is flushed before the rename, and once more after the directories.
        assert_eq!(f.faults.paths(DiskOp::Flush).len(), 2);
        f.faults.clear_log();
        assert_eq!(&*f.files.read(&p).unwrap().unwrap(), b"one");
        assert_eq!(f.files.read(&path("state/none/x")).unwrap(), None);
        assert!(f.faults.log().is_empty());
    }

    #[test]
    fn a_failed_directory_flush_fails_the_round_for_new_and_identical_bytes_and_is_not_retried() {
        for existing in [false, true] {
            let f = fixture();
            let p = path("state/a.yaml");
            if existing {
                f.files
                    .materialize(&[version(&p, FileOp::Create, Some(b"same"), None)])
                    .unwrap();
                f.faults.clear_log();
            }
            f.faults.fail(DiskOp::SyncDir, Target::Directory, 2);
            let error = f
                .files
                .materialize(&[version(&p, FileOp::Create, Some(b"same"), None)])
                .unwrap_err();
            assert!(
                matches!(&error, FilesError::Io { flush: true, path, .. } if *path == f.root.display().to_string()),
                "{error:?}"
            );
            // Two calls: the one before it and the failed one. Nothing after it.
            assert_eq!(f.faults.paths(DiskOp::SyncDir).len(), 2);
        }
    }

    #[test]
    fn files_and_directories_are_private_to_the_owner() {
        let f = fixture();
        let p = path("state/teams/t/team.yaml");
        f.files
            .materialize(&[version(&p, FileOp::Put, Some(b"x"), None)])
            .unwrap();
        let mode = |path: PathBuf| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(f.root.join("state/teams/t/team.yaml")), 0o600);
        for directory in ["", "state", "state/teams", "state/teams/t"] {
            assert_eq!(mode(f.root.join(directory)), 0o700, "{directory}");
        }
    }

    #[test]
    fn create_is_exclusive_identical_bytes_are_kept_and_other_bytes_stop() {
        let f = fixture();
        let p = path("messaging/m/1.json");
        let create = [version(&p, FileOp::Create, Some(b"event"), None)];
        f.files.materialize(&create).unwrap();
        assert_eq!(on_disk(&f, "messaging/m/1.json").unwrap(), b"event");
        f.faults.clear_log();
        f.files.materialize(&create).unwrap();
        assert!(f.faults.paths(DiskOp::Link).is_empty());

        fs::write(f.root.join("messaging/m/1.json"), b"other").unwrap();
        assert_eq!(
            f.files.materialize(&create),
            Err(FilesError::Foreign { path: p.clone() })
        );
        assert_eq!(on_disk(&f, "messaging/m/1.json").unwrap(), b"other");
        // No temporary file is left where a walk would see it.
        assert_eq!(
            f.files.walk(&path("messaging")).unwrap(),
            std::slice::from_ref(&p)
        );
    }

    #[test]
    fn put_replaces_only_a_version_this_office_wrote() {
        let f = fixture();
        let p = path("state/t.yaml");
        f.files
            .materialize(&[version(&p, FileOp::Put, Some(b"v1"), None)])
            .unwrap();
        f.files
            .materialize(&[version(&p, FileOp::Put, Some(b"v2"), Some(b"v1"))])
            .unwrap();
        assert_eq!(on_disk(&f, "state/t.yaml").unwrap(), b"v2");

        // Foreign bytes: stop and name the path, replacing nothing.
        fs::write(f.root.join("state/t.yaml"), b"edited by hand").unwrap();
        let put = [version(&p, FileOp::Put, Some(b"v3"), Some(b"v2"))];
        assert_eq!(
            f.files.materialize(&put),
            Err(FilesError::Foreign { path: p.clone() })
        );
        assert_eq!(on_disk(&f, "state/t.yaml").unwrap(), b"edited by hand");

        // A missing file is recreated from the journal.
        fs::remove_file(f.root.join("state/t.yaml")).unwrap();
        f.files.materialize(&put).unwrap();
        assert_eq!(on_disk(&f, "state/t.yaml").unwrap(), b"v3");
    }

    #[test]
    fn a_round_writes_the_final_state_and_accepts_any_retained_version_after_a_crash() {
        let f = fixture();
        let (a, b) = (path("state/a"), path("state/b"));
        let round = [
            version(&a, FileOp::Put, Some(b"a1"), None),
            version(&b, FileOp::Create, Some(b"b1"), None),
            version(&a, FileOp::Put, Some(b"a2"), Some(b"a1")),
            version(&b, FileOp::Remove, None, Some(b"b1")),
            version(&a, FileOp::Put, Some(b"a3"), Some(b"a2")),
        ];
        f.files.materialize(&round).unwrap();
        assert_eq!(on_disk(&f, "state/a").unwrap(), b"a3");
        assert_eq!(on_disk(&f, "state/b"), None);
        // Only the final version of `a` was ever written.
        assert_eq!(f.faults.paths(DiskOp::Rename).len(), 1);

        // A crash left an intermediate or the final version: the round runs again.
        for left in [&b"a2"[..], b"a3"] {
            fs::write(f.root.join("state/a"), left).unwrap();
            f.files.materialize(&round).unwrap();
            assert_eq!(on_disk(&f, "state/a").unwrap(), b"a3");
        }
    }

    #[test]
    fn remove_unlinks_only_a_version_this_office_wrote() {
        let f = fixture();
        let p = path("state/r");
        f.files
            .materialize(&[version(&p, FileOp::Put, Some(b"v1"), None)])
            .unwrap();
        let remove = [version(&p, FileOp::Remove, None, Some(b"v1"))];
        fs::write(f.root.join("state/r"), b"foreign").unwrap();
        assert_eq!(
            f.files.materialize(&remove),
            Err(FilesError::Foreign { path: p.clone() })
        );
        fs::write(f.root.join("state/r"), b"v1").unwrap();
        f.files.materialize(&remove).unwrap();
        assert_eq!(on_disk(&f, "state/r"), None);
        f.files.materialize(&remove).unwrap();
    }

    #[test]
    fn a_link_in_the_path_is_refused_and_nothing_is_written_through_it() {
        let f = fixture();
        let outside = f.root.parent().unwrap().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(f.root.join("state")).unwrap();
        std::os::unix::fs::symlink(&outside, f.root.join("state/escape")).unwrap();
        let p = path("state/escape/x");
        let error = f
            .files
            .materialize(&[version(&p, FileOp::Put, Some(b"x"), None)])
            .unwrap_err();
        assert!(matches!(error, FilesError::Io { flush: false, .. }));
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        assert!(f.files.read(&p).is_err());
        assert!(f.files.walk(&path("state")).is_err());

        // A link where the file itself would be is not followed either.
        fs::remove_file(f.root.join("state/escape")).unwrap();
        fs::write(outside.join("target"), b"keep").unwrap();
        std::os::unix::fs::symlink(outside.join("target"), f.root.join("state/link")).unwrap();
        let p = path("state/link");
        assert!(f.files.read(&p).is_err());
        assert!(f
            .files
            .materialize(&[version(&p, FileOp::Put, Some(b"x"), None)])
            .is_err());
        assert_eq!(fs::read(outside.join("target")).unwrap(), b"keep");
    }

    #[test]
    fn a_file_is_wholly_old_or_wholly_new_at_every_injected_failure() {
        for crash in [false, true] {
            let mut nth = 1;
            loop {
                let f = fixture();
                let p = path("state/deep/t.yaml");
                f.files
                    .materialize(&[version(&p, FileOp::Put, Some(b"old bytes"), None)])
                    .unwrap();
                if crash {
                    f.faults.crash_before_call(nth);
                } else {
                    f.faults.fail_call(nth);
                }
                let result = f.files.materialize(&[version(
                    &p,
                    FileOp::Put,
                    Some(b"new bytes, longer"),
                    Some(b"old bytes"),
                )]);
                let now = on_disk(&f, "state/deep/t.yaml").unwrap();
                assert!(
                    now == b"old bytes" || now == b"new bytes, longer",
                    "call {nth} (crash {crash}) left {now:?}"
                );
                if !f.faults.fired() {
                    result.unwrap();
                    assert_eq!(now, b"new bytes, longer");
                    assert!(nth > 5, "the loop covered {nth} calls");
                    break;
                }
                assert!(result.is_err(), "call {nth} failed silently");
                nth += 1;
            }
        }
    }

    #[test]
    fn a_failed_file_flush_is_returned_and_never_retried() {
        let f = fixture();
        let p = path("state/t.yaml");
        f.faults.fail(DiskOp::Flush, Target::File, 1);
        let error = f
            .files
            .materialize(&[version(&p, FileOp::Put, Some(b"x"), None)])
            .unwrap_err();
        assert!(matches!(error, FilesError::Io { flush: true, .. }));
        assert!(error.path().contains(TEMP_PREFIX));
        assert_eq!(f.faults.paths(DiskOp::Flush).len(), 1);
        assert!(f.faults.paths(DiskOp::Rename).is_empty());
        assert_eq!(on_disk(&f, "state/t.yaml"), None);
    }

    #[test]
    fn a_walk_is_sorted_by_path_bytes_and_skips_unfinished_publications() {
        let f = fixture();
        let names = ["state/b", "state/a/z", "state/a-", "state/a/y/1", "local/x"];
        let paths: Vec<_> = names.iter().map(|n| path(n)).collect();
        let versions: Vec<_> = paths
            .iter()
            .map(|p| version(p, FileOp::Put, Some(b"x"), None))
            .collect();
        f.files.materialize(&versions).unwrap();
        fs::write(f.root.join(format!("state/a/{TEMP_PREFIX}9-9")), b"partial").unwrap();
        let walked = f.files.walk(&path("state")).unwrap();
        let texts: Vec<_> = walked.iter().map(DurablePath::as_str).collect();
        assert_eq!(texts, ["state/a-", "state/a/y/1", "state/a/z", "state/b"]);
        assert_eq!(
            f.files.walk(&path("state/a/z")).unwrap(),
            [path("state/a/z")]
        );
        assert!(f.files.walk(&path("budgets")).unwrap().is_empty());
        assert!(f
            .files
            .walk(&path("state/a/none/deeper"))
            .unwrap()
            .is_empty());
    }
}
