//! An office server's stderr log: `<home>/logs/<handle>.log`, owner-only, rotated at a size cap
//! to `<handle>.log.1` (one old file kept), so a crash loop can't fill the disk.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The default size cap of one log file.
pub const DEFAULT_LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// One office's log. Opened lazily, so a log that can't be opened only loses lines.
pub(crate) struct RotatingLog {
    path: PathBuf,
    old: PathBuf,
    max: u64,
    file: Option<File>,
    size: u64,
}

impl RotatingLog {
    /// The log of the office whose socket handle is `handle`, in `logs_dir`.
    pub(crate) fn new(logs_dir: &Path, handle: &str, max: u64) -> Self {
        Self {
            path: logs_dir.join(format!("{handle}.log")),
            old: logs_dir.join(format!("{handle}.log.1")),
            max: max.max(1),
            file: None,
            size: 0,
        }
    }

    /// Append `buf`, rotating whenever the file reaches the cap, so no file exceeds it.
    pub(crate) fn write(&mut self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            if self.file.is_none() {
                self.open()?;
            }
            if self.size >= self.max {
                self.rotate()?;
            }
            let room = usize::try_from(self.max - self.size).unwrap_or(usize::MAX);
            let n = buf.len().min(room);
            let file = self.file.as_mut().expect("opened above");
            if let Err(e) = file.write_all(&buf[..n]) {
                self.file = None; // reopen (and re-measure) next time
                return Err(e);
            }
            self.size += n as u64;
            buf = &buf[n..];
        }
        Ok(())
    }

    fn open(&mut self) -> io::Result<()> {
        let file = open_owner_only(&self.path)?;
        self.size = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file = None;
        fs::rename(&self.path, &self.old)?;
        self.open()
    }
}

/// Opens for appending, creating it 0600; never follows a symlink, and tightens a file that was
/// left with looser permissions.
fn open_owner_only(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_at_the_cap_and_keeps_one_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = RotatingLog::new(dir.path(), "H", 10);
        log.write(b"0123456").unwrap();
        log.write(b"789abcdefghijklmnopqrstu").unwrap();
        let current = fs::read(dir.path().join("H.log")).unwrap();
        let old = fs::read(dir.path().join("H.log.1")).unwrap();
        // 31 bytes at a cap of 10: three full files, of which only the last is kept.
        assert_eq!(old, b"klmnopqrst");
        assert_eq!(current, b"u");
    }

    #[test]
    fn files_are_owner_only_and_reopen_at_their_size() {
        let dir = tempfile::tempdir().unwrap();
        RotatingLog::new(dir.path(), "H", 8).write(b"abc").unwrap();
        let mut again = RotatingLog::new(dir.path(), "H", 8);
        again.write(b"defghi").unwrap();
        assert_eq!(fs::read(dir.path().join("H.log")).unwrap(), b"i");
        assert_eq!(fs::read(dir.path().join("H.log.1")).unwrap(), b"abcdefgh");
        let mode = fs::metadata(dir.path().join("H.log"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
