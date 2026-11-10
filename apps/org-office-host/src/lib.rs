//! # org-office-host: the login service, as a library
//!
//! The one per-user login service on a Mac (see `docs/design/offices-hq-and-satellites.md`,
//! "Processes on one Mac", and the host/server boundary design). It:
//! - supervises one `org-server` per office folder on this Mac, restarting a crashed
//!   one with backoff (`supervisor`),
//! - creates organizations (an HQ office made by `org-server --create`), and
//! - tells the app and `org-agent` which offices run here and how to reach them, over
//!   `org-proto` on `<home>/host.sock` (`service`).
//!
//! It holds no organization data: the catalog is the folders under `<home>/offices/` that hold
//! a valid `office.json` (`catalog`). It talks to its servers only through
//! `org-office-launch`, and never depends on `org-office` (no SQLCipher in the login service).
//!
//! It holds every office's database key (`keys`): it creates each one, keeps it in its key store
//! (the Data Protection keychain in release builds, a development folder in debug builds) and
//! hands it to the office's server on the launch pipe. Its servers never touch a key store.
//!
//! `main.rs` only parses arguments, waits for a shutdown signal and calls [`Host`].

// A release-profile build (or any build for the app bundle, `ORG_BUNDLE=1`) with debug
// assertions on would ship every development path (see build.rs): refuse it, whatever turned
// them on (the profile, `-C debug-assertions`, `-C opt-level=0`). rustdoc compiles with debug
// assertions on whatever the profile, so documentation and doctests are exempt: they ship nothing.
// (`doc` and `doctest` are set only by rustdoc; passing `--cfg doc` to rustc on purpose would
// bypass this, which takes intent, not a mistake.)
#[cfg(all(
    any(release_profile, bundle_build),
    debug_assertions,
    not(doc),
    not(doctest)
))]
compile_error!(
    "org-office-host: refusing a release or app-bundle build with debug assertions on: it would \
     ship the host's development paths (dev keys, no server signature check, the unsigned \
     development app, the trust-changing arguments). Build it without debug assertions."
);

mod app_auth;
mod broadcast;
mod catalog;
mod clock;
mod cloexec;
mod code_signing;
mod error;
mod home;
mod host_log;
mod ids;
mod keys;
mod logs;
mod offices;
mod peer;
mod secrets;
mod server_check;
mod service;
pub mod signals;
mod spawn;
mod supervisor;
mod sync;
mod timings;

use std::fs;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app_auth::AppPolicy;
use crate::broadcast::Broadcast;
use crate::catalog::Catalog;
use crate::home::{HostLock, Layout};
use crate::offices::Offices;
use crate::server_check::ServerCheck;
use crate::service::listener::{self, BindError, Listener, ListenerConfig};
use crate::service::Service;
use crate::supervisor::{Supervisor, SupervisorConfig};

pub use crate::app_auth::APP_BUNDLE_ID;
pub use crate::server_check::SERVER_SIGNING_ID;

/// The host's own signing identifier (ADR-0003 §2.1), for the bundle's signing of
/// `Contents/MacOS/org-office-host`.
pub const HOST_SIGNING_ID: &str = "com.ellosoft.agentic-org.org-office-host";
pub use crate::error::HostError;
pub use crate::logs::DEFAULT_LOG_MAX_BYTES;
pub use crate::timings::Timings;

/// The office server's program name, looked for next to the host's own executable.
pub const SERVER_PROGRAM_NAME: &str = "org-server";

/// Requests one connection may have running at once.
const MAX_IN_FLIGHT_PER_CONNECTION: usize = 16;

/// The team id that signs the desktop app, fixed when the host is built (`ORG_APP_TEAM_ID`).
/// Without it no signed app can use the app-only calls (logged loudly at startup).
pub const APP_TEAM_ID: Option<&str> = option_env!("ORG_APP_TEAM_ID");

