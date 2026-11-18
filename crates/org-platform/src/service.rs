//! Service lifecycle: who starts, restarts and stops the host.
//!
//! ADR-0017 contract 4; Linux host design section 3.4. `agentic-org serve` is the same
//! foreground process in every mode. This contract is only about who runs it:
//!
//! - [`ServiceMode::Registered`]: the user's service manager. macOS: a per-user launchd
//!   agent. Linux: a systemd user unit.
//! - [`ServiceMode::Foreground`]: nothing is registered, because something else
//!   supervises the process (a container runtime, a developer's terminal). Setup is
//!   told the mode; nothing detects a container.
//!
//! The consumer (`org-host` setup and `serve`) decides which instance, program,
//! arguments and environment make a unit, and passes them as plain data. An adapter
//! only ever receives a checked [`InstanceId`] and a checked [`Unit`].
//!
//! # The rules every adapter keeps
//!
//! A *registration* is the record, under a name the adapter derives from the instance
//! id, that makes the user's service manager start the unit at each login of that
//! user and restart it when it fails. An adapter marks the registrations it writes
//! and recognises them by that mark. Anything else it finds under the name is
//! *foreign*: every method returns [`ServiceError::Foreign`] and changes nothing.
//!
//! In [`ServiceMode::Registered`]:
//!
//! - `register` writes the registration and does not start the instance. The same
//!   unit again succeeds, changes nothing and does not disturb a running instance. A
//!   registration of this product for the same instance with a different unit is
//!   [`ServiceError::Conflict`] and nothing changes; a unit is replaced by
//!   `unregister`, then `register`.
//! - `start` makes the manager run the instance now and succeeds when it already
//!   runs. Without a registration it is [`ServiceError::NotRegistered`].
//! - `stop` stops the instance and returns once the manager reports it stopped. The
//!   manager then leaves it stopped until `start` or the user's next login. It
//!   succeeds when the instance is already stopped. Without a registration it is
//!   [`ServiceError::NotRegistered`].
//! - `status` reports [`ServiceStatus::NotRegistered`], or
//!   [`ServiceStatus::Registered`] with whether the manager has the instance running
//!   (started and not seen to exit; an instance that failed, or that waits to be
//!   restarted, is not running) and when it runs.
//! - `unregister` stops the instance if it runs and removes the registration, so the
//!   manager never starts it again. It succeeds when there is none.
//!
//! In [`ServiceMode::Foreground`] there is nothing to register: `register` and
//! `unregister` succeed without effect, `status` reports
//! [`ServiceStatus::Unmanaged`], and `start` and `stop` are
//! [`ServiceError::Unmanaged`], because only whoever started `serve` can start or stop
//! it.
//!
//! Every method takes effect before it returns or fails; none leaves a partly written
//! registration behind. [`testing`] holds these rules as executable checks.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::Refused;

adapters!();

#[cfg(any(test, feature = "test-support"))]
pub mod testing;

/// The longest instance id. Adapters put the id in a file or label name.
pub const MAX_INSTANCE_ID_BYTES: usize = 64;

/// Who supervises `agentic-org serve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceMode {
    /// The user's service manager starts the instance at login and restarts it.
    Registered,
    /// The process that started `serve` supervises it; nothing is registered.
    Foreground,
}

/// An instance id an adapter can put in a unit or label name: 1 to
/// [`MAX_INSTANCE_ID_BYTES`] bytes of ASCII letters, digits, `-` and `_`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstanceId(String);

impl InstanceId {
    /// Checks `id` and wraps it.
    pub fn new(id: &str) -> Result<Self, ServiceError> {
        let valid = !id.is_empty()
            && id.len() <= MAX_INSTANCE_ID_BYTES
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if valid {
            Ok(Self(id.to_owned()))
        } else {
            Err(invalid(
                "the instance id is not 1 to 64 ASCII letters, digits, '-' or '_'",
            ))
        }
    }

    /// The id as given.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One instance's service: the program the manager runs, with its arguments and
/// environment. It can only be built through [`Unit::new`], so every unit an adapter
/// sees has passed the checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    instance: InstanceId,
    program: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

impl Unit {
    /// A unit for `instance` that runs `program` with `args` and, beyond the
    /// manager's own defaults, the variables in `env`.
    ///
    /// Refused, because a launchd property list and a systemd unit file must both be
    /// able to hold the unit exactly:
    ///
    /// - a program path that is not absolute or not UTF-8;
    /// - a control character (which includes NUL and line breaks) in the program
    ///   path, an argument or an environment value;
    /// - an environment name that is not an ASCII letter or `_` followed by ASCII
    ///   letters, digits or `_`, or that appears twice;
    /// - a variable that changes how the loader runs the image: any `LD_` or `DYLD_`
    ///   name, and `GLIBC_TUNABLES` (the rule `ChildEnv` applies to children).
    pub fn new(
        instance: InstanceId,
        program: PathBuf,
        args: Vec<String>,
        env: Vec<(String, String)>,
    ) -> Result<Self, ServiceError> {
        if !program.is_absolute() {
            return Err(invalid("the program path is not absolute"));
        }
        let Some(program_text) = program.to_str() else {
            return Err(invalid("the program path is not UTF-8"));
        };
        printable(program_text, "the program path has a control character")?;
        for arg in &args {
            printable(arg, "an argument has a control character")?;
        }
        for (index, (name, value)) in env.iter().enumerate() {
            let mut bytes = name.bytes();
            let well_formed = bytes
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_');
            if !well_formed {
                return Err(invalid(
                    "an environment name is not a letter or '_' followed by letters, digits or '_'",
                ));
            }
            if env[..index].iter().any(|(earlier, _)| earlier == name) {
                return Err(invalid("an environment name appears twice"));
            }
            if loader_variable(name.as_bytes()) {
                return Err(invalid("the environment sets a loader variable"));
            }
            printable(value, "an environment value has a control character")?;
        }
        Ok(Self {
            instance,
            program,
            args,
            env,
        })
    }

