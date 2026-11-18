//! `agentic-org setup` (A2 B4, SRV-2..SRV-7, ENC-3): the steps, in order, with the platform's
//! service manager and disk-encryption probe injected by the caller.
//!
//! 1. Platform gate: a release build without a pinned runtime release refuses (SRV-1); the
//!    caller's [`crate::human::paths`] refuses systems without instance locations.
//! 2. Lock and freeze the setup record (`setup.lock`). The address is chosen before anything
//!    is written: port 7420 for `default`, the first free port from 7421 for a named instance;
//!    an occupied requested port is an actionable error and nothing is saved (SRV-4).
//! 3. Reserve the instance, record its listener and prepare the layout. Where a release
//!    host keeps its secrets in a credential directory (Linux, ADR-0017 decision 8), make
//!    that directory, check it as the host will, and record its absolute path with the
//!    instance: once, here, so that the host never derives it from its environment.
//! 4. Report disk encryption and the folder modes, read back from disk (ENC-3).
//! 5. Register the service and start it unless it already runs (SRV-6).
//! 6. Wait until `host.sock` answers hello and `instance.status` for this instance.
//! 7. Ask for the initial registration link once (`admin.link.initial`) and print it, only
//!    when the output is a terminal. Noninteractive setup asks for no link and names
//!    `agentic-org admin reset` instead (ADR-0022, UX §4a.2).
//!
//! Every step is idempotent; a second run reuses the frozen record and prints no link.
//! Ports other instances saved are never chosen, and a saved port another program holds
//! is reported before the service starts.
//!
//! Before step 1, [`run`] asks the service manager whether it answers at all (a Linux
//! machine or container without a systemd user manager is refused there, with the
//! foreground mode named) and makes the account folders a fresh Linux account lacks.
//!
//! Non-interactive setup ([`configuration`], [`Request::configuration`]) takes step 2's
//! whole input from a strict file instead of choosing a port, is offline, and never asks
//! for a link. With the file's service mode `foreground` (a container) the credential
//! directory of step 3 is a mounted volume that setup checks and never makes, and setup
//! ends after step 4: nothing is registered, started or waited for, and no service manager
//! is asked anything. `agentic-org serve --setup <file>` runs exactly that and then serves.
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use org_host_control::admin::{
    AdminLinkInitial, AdminLinkInitialParams, AdminLinkResult, AdminReset, AdminResetParams,
};
use org_host_control::listen::ListenConfig;
use org_host_control::{
    Administrator, GatewayState, InstanceStatus, InstanceStatusParams, InstanceStatusResult,
};
use org_platform::disk::{DiskEncryption, EncryptionState, Wording};
use org_platform::service::{Lingering, ServiceError, ServiceManager, ServiceStatus, Unit};
use org_proto::{Hello, HelloParams};

use crate::human::Wire;
use crate::instance::registry::Registry;
use crate::instance::setup::{Input, Journal, Paths, Record, Service};
use crate::release::ReleasePin;

#[cfg(all(feature = "test-support", debug_assertions))]
pub mod fixture;
#[cfg(test)]
mod tests;

/// The port of the `default` instance; named instances take the first free one after it.
pub const DEFAULT_PORT: u16 = 7420;
const READY_WITHIN: Duration = Duration::from_secs(60);
/// How long a command waits for the instance locator while a starting service holds it.
const LOCATOR_WAIT: Duration = Duration::from_secs(5);

/// The environment variables that decided this account's instance locations in this
/// process, with their values: what a registered service must be given so that it resolves
/// the same paths as the setup that registered it (X5 review V7). A service manager starts
/// its units with its own environment, not the shell's that ran setup: without these, a
/// base moved by a variable is looked for elsewhere and the service never becomes ready.
///
/// Only variables that are set to an absolute path are carried; anything else is ignored
/// by the path rule itself. The names are the system's: where no variable can move the
/// application base (macOS) there are none, and where one can (GNU/Linux, the XDG base
/// directories) its two siblings, which place the user temp base and the default
/// credential directory by the same rule, are carried with it.
pub fn path_environment() -> Vec<(OsString, OsString)> {
    path_environment_from(
        org_platform::dirs::application_base_variables().unwrap_or(&[]),
        |name| std::env::var_os(name),
    )
}

fn path_environment_from(
    application_base: &[&str],
    value: impl Fn(&str) -> Option<OsString>,
) -> Vec<(OsString, OsString)> {
    let mut names: Vec<&str> = application_base.to_vec();
    if names.contains(&"XDG_DATA_HOME") {
        names.extend(["XDG_CACHE_HOME", "XDG_STATE_HOME"]);
    }
    names
        .into_iter()
        .filter_map(|name| {
            let value = value(name).filter(|v| Path::new(v).is_absolute())?;
            Some((OsString::from(name), value))
        })
        .collect()
}

/// What setup and the host say, once each, of a listener that is plain HTTP on a published
/// port (`http_published`): where it is safe, what happens elsewhere, and that there is no
/// HTTPS yet. `address` is the listen address and port.
pub(crate) fn published_http_notice(listen: &ListenConfig) -> String {
    format!(
        "serving plain HTTP on a published port ({}:{}): publish it on the Docker host's \
         127.0.0.1 only (docker run -p 127.0.0.1:{port}:{port}); published on another \
         interface, sign-in and every session travel unencrypted on that network. Open {}. \
         HTTPS is not available yet.",
        listen.address(),
        listen.port(),
        listen.origin().as_str(),
        port = listen.port(),
    )
}

/// The platform adapters setup composes.
pub struct Platform {
    pub service: Box<dyn ServiceManager>,
    pub disk: Box<dyn DiskEncryption>,
    pub credentials: CredentialDirectory,
}

/// Where the host of the instance being set up keeps its secrets, as far as setup has to
/// provide it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialDirectory {
    /// Not in a directory setup makes: the Keychain, or a development build's key folder.
    /// The record names none.
    Unused,
    /// In `<base>/<instance id>`: setup makes it (mode 700, with the folders of `base`
    /// that are missing), checks it and records it. `base` is absolute.
    Below(PathBuf),
}

impl CredentialDirectory {
    /// What a host of this build keeps its secrets in on this system, for the account
    /// whose home is `home`: a development build its key folder; a release build the
    /// system's protected store, which is a directory on Linux
    /// (`${XDG_STATE_HOME:-~/.local/state}/agentic-org/credentials`, read here and never
    /// again) and the Keychain on macOS.
    pub fn for_host(home: &Path) -> Result<Self, String> {
        use org_platform_host::credentials::{protected_store, ProtectedStore};
        if cfg!(debug_assertions) {
            return Ok(Self::Unused);
        }
        match protected_store().map_err(|e| e.to_string())? {
            ProtectedStore::Keychain => Ok(Self::Unused),
            ProtectedStore::ReleaseDirectory => org_platform::dirs::credential_directory_base(home)
                .map(Self::Below)
                .map_err(|e| e.to_string()),
        }
    }
}

