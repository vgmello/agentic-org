//! Service lifecycle: who starts and restarts the instance's `agentic-org serve`
//! ([Linux host design](../../../../docs/design/linux-host.md) section 3.4).
//!
//! # Modes
//!
//! - [`ServiceMode::Registered`]: the system's service manager starts the instance at
//!   login, restarts it after a crash and leaves it stopped after a clean stop.
//! - [`ServiceMode::Foreground`]: nothing is registered. Whoever runs `agentic-org serve`
//!   (a container runtime, a person at a terminal) is the supervisor. It is the same on
//!   every system, the refusing ones included, and is chosen by the caller: nothing here
//!   detects a container.
//!
//! # Systems (`Registered`)
//!
//! - macOS: a per-user launchd agent in `~/Library/LaunchAgents`.
//! - GNU/Linux: a systemd user unit in `~/.config/systemd/user`. A machine or container
//!   without a systemd user manager has no adapter to register with: every operation, and
//!   [`ServiceManager::available`] first, answers [`ServiceError::Unavailable`], and the
//!   caller offers the foreground mode.
//! - Every other Unix target refuses.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as imp;

#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(
        dead_code,
        reason = "refusing fallback module is used on unsupported targets and in unit tests"
    )
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as imp;

mod foreground;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;

use crate::Refused;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceMode {
    Registered,
    Foreground,
}

/// What the service runs. The consumer decides the program, its arguments and its
/// complete environment; nothing is inherited from the registering process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub instance_id: String,
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

/// Where registrations live and how they are named. `user` is the production placement;
/// tests use a temporary directory and a disposable prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The service name is this prefix followed by the instance ID.
    pub prefix: String,
    /// The directory that holds the service definitions.
    pub directory: PathBuf,
}