    /// The instance this unit serves.
    pub fn instance(&self) -> &InstanceId {
        &self.instance
    }

    /// The absolute path of the pinned `agentic-org` executable.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Arguments after the program name, for example `serve --instance <id>`.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Environment the manager sets for the program, beyond its own defaults.
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }
}

fn invalid(reason: &'static str) -> ServiceError {
    ServiceError::InvalidUnit { reason }
}

fn printable(value: &str, reason: &'static str) -> Result<(), ServiceError> {
    if value.chars().any(char::is_control) {
        return Err(invalid(reason));
    }
    Ok(())
}

/// Whether `name` changes how the dynamic loader runs an image: any name beginning
/// `LD_` or `DYLD_`, and `GLIBC_TUNABLES` (Linux host design section 3.1).
pub(crate) fn loader_variable(name: &[u8]) -> bool {
    name.starts_with(b"LD_") || name.starts_with(b"DYLD_") || name == b"GLIBC_TUNABLES"
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
pub enum ServiceStatus {
    /// Foreground mode: nothing is registered, and whoever started `serve`
    /// supervises it.
    Unmanaged,
    /// The manager has no registration for the instance.
    NotRegistered,
    /// The instance is registered.
    Registered {
        /// The manager started the instance and has not seen it exit.
        running: bool,
        /// When the manager runs it.
        availability: Availability,
    },
}

/// Why a service operation failed. No variant carries a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceError {
    /// The instance id or the unit breaks a rule of [`InstanceId::new`] or
    /// [`Unit::new`].
    InvalidUnit { reason: &'static str },
    /// `start` or `stop` named an instance the manager has no registration for.
    NotRegistered,
    /// `register` found this product's registration for the instance with a
    /// different unit. Nothing was changed.
    Conflict,
    /// Something this product did not write holds the instance's name in the service
    /// manager. Nothing was changed: an adapter never takes over or removes it.
    Foreign,
    /// `start` or `stop` in foreground mode: only whoever started `serve` can.
    Unmanaged,
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
                f.write_str("the instance is already registered with a different service unit")
            }
            Self::Foreign => f.write_str(
                "something else is registered under this instance's service name; it was left alone",
            ),
            Self::Unmanaged => f.write_str(
                "the instance runs in the foreground; whoever started it starts and stops it",
            ),
            Self::Manager { action, detail } => {
                write!(f, "the service manager failed at {action}: {detail}")
            }
        }
    }
}

impl std::error::Error for ServiceError {}

/// One user's service manager, for the instances of that user. The module
/// documentation states what each method does in each mode.
pub trait ServiceManager: Send + Sync {
    /// Registers `unit` without starting it.
    fn register(&self, unit: &Unit) -> Result<(), ServiceError>;
    /// Makes the manager run the registered instance now.
    fn start(&self, instance: &InstanceId) -> Result<(), ServiceError>;
    /// Stops the registered instance and waits until it has stopped.
    fn stop(&self, instance: &InstanceId) -> Result<(), ServiceError>;
    /// Whether the instance is registered and running, and when it runs.
    fn status(&self, instance: &InstanceId) -> Result<ServiceStatus, ServiceError>;
    /// Stops the instance and removes its registration.
    fn unregister(&self, instance: &InstanceId) -> Result<(), ServiceError>;
}

/// The service manager for `mode` on the build target, or the refusal.
///
/// Foreground mode works on macOS and GNU/Linux today. Registered mode refuses on
/// every target until its adapters are written: the launchd agent in
/// `service/macos.rs` (increment A2) and the systemd user unit in `service/linux.rs`
/// (increment X5).
pub fn service(mode: ServiceMode) -> Result<Box<dyn ServiceManager>, Refused> {
    native::service(mode)
}

supported! {
    /// Foreground mode, the same on every system that has it: there is nothing to
    /// register, start or stop.
    struct Foreground;

    impl ServiceManager for Foreground {
        fn register(&self, _: &Unit) -> Result<(), ServiceError> {
            Ok(())
        }
        fn start(&self, _: &InstanceId) -> Result<(), ServiceError> {
            Err(ServiceError::Unmanaged)
        }
        fn stop(&self, _: &InstanceId) -> Result<(), ServiceError> {
            Err(ServiceError::Unmanaged)
        }
        fn status(&self, _: &InstanceId) -> Result<ServiceStatus, ServiceError> {
            Ok(ServiceStatus::Unmanaged)
        }
        fn unregister(&self, _: &InstanceId) -> Result<(), ServiceError> {
            Ok(())
        }
    }

    /// What the macOS and Linux adapters answer for registered mode until they have
    /// a manager for it.
    fn registered_mode_unwritten() -> Refused {
        Refused::unsupported("registered service")
    }
}

#[cfg(test)]
mod tests;