/// Makes the credential directory of instance `instance_id` below `base` and checks it as
/// the host will at every start: outside the data root `data_root`, owner-only, on a
/// filesystem that can hold it, below directories nobody else can change. Returns its
/// absolute path, for the record. Every folder this makes has mode 700; an existing one is
/// left as it is, and a wrong one is refused with its path and the rule it broke, never
/// repaired. A refused location is left exactly as it was found: nothing this call made
/// remains.
///
/// `base` is normalized first, as the record will demand, so that a base spelled with `.`
/// or a repeated separator is recorded in its one spelling and one with `..` is refused
/// before anything is made. `judged_from` is where the walk that judges the directories
/// above the store begins: `/` in the host's composition; a test whose temporary directory
/// is below a shared one passes its own base, as it passes the platform's test entry.
pub(crate) fn make_credential_directory(
    base: &Path,
    instance_id: &str,
    data_root: std::os::fd::BorrowedFd<'_>,
    judged_from: &Path,
    enter: impl FnOnce(
        org_platform_host::credentials::ReleaseDirectory<'_>,
    ) -> Result<
        org_platform_host::credentials::ReleaseRoot,
        org_platform_host::credentials::CredentialError,
    >,
) -> Result<PathBuf, String> {
    const NO_PATH: &str = "the credential directory has no absolute path; nothing was saved";
    if !crate::instance::registry::valid_instance_id(instance_id) {
        return Err(NO_PATH.into());
    }
    let base = crate::instance::setup::normalized(base).map_err(|_| NO_PATH.to_owned())?;
    // A registered service's store is a directory of the account, not a mount.
    use_credential_directory(
        &base.join(instance_id),
        false,
        data_root,
        judged_from,
        enter,
    )
}

/// Opens a credential directory exactly as the host opens it at every start
/// (`release_store::open`): with the host's two item stores declared, so that setup and the
/// host judge the same directories by the same rules.
fn open_as_the_host_will(
    location: org_platform_host::credentials::ReleaseDirectory<'_>,
) -> Result<
    org_platform_host::credentials::ReleaseRoot,
    org_platform_host::credentials::CredentialError,
> {
    org_platform_host::credentials::release_directory_with_stores(
        location,
        &crate::release_store::ITEM_STORES,
    )
}

/// What can refuse a configuration from a file, asked before anything is saved (X5 review
/// V1), so that a refused file leaves nothing and a corrected one works on the next run:
///
/// - a registered service's address must be free now (its service is started by setup);
/// - the credential directory the file names must pass the host's own check. The check
///   compares it with the data folder, which is made for the question when it is missing
///   and removed again before this returns, whatever the answer.
fn preflight(input: &Input) -> Result<(), String> {
    use std::os::fd::AsFd;
    use std::os::unix::fs::DirBuilderExt;
    let listen = &input.listen;
    if input.service == Service::Registered
        && std::net::TcpListener::bind((listen.address(), listen.port())).is_err()
    {
        return Err(format!(
            "port {} on {} is in use by another program; choose another port in the file, or \
             stop that program; nothing was saved",
            listen.port(),
            listen.address()
        ));
    }
    let Some(credentials) = &input.credentials else {
        return Ok(());
    };
    let made = match std::fs::DirBuilder::new().mode(0o700).create(&input.root) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => {
            return Err(format!(
                "the data folder {} cannot be made: {e}; nothing was saved",
                input.root.display()
            ))
        }
    };
    let checked = std::fs::File::open(&input.root)
        .map_err(|e| format!("the data folder {}: {e}", input.root.display()))
        .and_then(|root| {
            use_credential_directory(
                credentials,
                input.service == Service::Foreground,
                root.as_fd(),
                Path::new("/"),
                open_as_the_host_will,
            )
        });
    if made {
        let _ = std::fs::remove_dir(&input.root);
    }
    checked
        .map(drop)
        .map_err(|refused| format!("{refused}; nothing was saved"))
}

/// The credential directory at exactly `path` (absolute and normalized), checked as the
/// host will check it at every start, and returned for the record.
///
/// In the foreground mode (`mounted`) it is a mounted volume: setup never makes it, and
/// without a mount at the path it refuses. Otherwise the directories that are missing are
/// made first, below ancestors that were judged, and taken away again when the check
/// refuses. No item is expected yet.
pub(crate) fn use_credential_directory(
    path: &Path,
    mounted: bool,
    data_root: std::os::fd::BorrowedFd<'_>,
    judged_from: &Path,
    enter: impl FnOnce(
        org_platform_host::credentials::ReleaseDirectory<'_>,
    ) -> Result<
        org_platform_host::credentials::ReleaseRoot,
        org_platform_host::credentials::CredentialError,
    >,
) -> Result<PathBuf, String> {
    let made = if mounted {
        None
    } else {
        Some(
            store_directories::make(path, Some(data_root), judged_from)
                .map_err(|refused| format!("the credential directory cannot be used: {refused}"))?,
        )
    };
    // Open it once as the host does, so that a directory the host would refuse is refused
    // now, with the same words, and not at the service's first start.
    match enter(org_platform_host::credentials::ReleaseDirectory {
        path,
        data_root,
        mount_point_required: mounted,
        expect_items: false,
    }) {
        Ok(root) => drop(root),
        Err(refused) => {
            if let Some(made) = made {
                made.remove();
            }
            let hint = if mounted {
                "; in the foreground mode it is a mounted volume that belongs to the service \
                 user with mode 700, and setup does not make it"
            } else {
                ""
            };
            return Err(format!(
                "the credential directory cannot be used: {refused}{hint}"
            ));
        }
    }
    Ok(path.to_owned())
}