/// How to run the host.
///
/// A release build trusts only what it was built with: the app's bundle id
/// ([`APP_BUNDLE_ID`]) and team id ([`APP_TEAM_ID`]), its Keychain access group, and the office
/// server next to its own executable. So the trust inputs can't be set from outside: the only
/// release constructor is [`Config::trusted`], and the setters that change them
/// ([`Config::new`], [`Config::set_server_program`], [`Config::set_app_team_id`] and the
/// development fields) exist in debug builds only. The timings and limits stay settable.
#[derive(Debug, Clone)]
pub struct Config {
    /// The office home (absolute).
    home: PathBuf,
    /// The office server program (see [`default_server_program`]).
    server_program: PathBuf,
    pub timings: Timings,
    /// The size at which an office's stderr log is rotated.
    pub log_max_bytes: u64,
    /// Client connections beyond this are closed on arrival.
    pub max_connections: usize,
    /// The team id that signs the desktop app. Required for the app-only calls: with none,
    /// every caller is refused (`not_authorized`).
    app_team_id: Option<String>,
    /// Debug builds only (release builds don't have the field): an unsigned development app
    /// (this exact executable, `Contents/MacOS/<exe>`, run by this user) that the app-only calls
    /// accept without a code signature. The binary sets it from the build-time constant
    /// `ORG_DEV_APP_PATH`, never from run-time input; tests set it directly. If the path doesn't
    /// exist when the host starts (the app isn't built yet) or isn't an executable file, that's
    /// logged and the fallback stays off. Every use is logged.
    #[cfg(debug_assertions)]
    pub dev_app_path: Option<PathBuf>,
    /// Debug builds only (release builds don't have the field): the folder (absolute) where the
    /// host keeps the offices' database keys, as plain files, instead of the Keychain. A debug
    /// host without it refuses to start ([`HostError::NoDevKeyDir`]), so it never writes a real
    /// Keychain item. The binary takes it from `ORG_OFFICE_DEV_KEY_DIR`; tests set it directly.
    /// It must not be inside the office home, and two homes must not share one.
    #[cfg(debug_assertions)]
    pub dev_key_dir: Option<PathBuf>,
}

impl Config {
    /// What this build trusts: the office home (`trusted_office_home`), the office server
    /// next to this executable, and the team id it was built with. The only way to make a
    /// release config.
    pub fn trusted() -> Result<Self, String> {
        let home = trusted_office_home()?;
        let server =
            default_server_program().map_err(|e| format!("finding the office server: {e}"))?;
        Ok(Self::with(home, server, APP_TEAM_ID))
    }

    /// Debug builds only: the agreed defaults for this home and server program, with no app
    /// team id.
    #[cfg(debug_assertions)]
    pub fn new(home: impl Into<PathBuf>, server_program: impl Into<PathBuf>) -> Self {
        Self::with(home.into(), server_program.into(), None)
    }

    fn with(home: PathBuf, server_program: PathBuf, app_team_id: Option<&str>) -> Self {
        Self {
            home,
            server_program,
            timings: Timings::default(),
            log_max_bytes: DEFAULT_LOG_MAX_BYTES,
            max_connections: 64,
            app_team_id: app_team_id.map(str::to_owned),
            #[cfg(debug_assertions)]
            dev_app_path: None,
            #[cfg(debug_assertions)]
            dev_key_dir: None,
        }
    }

    /// Debug builds only: another office server program.
    #[cfg(debug_assertions)]
    pub fn set_server_program(&mut self, program: impl Into<PathBuf>) {
        self.server_program = program.into();
    }

    /// Debug builds only: another app team id (or none).
    #[cfg(debug_assertions)]
    pub fn set_app_team_id(&mut self, team_id: Option<String>) {
        self.app_team_id = team_id;
    }

    /// The office home.
    pub fn home(&self) -> &Path {
        &self.home
    }
}

/// The office home a trusted (release) host serves: its account's own office home,
/// `<pw_dir>/Library/Application Support/agentic.org`, and only when `org_proto::location`'s
/// shared rule (`ORG_OFFICE_HOME`, else `$HOME/…`), the one the app and `org-agent` use to find
/// `host.sock`, gives that same home in this environment ([`home::pinned_office_home`]).
/// Otherwise the host doesn't start, saying why: its home can't be moved by its environment,
/// and it never serves a home its clients don't look in.
fn trusted_office_home() -> Result<PathBuf, String> {
    let home_env = std::env::var_os("HOME").map(PathBuf::from);
    home::pinned_office_home(
        org_proto::location::default_office_home().as_deref(),
        home_env.as_deref(),
        home::account_dir().as_deref(),
        home::same_path,
    )
}

