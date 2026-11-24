//! Owner-only filesystem and durability helpers shared by the office and its storage.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static ATOMIC_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

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

/// Creates every missing directory component from `established_root` down to `target`
/// with mode `0700` and durably `full_sync`s the ENTIRE ancestor chain from `established_root`
/// through `target` so retries after an interrupted directory creation never leave an
/// already-visible directory entry unsynced in its parent (`TASK-97` / Pass 2 Finding #4).
pub(crate) fn private_dir_synced(established_root: &Path, target: &Path) -> std::io::Result<()> {
    let Ok(rel) = target.strip_prefix(established_root) else {
        private_dir(target)?;
        tighten_dir(target)?;
        return sync_dir(target);
    };
    let mut current = established_root.to_path_buf();
    for component in rel.components() {
        let parent = current.clone();
        current.push(component);
        if !current.exists() {
            match DirBuilder::new().mode(0o700).create(&current) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(err) => return Err(err),
            }
        }
        sync_dir(&parent)?;
    }
    tighten_dir(target)?;
    sync_dir(target)
}

/// Makes an existing `path` owner-only (0700 for directories).
pub(crate) fn tighten_dir(path: &Path) -> std::io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

/// Makes an existing file at `path` owner-only (0600). No-op if missing.
pub(crate) fn tighten_file(path: &Path) -> std::io::Result<()> {
    match fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Tightens a SQLite database file and its `-wal` / `-shm` sidecars to `0600`.
pub(crate) fn tighten_sqlite_sidecars(db_path: &Path) -> std::io::Result<()> {
    tighten_file(db_path)?;
    for suffix in ["-wal", "-shm"] {
        let mut s = db_path.as_os_str().to_os_string();
        s.push(suffix);
        tighten_file(Path::new(&s))?;
    }
    Ok(())
}

/// Removes a SQLite database file and any `-wal`, `-shm`, or `-journal` sidecars,
/// then durably syncs the parent directory.
pub(crate) fn remove_sqlite_files(db_path: &Path) -> std::io::Result<()> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut s = db_path.as_os_str().to_os_string();
        s.push(suffix);
        match fs::remove_file(Path::new(&s)) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    if let Some(parent) = db_path.parent() {
        if parent.is_dir() {
            sync_dir(parent)?;
        }
    }
    Ok(())
}

/// Flushes a file's data and metadata to durable storage (`F_FULLFSYNC` on macOS).
pub(crate) fn full_sync(file: &File) -> std::io::Result<()> {
    org_platform::fs::full_sync(file)
}

/// Flushes a directory's entries to durable storage (`F_FULLFSYNC` on macOS).
pub(crate) fn sync_dir(dir: &Path) -> std::io::Result<()> {
    let handle = File::open(dir)?;
    org_platform::fs::full_sync(&handle)
}

/// Atomically writes `bytes` to `path` with mode `0600`, `full_sync` on the temp file,
/// `fs::rename`, and `full_sync` on the parent directory (propagating all sync errors).
pub(crate) fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    let seq = ATOMIC_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".atomic.{}.{seq}.tmp", std::process::id()));
    let write_res = (|| -> std::io::Result<()> {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        full_sync(&f)?;
        Ok(())
    })();
    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    if let Err(err) = tighten_file(&tmp).and_then(|()| fs::rename(&tmp, path)) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    sync_dir(parent)?;
    tighten_file(path)
}

/// Makes `path` absolute without touching the filesystem. Needed because the database path
/// goes to SQLite, whose bundled build reads any path starting with `file:` as a URI.
pub(crate) fn absolute(path: PathBuf) -> std::io::Result<PathBuf> {
    std::path::absolute(path)
}