/// INTERIM, in one place (X3 review, finding S4): making the directories of a credential
/// store below ancestors that were judged first.
///
/// The platform owns the store's ancestor rule (`org_platform_host::credentials`), and the
/// right home for this is a function of that crate that walks the path as
/// `release_directory` does and makes the missing directories below ancestors it judged:
///
/// ```text
/// pub fn create_release_directory(location: ReleaseDirectory<'_>)
///     -> Result<(), CredentialError>;
/// ```
///
/// (with the test entry `testing::credentials::create_release_directory_below(base, ..)`).
/// Until it exists, this module makes nothing below a chain it did not judge itself: it
/// walks from the start one component at a time with `openat(O_DIRECTORY | O_NOFOLLOW)`,
/// so no symbolic link is followed; every existing directory on the way must be owned by
/// root or this user, must not be writable by others, and must not be the data root; and
/// only then is each missing directory made with `mkdirat` (mode 700) below the directory
/// it holds open. These are the rules that hold on every system. The platform's own check
/// runs afterwards on the whole path (the group rule, the filesystem, the mode, the mount),
/// and [`Made::remove`] takes away what was made here when that check refuses.
mod store_directories {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Component, Path};

    /// The directories one call made, shallowest first, each with its held parent.
    #[derive(Debug)]
    pub(super) struct Made(Vec<(OwnedFd, CString)>);

    impl Made {
        /// Removes what the call made, deepest first, with the lock file the platform's
        /// check may have left in the store. Nothing that existed before is touched.
        pub(super) fn remove(self) {
            let mut deepest = true;
            for (parent, name) in self.0.into_iter().rev() {
                if deepest {
                    // SAFETY: valid descriptor and NUL-terminated names; errors are ignored.
                    unsafe {
                        let store = libc::openat(
                            parent.as_raw_fd(),
                            name.as_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        );
                        if store >= 0 {
                            libc::unlinkat(store, c".lock".as_ptr(), 0);
                            libc::close(store);
                        }
                    }
                    deepest = false;
                }
                // SAFETY: as above; a directory that is not empty stays.
                unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
            }
        }

        #[cfg(test)]
        pub(super) fn count(&self) -> usize {
            self.0.len()
        }
    }

    fn open_directory(parent: Option<&OwnedFd>, name: &CString) -> std::io::Result<OwnedFd> {
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: `name` is NUL-terminated and `parent`, when given, is an open directory.
        let fd = unsafe {
            match parent {
                Some(parent) => libc::openat(parent.as_raw_fd(), name.as_ptr(), flags),
                None => libc::open(name.as_ptr(), flags),
            }
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `fd` is a new descriptor owned only here.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    fn status(fd: BorrowedFd<'_>) -> std::io::Result<libc::stat> {
        // SAFETY: stat is plain data that fstat fills in; zeroed is a valid start.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: an open descriptor and a writable stat.
        if unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(stat)
    }

    /// Makes the directories of `path` that are missing, below ancestors judged first (see
    /// the module). `path` is absolute and normalized, and `judged_from` is `path` or above
    /// it. `data_root`, when given, is a directory the path must not pass through. A
    /// refusal names the directory and the rule, and nothing was made.
    pub(super) fn make(
        path: &Path,
        data_root: Option<BorrowedFd<'_>>,
        judged_from: &Path,
    ) -> Result<Made, String> {
        use std::os::fd::AsFd;
        let os = |at: &Path, e: std::io::Error| format!("{}: {e}", at.display());
        let rest = path
            .strip_prefix(judged_from)
            .map_err(|_| format!("{}: is not below {}", path.display(), judged_from.display()))?;
        let data = data_root
            .map(|root| status(root).map(|found| (found.st_dev, found.st_ino)))
            .transpose()
            .map_err(|e| format!("the data root: {e}"))?;
        // SAFETY: geteuid has no preconditions and cannot fail.
        let user = unsafe { libc::geteuid() };
        let judge = |at: &Path, fd: &OwnedFd| -> Result<(), String> {
            let found = status(fd.as_fd()).map_err(|e| os(at, e))?;
            let rule = if Some((found.st_dev, found.st_ino)) == data {
                "is inside the data root"
            } else if found.st_uid != 0 && found.st_uid != user {
                "is owned by another user"
            } else if found.st_mode & 0o002 != 0 {
                "is writable by others"
            } else {
                return Ok(());
            };
            Err(format!("{}: {rule}", at.display()))
        };
        let start = CString::new(judged_from.as_os_str().as_bytes())
            .map_err(|_| format!("{}: is not a path", judged_from.display()))?;
        let mut current = open_directory(None, &start).map_err(|e| os(judged_from, e))?;
        let mut at = judged_from.to_owned();
        judge(&at, &current)?;
        let mut made = Vec::new();
        for component in rest.components() {
            let Component::Normal(part) = component else {
                return Err(format!("{}: is not a normalized path", path.display()));
            };
            let name = CString::new(part.as_bytes())
                .map_err(|_| format!("{}: is not a path", path.display()))?;
            at.push(part);
            let next = match open_directory(Some(&current), &name) {
                Ok(next) if made.is_empty() => {
                    judge(&at, &next)?;
                    next
                }
                Ok(next) => next,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // SAFETY: an open, judged parent and a NUL-terminated single name.
                    if unsafe { libc::mkdirat(current.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                        let error = std::io::Error::last_os_error();
                        Made(made).remove();
                        return Err(os(&at, error));
                    }
                    let parent = current.try_clone().map_err(|e| os(&at, e));
                    let opened = open_directory(Some(&current), &name).map_err(|e| os(&at, e));
                    match (parent, opened) {
                        (Ok(parent), Ok(next)) => {
                            made.push((parent, name));
                            next
                        }
                        (parent, opened) => {
                            // SAFETY: as above; the directory this call just made.
                            unsafe {
                                libc::unlinkat(
                                    current.as_raw_fd(),
                                    name.as_ptr(),
                                    libc::AT_REMOVEDIR,
                                )
                            };
                            Made(made).remove();
                            return Err(parent.err().or(opened.err()).unwrap_or_default());
                        }
                    }
                }
                Err(e) => {
                    Made(made).remove();
                    // A symbolic link, or something that is not a directory, on the way.
                    return Err(format!(
                        "{}: {} is not a directory this user's store can be below ({e})",
                        path.display(),
                        at.display()
                    ));
                }
            };
            current = next;
        }
        Ok(Made(made))
    }
}

/// Asked once, on the first run only, with the default address; `Ok(None)` accepts it.
pub type PortPrompt<'a> = &'a mut dyn FnMut(&str) -> Result<Option<u16>, String>;

/// What the person asked for.
pub struct Request<'a> {
    pub name: Option<String>,
    /// `--port`; without it the default is offered to `prompt` (if any) and then used.
    pub port: Option<u16>,
    pub prompt: Option<PortPrompt<'a>>,
    /// The service program (`agentic-org` itself) and its explicit environment.
    pub program: PathBuf,
    pub env: Vec<(OsString, OsString)>,
    /// Whether `out` is a terminal. The one-time link is printed only to a terminal.
    pub terminal: bool,
    /// Non-interactive setup: the whole input comes from a file, nothing is asked, `port`
    /// and `prompt` are not used, and no link is asked for or printed (ADR-0022, UX §4a.2).
    pub configuration: Option<Configuration>,
}

/// The configuration of non-interactive, offline setup (`setup --non-interactive --config
/// <file>`, `serve --setup <file>`): the setup record's own input, read from a strict JSON
/// file. See [`configuration`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration(Input);

impl Configuration {
    /// Whether the instance runs in the foreground, with nothing registered.
    pub fn foreground(&self) -> bool {
        self.0.service == Service::Foreground
    }
}

/// Reads the setup configuration at `path`: one JSON object, version 1, strict (an
/// unknown or repeated property is refused), at most 64 KiB. Its properties:
///
/// - `version`: `1`.
/// - `service`: `"registered"` (the account's service manager) or `"foreground"` (nothing
///   is registered; whoever runs `agentic-org serve --setup` is the supervisor).
/// - `root`: the instance's data folder, absolute.
/// - `credentials`: the directory that holds the host's secrets, absolute. Required with
///   `foreground` on a system that keeps them in a directory (Linux), where it must be a
///   mounted volume that setup never makes; refused on a system that keeps them in the
///   Keychain.
/// - `listen`: `address`, `port`, `scheme`, `origin` and `hsts` (`"off"`, `"on"`,
///   `"clear"`), as the instance stores them. `scheme` is `"http_loopback"` (plain HTTP on
///   exactly `127.0.0.1` or `::1`) or `"http_published"`: plain HTTP on an
///   address that is not loopback, with `localhost`, `127.0.0.1` or `[::1]` as the origin's
///   host, accepted only with `"service": "foreground"` (a container whose port the
///   runtime publishes on the Docker host's loopback). It is never implied: `http_loopback`
///   with another address is refused. `"https"` is refused: no build serves it yet.
///
/// Nothing is created or changed by reading it.
pub fn configuration(path: &Path) -> Result<Configuration, String> {
    use std::io::Read;
    let shown = path.display();
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(64 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("reading the setup configuration {shown}: {e}"))?;
    let input = Input::from_configuration(&bytes).map_err(|e| {
        // The rule alone: the error's own prefix speaks of choosing an instance.
        let why = match e {
            crate::instance::Error::Invalid(why) => why.to_owned(),
            other => other.to_string(),
        };
        format!("{shown}: {why}; nothing was changed")
    })?;
    input
        .fits(org_platform_host::credentials::protected_store().ok())
        .map_err(|why| format!("{shown}: {why}; nothing was changed"))?;
    Ok(Configuration(input))
}

fn fail(context: &str, why: impl std::fmt::Display) -> String {
    format!("{context}: {why}")
}

