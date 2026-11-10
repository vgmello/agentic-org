//! Socket cleanup while the owned leader is still unreaped.
use std::ffi::CString;
use std::fmt;
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::process::ExitStatus;

use crate::{files, invalid, OwnedChild};

/// A socket template confined to an allowed real-path root. `{pid}` is filled from the
/// owned, unreaped leader. Every parent component is opened without following symlinks.
#[derive(Debug, Clone)]
pub struct SocketCleanup {
    root: PathBuf,
    relative: String,
}

impl SocketCleanup {
    /// Validate an absolute non-root allowed directory and a relative template containing
    /// `{pid}`. Empty, dot, parent and unknown placeholder components are refused.
    pub fn new(root: PathBuf, relative_template: String) -> io::Result<Self> {
        if !root.is_absolute()
            || root == Path::new("/")
            || root
                .components()
                .any(|p| !matches!(p, Component::RootDir | Component::Normal(_)))
        {
            return Err(invalid(
                "cleanup root must be an absolute normalized non-root path",
            ));
        }
        let sample = relative_template.replace("{pid}", "1");
        if !relative_template.contains("{pid}")
            || sample.contains(['{', '}', '\0'])
            || sample
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(invalid("invalid relative socket cleanup template"));
        }
        Ok(Self {
            root,
            relative: relative_template,
        })
    }

    fn path(&self, pid: u32) -> PathBuf {
        self.root
            .join(self.relative.replace("{pid}", &pid.to_string()))
    }

    fn remove(&self, pid: u32) -> io::Result<bool> {
        let path = self.path(pid);
        let parent = files::root(path.parent().unwrap())?;
        files::private_directory(&parent)?;
        let leaf = CString::new(path.file_name().unwrap().as_encoded_bytes())
            .map_err(|_| invalid("NUL in cleanup path"))?;
        let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: held private parent dirfd; no leaf symlink is followed.
        if unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                leaf.as_ptr(),
                info.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(error)
            };
        }
        // SAFETY: successful fstatat initialized the record; geteuid only queries credentials.
        let info = unsafe { info.assume_init() };
        socket_owned(&info, unsafe { libc::geteuid() })?;
        // The pinned current-UID/0700 parent narrows the window, not atomic. Deliberate
        // same-UID leaf replacement is outside v1's threat model (D-B22/D-B33).
        // SAFETY: literal leaf relative to held private parent, verified own-UID socket.
        if unsafe { libc::unlinkat(parent.as_raw_fd(), leaf.as_ptr(), 0) } != 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(error)
            };
        }
        Ok(true)
    }
}

fn socket_owned(info: &libc::stat, uid: libc::uid_t) -> io::Result<()> {
    if info.st_mode & libc::S_IFMT != libc::S_IFSOCK || info.st_uid != uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cleanup leaf must be a socket owned by the current user",
        ));
    }
    Ok(())
}

/// Successful filesystem cleanup and owned-child reaping evidence.
#[derive(Debug)]
pub struct CleanupReport {
    /// Terminal status obtained by waitpid.
    pub status: ExitStatus,
    /// Paths removed, including a probe's private cwd when applicable.
    pub removed: Vec<PathBuf>,
    /// Socket leaves or probe directories already absent.
    pub absent: Vec<PathBuf>,
}

/// Unresolved cleanup. The caller must retain its run lock and admit no replacement.
#[derive(Debug)]
pub struct CleanupFailure {
    /// Underlying process or filesystem error.
    pub error: io::Error,
    /// Paths already removed before this error.
    pub removed: Vec<PathBuf>,
    /// Paths still requiring cleanup or verification.
    pub unresolved: Vec<PathBuf>,
    /// Whether this handle still owns an unreaped child. False signals an embedding
    /// ownership violation (ECHILD); this handle will never signal its numeric IDs again.
    pub owner_retained: bool,
}

impl fmt::Display for CleanupFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "process cleanup unresolved: {}", self.error)
    }
}

impl std::error::Error for CleanupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl OwnedChild {
    /// After natural or forced exit: kill residual owned-group members, remove validated
    /// sockets and the probe cwd, then reap only this leader. No pipe is read. Socket parents
    /// must already be own-UID/0700; they are never chmodded. Missing parents, symlinks,
    /// non-sockets and foreign ownership fail closed with the leader retained for retry.
    /// Detached-process discovery remains a separate prerequisite for real harness runs.
    pub fn cleanup_and_reap(
        &mut self,
        paths: &[SocketCleanup],
    ) -> Result<CleanupReport, CleanupFailure> {
        let mut removed = Vec::new();
        let mut absent = Vec::new();
        let mut unresolved: Vec<_> = paths
            .iter()
            .map(|path| path.path(self.pid as u32))
            .collect();
        if let Some(directory) = &self.cleanup_dir {
            unresolved.push(directory.path().to_owned());
        }
        let result = (|| {
            if self.try_exit()?.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "cleanup requires an exited unreaped child",
                ));
            }
            self.kill_owned_group()?;
            for path in paths {
                let name = path.path(self.pid as u32);
                if path.remove(self.pid as u32)? {
                    removed.push(name.clone());
                } else {
                    absent.push(name.clone());
                }
                unresolved.retain(|p| p != &name);
            }
            if let Some(directory) = self.cleanup_dir.as_mut() {
                let name = directory.path().to_owned();
                if directory.remove()? {
                    removed.push(name.clone());
                } else {
                    absent.push(name.clone());
                }
                unresolved.retain(|p| p != &name);
            }
            self.reap_after_cleanup()
        })();
        match result {
            Ok(status) => Ok(CleanupReport {
                status,
                removed,
                absent,
            }),
            Err(error) => Err(CleanupFailure {
                error,
                removed,
                unresolved,
                owner_retained: self.is_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    #[test]
    fn socket_cleanup_refuses_nonprivate_and_symlinked_parents_and_non_socket_leaves() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().canonicalize().unwrap();
        let path = real.join("19.sock");
        let socket = UnixListener::bind(&path).unwrap();
        let cleanup = SocketCleanup::new(real.clone(), "{pid}.sock".into()).unwrap();
        for mode in [0o770, 0o707, 0o750, 0o1700] {
            std::fs::set_permissions(&real, std::fs::Permissions::from_mode(mode)).unwrap();
            assert!(cleanup.remove(19).is_err());
            assert!(path.exists());
        }
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
        let link = real.join("linked");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(SocketCleanup::new(link, "{pid}.sock".into())
            .unwrap()
            .remove(19)
            .is_err());
        assert!(cleanup.remove(19).unwrap());
        assert!(!cleanup.remove(19).unwrap());
        drop(socket);
        std::fs::write(&path, b"keep").unwrap();
        assert!(cleanup.remove(19).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink("elsewhere", &path).unwrap();
        assert!(cleanup.remove(19).is_err());
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn foreign_uid_socket_policy_is_refused_without_privileged_fixtures() {
        // No chown/root fixture: policy uses the same stat record as real fstatat.
        let mut info: libc::stat = unsafe { std::mem::zeroed() };
        info.st_mode = libc::S_IFSOCK | 0o600;
        info.st_uid = 42;
        assert!(socket_owned(&info, 43).is_err());
        assert!(socket_owned(&info, 42).is_ok());
        for invalid in [
            "../{pid}", "/{pid}", "x//{pid}", "{other}", "fixed", "{pid}/..",
        ] {
            assert!(SocketCleanup::new("/private/tmp".into(), invalid.into()).is_err());
        }
    }
}