/// Whether this host may sweep the keys of offices with no folder at startup
/// (`keys::sweep_orphans`, which deletes one only once it has stayed orphaned for 7 days). That
/// is safe only when the key store belongs to this office home alone:
/// - a release build: yes. Its Keychain group is shared by every release host of this account,
///   but every release host serves the account's one office home ([`trusted_office_home`]), so
///   no host can take another home's keys for orphans. Checked again here: a release host on
///   any other home doesn't sweep;
/// - a debug build: never, since its development key folder may be shared by several homes.
///
/// Logged either way.
fn may_sweep_orphan_keys(home: &Path) -> bool {
    let reason = sweep_refusal(
        cfg!(debug_assertions),
        home::account_office_home().as_deref(),
        home,
        home::same_path,
    );
    match reason {
        Some(reason) => {
            host_log::log(&format!(
                "no orphan-key sweep for {}: {reason}",
                home.display()
            ));
            false
        }
        None => {
            host_log::log(&format!(
                "orphan-key sweep on for {}: a key whose office has no folder is marked, and \
                 deleted once it has stayed so for 7 days",
                home.display()
            ));
            true
        }
    }
}

/// Why a host may not sweep orphan keys (see [`may_sweep_orphan_keys`]), or `None` if it may: a
/// release build (`debug` false) serving its account's own office home `own`. Pure: `same`
/// compares paths.
fn sweep_refusal(
    debug: bool,
    own: Option<&Path>,
    home: &Path,
    same: impl Fn(&Path, &Path) -> bool,
) -> Option<&'static str> {
    if debug {
        Some("a debug build's development key folder may be shared by several homes")
    } else if own.is_some_and(|own| same(own, home)) {
        None
    } else {
        Some("it isn't this account's office home")
    }
}

/// `org-server` next to this executable (both live in the app bundle), found through the
/// executable's canonical path (symlinks resolved). Defence in depth only: a hard link or a
/// copied bundle still puts another server next to the host, which is why a release host checks
/// every server's code signature before it gives it a key (`server_check`).
pub fn default_server_program() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let dir = exe
        .parent()
        .ok_or_else(|| io::Error::other("the host's executable has no folder"))?;
    Ok(dir.join(SERVER_PROGRAM_NAME))
}

/// A running host. [`Host::shutdown`] (or dropping it) stops it.
pub struct Host {
    layout: Layout,
    offices: Arc<Offices>,
    listener: Option<Listener>,
    lock: Option<HostLock>,
}