/// The release this setup records. Development builds record a fixed development pin; a
/// release build refuses until it carries a pinned runtime release (SRV-1).
pub(crate) fn release_pin() -> Result<ReleasePin, String> {
    if cfg!(debug_assertions) {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(
            format!("agentic-org development {}", env!("CARGO_PKG_VERSION")).as_bytes(),
        );
        Ok(ReleasePin {
            release_id: "development".into(),
            sequence: 1,
            manifest_sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
        })
    } else {
        Err(
            "setup needs a pinned runtime release, which this build does not carry yet \
             (SRV-1); nothing was changed"
                .into(),
        )
    }
}

pub(crate) fn loopback(port: u16) -> Result<ListenConfig, String> {
    serde_json::from_value(serde_json::json!({
        "address": "127.0.0.1", "port": port, "scheme": "http_loopback",
        "origin": format!("http://127.0.0.1:{port}"), "hsts": "off"
    }))
    .map_err(|e| fail("building the address", e))
}

fn port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// The ports other instances saved, by name: their frozen setup records and the browser
/// addresses in the instance locator. Unreadable entries are skipped; this only steers
/// the choice of a new port.
fn saved_ports(paths: &Paths, name: Option<&str>) -> Vec<(String, u16)> {
    let shown = name.unwrap_or("default");
    let mut saved = Vec::new();
    if let Ok(entries) = std::fs::read_dir(paths.base.join("setup")) {
        for entry in entries.flatten() {
            let Some(other) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if other == shown {
                continue;
            }
            if let Ok(record) = Journal::snapshot(&paths.base, Some(&other)) {
                saved.push((other, record.input.listen.port()));
            }
        }
    }
    if let Ok(registry) = Registry::open(&paths.locator, &paths.user_temp) {
        if let Ok(ports) = registry.saved_browser_ports() {
            saved.extend(ports.into_iter().filter(|(other, _)| other != shown));
        }
    }
    saved.sort();
    saved.dedup();
    saved
}

/// Refuses a saved port that another program holds while this instance's service is not
/// running: the service could not serve its address, and the address does not change.
fn port_held_elsewhere(port: u16, name: Option<&str>) -> String {
    format!(
        "port {port} on 127.0.0.1, this instance's saved address, is in use by another \
         program, so its service cannot serve it; stop that program, then run {}",
        match name {
            None | Some("default") => "agentic-org setup".to_owned(),
            Some(name) => format!("agentic-org setup --name {name}"),
        }
    )
}

/// The first run's port: the requested one if free, otherwise the instance's default.
/// `saved` holds the ports other instances saved, by name; none of them is chosen.
pub(crate) fn choose_port(
    name: Option<&str>,
    requested: Option<u16>,
    free: impl Fn(u16) -> bool,
    saved: &[(String, u16)],
) -> Result<u16, String> {
    let owner = |port: u16| saved.iter().find(|(_, p)| *p == port).map(|(n, _)| n);
    if let Some(port) = requested {
        if port == 0 {
            return Err("port 0 is not a fixed address; choose a port from 1 to 65535".into());
        }
        if let Some(other) = owner(port) {
            return Err(format!(
                "port {port} is the saved address of instance \"{other}\"; choose another \
                 port with --port; nothing was saved"
            ));
        }
        if !free(port) {
            return Err(format!(
                "port {port} on 127.0.0.1 is in use by another program; choose another \
                 port with --port, or stop that program; nothing was saved"
            ));
        }
        return Ok(port);
    }
    match name {
        None | Some("default") => {
            if let Some(other) = owner(DEFAULT_PORT) {
                Err(format!(
                    "port {DEFAULT_PORT} is the saved address of instance \"{other}\"; choose \
                     another port with --port; nothing was saved"
                ))
            } else if free(DEFAULT_PORT) {
                Ok(DEFAULT_PORT)
            } else {
                Err(format!(
                    "port {DEFAULT_PORT} on 127.0.0.1 is in use by another program; choose \
                     another port with --port, or stop that program; nothing was saved"
                ))
            }
        }
        Some(_) => (DEFAULT_PORT + 1..=u16::MAX)
            .find(|p| owner(*p).is_none() && free(*p))
            .ok_or_else(|| "no free port on 127.0.0.1 from 7421; nothing was saved".into()),
    }
}

/// Folder and file modes under `root`, read back from disk: the paths whose mode is not
/// 700 (folders) or 600 (files). Symbolic links are reported, never followed.
pub(crate) fn mode_exceptions(root: &Path) -> Result<Vec<(PathBuf, u32)>, String> {
    let mut exceptions = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        let meta = std::fs::symlink_metadata(&path).map_err(|e| fail("reading modes", e))?;
        let mode = meta.permissions().mode() & 0o7777;
        let expected = if meta.is_dir() { 0o700 } else { 0o600 };
        if meta.file_type().is_symlink() || mode != expected {
            exceptions.push((path.clone(), mode));
        }
        if meta.is_dir() {
            for entry in std::fs::read_dir(&path).map_err(|e| fail("reading modes", e))? {
                pending.push(entry.map_err(|e| fail("reading modes", e))?.path());
            }
        }
    }
    exceptions.sort();
    Ok(exceptions)
}

/// The ENC-3 lines: disk encryption, the modes actually found and the isolation mode.
/// `wording` is what this system calls its disk encryption and where a person turns it on
/// (`org_platform::disk::wording`).
pub(crate) fn protection_report(
    encryption: &EncryptionState,
    wording: Wording,
    user: &str,
    exceptions: &[(PathBuf, u32)],
) -> Vec<String> {
    let Wording { name, setting } = wording;
    let mut lines = vec![match encryption {
        EncryptionState::On => format!("Disk encryption ({name}): on."),
        EncryptionState::Off => format!(
            "Disk encryption is off. Agentic Org does not encrypt its data; turn on {setting}."
        ),
        EncryptionState::Unknown(why) => format!(
            "Disk encryption could not be determined ({why}). Agentic Org does not encrypt \
             its data; check {setting}."
        ),
    }];
    if exceptions.is_empty() {
        lines.push(format!(
            "File permissions: the instance folder and everything in it are readable only by \
             {user} (folders 700, files 600)."
        ));
    } else {
        lines.push(format!(
            "File permissions: {} path(s) in the instance folder are not private to {user}:",
            exceptions.len()
        ));
        for (path, mode) in exceptions.iter().take(10) {
            lines.push(format!("  {:o} {}", mode, path.display()));
        }
    }
    lines.push(format!(
        "Agents run as {user} (same-user mode): only the tool sandbox separates agents from \
         the server's data."
    ));
    lines
}

fn user_name() -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty() && !u.contains(char::is_control))
        .unwrap_or_else(|| format!("uid {}", crate::home::euid()))
}

fn say(out: &mut dyn Write, line: impl std::fmt::Display) -> Result<(), String> {
    writeln!(out, "{line}").map_err(|e| fail("writing output", e))
}

/// The platform's own adapters: the account's service manager and disk-encryption probe.
/// Systems without them refuse here, before anything is changed. `foreground` composes
/// the manager that registers nothing, which every system has.
pub fn platform(foreground: bool) -> Result<Platform, String> {
    use org_platform::service::ServiceMode;
    let home = crate::home::account_dir().ok_or("the account home folder is unavailable")?;
    let refused = |e: org_platform::Refused| e.to_string();
    let mode = if foreground {
        ServiceMode::Foreground
    } else {
        ServiceMode::Registered
    };
    Ok(Platform {
        service: org_platform::service::service(mode, &home).map_err(refused)?,
        disk: org_platform::disk::disk_encryption().map_err(refused)?,
        credentials: CredentialDirectory::for_host(&home)?,
    })
}

