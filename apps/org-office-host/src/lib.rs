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
//! `main.rs` only parses arguments, waits for a shutdown signal and calls [`Host`].

mod app_token;
mod catalog;
mod cloexec;
mod error;
mod home;
mod host_log;
mod ids;
mod logs;
mod offices;
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

use crate::catalog::Catalog;
use crate::home::{HostLock, Layout};
use crate::offices::Offices;
use crate::service::listener::{self, BindError, Listener, ListenerConfig};
use crate::service::Service;
use crate::supervisor::{Supervisor, SupervisorConfig};

pub use crate::error::HostError;
pub use crate::logs::DEFAULT_LOG_MAX_BYTES;
pub use crate::timings::Timings;

/// The office server's program name, looked for next to the host's own executable.
pub const SERVER_PROGRAM_NAME: &str = "org-office-server";

/// Requests one connection may have running at once.
const MAX_IN_FLIGHT_PER_CONNECTION: usize = 16;

/// How to run the host.
#[derive(Debug, Clone)]
pub struct Config {
    /// The office home (absolute). Normally `org_proto::location::default_office_home()`.
    pub home: PathBuf,
    /// The office server program (see [`default_server_program`]).
    pub server_program: PathBuf,
    pub timings: Timings,
    /// The size at which an office's stderr log is rotated.
    pub log_max_bytes: u64,
    /// Client connections beyond this are closed on arrival.
    pub max_connections: usize,
}

impl Config {
    /// The agreed defaults for this home and server program.
    pub fn new(home: impl Into<PathBuf>, server_program: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            server_program: server_program.into(),
            timings: Timings::default(),
            log_max_bytes: DEFAULT_LOG_MAX_BYTES,
            max_connections: 64,
        }
    }
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
    /// Starts the host: checks the socket paths fit, makes the office home owner-only, takes
    /// `host.lock`, binds `host.sock`, starts a server for every office in the catalog, has the
    /// unfinished office folders discarded in the background, and serves `host.sock` at once
    /// (only `organization.create` waits for the discards).
    pub fn start(config: Config) -> Result<Self, HostError> {
        if !config.home.is_absolute() {
            return Err(HostError::RelativeHome { path: config.home });
        }
        let layout = Layout::new(&config.home);
        layout.check_socket_paths()?;
        layout.prepare()?;
        let lock = HostLock::acquire(&layout)?;
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
        });
        let offices = Offices::start(
            &layout.home,
            catalog,
            unfinished,
            supervisor,
            &config.timings,
        );

        let listener = {
            let config = ListenerConfig {
                max_connections: config.max_connections,
                max_in_flight: MAX_IN_FLIGHT_PER_CONNECTION,
                expected_uid: home::euid(),
            };
            let service = Arc::new(Service::new(Arc::clone(&offices)));
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
