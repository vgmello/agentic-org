//! # org-office-host: the login service, as a library
//!
//! The one per-user login service on a Mac (see `docs/design/offices-hq-and-satellites.md`,
//! "Processes on one Mac", and the host/server boundary design). It:
//! - supervises one `org-office-server` per office folder on this Mac, restarting a crashed
//!   one with backoff (`supervisor`),
//! - creates organizations (an HQ office made by `org-office-server --create`), and
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

mod app_auth;
mod broadcast;
mod catalog;
mod clock;
mod cloexec;
mod error;
mod home;
mod host_log;
mod ids;
mod keys;
mod logs;
mod offices;
mod peer;
mod secrets;
mod service;
pub mod signals;
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
use crate::service::listener::{self, BindError, Listener, ListenerConfig};
use crate::service::Service;
use crate::supervisor::{Supervisor, SupervisorConfig};

pub use crate::app_auth::APP_BUNDLE_ID;
pub use crate::error::HostError;
pub use crate::logs::DEFAULT_LOG_MAX_BYTES;
pub use crate::timings::Timings;

/// The office server's program name, looked for next to the host's own executable.
pub const SERVER_PROGRAM_NAME: &str = "org-office-server";

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
    /// What this build trusts: the office home ([`trusted_office_home`]), the office server
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

/// The office home a release host serves: today `org_proto::location`'s rule
/// (`ORG_OFFICE_HOME`, else `$HOME/Library/Application Support/agentic.org`), the one the app
/// and `org-agent` use to find `host.sock`.
// TODO(trust inputs, pending a ruling): pin a release host to
// `home::account_office_home()` (from the account's `pw_dir`, ignoring `ORG_OFFICE_HOME`). It
// can't be done here alone: the app and `org-agent` find the host through the same
// `org_proto::location` rule, so they would look elsewhere whenever the variable (or `HOME`)
// differs. Until then the orphan-key sweep is off (`may_sweep_orphan_keys`).
fn trusted_office_home() -> Result<PathBuf, String> {
    org_proto::location::default_office_home().ok_or_else(|| {
        "no office home: neither ORG_OFFICE_HOME nor HOME is an absolute path".into()
    })
}

/// Whether this host may delete the keys of offices with no folder at startup
/// (`keys::sweep_orphans`). That is safe only when the key store belongs to this office home
/// alone, and today no build can be sure of it:
/// - a debug build's development key folder may be shared by several homes;
/// - a release build's Keychain group is shared by every release host of this account, and a
///   release host's home can still be moved by `ORG_OFFICE_HOME` or `HOME` (see
///   [`trusted_office_home`]): a host on one home would take another home's keys for orphans.
///
/// So the sweep is off (logged) until a release host is pinned to its account's office home
/// (`home::account_office_home`); then it turns on for a release host serving exactly that home.
fn may_sweep_orphan_keys(home: &Path) -> bool {
    // TODO(trust inputs, pending the office-home ruling): once `trusted_office_home` pins a
    // release host to `home::account_office_home()`, return true for a release build whose
    // `home` is that one.
    host_log::log(&format!(
        "no orphan-key sweep for {}: {}",
        home.display(),
        if cfg!(debug_assertions) {
            "a debug build's development key folder may be shared by several homes"
        } else {
            "another release host on another office home may share this Keychain group"
        }
    ));
    false
}

/// `org-office-server` next to this executable (both live in the app bundle).
pub fn default_server_program() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
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
        let keys = keys::store_for(&config, app_auth::own_keychain_access_groups)?;
        let layout = Layout::new(&config.home);
        layout.check_socket_paths()?;
        layout.prepare()?;
        let lock = HostLock::acquire(&layout)?;
        // Decided only by the host that holds the home.
        let sweep_orphans = may_sweep_orphan_keys(&layout.home);
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