impl Host {
    /// Starts the host: chooses its key store, checks the socket paths fit, makes the office
    /// home owner-only, takes `host.lock`, binds `host.sock`, starts a server for every office in
    /// the catalog, has the unfinished office folders discarded in the background, and serves
    /// `host.sock` at once (only `organization.create` waits for the discards).
    pub fn start(config: Config) -> Result<Self, HostError> {
        if !config.home.is_absolute() {
            return Err(HostError::RelativeHome { path: config.home });
        }
        let app = AppPolicy::new(APP_BUNDLE_ID, config.app_team_id.as_deref());
        #[cfg(debug_assertions)]
        let app = app.and_then(|app| match &config.dev_app_path {
            Some(path) => app.with_dev_app(path),
            None => Ok(app),
        });
        let app = app.map_err(|why| HostError::InvalidAppIdentity { why })?;
        for line in app.describe() {
            host_log::log(&line);
        }
        // Before anything is made on disk or started: no office runs without its key store.
        #[cfg(debug_assertions)]
        let dev_key_dir = config.dev_key_dir.as_deref();
        #[cfg(not(debug_assertions))]
        let dev_key_dir = None;
        let keys =
            keys::store_for(dev_key_dir, &config.home, code_signing::own_signing).map_err(|e| {
                match e {
                    keys::StoreError::NoDevKeyDir => HostError::NoDevKeyDir,
                    keys::StoreError::NoKeychainGroup => HostError::NoKeychainGroup,
                    keys::StoreError::KeychainGroupNotEntitled { group } => {
                        HostError::KeychainGroupNotEntitled { group }
                    }
                    keys::StoreError::NotHardened => HostError::NotHardened,
                }
            })?;
        let layout = Layout::new(&config.home);
        layout.check_socket_paths()?;
        layout.prepare()?;
        let lock = HostLock::acquire(&layout)?;
        // Decided only by the host that holds the home.
        let sweep_orphans = may_sweep_orphan_keys(&layout.home);
        let server_check =
            ServerCheck::for_build(cfg!(debug_assertions), config.app_team_id.as_deref())
                .map_err(|why| HostError::InvalidAppIdentity { why })?;
        host_log::log(&server_check.describe());
        let io = |context| move |source| HostError::Io { context, source };
        let catalog = Catalog::scan(&layout.home).map_err(io("reading the office folders"))?;
        let unfinished =
            catalog::unfinished_folders(&layout.home).map_err(io("reading the office folders"))?;
        // Bound first, so nothing has started when binding fails; clients that connect before
        // the listener starts (moments later) wait in the backlog.
        let bound = listener::bind(&layout.host_socket).map_err(|e| match e {
            BindError::NotASocket => HostError::SocketInTheWay {
                path: layout.host_socket.clone(),
            },
            BindError::Io(source) => HostError::Io {
                context: "binding host.sock",
                source,
            },
        })?;

        let supervisor = Supervisor::new(SupervisorConfig {
            program: config.server_program.clone(),
            home: layout.home.clone(),
            logs_dir: layout.logs.clone(),
            log_max_bytes: config.log_max_bytes,
            timings: config.timings.clone(),
            keys,
            server_check: Arc::new(server_check),
            sweep_orphans,
        });
        let broadcast = Arc::new(Broadcast::default());
        let offices = Offices::start(
            &layout.home,
            catalog,
            unfinished,
            supervisor,
            Arc::clone(&broadcast),
            &config.timings,
        );

        let listener = {
            let config = ListenerConfig {
                max_connections: config.max_connections,
                max_in_flight: MAX_IN_FLIGHT_PER_CONNECTION,
                expected_uid: home::euid(),
            };
            let service = Arc::new(Service::new(Arc::clone(&offices), broadcast, app));
            Listener::start(bound, config, service).map_err(io("starting the listener"))
        };
        let listener = match listener {
            Ok(listener) => listener,
            Err(e) => {
                offices.shutdown();
                return Err(e);
            }
        };
        Ok(Self {
            layout,
            offices,
            listener: Some(listener),
            lock: Some(lock),
        })
    }

    /// The office home this host serves.
    pub fn home(&self) -> &Path {
        &self.layout.home
    }

    /// `host.sock`, where clients connect.
    pub fn socket_path(&self) -> &Path {
        &self.layout.host_socket
    }

    /// Stops accepting, stops every office server (stdin closed, then SIGKILL after the grace
    /// period), removes `host.sock` and releases `host.lock`.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let Some(mut listener) = self.listener.take() else {
            return;
        };
        listener.close();
        // Before joining the listener: a create still running answers as soon as the
        // supervisor stops its server.
        self.offices.shutdown();
        listener.join();
        if let Ok(meta) = fs::symlink_metadata(&self.layout.host_socket) {
            if meta.file_type().is_socket() {
                let _ = fs::remove_file(&self.layout.host_socket);
            }
        }
        self.lock.take();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_release_host_on_its_accounts_office_home_sweeps() {
        let lexical = |a: &Path, b: &Path| a == b;
        let own = Path::new("/Users/me/Library/Application Support/agentic.org");
        assert_eq!(sweep_refusal(false, Some(own), own, lexical), None);
        assert!(sweep_refusal(true, Some(own), own, lexical).is_some());
        assert!(sweep_refusal(false, Some(own), Path::new("/tmp/h"), lexical).is_some());
        assert!(sweep_refusal(false, None, own, lexical).is_some());
    }
}
