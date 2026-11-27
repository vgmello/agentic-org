//! # agentic-org-office: one process per office
//!
//! Started by the host on the launch protocol (`org-office-launch`):
//! ```text
//! agentic-org-office --create        # first stdin line: the create message; makes the office, serves it
//! agentic-org-office --office <id>   # first stdin line: the start message; serves the office
//! agentic-org-office --discard <id>  # first stdin line: the discard message (the key or null); removes an unfinished create
//! ```
//! One launch version: events travel on the authenticated rendezvous; its EOF closes the office
//! and exits 0. stdin is drained unparsed after the first line and is not a lifetime signal;
//! stdout carries nothing. Logs go to stderr.
//! The office, its folder, lock, key and store are `org_office::Office`; this
//! crate is the process around it: launch control, the host's writer custody, typed authority
//! and the socket. Every create and start takes the host's writer control first, before it
//! creates or opens anything. The server never touches a Keychain.

// No unsafe code: descriptors, signals and the immediate exit are `org_platform::posix`'s.
#![forbid(unsafe_code)]

mod attachment_calls;
mod bounded;
mod chat_calls;
mod chief_of_staff_port;
mod cost_calls;
mod directory;
mod doc_calls;
mod failure;
pub mod fdswap;
pub use org_federation as federation;
mod federation_calls;
#[cfg(test)]
pub(crate) use org_federation::testkit::federation_link;
mod federation_runtime;
pub(crate) use org_federation::stream as federation_stream;
mod follow;
mod git_calls;
mod grant;
mod hq_creation;
pub mod inbox;
mod inspect;
mod join;
mod launch;
mod live;
mod mailbox_store;
mod modes;
mod msg_calls;
mod office_ports;
mod office_runtime;
mod org_feed;
mod process_calls;
mod provider_calls;
mod provider_refresh;
mod revocations;
mod search_calls;
mod serve;
mod session_proxy;
mod signals;
#[cfg(test)]
pub(crate) use org_federation::testkit::snapshot_transfer;
mod startup;
mod task_calls;
mod tess_calls;
#[cfg(any(test, all(feature = "test-support", debug_assertions)))]
pub mod test_support;
mod transfer;
mod wake_bridge;
pub(crate) use org_channel_watcher as watcher;
#[cfg(test)]
pub(crate) use org_channel_watcher::store as watcher_store;
#[cfg(test)]
mod watcher_tests;
mod zeroed;

#[cfg(test)]
mod test_teardown;

use std::path::Path;
use std::sync::Arc;

use org_office_launch::{
    FailCode, CREATE_FLAG, DISCARD_FLAG, INSPECT_IMPORT_FLAG, JOIN_FLAG, OFFICE_FLAG,
};
use org_proto::location::{office_home_from, OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};

use crate::failure::fail;
use crate::inspect::inspect_import;
use crate::join::join_and_serve;
use crate::modes::{create_and_serve, discard, open_and_serve};
use fdswap::ControlPipe;
use startup::Control;

pub(crate) use crate::failure::logged;

/// A panic prints where it happened and nothing else: its message could carry a path or data.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let place = info
            .location()
            .map_or_else(String::new, |l| format!(" at {}:{}", l.file(), l.line()));
        eprintln!("agentic-org-office: internal error{place}");
    }));
}

/// The command line the host gives the server; `--help` and `--version` the executable
/// answers itself, before it touches a descriptor.
pub const USAGE: &str =
    "usage: agentic-org-office --create | --join | --office <id> | --discard <id> | --inspect-import";

/// What the server was asked to do.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Create,
    Join,
    Serve(String),
    Discard(String),
    InspectImport,
}

fn parse_args(args: &[String]) -> Result<Mode, &'static str> {
    match args {
        [flag] if flag == CREATE_FLAG => Ok(Mode::Create),
        [flag] if flag == JOIN_FLAG => Ok(Mode::Join),
        [flag, id] if flag == OFFICE_FLAG => Ok(Mode::Serve(id.clone())),
        [flag, id] if flag == DISCARD_FLAG => Ok(Mode::Discard(id.clone())),
        [flag] if flag == INSPECT_IMPORT_FLAG => Ok(Mode::InspectImport),
        _ => Err(USAGE),
    }
}

/// Runs the server with an empty (fail-closed) runtime registry and an explicit storage backend,
/// returning its exit code. Production composition roots (`apps/agentic-org-office`) MUST call
/// [`run_with_runtimes`] with explicitly constructed runtime and storage adapters.
pub fn run(
    args: &[String],
    control: ControlPipe,
    backend: Arc<dyn org_office::OfficeStorageBackend>,
) -> i32 {
    run_with_runtimes(
        args,
        control,
        org_office_sessions::RuntimeRegistry::empty(),
        backend,
    )
}

/// Runs the server with an explicit composition-root [`org_office_sessions::RuntimeRegistry`] and
/// [`org_office::OfficeStorageBackend`] and returns its exit code (`TASK-216`, `TASK-282`).
pub fn run_with_runtimes(
    args: &[String],
    control: ControlPipe,
    runtimes: org_office_sessions::RuntimeRegistry,
    backend: Arc<dyn org_office::OfficeStorageBackend>,
) -> i32 {
    if matches!(parse_args(args), Ok(Mode::InspectImport)) {
        return inspect_import(control, backend.as_ref());
    }
    // No child-spawn paths exist today. Any future path must share this receive/spawn fence.
    let mut control = Control::new(control, Arc::new(std::sync::Mutex::new(())));
    let mode = match parse_args(args) {
        Ok(mode) => mode,
        Err(usage) => return fail(&mut control, FailCode::InvalidRequest, usage),
    };
    let Some(home) = office_home(std::env::var_os(OFFICE_HOME_ENV), std::env::var_os("HOME"))
    else {
        return fail(&mut control, FailCode::InvalidRequest, "no office home");
    };
    let runtime = match runtime_dir_from(
        &home,
        std::env::var_os(OFFICE_RUNTIME_DIR_ENV),
        cfg!(debug_assertions),
    ) {
        Ok(runtime) => runtime,
        Err(message) => return fail(&mut control, FailCode::InvalidRequest, message),
    };
    match mode {
        Mode::Discard(id) => discard(control, &home, &id),
        Mode::Create => create_and_serve(control, &home, &runtime, runtimes, backend),
        Mode::Join => join_and_serve(control, &home, &runtime, runtimes, backend),
        Mode::Serve(id) => open_and_serve(control, &home, &runtime, &id, runtimes, backend),
        Mode::InspectImport => unreachable!(),
    }
}

/// The office home: `ORG_OFFICE_HOME` when it is an absolute path, else this system's
/// application base for the user whose home folder `HOME` names (`org_platform::dirs`), the
/// rule the host and every client use. `None` if neither gives one.
fn office_home(
    override_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    let base = home
        .map(std::path::PathBuf::from)
        .filter(|home| home.is_absolute())
        .and_then(|home| org_platform::dirs::application_base(&home).ok());
    office_home_from(override_home, base)
}

/// The runtime directory comes from the host's clean environment. No invalid override falls back.
fn runtime_dir_from(
    home: &Path,
    value: Option<std::ffi::OsString>,
    debug: bool,
) -> Result<std::path::PathBuf, &'static str> {
    match value {
        Some(value) if Path::new(&value).is_absolute() => Ok(value.into()),
        Some(_) => Err("office runtime directory must be absolute"),
        None if debug => Ok(home.join("run")),
        None => Err("release worker requires an explicit office runtime directory"),
    }
}

#[cfg(test)]
mod tests;
