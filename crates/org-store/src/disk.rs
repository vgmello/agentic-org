//! The one place the store changes the file system. Publication and the journal call this
//! seam; the testkit wraps it to inject errors, short writes and crashes, so the logic
//! carries no test hook.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
    sync::Arc,
};

pub(crate) trait Disk: Send + Sync {
    /// Opens a file for appending, creating it `0600` if it is missing.
    fn open_append(&self, path: &Path) -> io::Result<Arc<dyn DiskFile>>;
    /// Creates a file `0600`; fails if anything is there.
    fn create_new(&self, path: &Path) -> io::Result<Arc<dyn DiskFile>>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// A hard link; fails if `to` exists.
    fn link(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn unlink(&self, path: &Path) -> io::Result<()>;
    /// Creates one directory `0700`.
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    /// Issues a directory's entries to the device. Not a full flush by itself.
    fn sync_dir(&self, path: &Path) -> io::Result<()>;
}

pub(crate) trait DiskFile: Send + Sync {
    /// One write; may be short.
    fn write(&self, bytes: &[u8]) -> io::Result<usize>;
    /// Flushes the file to the drive itself. An error is final: the caller never retries,
    /// because the kernel may have dropped the dirty pages.
    fn flush_full(&self) -> io::Result<()>;
    fn truncate(&self, len: u64) -> io::Result<()>;
}

pub(crate) fn write_all(file: &dyn DiskFile, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        match file.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

pub(crate) struct RealDisk;

struct RealFile(File);

impl Disk for RealDisk {
    fn open_append(&self, path: &Path) -> io::Result<Arc<dyn DiskFile>> {
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .custom_flags(org_platform::fs::O_NOFOLLOW)
            .open(path)?;
        Ok(Arc::new(RealFile(file)))
    }

    fn create_new(&self, path: &Path) -> io::Result<Arc<dyn DiskFile>> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(org_platform::fs::O_NOFOLLOW)
            .open(path)?;
        Ok(Arc::new(RealFile(file)))
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn link(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::hard_link(from, to)
    }

    fn unlink(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        fs::DirBuilder::new().mode(0o700).create(path)
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        File::open(path)?.sync_all()
    }
}

impl DiskFile for RealFile {
    fn write(&self, bytes: &[u8]) -> io::Result<usize> {
        (&self.0).write(bytes)
    }

    fn flush_full(&self) -> io::Result<()> {
        full_flush(&self.0)
    }

    fn truncate(&self, len: u64) -> io::Result<()> {
        self.0.set_len(len)
    }
}

fn full_flush(file: &File) -> io::Result<()> {
    // The device flush, with macOS's drive-cache flush, is the platform's (ADR-0017).
    org_platform::fs::full_sync(file)
}
