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

    pub(crate) fn with_runtime(home: &Path, runtime: &Path) -> Self {
        Self {
            run: runtime.to_owned(),
            host_socket: location::host_socket_path(runtime),
            ..Self::new(home)
        }
    }

    /// Every office socket in a home has the same length (12-character handles), so checking
    /// `host.sock` and one sample office socket up front means no create can fail on it later.
    pub(crate) fn check_socket_paths(&self) -> Result<(), HostError> {
        let sample_id = "X".repeat(OFFICE_SOCKET_HANDLE_CHARS);
        let sample =
            location::office_socket_in(&self.run, &sample_id).expect("the sample id is plain");
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

/// Where the office home is inside an account's home folder (the shared rule's default).
const OFFICE_HOME_IN_ACCOUNT: &str = "Library/Application Support/agentic.org";

/// The office home of this process's user account: `<pw_dir>/Library/Application
/// Support/agentic.org`, from the account database (`getpwuid_r`), never from the environment.
/// `None` if the account has no absolute home folder.
pub(crate) fn account_office_home() -> Option<PathBuf> {
    account_dir().map(|dir| dir.join(OFFICE_HOME_IN_ACCOUNT))
}

/// This process's account home folder (`pw_dir`), if it is absolute.
pub(crate) fn account_dir() -> Option<PathBuf> {
    account_home(euid())
}

/// The office home a release host may serve: the one the shared rule
/// (`org_proto::location::default_office_home`, which the app and `org-agent` also use to find
/// `host.sock`) gives in this process's environment, `shared`, but only when it is the account's
/// own office home, `<account_dir>/Library/Application Support/agentic.org`. So:
/// `ORG_OFFICE_HOME` is unset (or not absolute, which the rule ignores) or names that home, and
/// `HOME` (`home_env`) is the account's home folder. Anything else is refused with why, so a
/// release host's home can't be moved by its environment, and the app and the host can't
/// silently look in different places.
///
/// Pure: `same` compares two paths (the host compares them canonically where they exist,
/// lexically otherwise: [`same_path`]).
pub(crate) fn pinned_office_home(
    shared: Option<&Path>,
    home_env: Option<&Path>,
    account_dir: Option<&Path>,
    same: impl Fn(&Path, &Path) -> bool,
) -> Result<PathBuf, String> {
    let Some(account_dir) = account_dir.filter(|dir| dir.is_absolute()) else {
        return Err("this account has no home folder, so it has no office home".into());
    };
    let account_home = account_dir.join(OFFICE_HOME_IN_ACCOUNT);
    match home_env {
        // Absolute only, as the shared rule reads it.
        Some(home) if home.is_absolute() && same(home, account_dir) => {}
        Some(home) => {
            return Err(format!(
                "HOME is {}, not this account's home folder {}: a release host serves only the \
                 account's office home",
                home.display(),
                account_dir.display()
            ))
        }
        None => {
            return Err(format!(
                "HOME isn't set: a release host serves only the account's office home {}",
                account_home.display()
            ))
        }
    }
    let home = home_env.unwrap_or(account_dir);
    match shared {
        // Through HOME too (the same folder by now), since the home may not exist yet.
        Some(shared)
            if same(shared, &account_home) || same(shared, &home.join(OFFICE_HOME_IN_ACCOUNT)) =>
        {
            Ok(account_home)
        }
        Some(shared) => Err(format!(
            "the office home is {} (from ORG_OFFICE_HOME), but a release host serves only this \
             account's office home {}: unset ORG_OFFICE_HOME",
            shared.display(),
            account_home.display()
        )),
        None => Err(format!(
            "no office home: a release host serves only this account's office home {}",
            account_home.display()
        )),
    }
}

/// Whether `a` and `b` name the same place: compared canonically when both exist, lexically
/// otherwise.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The home folder (`pw_dir`) of the account `uid`, if it is absolute.
fn account_home(uid: u32) -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;

    let mut buf = vec![0 as libc::c_char; 4096];
    loop {
        // SAFETY: an all-zero `passwd` is a valid value for the call to fill in.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: `entry`, `buf` (of the length given) and `found` are valid for the call,
        // which writes the strings `entry` points to into `buf`.
        let rc =
            unsafe { libc::getpwuid_r(uid, &mut entry, buf.as_mut_ptr(), buf.len(), &mut found) };
        if rc == libc::ERANGE && buf.len() < 1 << 20 {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        if rc != 0 || found.is_null() || entry.pw_dir.is_null() {
            return None;
        }
        // SAFETY: `pw_dir` is a NUL-terminated string inside `buf`, which is still alive.
        let dir = unsafe { CStr::from_ptr(entry.pw_dir) };
        let dir = PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes()));
        return dir.is_absolute().then_some(dir);
    }
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

