//! Service lifecycle: who starts, restarts and stops the host.
//!
//! ADR-0017 contract 4; Linux host design section 3.4. `agentic-org serve` is the same
//! foreground process in every mode. This contract is only about who runs it:
//!
//! - [`ServiceMode::Registered`]: the user's service manager. macOS: a per-user launchd
//!   agent. Linux: a systemd user unit.
//! - [`ServiceMode::Foreground`]: nothing is registered, because something else
//!   supervises the process (a container runtime). Setup is told the mode; nothing
//!   detects a container.
//!
//! The consumer (`org-host` setup and `serve`) decides which instance, program,
//! arguments and environment make a unit, and passes them as plain data.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use crate::Refused;

mod refused;

/// The longest instance id a unit may carry. Adapters put the id in a file or label
/// name, so it is short and has no separator.
pub const MAX_INSTANCE_ID_BYTES: usize = 64;

/// Who supervises `agentic-org serve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceMode {
    /// The user's service manager starts the instance at login and restarts it.
    Registered,
    /// The process that started `serve` supervises it; nothing is registered.
    Foreground,
}

/// One instance's service: the program the manager runs, with its arguments and
/// environment. The manager derives its own unit or label name from `instance_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The instance this unit serves. See [`check_instance_id`].
    pub instance_id: String,
    /// The absolute path of the pinned `agentic-org` executable.
    pub program: PathBuf,
    /// Arguments after the program name, for example `serve --instance <id>`.
    pub args: Vec<OsString>,
    /// Environment the manager sets for the program, beyond its own defaults.
    pub env: Vec<(OsString, OsString)>,
}

impl Unit {
    /// The rules every adapter applies before it writes anything: a valid instance
    /// id, an absolute program path, no NUL or line break anywhere, well-formed
    /// environment names, and no variable that changes how the loader runs the image
    /// (any `LD_` or `DYLD_` name, and `GLIBC_TUNABLES`).
    pub fn check(&self) -> Result<(), ServiceError> {
        check_instance_id(&self.instance_id)?;
        if !self.program.is_absolute() {
            return Err(invalid("the program path is not absolute"));
        }
        plain(
            self.program.as_os_str(),
            "the program path has a NUL or a line break",
        )?;
        for arg in &self.args {
            plain(arg, "an argument has a NUL or a line break")?;
        }
        for (name, value) in &self.env {
            let bytes = name.as_bytes();
            if bytes.is_empty() || bytes.contains(&b'=') {
                return Err(invalid("an environment name is empty or contains '='"));
            }
            plain(name, "an environment name has a NUL or a line break")?;
            plain(value, "an environment value has a NUL or a line break")?;
            if loader_variable(name) {
                return Err(invalid("the environment sets a loader variable"));
            }
        }
        Ok(())
    }
}

/// An instance id an adapter can put in a unit or label name: 1 to
/// [`MAX_INSTANCE_ID_BYTES`] bytes of ASCII letters, digits, `-` and `_`.
pub fn check_instance_id(instance_id: &str) -> Result<(), ServiceError> {
    let valid = !instance_id.is_empty()
        && instance_id.len() <= MAX_INSTANCE_ID_BYTES
        && instance_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if valid {
        Ok(())
    } else {
        Err(invalid(
            "the instance id is not 1 to 64 ASCII letters, digits, '-' or '_'",
        ))
    }
}

fn invalid(reason: &'static str) -> ServiceError {
    ServiceError::InvalidUnit { reason }
}

/// Unit files are line-oriented on both systems: no NUL, line feed or carriage return.
fn plain(value: &OsStr, reason: &'static str) -> Result<(), ServiceError> {
    if value
        .as_bytes()
        .iter()
        .any(|b| matches!(b, 0 | b'\n' | b'\r'))
    {
        return Err(invalid(reason));
    }
    Ok(())
}

/// Whether `name` changes how the dynamic loader runs an image: any name beginning
/// `LD_` or `DYLD_`, and `GLIBC_TUNABLES` (Linux host design section 3.1). Such a
/// variable is refused in a unit's environment on every system.
pub(crate) fn loader_variable(name: &OsStr) -> bool {
    let name = name.as_bytes();
    name.starts_with(b"LD_") || name.starts_with(b"DYLD_") || name == b"GLIBC_TUNABLES"
}

/// Whether the manager is running the instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// The manager has no registration for the instance.
    NotRegistered,
    /// Registered and not running.
    Stopped,
    /// Registered and running.
    Running,
}

/// When a registered instance runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// The manager runs the instance whether or not the user is logged in.
    Always,
    /// The manager runs the instance only while the user has a login session.
    /// `remedy` is the command that lifts the limit when the system has one (Linux:
    /// `loginctl enable-linger <user>`). Setup prints it and never runs it unasked.
    WhileLoggedIn { remedy: Option<String> },
}

/// What [`ServiceManager::status`] reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatus {
    pub state: ServiceState,
    pub availability: Availability,
}

/// Why a service operation failed. No variant carries a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceError {
    /// The unit or instance id breaks a rule of [`Unit::check`].
    InvalidUnit { reason: &'static str },
    /// `start` or `stop` named an instance the manager has no registration for.
    NotRegistered,
    /// `register` found a registration under this instance's name that differs from
    /// the unit. Nothing was changed: the manager never takes over another unit.
    Conflict,
    /// The service manager itself failed or could not be reached. `action` is the
    /// step (for example `daemon-reload`), `detail` the manager's own message.
    Manager {
        action: &'static str,
        detail: String,
    },
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUnit { reason } => write!(f, "invalid service unit: {reason}"),
            Self::NotRegistered => f.write_str("the instance has no registered service"),
            Self::Conflict => {
                f.write_str("a different service is already registered under this instance's name")
            }
            Self::Manager { action, detail } => {
                write!(f, "the service manager failed at {action}: {detail}")
            }
        }
    }
}

impl std::error::Error for ServiceError {}

/// One user's service manager, for the instances of that user.
///
/// Every method takes effect before it returns or fails; none leaves a partly
/// written registration behind.
pub trait ServiceManager: Send + Sync {
    /// Registers `unit` so the manager starts it at login and restarts it on failure.
    /// It does not start it now; call [`ServiceManager::start`]. Idempotent: the same
    /// unit again succeeds and changes nothing. A different registration already
    /// under this instance's name is [`ServiceError::Conflict`]; to replace a unit,
    /// `unregister` it first.
    fn register(&self, unit: &Unit) -> Result<(), ServiceError>;
    /// Starts the registered instance. Succeeds when it is already running.
    fn start(&self, instance_id: &str) -> Result<(), ServiceError>;
    /// Stops the registered instance and waits for the manager to report it stopped.
    /// Succeeds when it is already stopped.
    fn stop(&self, instance_id: &str) -> Result<(), ServiceError>;
    /// Whether the instance is registered and running, and when it runs. An instance
    /// without a registration is [`ServiceState::NotRegistered`], not an error.
    fn status(&self, instance_id: &str) -> Result<ServiceStatus, ServiceError>;
    /// Stops the instance and removes its registration. Succeeds when there is none.
    fn unregister(&self, instance_id: &str) -> Result<(), ServiceError>;
}

/// The service manager for `mode` on the build target, or the refusal.
///
/// Today every target refuses. The macOS adapter (a per-user launchd agent,
/// increment A2) and the Linux adapter (a systemd user unit and the foreground mode,
/// increment X5) are selected here, by build target, when they are written.
pub fn service(mode: ServiceMode) -> Result<Box<dyn ServiceManager>, Refused> {
    refused::service(mode)
}

#[cfg(test)]
mod tests;