/// Runs setup for `request`. See the module docs for the steps.
pub fn run(platform: &Platform, request: Request<'_>, out: &mut dyn Write) -> Result<(), String> {
    // A release build without a pinned runtime refuses before any path is resolved or made.
    release_pin()?;
    service_gate(platform, &request)?;
    account_folders();
    let paths = crate::human::paths(request.name.as_deref())?;
    run_at(&paths, platform, request, out)
}

/// A system whose service manager does not answer (a Linux machine or container without a
/// systemd user manager) is refused before anything is made or saved, with the way to run
/// without one. The foreground mode never asks a service manager anything.
fn service_gate(platform: &Platform, request: &Request<'_>) -> Result<(), String> {
    if request
        .configuration
        .as_ref()
        .is_some_and(Configuration::foreground)
    {
        return Ok(());
    }
    platform.service.available().map_err(no_service_manager)
}

/// Makes the account's own folders that the instance locations need and that a fresh
/// account does not have: the folder the application base is in and the user temp base,
/// where the system puts them below the account's home (on Linux `~/.local/share` and
/// `~/.cache/agentic-org/tmp`; an account or a container's service user that never ran a
/// desktop session has neither; on macOS only a missing `~/Library/Application Support`).
/// Each missing folder is made with mode 700, one component at a time below the home,
/// without following a link below it. A location outside the home (the system's own temp
/// folder on macOS, an `XDG_*` variable that points elsewhere) is left to exist or not,
/// and a development base makes its own folders.
///
/// This step refuses nothing. The home is resolved as the system resolves it (a home that
/// is itself a symbolic link, as network and managed homes are, is followed), and where a
/// folder cannot be made here (a link below the home, a folder of another user) nothing is
/// made and the instance locations are judged by their own rules right after, exactly as
/// they were before this step existed.
fn account_folders() {
    if std::env::var_os(crate::human::DEV_ACCOUNT_BASE).is_some_and(|base| !base.is_empty()) {
        return;
    }
    let Some(home) = crate::home::account_dir() else {
        return;
    };
    let Ok(resolved) = std::fs::canonicalize(&home) else {
        return;
    };
    let base = org_platform::dirs::application_base(&home).ok();
    let wanted = [
        base.as_deref().and_then(Path::parent).map(Path::to_owned),
        org_platform::dirs::user_temp_base(&home).ok(),
    ];
    for folder in wanted.into_iter().flatten() {
        let below = crate::instance::setup::normalized(&folder)
            .ok()
            .and_then(|folder| folder.strip_prefix(&home).ok().map(Path::to_owned));
        if let Some(below) = below {
            let _ = store_directories::make(&resolved.join(below), None, &resolved);
        }
    }
}