impl Drop for HostLock {
    fn drop(&mut self) {
        // A forked child may retain the open file description after this File closes.
        // Drop cannot report unlock errors; the File still closes afterwards.
        let _ = self._file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_home_must_be_the_accounts_own_office_home() {
        let lexical = |a: &Path, b: &Path| a == b;
        let account = Path::new("/Users/me");
        let pinned = Path::new("/Users/me/Library/Application Support/agentic.org");
        let pin = |shared: Option<&str>, home: Option<&str>, account: Option<&Path>| {
            pinned_office_home(shared.map(Path::new), home.map(Path::new), account, lexical)
        };
        // ORG_OFFICE_HOME unset (the rule falls back to HOME) or naming the account's home.
        let ok = pin(
            Some(pinned.to_str().unwrap()),
            Some("/Users/me"),
            Some(account),
        );
        assert_eq!(ok.unwrap(), pinned);
        // ORG_OFFICE_HOME somewhere else.
        let err = pin(Some("/tmp/elsewhere"), Some("/Users/me"), Some(account)).unwrap_err();
        assert!(
            err.contains("ORG_OFFICE_HOME") && err.contains("/tmp/elsewhere"),
            "{err}"
        );
        // HOME isn't the account's (the rule would give another home), or isn't set.
        let err = pin(
            Some("/Users/other/Library/Application Support/agentic.org"),
            Some("/Users/other"),
            Some(account),
        )
        .unwrap_err();
        assert!(err.contains("HOME is /Users/other"), "{err}");
        assert!(pin(Some(pinned.to_str().unwrap()), None, Some(account)).is_err());
        // HOME is the account's, but ORG_OFFICE_HOME names another home.
        assert!(pin(Some("/Users/me/other"), Some("/Users/me"), Some(account)).is_err());
        // No office home at all, or no account home folder.
        assert!(pin(None, Some("/Users/me"), Some(account)).is_err());
        assert!(pin(Some(pinned.to_str().unwrap()), Some("/Users/me"), None).is_err());
        // A relative HOME, even one naming the account's folder from `/`.
        assert!(pin(
            Some(pinned.to_str().unwrap()),
            Some("Users/me"),
            Some(account)
        )
        .is_err());
        let relative = Path::new("Users/me");
        assert!(pin(
            Some("Users/me/Library/Application Support/agentic.org"),
            Some("Users/me"),
            Some(relative)
        )
        .is_err());
    }

    #[test]
    fn paths_are_the_same_canonically_where_they_exist_and_lexically_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(same_path(&real, &link));
        assert!(same_path(&real.join("."), &real));
        assert!(!same_path(&real, dir.path()));
        // Missing paths: lexically.
        let missing = dir.path().join("missing");
        assert!(same_path(&missing, &missing));
        assert!(!same_path(&missing, &link.join("missing")));
        // A release pin through a symlinked home folder, before the office home exists (the
        // shared rule gives it through HOME, the symlink).
        let pinned = pinned_office_home(
            Some(&link.join(OFFICE_HOME_IN_ACCOUNT)),
            Some(&link),
            Some(&real),
            same_path,
        );
        assert_eq!(pinned.unwrap(), real.join(OFFICE_HOME_IN_ACCOUNT));
        // And once it exists.
        fs::create_dir_all(real.join(OFFICE_HOME_IN_ACCOUNT)).unwrap();
        let pinned = pinned_office_home(
            Some(&link.join(OFFICE_HOME_IN_ACCOUNT)),
            Some(&link),
            Some(&real),
            same_path,
        );
        assert_eq!(pinned.unwrap(), real.join(OFFICE_HOME_IN_ACCOUNT));
    }

    #[test]
    fn the_account_office_home_comes_from_the_account_database() {
        let Some(home) = account_office_home() else {
            // A uid with no account entry (some CI containers): nothing to compare.
            assert!(account_home(euid()).is_none());
            eprintln!("skipped: this uid has no account home folder");
            return;
        };
        assert!(home.is_absolute(), "{}", home.display());
        assert!(
            home.ends_with("Library/Application Support/agentic.org"),
            "{}",
            home.display()
        );
        // Built from the account's `pw_dir`.
        assert_eq!(
            home.parent().and_then(Path::parent).and_then(Path::parent),
            account_home(euid()).as_deref()
        );
        assert_eq!(account_home(u32::MAX - 7), None);
    }

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
    fn drop_unlocks_while_a_forked_child_holds_the_descriptor() {
        use std::os::fd::AsRawFd;
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path());
        let held = HostLock::acquire(&layout).unwrap();
        // An explicit byte releases the child even if other forks inherit socket copies.
        let (mut release, wait) = std::os::unix::net::UnixStream::pair().unwrap();
        let (release_fd, wait_fd) = (release.as_raw_fd(), wait.as_raw_fd());
        // SAFETY: the child uses only async-signal-safe close/read/_exit, retaining the lock fd.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            // SAFETY: close the parent's socket and wait until its post-drop reacquire attempt.
            unsafe {
                libc::close(release_fd);
                let mut byte = 0u8;
                let read = libc::read(wait_fd, (&mut byte as *mut u8).cast(), 1);
                libc::_exit(if read == 1 && byte == 1 { 0 } else { 42 });
            }
        }
        assert!(pid > 0, "fork failed");
        drop(wait);
        drop(held);
        // Assert while the child still holds the fd; catch only to guarantee cleanup on failure.
        let checked = std::panic::catch_unwind(|| {
            let reopened = HostLock::acquire(&layout);
            let mut status = 0;
            // SAFETY: query only our child without waiting or releasing its descriptor.
            assert_eq!(unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) }, 0);
            assert!(
                reopened.is_ok(),
                "HostLock drop must release the lock even with an inherited fd: {:?}",
                reopened.err()
            );
        });
        let released = std::io::Write::write_all(&mut release, &[1]);
        drop(release);
        // Reap before assertions so the regression failure never leaves a child behind.
        let mut status = 0;
        // SAFETY: wait only for the child this test created.
        let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
        released.unwrap();
        assert_eq!(waited, pid);
        assert_eq!(status, 0);
        if let Err(panic) = checked {
            std::panic::resume_unwind(panic);
        }
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
