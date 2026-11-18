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
/// - `listen`: `address`, `port`, `scheme` (`"http_loopback"` or `"https"`), `origin` and
///   `hsts` (`"off"`, `"on"`, `"clear"`), as the instance stores them.
/// - `certificate` (optional, HTTPS only): `chain` and `key`, the absolute paths of the
///   listener's certificate chain and private key.
///
/// Nothing is created or changed by reading it.
pub fn configuration(path: &Path) -> Result<Configuration, String> {
    use std::io::Read;
    let shown = path.display();
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(64 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("reading the setup configuration {shown}: {e}"))?;
    let input = Input::from_configuration(&bytes)
        .map_err(|e| format!("{shown}: {e}; nothing was changed"))?;
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
    account_folders()?;
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
/// desktop session has neither). Each missing folder is made with mode 700, one component
/// at a time below the home, without following a link. A location outside the home (the
/// system's own temp folder on macOS, an `XDG_*` variable that points elsewhere) is left
/// to exist or not, and a development base makes its own folders.
fn account_folders() -> Result<(), String> {
    if std::env::var_os(crate::human::DEV_ACCOUNT_BASE).is_some_and(|base| !base.is_empty()) {
        return Ok(());
    }
    let Some(home) = crate::home::account_dir() else {
        return Ok(());
    };
    let Ok(home) = crate::instance::setup::normalized(&home) else {
        return Ok(());
    };
    let base = org_platform::dirs::application_base(&home).ok();
    let wanted = [
        base.as_deref().and_then(Path::parent).map(Path::to_owned),
        org_platform::dirs::user_temp_base(&home).ok(),
    ];
    for folder in wanted.into_iter().flatten() {
        let Ok(folder) = crate::instance::setup::normalized(&folder) else {
            continue;
        };
        if folder.starts_with(&home) {
            store_directories::make(&folder, None, &home)
                .map_err(|refused| format!("preparing this account's folders: {refused}"))?;
        }
    }
    Ok(())
}

pub(crate) fn run_at(
    paths: &Paths,
    platform: &Platform,
    mut request: Request<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    // 1. Platform gate. A system whose service manager does not answer (a Linux machine or
    // container without a systemd user manager) is refused here, before anything is saved,
    // with the way to run without one.
    let pin = release_pin()?;
    service_gate(platform, &request)?;
    let name = request.name.as_deref();
    let shown = name.unwrap_or("default");
    say(out, format!("Setting up instance \"{shown}\"."))?;

    // 2. Lock and freeze the record. The address is decided before anything is written.
    let journal = Journal::open(&paths.base, name).map_err(|e| fail("opening setup", e))?;
    let configured = request
        .configuration
        .take()
        .map(|Configuration(input)| input);
    let mut record = match journal.read().map_err(|e| fail("reading setup", e))? {
        Some(record)
            if configured
                .as_ref()
                .is_some_and(|input| *input != record.input) =>
        {
            return Err(format!(
                "instance \"{shown}\" was set up with a different configuration (service \
                 mode, data folder, credential directory, address or certificate); its \
                 configuration does not change on a later run; nothing was changed"
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
                certificate: None,
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
                        org_platform_host::credentials::release_directory,
                    )?),
                    (None, CredentialDirectory::Below(base)) => Some(make_credential_directory(
                        base,
                        instance.id(),
                        instance.root_descriptor(),
                        Path::new("/"),
                        org_platform_host::credentials::release_directory,
                    )?),
                    (None, CredentialDirectory::Unused) => None,
                };
                (
                    instance.id().to_owned(),
                    instance.temp_folder().join("host.sock"),
                )
            };
            record.instance_id = Some(reserved.0.clone());
            journal
                .write(&record)
                .map_err(|e| fail("saving setup", e))?;
            reserved
        }
    };
    if let Some(credentials) = &record.credentials {
        say(out, format!("Secrets: {}", credentials.display()))?;
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
                 registration link, run: {}",
                reset_hint(name)
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
    show_link(out, result, name)
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

fn show_link(
    out: &mut dyn Write,
    result: AdminLinkResult,
    name: Option<&str>,
) -> Result<(), String> {
    match result {
        AdminLinkResult::Issued { url, expires_at_ms } => {
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
            say(out, format!("  {}", url.expose()))?;
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
    let stream = UnixStream::connect(socket).map_err(|e| fail("connecting to the service", e))?;
    crate::human::verify_host(&stream)?;
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
    let (id, temp) = registry
        .locate(name, &record.input.root)
        .map_err(|e| fail("locating the instance", e))?;
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
pub fn admin_reset(name: Option<&str>, terminal: bool, out: &mut dyn Write) -> Result<(), String> {
    if !terminal {
        // Spec §4a.4 copy. SIGN-34's SSH refusal ("shown before anything else") needs
        // SSH-session detection, which is not built yet; it goes before this check.
        return Err(NOT_A_TERMINAL.into());
    }
    let (_, id, socket) = registered(&crate::human::paths(name)?, name)?;
    let mut wire = connect(&socket, &id).map_err(|why| {
        format!(
            "{why}; the service is not running: start it with agentic-org service start{}",
            name.map(|n| format!(" --name {n}")).unwrap_or_default()
        )
    })?;
    let result = wire
        .call::<AdminReset>(&AdminResetParams::default(), None)
        .map_err(|e| fail("asking for a new registration link", e))?;
    if matches!(result, AdminLinkResult::AdministratorPresent) {
        return say(
            out,
            "The instance has an administrator; resetting its password is not available yet.",
        );
    }
    show_link(out, result, name)
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