pub(crate) fn run_at(
    paths: &Paths,
    platform: &Platform,
    mut request: Request<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    // 1. Platform gate. [`run`] has already asked the service manager whether it answers,
    // before any folder was made.
    let pin = release_pin()?;
    let name = request.name.as_deref();
    let shown = name.unwrap_or("default");
    say(out, format!("Setting up instance \"{shown}\"."))?;

    // 2. Lock and freeze the record. The address is decided before anything is written.
    let journal = Journal::open(&paths.base, name).map_err(|e| fail("opening setup", e))?;
    let configured = request
        .configuration
        .take()
        .map(|Configuration(input)| input);
    let existing = journal.read().map_err(|e| fail("reading setup", e))?;
    // A record without a reserved instance is what an earlier run froze and could not
    // continue. It pins nothing: a corrected file replaces it (X5 review V1). A record
    // that names its instance is the instance's configuration and never changes.
    let existing = match (existing, &configured) {
        (Some(record), Some(input)) if record.instance_id.is_none() && *input != record.input => {
            journal
                .abandon()
                .map_err(|e| fail("replacing an unfinished setup", e))?;
            None
        }
        (existing, _) => existing,
    };
    // Whether this run froze the record from a file: if a later step of the reservation
    // refuses, the record is taken away again, so that nothing this run saved remains.
    let mut froze_from_file = false;
    let mut record = match existing {
        Some(record)
            if configured
                .as_ref()
                .is_some_and(|input| *input != record.input) =>
        {
            return Err(format!(
                "instance \"{shown}\" was set up with a different configuration (service \
                 mode, data folder, credential directory or address); its configuration \
                 does not change on a later run; nothing was changed"
            ));
        }
        Some(record) => {
            if request
                .port
                .is_some_and(|p| p != record.input.listen.port())
            {
                return Err(format!(
                    "this instance was set up on port {}; its address does not change on a \
                     later run; nothing was changed",
                    record.input.listen.port()
                ));
            }
            record
        }
        None if configured.is_some() => {
            // Offline: nothing is asked and nothing is fetched. The address is the file's.
            let input = configured.expect("checked above");
            // Everything that can refuse this input is asked before anything is saved.
            preflight(&input)?;
            froze_from_file = true;
            journal
                .freeze(input, true, pin)
                .map_err(|e| fail("saving setup", e))?
        }
        None => {
            let saved = saved_ports(paths, name);
            let mut port = choose_port(name, request.port, port_free, &saved)?;
            if request.port.is_none() {
                if let Some(prompt) = request.prompt.as_mut() {
                    if let Some(chosen) = prompt(&format!("http://127.0.0.1:{port}"))? {
                        port = choose_port(name, Some(chosen), port_free, &saved)?;
                    }
                }
            }
            let root = paths
                .instance_root(name)
                .map_err(|e| fail("preparing the instance folder", e))?;
            let input = Input {
                version: 1,
                service: Service::Registered,
                root,
                credentials: None,
                listen: loopback(port)?,
            };
            journal
                .freeze(input, false, pin)
                .map_err(|e| fail("saving setup", e))?
        }
    };
    say(out, format!("Data: {}", record.input.root.display()))?;
    say(
        out,
        format!("Address: {}", record.input.listen.origin().as_str()),
    )?;

    // 3. Reserve the instance, record its listener and prepare its layout, then record its
    // ID. Once the ID is recorded all three are done, and a running host holds the root lock,
    // so a later run only locates the instance. The root lock is released before the service
    // starts, since the host reserves the instance again.
    let registry = Registry::open(&paths.locator, &paths.user_temp)
        .map_err(|e| fail("opening the instance locator", e))?;
    let (instance_id, socket) = match &record.instance_id {
        Some(recorded) => {
            let (id, temp) = registry
                .locate(name, &record.input.root)
                .map_err(|e| fail("locating the instance", e))?;
            if &id != recorded {
                return Err(
                    "the reserved instance is not the one setup recorded; nothing was changed"
                        .into(),
                );
            }
            (id, temp.join("host.sock"))
        }
        None => {
            let reserve = |record: &mut Record| -> Result<(String, PathBuf), String> {
                registry
                    .restore_temp_folders()
                    .map_err(|e| fail("restoring instance temp folders", e))?;
                let reserved = {
                    let instance = registry
                        .reserve(name, &record.input.root)
                        .map_err(|e| fail("reserving the instance", e))?;
                    instance
                        .set_browser(&record.input.listen)
                        .map_err(|e| fail("recording the instance address", e))?;
                    instance
                        .prepare_layout()
                        .map_err(|e| fail("preparing the instance folder", e))?;
                    // While the root lock is held: the directory is checked against the root
                    // itself, and recorded together with the instance it belongs to.
                    record.credentials = match (&record.input.credentials, &platform.credentials) {
                        // The configuration names it. In the foreground mode it is a mounted
                        // volume, which setup never makes.
                        (Some(configured), _) => Some(use_credential_directory(
                            configured,
                            record.input.service == Service::Foreground,
                            instance.root_descriptor(),
                            Path::new("/"),
                            open_as_the_host_will,
                        )?),
                        (None, CredentialDirectory::Below(base)) => {
                            Some(make_credential_directory(
                                base,
                                instance.id(),
                                instance.root_descriptor(),
                                Path::new("/"),
                                open_as_the_host_will,
                            )?)
                        }
                        (None, CredentialDirectory::Unused) => None,
                    };
                    (
                        instance.id().to_owned(),
                        instance.temp_folder().join("host.sock"),
                    )
                };
                record.instance_id = Some(reserved.0.clone());
                journal.write(record).map_err(|e| fail("saving setup", e))?;
                Ok(reserved)
            };
            match reserve(&mut record) {
                Ok(reserved) => reserved,
                Err(refused) => {
                    // What this run froze from a file is not left to refuse the next,
                    // corrected one. (A reservation that was made stays the registry's:
                    // the same data folder is reserved again by the next run.)
                    if froze_from_file {
                        let _ = journal.abandon();
                    }
                    return Err(refused);
                }
            }
        }
    };
    if let Some(credentials) = &record.credentials {
        say(out, format!("Secrets: {}", credentials.display()))?;
    }
    if record.input.listen.scheme() == org_host_control::listen::ListenMode::HttpPublished {
        say(
            out,
            format!(
                "Browser access: {}",
                published_http_notice(&record.input.listen)
            ),
        )?;
    }

    // 4. ENC-3, with the modes read back from disk.
    let exceptions = mode_exceptions(&record.input.root)?;
    for line in protection_report(
        &platform.disk.state(&record.input.root),
        org_platform::disk::wording(),
        &user_name(),
        &exceptions,
    ) {
        say(out, line)?;
    }

    // 5. The service. In the foreground mode nothing is registered, started or waited
    // for, and no link is asked for: whoever runs `serve` is the supervisor, and the
    // administrator's first link comes from `admin reset`.
    if record.input.service == Service::Foreground {
        return say(
            out,
            format!(
                "Service: none is registered (foreground mode). Instance \"{shown}\" runs \
                 while agentic-org serve --setup <file> runs. For the administrator's \
                 registration link, run as root: {} --user {}",
                reset_hint(name),
                user_name()
            ),
        );
    }
    let service_id = service_id(&instance_id);
    let unit = Unit {
        instance_id: service_id.clone(),
        program: request.program.clone(),
        args: ["serve", "--name", shown].map(OsString::from).to_vec(),
        env: std::mem::take(&mut request.env),
    };
    platform
        .service
        .register(&unit)
        .map_err(|e| fail("registering the service", e))?;
    if !record.service_registered {
        record.service_registered = true;
        journal
            .write(&record)
            .map_err(|e| fail("saving setup", e))?;
    }
    say(out, "Service registered.")?;
    for line in lingering_report(&platform.service.lingering(), &user_name()) {
        say(out, line)?;
    }
    let port = record.input.listen.port();
    match platform.service.status(&service_id) {
        Ok(ServiceStatus::Running { .. }) => {}
        _ => {
            // Nothing of this instance runs, so a held port belongs to another program.
            if !port_free(port) {
                return Err(port_held_elsewhere(port, name));
            }
            platform
                .service
                .start(&service_id)
                .map_err(|e| fail("starting the service", e))?
        }
    }

    // 6. Readiness: hello and instance.status for this instance within the deadline.
    let status = ready(&socket, &instance_id, READY_WITHIN)?;
    // A failed browser service no longer holds its port; if something else does, say so.
    if status.gateway.state == GatewayState::Failed && !port_free(port) {
        return Err(port_held_elsewhere(port, name));
    }
    let administrator = status.administrator;
    say(out, "Service running.")?;
    if !record.registry_created {
        record.registry_created = true;
        journal
            .write(&record)
            .map_err(|e| fail("saving setup", e))?;
    }

    // 7. The initial link, at most once.
    if administrator == Administrator::Present {
        return say(
            out,
            running_with_administrator(shown, record.input.listen.origin().as_str()),
        );
    }
    say(out, "Instance ready. It has no administrator yet.")?;
    // The link is shown only in a terminal; noninteractive setup does not ask for it.
    let result = if record.initial_link_issued || !request.terminal {
        AdminLinkResult::AlreadyIssued
    } else {
        let mut wire = connect(&socket, &instance_id)?;
        let result = wire
            .call::<AdminLinkInitial>(
                &AdminLinkInitialParams {
                    setup_operation: record.setup_operation.clone(),
                },
                None,
            )
            .map_err(|e| fail("asking for the registration link", e))?;
        record.initial_link_issued = true;
        journal
            .write(&record)
            .map_err(|e| fail("saving setup", e))?;
        result
    };
    show_link(out, result, name, record.input.listen.origin())
}

/// What setup and the service commands say when no service manager can register a service
/// here: the platform's reason, then the command that runs an instance without one
/// (SRV-34). Every other service error is shown as it is.
pub(crate) fn no_service_manager(error: ServiceError) -> String {
    match error {
        ServiceError::Unavailable(_) => format!(
            "{error}: write a setup file with \"service\": \"foreground\" and run \
             agentic-org serve --setup <file>; nothing was changed"
        ),
        error => fail("asking the service manager", error),
    }
}

/// What setup and `service status` say about a service that stops when its user logs out
/// (Linux host design section 3.4). The command is printed, never run.
pub(crate) fn lingering_report(lingering: &Lingering, user: &str) -> Vec<String> {
    match lingering {
        Lingering::NotApplicable | Lingering::On => Vec::new(),
        Lingering::Off { enable } => vec![
            format!(
                "The service runs only while {user} is logged in: it stops at logout and \
                 does not start when the machine starts."
            ),
            format!("To keep it running without a login session, run: {enable}"),
        ],
        Lingering::Unknown(why) => vec![format!(
            "Whether the service keeps running after {user} logs out could not be \
             determined ({why})."
        )],
    }
}

/// The service manager's name for an instance: its ID in lowercase (the label is
/// `com.ellosoft.agentic-org.instance.<id>`; instance IDs are canonical lowercase UUIDv7 text, so this is a no-op kept for older input).
fn service_id(instance_id: &str) -> String {
    instance_id.to_ascii_lowercase()
}

fn reset_hint(name: Option<&str>) -> String {
    match name {
        None | Some("default") => "agentic-org admin reset".into(),
        Some(name) => format!("agentic-org admin reset --name {name}"),
    }
}

/// What a command says when a service answers a link call with anything but this
/// instance's own registration link. Nothing of the answer is shown.
pub(crate) const NOT_THIS_INSTANCES_LINK: &str = "the service answered with something that is \
     not this instance's registration link, so nothing of its answer is shown; check that the \
     instance's own service is the one that runs";

/// `url` if it is exactly this instance's registration link: the configured `origin`, then
/// `/register#`, then a token in the product's own form (32 bytes as unpadded base64url).
/// On a system that cannot identify the program behind `host.sock`, the answer comes from
/// a peer the service user controls, and it is printed to a terminal that may be root's:
/// nothing else is ever printed from it (X5 review V4).
pub(crate) fn this_instances_link<'a>(
    url: &'a str,
    origin: &org_host_control::listen::ListenOrigin,
) -> Result<&'a str, String> {
    let token = url
        .strip_prefix(origin.as_str().trim_end_matches('/'))
        .and_then(|rest| rest.strip_prefix("/register#"));
    match token {
        Some(token) if crate::links::Token::parse(token).is_some() => Ok(url),
        _ => Err(NOT_THIS_INSTANCES_LINK.into()),
    }
}