impl Placement {
    /// The account's own service placement, for the account whose home is `home`.
    pub fn user(home: &Path) -> Result<Self, Refused> {
        imp::user_placement(home)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Registration {
    /// The definition was already in place and loaded; nothing was done.
    Unchanged,
    /// This call wrote the definition, loaded it, or both.
    Loaded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceStatus {
    /// No definition and nothing loaded.
    NotRegistered,
    /// The definition exists but the service manager has not loaded it.
    Unloaded,
    /// Loaded, not running.
    Stopped,
    Running {
        pid: u32,
    },
}

/// Whether a registered service runs while its user has no login session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lingering {
    /// The service manager has no such setting, so there is nothing to report (launchd:
    /// an agent belongs to the login session; the foreground mode: nothing is registered).
    NotApplicable,
    /// The user's services start at boot and outlive the user's sessions.
    On,
    /// The service runs only while its user is logged in. `enable` is the command that
    /// changes that; nothing in this crate runs it.
    Off { enable: String },
    /// The system could not say, for this reason.
    Unknown(String),
}

#[derive(Debug)]
pub enum ServiceError {
    Refused(Refused),
    /// The unit cannot be expressed safely.
    Invalid(&'static str),
    /// Something else already uses this service name; it is never taken over.
    Conflict(String),
    /// The service manager failed.
    System(String),
    /// This system has an adapter, but no service manager answers it here (a machine or
    /// container without a systemd user manager). Nothing was changed. The text says what
    /// is missing; an instance on such a system runs in [`ServiceMode::Foreground`].
    Unavailable(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(r) => r.fmt(f),
            Self::Invalid(why) => write!(f, "service definition refused: {why}"),
            Self::Conflict(why) => write!(f, "service conflict: {why}"),
            Self::System(why) => write!(f, "service manager failed: {why}"),
            Self::Unavailable(why) => write!(
                f,
                "no service manager can register a service here: {why}; an instance \
                 without one runs in the foreground (service mode \"foreground\")"
            ),
        }
    }
}

impl std::error::Error for ServiceError {}

pub trait ServiceManager: Send + Sync {
    /// Idempotent. An identical, loaded registration is left alone so a healthy service
    /// is not restarted. A definition under the same name that is exactly what this adapter
    /// rendered for another unit of the same instance (an earlier program, arguments or
    /// environment) is replaced: its service is stopped and unloaded first. Any other
    /// definition under the name is a conflict, never taken over.
    fn register(&self, unit: &Unit) -> Result<Registration, ServiceError>;
    /// Starts a loaded service; a running one is left alone. A definition the manager has
    /// not loaded is refused: only `register`, which compares it with the unit, loads it.
    fn start(&self, instance_id: &str) -> Result<(), ServiceError>;
    /// Asks the service to stop. A clean exit is not restarted by the manager.
    fn stop(&self, instance_id: &str) -> Result<(), ServiceError>;
    /// A job under the service name that is not ours is a conflict, never a status.
    fn status(&self, instance_id: &str) -> Result<ServiceStatus, ServiceError>;
    /// Unloads the service and removes its definition.
    fn unregister(&self, instance_id: &str) -> Result<(), ServiceError>;
    /// Whether the service manager answers at all and would load a definition from this
    /// adapter's placement, asked before anything is changed. [`ServiceError::Unavailable`]
    /// where the system has none to register with; another error where it answers but
    /// reads its definitions elsewhere. Managers that are always there answer `Ok` without
    /// asking.
    fn available(&self) -> Result<(), ServiceError> {
        Ok(())
    }
    /// Whether a registered service runs while its user is not logged in. Read-only:
    /// nothing is changed, and a manager without such a setting has nothing to report.
    fn lingering(&self) -> Lingering {
        Lingering::NotApplicable
    }
}

/// The build target's service manager in the account's own placement. The foreground
/// mode has no placement and exists on every system.
pub fn service(mode: ServiceMode, home: &Path) -> Result<Box<dyn ServiceManager>, Refused> {
    match mode {
        ServiceMode::Registered => imp::registered(Placement::user(home)?),
        ServiceMode::Foreground => Ok(Box::new(foreground::Foreground)),
    }
}

/// The build target's service manager in an explicit placement, which the foreground
/// mode does not use.
pub fn service_in(
    mode: ServiceMode,
    placement: Placement,
) -> Result<Box<dyn ServiceManager>, Refused> {
    match mode {
        ServiceMode::Registered => imp::registered(placement),
        ServiceMode::Foreground => Ok(Box::new(foreground::Foreground)),
    }
}

/// Checks shared by every adapter and by the foreground mode, so a definition never
/// carries path syntax in its name, a relative program, or loader variables. An adapter
/// adds what its own definition format cannot carry (the systemd adapter: environment
/// names that are not identifiers, among others).
pub(crate) fn validate(unit: &Unit) -> Result<(), ServiceError> {
    valid_instance_id(&unit.instance_id)?;
    if !unit.program.is_absolute() || !unit.program.to_str().is_some_and(xml_text) {
        return Err(ServiceError::Invalid(
            "the program must be an absolute UTF-8 path without control characters",
        ));
    }
    if !unit.args.iter().all(|a| a.to_str().is_some_and(xml_text)) {
        return Err(ServiceError::Invalid(
            "arguments must be UTF-8 without control characters",
        ));
    }
    for (key, value) in &unit.env {
        let (Some(key), Some(value)) = (key.to_str(), value.to_str()) else {
            return Err(ServiceError::Invalid("the environment must be UTF-8"));
        };
        if !xml_text(key) || !xml_text(value) {
            return Err(ServiceError::Invalid(
                "the environment must not contain control characters",
            ));
        }
        if key.is_empty()
            || key.contains('=')
            || key.starts_with("DYLD_")
            || key.starts_with("LD_")
            || key == "GLIBC_TUNABLES"
        {
            return Err(ServiceError::Invalid(
                "the environment has an invalid or loader variable",
            ));
        }
    }
    let mut keys: Vec<_> = unit.env.iter().map(|(k, _)| k).collect();
    keys.sort();
    if keys.windows(2).any(|w| w[0] == w[1]) {
        return Err(ServiceError::Invalid("the environment repeats a variable"));
    }
    Ok(())
}

/// Text every definition format can carry: XML 1.0 has no NUL, no C0 control other than
/// tab, line feed and carriage return, and no U+FFFE or U+FFFF.
fn xml_text(text: &str) -> bool {
    !text.chars().any(|c| {
        (c < ' ' && !matches!(c, '\t' | '\n' | '\r')) || matches!(c, '\u{FFFE}' | '\u{FFFF}')
    })
}

pub(crate) fn valid_instance_id(id: &str) -> Result<(), ServiceError> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(ServiceError::Invalid(
            "the instance ID must be lowercase ASCII letters, digits and hyphens",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn unit() -> Unit {
        Unit {
            instance_id: "01965d2e-8b1c-7c3a-9f00-0123456789ab".into(),
            program: "/opt/agentic-org/agentic-org".into(),
            args: vec!["serve".into(), "--name".into(), "default".into()],
            env: vec![("HOME".into(), "/Users/a".into())],
        }
    }

    #[test]
    fn units_with_unsafe_names_programs_or_environments_are_refused() {
        assert!(validate(&unit()).is_ok());
        assert!(
            validate(&Unit {
                args: vec!["a\tb\nc\rd".into()],
                env: vec![("A".into(), "multi\nline".into())],
                ..unit()
            })
            .is_ok(),
            "tab, line feed and carriage return are valid XML text"
        );
        for broken in [
            Unit {
                instance_id: "../x".into(),
                ..unit()
            },
            Unit {
                instance_id: "A".into(),
                ..unit()
            },
            Unit {
                instance_id: String::new(),
                ..unit()
            },
            Unit {
                program: "agentic-org".into(),
                ..unit()
            },
            Unit {
                env: vec![("DYLD_INSERT_LIBRARIES".into(), "/x".into())],
                ..unit()
            },
            Unit {
                env: vec![("LD_PRELOAD".into(), "/x".into())],
                ..unit()
            },
            Unit {
                env: vec![("A=B".into(), "x".into())],
                ..unit()
            },
            Unit {
                env: vec![("A".into(), "1".into()), ("A".into(), "2".into())],
                ..unit()
            },
            Unit {
                program: "/opt/a\0b".into(),
                ..unit()
            },
            Unit {
                program: "/opt/a\u{1b}b".into(),
                ..unit()
            },
            Unit {
                args: vec!["serve".into(), "a\u{1}".into()],
                ..unit()
            },
            Unit {
                args: vec!["\u{FFFF}".into()],
                ..unit()
            },
            Unit {
                env: vec![("A\u{7}".into(), "x".into())],
                ..unit()
            },
            Unit {
                env: vec![("A".into(), "x\0".into())],
                ..unit()
            },
        ] {
            assert!(
                matches!(validate(&broken), Err(ServiceError::Invalid(_))),
                "{broken:?}"
            );
        }
    }

    #[test]
    fn the_refusing_adapter_refuses_registration_and_has_no_placement() {
        let placement = Placement {
            prefix: "p.".into(),
            directory: "/nonexistent".into(),
        };
        let Err(Refused::Unsupported { capability, .. }) = refused::registered(placement) else {
            panic!("the refusing adapter must refuse");
        };
        assert_eq!(capability, "service registration");
        assert!(refused::user_placement(Path::new("/home/a")).is_err());
    }

    /// The foreground mode is the same on every target: nothing is registered, every
    /// operation succeeds without touching anything, and a unit or an instance ID that no
    /// definition could carry is still refused.
    #[test]
    fn the_foreground_mode_registers_nothing_and_succeeds_on_every_system() {
        let temp = tempfile::tempdir().unwrap();
        let placement = Placement {
            prefix: "p.".into(),
            directory: temp.path().join("definitions"),
        };
        for manager in [
            service_in(ServiceMode::Foreground, placement).unwrap(),
            // No placement is asked for, so a home that does not exist is not looked at.
            service(ServiceMode::Foreground, Path::new("/nonexistent/home")).unwrap(),
        ] {
            let id = unit().instance_id;
            assert!(manager.available().is_ok());
            assert_eq!(manager.register(&unit()).unwrap(), Registration::Unchanged);
            assert_eq!(manager.status(&id).unwrap(), ServiceStatus::NotRegistered);
            manager.start(&id).unwrap();
            manager.stop(&id).unwrap();
            assert_eq!(manager.status(&id).unwrap(), ServiceStatus::NotRegistered);
            manager.unregister(&id).unwrap();
            assert_eq!(manager.lingering(), Lingering::NotApplicable);
            let broken = Unit {
                program: "agentic-org".into(),
                ..unit()
            };
            assert!(matches!(
                manager.register(&broken),
                Err(ServiceError::Invalid(_))
            ));
            for call in [
                manager.start("../x"),
                manager.stop("../x"),
                manager.unregister("../x"),
                manager.status("../x").map(drop),
            ] {
                assert!(matches!(call, Err(ServiceError::Invalid(_))));
            }
        }
        assert!(
            std::fs::read_dir(temp.path()).unwrap().next().is_none(),
            "the foreground mode wrote something"
        );
    }

    #[test]
    fn an_unavailable_manager_is_reported_with_the_foreground_mode() {
        let text = ServiceError::Unavailable("XDG_RUNTIME_DIR is not set".into()).to_string();
        assert!(text.contains("XDG_RUNTIME_DIR is not set"), "{text}");
        assert!(text.contains("foreground"), "{text}");
    }
}
