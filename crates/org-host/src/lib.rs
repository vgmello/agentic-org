//! # agentic-org: the login service, as a library
//!
//! The one per-user login service on a Mac (see `docs/design/offices-hq-and-satellites.md`,
//! "Processes on one Mac", and the host/server boundary design). It:
//! - supervises one `agentic-org-office` per office folder on this Mac, restarting a crashed
//!   one with backoff (`supervisor`),
//! - maintains the native office catalog and lifecycle, and
//! - serves registered-instance hello and count-only status over `org-host-control` on
//!   `<home>/host.sock` (`service`). Retired session-less operations are refused.
//!
//! Organization content stays in office databases: the catalog is the folders under `<home>/offices/` that hold
//! a valid `office.json` (`catalog`). It talks to its servers only through
//! `org-office-launch`, and never depends on `org-office`. The private `hostdb` stores the
//! host's registry in plain SQLite; organization databases remain in office servers.
//!
//! It holds every office's secrets key (`keys`): it creates each one, keeps it in its key store
//! (in release builds the system's protected store: the Data Protection keychain on macOS, the
//! credential directory setup recorded on Linux; a development folder in debug builds) and
//! hands it to the office's server on the launch pipe. Its servers never touch a key store.
//!
//! The agentic-org serve composition parses arguments, waits for shutdown and calls [`Host`].

// No unsafe code: the system calls are `org-platform`'s (`posix` and the capabilities).
#![forbid(unsafe_code)]

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
    "agentic-org: refusing a release or app-bundle build with debug assertions on: it would \
     ship the host's development paths (dev keys, no server signature check, the unsigned \
     development app, the trust-changing arguments). Build it without debug assertions."
);

mod accounts;
mod admin;
mod app_auth;
mod authentication;
mod broadcast;
mod browser_listener;
mod catalog;
mod cli_error;
mod clock;
mod cloexec;
mod code_signing;
mod config;
mod control_listener;
mod credential_store_root;
mod error;
mod home;
mod host;
mod host_log;
mod host_secrets;
mod hostdb;
pub mod human;
mod ids;
mod instance;
mod keys;
mod links;
mod logs;
mod offices;
mod peer;
pub mod ports;
mod presence;
mod provisioning;
mod recovery;
mod recovery_permission;
mod registered;
pub mod release;
mod secrets;
mod server_check;
mod service;
mod sessions;
pub mod setup_flow;
pub mod signals;
mod startup;
pub mod stop;
mod supervisor;
mod sync;
mod timings;
mod web;
mod web_tls;
mod writer;

#[cfg(test)]
mod test_programs;
// Real-process tests (debug builds only).
#[cfg(all(test, debug_assertions))]
mod enc1_process_tests;
#[cfg(all(test, debug_assertions))]
mod https_process_tests;
#[cfg(all(test, debug_assertions))]
mod web_process_tests;

pub use config::{
    default_office_home, default_release_store_dir, default_server_program, harden_process, Config,
};
#[cfg(all(feature = "test-support", debug_assertions))]
pub use host::start_instance_for_test;
pub use host::Host;

#[cfg(all(feature = "test-support", debug_assertions))]
pub mod test_support;

pub use crate::app_auth::APP_BUNDLE_ID;
pub use crate::server_check::OFFICE_SIGNING_ID;
pub use crate::server_check::WEB_SIGNING_ID;

/// The host's own signing identifier (ADR-0003 §2.1), for the bundle's signing of
/// the agentic-org executable in host mode. The signing identifier remains stable
/// across the executable migration; agent-os must never receive this entitlement.
pub const HOST_SIGNING_ID: &str = "com.vgmello.agentic-org.org-office-host";
pub use crate::error::HostError;
pub use crate::logs::DEFAULT_LOG_MAX_BYTES;
pub use crate::timings::Timings;
pub use crate::web::BrowserListener;

/// The office server's program name, looked for next to the host's own executable.
pub const SERVER_PROGRAM_NAME: &str = "agentic-org-office";

/// Requests one connection may have running at once.
const MAX_IN_FLIGHT_PER_CONNECTION: usize = 16;

/// How long past the servers' grace period a stopping host waits for its own threads: the
/// requests still running on its connections and the web process's supervision. A request
/// that waits for a create ends when its server is stopped; one still waiting after this
/// (for a write lock of the registry, say) is left, its answer dropped, and ends with the
/// process. With the grace period it bounds the stop (`Timings::shutdown_grace`).
const THREAD_END_MARGIN: std::time::Duration = std::time::Duration::from_millis(500);

/// The team id that signs the desktop app, fixed when the host is built (`ORG_APP_TEAM_ID`).
/// Without it no signed app can use the app-only calls (logged loudly at startup).
pub const APP_TEAM_ID: Option<&str> = option_env!("ORG_APP_TEAM_ID");

#[cfg(test)]
mod tests;