fn show_link(
    out: &mut dyn Write,
    result: AdminLinkResult,
    name: Option<&str>,
    origin: &org_host_control::listen::ListenOrigin,
) -> Result<(), String> {
    match result {
        AdminLinkResult::Issued { url, expires_at_ms } => {
            let url = this_instances_link(url.expose(), origin)?;
            say(out, "")?;
            say(
                out,
                "Open this link in a browser to create the administrator account.",
            )?;
            say(
                out,
                format!(
                    "It works once and expires in 24 hours (at {}):",
                    local_time(expires_at_ms)
                ),
            )?;
            say(out, format!("  {url}"))?;
            say(
                out,
                format!("If it expires or is lost, run: {}", reset_hint(name)),
            )
        }
        AdminLinkResult::AlreadyIssued => {
            say(out, format!("For a new link, run: {}", reset_hint(name)))
        }
        AdminLinkResult::AdministratorPresent => say(out, "It has an administrator."),
    }
}

/// `expires_at_ms` as local `YYYY-MM-DD HH:MM`, or UTC milliseconds if the conversion fails.
fn local_time(ms: i64) -> String {
    let secs = ms.div_euclid(1000) as libc::time_t;
    // SAFETY: `tm` is plain data written by localtime_r from a valid time_t.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return format!("{ms} ms after the Unix epoch, UTC");
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

fn connect(socket: &Path, instance_id: &str) -> Result<Wire, String> {
    connect_verified(socket, instance_id, |stream| {
        crate::human::verify_host(stream).map(drop)
    })
}

/// Connects to `socket`, has `verify` judge who listens there, and greets instance
/// `instance_id`.
fn connect_verified(
    socket: &Path,
    instance_id: &str,
    verify: impl FnOnce(&UnixStream) -> Result<(), String>,
) -> Result<Wire, String> {
    let stream = UnixStream::connect(socket).map_err(|e| fail("connecting to the service", e))?;
    verify(&stream)?;
    let mut wire = Wire::new(stream)?;
    let hello = wire
        .call::<Hello>(
            &HelloParams::offering(org_host_control::SUPPORTED_API, "agentic-org"),
            None,
        )
        .map_err(|e| fail("greeting the service", e))?;
    if hello.instance_id.as_deref() != Some(instance_id) {
        return Err("the service answering is not this instance".into());
    }
    Ok(wire)
}

/// Polls until the service answers for `instance_id`, or the deadline passes.
fn ready(
    socket: &Path,
    instance_id: &str,
    within: Duration,
) -> Result<InstanceStatusResult, String> {
    let deadline = Instant::now() + within;
    loop {
        let attempt = connect(socket, instance_id).and_then(|mut wire| {
            wire.call::<InstanceStatus>(&InstanceStatusParams {}, None)
                .map_err(|e| fail("reading the instance status", e))
        });
        match attempt {
            Ok(status) if status.instance_id == instance_id => return Ok(status),
            Ok(_) => return Err("the service answering is not this instance".into()),
            Err(why) if Instant::now() >= deadline => {
                return Err(format!(
                    "the service did not become ready within {}s ({why}); setup can be run \
                     again and continues where it stopped",
                    within.as_secs()
                ))
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Finds the registered instance `name`: its ID and `host.sock`.
fn registered(paths: &Paths, name: Option<&str>) -> Result<(Record, String, PathBuf), String> {
    let record =
        Journal::snapshot(&paths.base, name).map_err(|e| fail("reading the setup record", e))?;
    let registry = Registry::open(&paths.locator, &paths.user_temp)
        .map_err(|e| fail("opening the instance locator", e))?;
    // A service that is just starting holds the locator's lock for a moment (it reserves
    // its instance again): a command that arrives then waits for it instead of failing.
    let deadline = Instant::now() + LOCATOR_WAIT;
    let (id, temp) = loop {
        match registry.locate(name, &record.input.root) {
            Ok(located) => break located,
            Err(crate::instance::Error::Busy(_)) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(fail("locating the instance", e)),
        }
    };
    if record.instance_id.as_deref() != Some(id.as_str()) {
        return Err("the instance is not the one setup recorded; run agentic-org setup".into());
    }
    Ok((record, id, temp.join("host.sock")))
}

/// What `admin reset` answers when its output is not a terminal (spec §4a.4).
pub(crate) const NOT_A_TERMINAL: &str = "Run this in a terminal.";

/// What a repeated setup says once the instance has an administrator (spec §4a.2).
fn running_with_administrator(shown: &str, origin: &str) -> String {
    format!("Instance {shown} is running. Open {origin} and sign in.")
}

/// `agentic-org admin reset`: a new registration link after presence at the host.
/// `terminal` says whether `out` is a terminal: the link is printed only there, so
/// otherwise this refuses before asking the host, and no link is issued or cancelled.
///
/// Run by root for the service user's instance (SIGN-33; ADR-0017 decisions 10 and 11): the
/// account is `user`, or the one `sudo` names (`SUDO_UID`). Root finds the instance as that
/// user ([`as_account`]), so nothing in the instance's folders is made or changed by root,
/// and then connects as root, which is what the service admits for this one call. Any other
/// user acts only for itself.
pub fn admin_reset(
    name: Option<&str>,
    user: Option<&str>,
    terminal: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    if !terminal {
        // Spec §4a.4 copy. SIGN-34's SSH refusal ("shown before anything else") needs
        // SSH-session detection, which is not built yet; it goes before this check.
        return Err(NOT_A_TERMINAL.into());
    }
    let acting_for = service_account(
        crate::home::euid(),
        user,
        std::env::var("SUDO_UID").ok().as_deref(),
        account_by_name,
        account_by_id,
    )?;
    // SIGN-33: where the system names a peer by its user alone (Linux), only root resets,
    // and anyone else is told so before the service is asked anything.
    if acting_for.is_none() && crate::human::host_is_named_by_user_alone() {
        return Err(reset_needs_root(name));
    }
    let not_running = |why: String| {
        format!(
            "{why}; the service is not running: start it with agentic-org service start{}",
            name.map(|n| format!(" --name {n}")).unwrap_or_default()
        )
    };
    let (record, wire) = match acting_for {
        Some(account) => {
            let (record, id, socket) =
                as_account(account, || registered(&crate::human::paths(name)?, name))?;
            // The first line names the user whose instance this is, so that a reset never
            // lands on another user's instance unnoticed (SIGN-33).
            say(
                out,
                format!(
                    "Instance {} of Linux user {}.",
                    name.unwrap_or("default"),
                    account_name(account.uid)
                ),
            )?;
            let wire = connect_verified(&socket, &id, |stream| {
                crate::human::verify_service_host(stream, account.uid)
            })
            .map_err(not_running)?;
            (record, wire)
        }
        None => {
            let (record, id, socket) = registered(&crate::human::paths(name)?, name)?;
            (record, connect(&socket, &id).map_err(not_running)?)
        }
    };
    reset_over(wire, record.input.listen.origin(), name, out)
}

/// What `admin reset` answers on Linux to anyone but root (spec §4a.2, SIGN-33).
pub(crate) fn reset_needs_root(name: Option<&str>) -> String {
    format!(
        "Resetting the administrator's password on Linux needs root. Run it with sudo: sudo {}",
        reset_hint(name)
    )
}

/// The reset itself, over a connection to the instance's service: one call, and of its
/// answer only this instance's own link is ever shown.
fn reset_over(
    mut wire: Wire,
    origin: &org_host_control::listen::ListenOrigin,
    name: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let result = wire
        .call::<AdminReset>(&AdminResetParams::default(), None)
        .map_err(|e| fail("asking for a new registration link", e))?;
    if matches!(result, AdminLinkResult::AdministratorPresent) {
        return say(
            out,
            "The instance has an administrator; resetting its password is not available yet.",
        );
    }
    show_link(out, result, name, origin)
}

/// The account name of `uid` for a line of output: letters, digits, `_`, `-` and `.` only,
/// or the number where the account database has no such name.
fn account_name(uid: u32) -> String {
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: an all-zero `passwd` is a valid value for the call to fill in.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: an entry, buffer and result valid for the call.
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut found,
        )
    };
    if status == 0 && !found.is_null() && !entry.pw_name.is_null() {
        // SAFETY: on success `pw_name` is a NUL-terminated string inside `buffer`.
        let name = unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }.to_string_lossy();
        let plain = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
        if plain {
            return name.into_owned();
        }
    }
    format!("uid {uid}")
}

/// An account of this machine: its user and primary group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Account {
    pub uid: u32,
    pub gid: u32,
}

/// Whose instance `admin reset` acts on, pure. `me` is this process's effective user,
/// `named` the `--user` option and `sudo_uid` the `SUDO_UID` variable. `None`: this
/// user's own, as itself. Root acts for the account it names and never for itself (the
/// service does not run as root); anyone else acts only for itself.
pub(crate) fn service_account(
    me: u32,
    named: Option<&str>,
    sudo_uid: Option<&str>,
    by_name: impl Fn(&str) -> Option<Account>,
    by_id: impl Fn(u32) -> Option<Account>,
) -> Result<Option<Account>, String> {
    let unknown = |account: &str| format!("this machine has no account {account:?}");
    if me != 0 {
        return match named {
            None => Ok(None),
            Some(account) => match by_name(account) {
                Some(found) if found.uid == me => Ok(None),
                Some(_) => Err(format!(
                    "--user {account} names another account: only root acts for another user"
                )),
                None => Err(unknown(account)),
            },
        };
    }
    let account = match (named, sudo_uid) {
        (Some(account), _) => by_name(account).ok_or_else(|| unknown(account))?,
        (None, Some(uid)) => uid
            .parse()
            .ok()
            .and_then(by_id)
            .ok_or_else(|| unknown(uid))?,
        (None, None) => {
            return Err(
                "as root, name the user the service runs as: agentic-org admin reset --user \
                 <service user>"
                    .into(),
            )
        }
    };
    if account.uid == 0 {
        return Err("the service does not run as root; name the user it runs as".into());
    }
    Ok(Some(account))
}

/// One entry of the account database, by name or by number.
fn account_entry(
    look: impl Fn(&mut libc::passwd, &mut [libc::c_char], &mut *mut libc::passwd) -> libc::c_int,
) -> Option<Account> {
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: an all-zero `passwd` is a valid value for the call to fill in.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    if look(&mut entry, &mut buffer, &mut found) != 0 || found.is_null() {
        return None;
    }
    Some(Account {
        uid: entry.pw_uid,
        gid: entry.pw_gid,
    })
}

fn account_by_name(name: &str) -> Option<Account> {
    let name = std::ffi::CString::new(name).ok()?;
    account_entry(|entry, buffer, found| {
        // SAFETY: a NUL-terminated name, and an entry, buffer and result valid for the call.
        unsafe {
            libc::getpwnam_r(
                name.as_ptr(),
                entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                found,
            )
        }
    })
}

fn account_by_id(uid: u32) -> Option<Account> {
    account_entry(|entry, buffer, found| {
        // SAFETY: an entry, buffer and result valid for the call.
        unsafe { libc::getpwuid_r(uid, entry, buffer.as_mut_ptr(), buffer.len(), found) }
    })
}

/// Runs `work` with this process's effective user and group set to `account`'s, and sets
/// them back. Root uses it to read another user's instance exactly as that user does: every
/// ownership check of the instance's folders holds, and a file `work` makes (a lock) belongs
/// to that user, never to root. Only root can do this, and the process has one thread when
/// a command calls it. If the identity cannot be set back the process ends: it must not
/// continue as a user it did not mean to be.
fn as_account<T>(account: Account, work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    // SAFETY: getegid and geteuid have no preconditions and cannot fail.
    let (group, user) = unsafe { (libc::getegid(), libc::geteuid()) };
    // The group first: only root may change it. SAFETY: plain identity changes.
    if unsafe { libc::setegid(account.gid) } != 0 {
        return Err(fail(
            "acting as the service user",
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: as above.
    if unsafe { libc::seteuid(account.uid) } != 0 {
        let error = std::io::Error::last_os_error();
        // SAFETY: as above; still root.
        if unsafe { libc::setegid(group) } != 0 {
            std::process::abort();
        }
        return Err(fail("acting as the service user", error));
    }
    let result = work();
    // SAFETY: as above; the saved user is still root's.
    if unsafe { libc::seteuid(user) } != 0 || unsafe { libc::setegid(group) } != 0 {
        std::process::abort();
    }
    result
}

/// `agentic-org service start|stop|status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceAction {
    Start,
    Stop,
    Status,
}

pub fn service(
    name: Option<&str>,
    manager: &dyn ServiceManager,
    action: ServiceAction,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (record, id, socket) = registered(&crate::human::paths(name)?, name)?;
    let shown = name.unwrap_or("default");
    if record.input.service == Service::Foreground {
        // Nothing is registered: the service manager is not asked. Whether the instance
        // runs is what its own socket says.
        return match action {
            ServiceAction::Status => say(
                out,
                match connect(&socket, &id) {
                    Ok(_) => "running in the foreground (no service is registered)",
                    Err(_) => {
                        "not running (foreground mode: it runs while agentic-org serve --setup \
                         <file> runs)"
                    }
                },
            ),
            ServiceAction::Start | ServiceAction::Stop => Err(format!(
                "instance \"{shown}\" runs in the foreground and has no registered service: \
                 whoever runs agentic-org serve --setup <file> starts and stops it (in a \
                 container, the container runtime)"
            )),
        };
    }
    let id = service_id(&id);
    match action {
        ServiceAction::Start => {
            manager
                .start(&id)
                .map_err(|e| fail("starting the service", e))?;
            say(out, format!("Service for \"{shown}\" started."))
        }
        ServiceAction::Stop => {
            manager
                .stop(&id)
                .map_err(|e| fail("stopping the service", e))?;
            say(out, format!("Service for \"{shown}\" stopped."))
        }
        ServiceAction::Status => {
            let status = manager
                .status(&id)
                .map_err(|e| fail("reading the service", e))?;
            say(
                out,
                match status {
                    ServiceStatus::Running { pid } => format!("running (pid {pid})"),
                    ServiceStatus::Stopped => "stopped".into(),
                    ServiceStatus::Unloaded => "registered, not loaded".into(),
                    ServiceStatus::NotRegistered => "not registered".into(),
                },
            )?;
            if status != ServiceStatus::NotRegistered {
                for line in lingering_report(&manager.lingering(), &user_name()) {
                    say(out, line)?;
                }
            }
            Ok(())
        }
    }
}
