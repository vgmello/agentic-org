//! Service lifecycle: who starts and restarts the instance's `agentic-org serve`
//! ([Linux host design](../../../../docs/design/linux-host.md) section 3.4).
//!
//! macOS registers a per-user launchd agent. Every other system refuses until its adapter
//! exists (milestone two). `Foreground` (the container mode) is not built yet and refuses
//! everywhere.
#[cfg(target_os = "macos")]
mod macos;
#[cfg_attr(target_os = "macos", allow(dead_code))]
mod refused;

#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(target_os = "macos"))]
use refused as imp;

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

#[derive(Debug)]
pub enum ServiceError {
    Refused(Refused),
    /// The unit cannot be expressed safely.
    Invalid(&'static str),
    /// Something else already uses this service name; it is never taken over.
    Conflict(String),
    /// The service manager failed.
    System(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(r) => r.fmt(f),
            Self::Invalid(why) => write!(f, "service definition refused: {why}"),
            Self::Conflict(why) => write!(f, "service conflict: {why}"),
            Self::System(why) => write!(f, "service manager failed: {why}"),
        }
    }
}

impl std::error::Error for ServiceError {}

pub trait ServiceManager: Send + Sync {
    /// Idempotent. An identical, loaded registration is left alone so a healthy service
    /// is not restarted. A different definition under the same name is a conflict.
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
}

/// The build target's service manager in the account's own placement.
pub fn service(mode: ServiceMode, home: &Path) -> Result<Box<dyn ServiceManager>, Refused> {
    service_in(mode, Placement::user(home)?)
}

/// The build target's service manager in an explicit placement.
pub fn service_in(
    mode: ServiceMode,
    placement: Placement,
) -> Result<Box<dyn ServiceManager>, Refused> {
    match mode {
        ServiceMode::Registered => imp::registered(placement),
        ServiceMode::Foreground => Err(Refused::unsupported("foreground service mode")),
    }
}

/// Checks shared by every adapter, so a definition never carries path syntax in its name,
/// a relative program, or loader variables.
// Used by the macOS adapter and by the testing adapter; the Linux adapter refuses until it is
// built (implementation plan X5), so a Linux build without tests does not call these.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn xml_text(text: &str) -> bool {
    !text.chars().any(|c| {
        (c < ' ' && !matches!(c, '\t' | '\n' | '\r')) || matches!(c, '\u{FFFE}' | '\u{FFFF}')
    })
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
    fn the_refusing_adapter_and_the_unbuilt_foreground_mode_refuse() {
        let placement = Placement {
            prefix: "p.".into(),
            directory: "/nonexistent".into(),
        };
        let Err(Refused::Unsupported { capability, .. }) = refused::registered(placement.clone())
        else {
            panic!("the refusing adapter must refuse");
        };
        assert_eq!(capability, "service registration");
        assert!(refused::user_placement(Path::new("/home/a")).is_err());
        assert!(service_in(ServiceMode::Foreground, placement).is_err());
    }
}
