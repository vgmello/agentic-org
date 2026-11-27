//! The office home as the host sees it: its layout, making it owner-only, the socket-length
//! check, and `host.lock`, the catalog lock that makes the host the only one on a home.

use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use org_office_launch::OFFICES_DIR_NAME;
use org_proto::location::{self, OFFICE_SOCKET_HANDLE_CHARS};

use crate::error::HostError;

/// The host's lock file in the office home (the launch contract names it for offline tools).
pub(crate) use org_office_launch::HOST_LOCK_FILE_NAME;

/// The paths the host uses inside one office home.
#[derive(Debug, Clone)]
pub struct Layout {
    pub home: PathBuf,
    pub run: PathBuf,
    pub logs: PathBuf,
    pub offices: PathBuf,
    pub host_socket: PathBuf,
    pub host_lock: PathBuf,
}

impl Layout {
    pub fn new(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            run: home.join("run"),
            logs: home.join("logs"),
            offices: home.join(OFFICES_DIR_NAME),
            host_socket: location::host_socket_path(home),
            host_lock: home.join(HOST_LOCK_FILE_NAME),
        }
    }

    pub fn with_runtime(home: &Path, runtime: &Path) -> Self {
        Self {
            run: runtime.to_owned(),
            host_socket: location::host_socket_path(runtime),
            ..Self::new(home)
        }
    }

    /// Every office socket and every launch rendezvous in a home has the same length, so
    /// checking `host.sock` and one sample of each up front means no create or start fails on
    /// it later.
    pub fn check_socket_paths(&self) -> Result<(), HostError> {
        let sample_id = "X".repeat(OFFICE_SOCKET_HANDLE_CHARS);
        let sample =
            location::office_socket_in(&self.run, &sample_id).expect("the sample id is plain");
        let limit = socket_path_limit().map_err(|source| HostError::Io {
            context: "finding this system's socket path limit",
            source,
        })?;
        // A spawn's rendezvous, `<run>/r/<16 hex>` (`supervisor::channels::Broker::bind`), is
        // three bytes shorter than an office socket (`<run>/o/<12>/s.sock`).
        let rendezvous = self.run.join("r").join("0".repeat(16));
        for path in [&self.host_socket, &sample, &rendezvous] {
            location::check_socket_path(path, limit).map_err(|e| HostError::SocketPathTooLong {
                path: path.clone(),
                len: e.len,
                limit: e.limit,
            })?;
        }
        Ok(())
    }

    /// Creates the home and its `run/` (with its office sockets folder, `run/o/`), `logs/` and
    /// `offices/` folders owner-only (0700), and tightens them if they already exist. Refuses a
    /// home or folder that isn't a real directory owned by this user (e.g. a symlink planted by
    /// someone else). Each office's own socket folder, `run/o/<handle>/`, is made by its server
    /// just before it binds (`org_proto::location`).
    pub fn prepare(&self) -> Result<(), HostError> {
        let io = |context| move |source| HostError::Io { context, source };
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.home)
            .map_err(io("creating the office home"))?;
        owner_only_dir(&self.home)?;
        let office_sockets = location::office_sockets_dir(&self.run);
        for dir in [&self.run, &office_sockets, &self.logs, &self.offices] {
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

/// Checks `dir` is a directory (not a symlink) owned by us, and sets it to exactly 0700,
/// clearing any setuid, setgid or sticky bit too.
pub(crate) fn owner_only_dir(dir: &Path) -> Result<(), HostError> {
    let meta = fs::symlink_metadata(dir).map_err(|source| HostError::Io {
        context: "reading the office home",
        source,
    })?;
    if !meta.file_type().is_dir() || meta.uid() != euid() {
        return Err(HostError::UnsafeHome { path: dir.into() });
    }
    if meta.mode() & 0o7777 != 0o700 {
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
pub fn euid() -> u32 {
    org_platform::posix::effective_uid()
}

/// The effective group id of this process.
#[cfg(any(test, all(feature = "test-support", debug_assertions)))]
pub fn egid() -> u32 {
    org_platform::posix::effective_gid()
}

/// The longest socket path this system takes, in bytes (`org_platform::dirs`).
pub(crate) fn socket_path_limit() -> io::Result<usize> {
    Ok(org_platform::dirs::socket_path_limit()?)
}

/// Where the office home of the account whose home folder is `dir` is, from the system's
/// defaults alone (`org_platform::dirs::account_application_base`): nothing in this process's
/// environment moves it. It is what a release host pins to. `None` on a system this release
/// has no locations for.
pub(crate) fn office_home_in(dir: &Path) -> Option<PathBuf> {
    org_platform::dirs::account_application_base(dir).ok()
}

/// Where the shared rule puts the office home for the home folder `dir` in this process's
/// environment (`org_platform::dirs::application_base`). It is [`office_home_in`] unless a
/// variable of the system moves it ([`moving_variables`]).
fn shared_home_in(dir: &Path) -> Option<PathBuf> {
    org_platform::dirs::application_base(dir).ok()
}

/// The environment variables that can move the shared rule's office home away from the
/// account's own: `ORG_OFFICE_HOME` and the system's (`XDG_DATA_HOME` on Linux), joined for
/// a message.
fn moving_variables() -> String {
    let mut names = vec![location::OFFICE_HOME_ENV];
    names.extend(org_platform::dirs::application_base_variables().unwrap_or(&[]));
    names.join(" and ")
}

/// The office home the shared rule gives in this process's environment, the one the app and
/// `agent-os` also use to find `host.sock`: `ORG_OFFICE_HOME` when it is an absolute path,
/// else the system's application base for the home folder `HOME` names. `None` if neither
/// gives one.
pub(crate) fn default_office_home() -> Option<PathBuf> {
    default_office_home_from(
        std::env::var_os(location::OFFICE_HOME_ENV),
        std::env::var_os("HOME"),
    )
}

/// The rule of [`default_office_home`] on the two variables' values.
fn default_office_home_from(
    override_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let base = home
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .and_then(|home| shared_home_in(&home));
    location::office_home_from(override_home, base)
}

/// This process's account home folder (`pw_dir`), if it is absolute.
pub fn account_dir() -> Option<PathBuf> {
    account_home(euid())
}

/// The office home a release host may serve: the one the shared rule
/// ([`default_office_home`], which the app and `agent-os` also use to find `host.sock`) gives
/// in this process's environment, `shared`, but only when it is the account's own office home,
/// `office_home_in(account_dir)`. So: `ORG_OFFICE_HOME` is unset (or not absolute, which the
/// rule ignores) or names that home, `HOME` (`home_env`) is the account's home folder, and no
/// variable of the system (`XDG_DATA_HOME` on Linux) points the shared rule elsewhere.
/// Anything else is refused with why, so a release host's home can't be moved by its
/// environment, and the app and the host can't silently look in different places.
///
/// Pure: `office_home_in` is the system's rule for an account's home folder with the
/// environment left out ([`office_home_in`]), and
/// `same` compares two paths (the host compares them canonically where they exist, lexically
/// otherwise: [`same_path`]).
pub(crate) fn pinned_office_home(
    shared: Option<&Path>,
    home_env: Option<&Path>,
    account_dir: Option<&Path>,
    office_home_in: impl Fn(&Path) -> Option<PathBuf>,
    same: impl Fn(&Path, &Path) -> bool,
) -> Result<PathBuf, String> {
    let Some(account_dir) = account_dir.filter(|dir| dir.is_absolute()) else {
        return Err("this account has no home folder, so it has no office home".into());
    };
    let Some(account_home) = office_home_in(account_dir) else {
        return Err("this release has no office home location on this system".into());
    };
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
            if same(shared, &account_home)
                || office_home_in(home).is_some_and(|through| same(shared, &through)) =>
        {
            Ok(account_home)
        }
        Some(shared) => Err(format!(
            "the office home is {} (from this environment), but a release host serves only \
             this account's office home {}: unset {}",
            shared.display(),
            account_home.display(),
            moving_variables()
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
    let home = org_platform::posix::account_by_uid(uid).ok()??.home?;
    home.is_absolute().then_some(home)
}

/// `host.lock`, held (an exclusive `flock`) for as long as this value lives.
#[derive(Debug)]
pub struct HostLock {
    _file: File,
}

impl HostLock {
    /// Takes the lock without waiting: another host on this home means
    /// [`HostError::AlreadyRunning`].
    pub fn acquire(layout: &Layout) -> Result<Self, HostError> {
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

    /// A system's rule for the office home in a home folder, as data for the pure rules.
    const OFFICE_HOME_IN_ACCOUNT: &str = "Library/Application Support/agentic.org";
    fn in_account(dir: &Path) -> Option<PathBuf> {
        Some(dir.join(OFFICE_HOME_IN_ACCOUNT))
    }

    #[test]
    fn a_release_home_must_be_the_accounts_own_office_home() {
        let lexical = |a: &Path, b: &Path| a == b;
        let account = Path::new("/Users/me");
        let pinned = Path::new("/Users/me/Library/Application Support/agentic.org");
        let pin = |shared: Option<&str>, home: Option<&str>, account: Option<&Path>| {
            pinned_office_home(
                shared.map(Path::new),
                home.map(Path::new),
                account,
                in_account,
                lexical,
            )
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
            in_account,
            same_path,
        );
        assert_eq!(pinned.unwrap(), real.join(OFFICE_HOME_IN_ACCOUNT));
        // And once it exists.
        fs::create_dir_all(real.join(OFFICE_HOME_IN_ACCOUNT)).unwrap();
        let pinned = pinned_office_home(
            Some(&link.join(OFFICE_HOME_IN_ACCOUNT)),
            Some(&link),
            Some(&real),
            in_account,
            same_path,
        );
        assert_eq!(pinned.unwrap(), real.join(OFFICE_HOME_IN_ACCOUNT));
    }

    #[test]
    fn the_account_office_home_comes_from_the_account_database() {
        let Some(home) = account_dir().and_then(|dir| office_home_in(&dir)) else {
            // A uid with no account entry (some CI containers): nothing to compare.
            assert!(account_home(euid()).is_none());
            eprintln!("skipped: this uid has no account home folder");
            return;
        };
        assert!(home.is_absolute(), "{}", home.display());
        // This system's base, built from the account's `pw_dir`.
        let account = account_home(euid()).expect("an account with an office home has a folder");
        assert_eq!(
            Some(home),
            org_platform::dirs::account_application_base(&account).ok()
        );
        assert_eq!(account_home(u32::MAX - 7), None);
    }

    #[test]
    fn a_home_the_environment_moved_is_not_the_accounts_and_is_refused() {
        // What the pin sees when a variable of the system (XDG_DATA_HOME on Linux) points
        // the shared rule somewhere else: HOME is right, ORG_OFFICE_HOME is unset, and the
        // shared home still is not the account's own.
        let account = Path::new("/home/me");
        let own = |dir: &Path| Some(dir.join(".local/share/agentic-org"));
        let refused = pinned_office_home(
            Some(Path::new("/srv/elsewhere/agentic-org")),
            Some(account),
            Some(account),
            own,
            |a, b| a == b,
        )
        .unwrap_err();
        assert!(
            refused.contains("/srv/elsewhere/agentic-org")
                && refused.contains("/home/me/.local/share/agentic-org")
                && refused.contains(location::OFFICE_HOME_ENV),
            "{refused}"
        );
        for variable in org_platform::dirs::application_base_variables().unwrap() {
            assert!(refused.contains(variable), "{refused}");
        }
        // The account's own home, reached through the same rule, is served.
        let served = pinned_office_home(
            Some(Path::new("/home/me/.local/share/agentic-org")),
            Some(account),
            Some(account),
            own,
            |a, b| a == b,
        );
        assert_eq!(
            served.unwrap(),
            Path::new("/home/me/.local/share/agentic-org")
        );
    }

    #[test]
    fn the_account_home_is_the_systems_default_whatever_the_environment_says() {
        let account = Path::new("/home/me");
        assert_eq!(
            office_home_in(account),
            org_platform::dirs::account_application_base(account).ok()
        );
        // Where the system has a variable that moves the shared rule, the two can differ;
        // where it has none, they are the same path.
        if org_platform::dirs::application_base_variables()
            .unwrap()
            .is_empty()
        {
            assert_eq!(office_home_in(account), shared_home_in(account));
        }
    }

    #[test]
    fn a_system_without_locations_pins_no_office_home() {
        let account = Path::new("/Users/me");
        let refused = pinned_office_home(
            Some(Path::new("/Users/me/anything")),
            Some(account),
            Some(account),
            |_| None,
            |a, b| a == b,
        );
        assert!(refused.unwrap_err().contains("no office home location"));
    }

    #[test]
    fn the_shared_rule_is_the_override_or_the_systems_base_for_an_absolute_home() {
        let home = "/Users/me";
        let base = shared_home_in(Path::new(home));
        assert_eq!(
            base,
            org_platform::dirs::application_base(Path::new(home)).ok()
        );
        assert!(base.is_some());
        assert_eq!(
            default_office_home_from(Some("/tmp/o1".into()), Some(home.into())),
            Some(PathBuf::from("/tmp/o1"))
        );
        assert_eq!(default_office_home_from(None, Some(home.into())), base);
        for ignored in ["", "tmp/o1"] {
            assert_eq!(
                default_office_home_from(Some(ignored.into()), Some(home.into())),
                base
            );
        }
        for no_home in [None, Some(""), Some("me")] {
            assert_eq!(
                default_office_home_from(None, no_home.map(Into::into)),
                None
            );
        }
        assert_eq!(
            default_office_home_from(Some("o1".into()), Some("me".into())),
            None
        );
    }

    #[test]
    fn prepare_makes_and_tightens_owner_only_folders() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o755)).unwrap();
        let layout = Layout::new(&home);
        fs::create_dir(&layout.run).unwrap();
        fs::create_dir(layout.run.join("o")).unwrap();
        fs::set_permissions(layout.run.join("o"), fs::Permissions::from_mode(0o755)).unwrap();
        // A special bit alone (here sticky) on an otherwise owner-only folder is cleared too.
        fs::create_dir(&layout.logs).unwrap();
        fs::set_permissions(&layout.logs, fs::Permissions::from_mode(0o1700)).unwrap();
        assert_eq!(fs::metadata(&layout.logs).unwrap().mode() & 0o7777, 0o1700);
        layout.prepare().unwrap();
        let sockets = location::office_sockets_dir(&layout.run);
        assert_eq!(sockets, home.join("run").join("o"));
        for d in [
            &layout.home,
            &layout.run,
            &sockets,
            &layout.logs,
            &layout.offices,
        ] {
            let mode = fs::metadata(d).unwrap().permissions().mode();
            assert_eq!(mode & 0o7777, 0o700, "{}", d.display());
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
    fn prepare_refuses_a_symlinked_office_sockets_folder() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(home.join("run")).unwrap();
        fs::create_dir(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, home.join("run").join("o")).unwrap();
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
        // The child closes the parent's socket and, retaining the lock fd, waits until its
        // post-drop reacquire attempt.
        let pid = org_platform::posix::testing::fork_child(
            org_platform::posix::testing::ForkedChild::HoldUntilByte {
                close: release.as_raw_fd(),
                wait: wait.as_raw_fd(),
            },
        )
        .expect("fork");
        drop(wait);
        drop(held);
        // Assert while the child still holds the fd; catch only to guarantee cleanup on failure.
        let checked = std::panic::catch_unwind(|| {
            let reopened = HostLock::acquire(&layout);
            // Query only our child without waiting or releasing its descriptor.
            assert_eq!(
                org_platform::posix::wait_pid(pid, libc::WNOHANG).unwrap(),
                None
            );
            assert!(
                reopened.is_ok(),
                "HostLock drop must release the lock even with an inherited fd: {:?}",
                reopened.err()
            );
        });
        let released = std::io::Write::write_all(&mut release, &[1]);
        drop(release);
        // Reap before assertions so the regression failure never leaves a child behind.
        let status = org_platform::posix::reap(pid);
        released.unwrap();
        assert_eq!(status.unwrap(), 0);
        if let Err(panic) = checked {
            std::panic::resume_unwind(panic);
        }
    }

    #[test]
    fn the_lock_fd_is_close_on_exec() {
        use std::os::fd::AsRawFd;
        let dir = tempfile::tempdir().unwrap();
        let held = HostLock::acquire(&Layout::new(dir.path())).unwrap();
        assert!(org_platform::posix::is_close_on_exec(held._file.as_raw_fd()).unwrap());
    }

    #[test]
    fn socket_paths_are_checked_against_the_systems_limit() {
        // The limit is 103 bytes on macOS and 107 on Linux; run/o/<12>/s.sock, the longest,
        // adds 26 bytes to the home (run/r/<16 hex> 23): a home of `limit - 26` bytes is the
        // longest that fits, one byte more is refused.
        let limit = socket_path_limit().unwrap();
        let ok = PathBuf::from(format!("/{}", "a".repeat(limit - 27)));
        assert_eq!(ok.as_os_str().len() + 26, limit);
        assert!(Layout::new(&ok).check_socket_paths().is_ok());
        let long = PathBuf::from(format!("/{}", "a".repeat(limit - 26)));
        match Layout::new(&long).check_socket_paths() {
            Err(HostError::SocketPathTooLong {
                len, limit: told, ..
            }) => {
                assert_eq!(len, limit + 1);
                assert_eq!(told, limit);
            }
            other => panic!("{other:?}"),
        }
    }
}
