//! The office home as the host sees it: its layout, making it owner-only, the socket-length
//! check, and `host.lock`, the catalog lock that makes the host the only one on a home.

use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use org_office_launch::OFFICES_DIR_NAME;
use org_proto::location::{self, OFFICE_SOCKET_HANDLE_CHARS};

use crate::error::HostError;

/// The host's lock file in the office home.
pub(crate) const HOST_LOCK_FILE_NAME: &str = "host.lock";

/// The paths the host uses inside one office home.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    pub(crate) home: PathBuf,
    pub(crate) run: PathBuf,
    pub(crate) logs: PathBuf,
    pub(crate) offices: PathBuf,
    pub(crate) host_socket: PathBuf,
    pub(crate) host_lock: PathBuf,
}

impl Layout {
    pub(crate) fn new(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            run: home.join("run"),
            logs: home.join("logs"),
            offices: home.join(OFFICES_DIR_NAME),
            host_socket: location::host_socket_path(home),
            host_lock: home.join(HOST_LOCK_FILE_NAME),
        }
    }

    /// Every office socket in a home has the same length (12-character handles), so checking
    /// `host.sock` and one sample office socket up front means no create can fail on it later.
    pub(crate) fn check_socket_paths(&self) -> Result<(), HostError> {
        let sample_id = "X".repeat(OFFICE_SOCKET_HANDLE_CHARS);
        let sample =
            location::office_socket_path(&self.home, &sample_id).expect("the sample id is plain");
        for path in [&self.host_socket, &sample] {
            location::check_socket_path(path).map_err(|e| HostError::SocketPathTooLong {
                path: path.clone(),
                len: e.len,
            })?;
        }
        Ok(())
    }

    /// Creates the home and its `run/`, `logs/` and `offices/` folders owner-only (0700), and
    /// tightens them if they already exist. Refuses a home or folder that isn't a real
    /// directory owned by this user (e.g. a symlink planted by someone else).
    pub(crate) fn prepare(&self) -> Result<(), HostError> {
        let io = |context| move |source| HostError::Io { context, source };
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.home)
            .map_err(io("creating the office home"))?;
        owner_only_dir(&self.home)?;
        for dir in [&self.run, &self.logs, &self.offices] {
            match DirBuilder::new().mode(0o700).create(dir) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(io("creating a folder in the office home")(e)),
            }
            owner_only_dir(dir)?;
        }
        Ok(())
    }
}

/// Checks `dir` is a directory (not a symlink) owned by us, and sets it to 0700.
fn owner_only_dir(dir: &Path) -> Result<(), HostError> {
    let meta = fs::symlink_metadata(dir).map_err(|source| HostError::Io {
        context: "reading the office home",
        source,
    })?;
    if !meta.file_type().is_dir() || meta.uid() != euid() {
        return Err(HostError::UnsafeHome { path: dir.into() });
    }
    if meta.mode() & 0o777 != 0o700 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|source| {
            HostError::Io {
                context: "making the office home owner-only",
                source,
            }
        })?;
    }
    Ok(())
}

/// The effective user id of this process.
pub(crate) fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions and can't fail.
    unsafe { libc::geteuid() }
}

/// `host.lock`, held (an exclusive `flock`) for as long as this value lives.
#[derive(Debug)]
pub(crate) struct HostLock {
    _file: File,
}

impl HostLock {
    /// Takes the lock without waiting: another host on this home means
    /// [`HostError::AlreadyRunning`].
    pub(crate) fn acquire(layout: &Layout) -> Result<Self, HostError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&layout.host_lock)
            .map_err(|source| HostError::Io {
                context: "opening host.lock",
                source,
            })?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(HostError::AlreadyRunning {
                home: layout.home.clone(),
            }),
            Err(TryLockError::Error(source)) => Err(HostError::Io {
                context: "locking host.lock",
                source,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_makes_and_tightens_owner_only_folders() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o755)).unwrap();
        let layout = Layout::new(&home);
        layout.prepare().unwrap();
        for d in [&layout.home, &layout.run, &layout.logs, &layout.offices] {
            let mode = fs::metadata(d).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "{}", d.display());
        }
    }

    #[test]
    fn prepare_refuses_a_symlinked_folder() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir(&home).unwrap();
        std::os::unix::fs::symlink(dir.path(), home.join("run")).unwrap();
        assert!(matches!(
            Layout::new(&home).prepare(),
            Err(HostError::UnsafeHome { .. })
        ));
    }

    #[test]
    fn the_lock_is_exclusive_until_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path());
        let held = HostLock::acquire(&layout).unwrap();
        assert!(matches!(
            HostLock::acquire(&layout),
            Err(HostError::AlreadyRunning { .. })
        ));
        drop(held);
        HostLock::acquire(&layout).unwrap();
    }

    #[test]
    fn the_lock_fd_is_close_on_exec() {
        use std::os::fd::AsRawFd;
        let dir = tempfile::tempdir().unwrap();
        let held = HostLock::acquire(&Layout::new(dir.path())).unwrap();
        // SAFETY: F_GETFD only reads the flags of an fd this test owns.
        let flags = unsafe { libc::fcntl(held._file.as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0, "{flags}");
    }

    #[test]
    fn socket_paths_are_checked_against_the_macos_limit() {
        // 103 bytes is the limit; run/<12>.sock adds 22 bytes to the home.
        let ok = PathBuf::from(format!("/{}", "a".repeat(80)));
        assert!(Layout::new(&ok).check_socket_paths().is_ok());
        let long = PathBuf::from(format!("/{}", "a".repeat(81)));
        match Layout::new(&long).check_socket_paths() {
            Err(HostError::SocketPathTooLong { len, .. }) => assert_eq!(len, 104),
            other => panic!("{other:?}"),
        }
    }
}
