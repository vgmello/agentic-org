//! Where an instance's per-user state lives by default.
//!
//! The consumer composes the rest (`instances/<name>`, `setup/<name>`, `locator`,
//! `host.sock`, `run/o/<handle>.sock`).
//!
//! # Systems
//!
//! - macOS: `~/Library/Application Support/agentic.org`, the per-user temporary
//!   directory the system names (`_CS_DARWIN_USER_TEMP_DIR`),
//!   `~/Library/Caches/agentic.org/cli-cache` for the command line's cache, socket paths
//!   of at most 103 bytes.
//! - GNU/Linux: `${XDG_DATA_HOME:-~/.local/share}/agentic-org`,
//!   `${XDG_CACHE_HOME:-~/.cache}/agentic-org/tmp`,
//!   `${XDG_CACHE_HOME:-~/.cache}/agentic-org/cli-cache` for the command line's cache,
//!   `${XDG_STATE_HOME:-~/.local/state}/agentic-org/credentials` for a release host's
//!   secrets, socket paths of at most 107 bytes.
//!   The XDG variables are read from the process environment here, because they are
//!   the system's own convention; a value that is not an absolute path is ignored.
//! - Every other Unix target: each function returns the `instance locations` refusal.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(
        dead_code,
        reason = "refusing fallback module is used on unsupported targets and in unit tests"
    )
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

use crate::Refused;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// The per-user application base for the account whose home is `home`.
/// The consumer validates the result as it validates every other path it opens.
pub fn application_base(home: &Path) -> Result<PathBuf, Refused> {
    native::application_base(home)
}

/// The application base of the account whose home is `home` from the system's defaults
/// alone: this process's environment is not consulted. macOS: the same as
/// [`application_base`]. GNU/Linux: `<home>/.local/share/agentic-org`, whatever
/// `XDG_DATA_HOME` says. A consumer that must not be moved by its environment compares
/// the two.
pub fn account_application_base(home: &Path) -> Result<PathBuf, Refused> {
    native::account_application_base(home)
}

/// The environment variables that can make [`application_base`] differ from
/// [`account_application_base`] on this system, for a consumer's message. macOS: none.
/// GNU/Linux: `XDG_DATA_HOME`.
pub fn application_base_variables() -> Result<&'static [&'static str], Refused> {
    native::application_base_variables()
}

/// The environment variables that can move any location of this module on this system:
/// [`application_base`], [`user_temp_base`] and [`credential_directory_base`]. A consumer
/// that has another program resolve the same locations (a service manager starts its units
/// with an environment of its own) gives it these, with their values. macOS: none.
/// GNU/Linux: `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_STATE_HOME`.
pub fn location_variables() -> Result<&'static [&'static str], Refused> {
    native::location_variables()
}

/// The [`location_variables`] of this process that move a location away from where it
/// is without them, for the account whose home is `home`, with their values: what a
/// consumer gives another program so that it resolves the same locations. A variable that
/// is unset, not an absolute path (which the path rule ignores), or that names the default
/// folder itself (spelled differently, or through a link) is not one: carried, it would only
/// make two environments that resolve the same locations look different. macOS: none.
pub fn location_overrides(home: &Path) -> Result<Vec<(&'static str, OsString)>, Refused> {
    native::location_overrides(home)
}

/// The default folder for the credential directories of the account whose home is
/// `home`, on a system whose release host keeps its secrets in a directory. The
/// consumer appends one name per instance, creates the result, and records the
/// absolute path once, at setup: a host never asks this function at its start, so its
/// environment cannot move its secrets later.
///
/// GNU/Linux: `<XDG_STATE_HOME>/agentic-org/credentials`, or
/// `<home>/.local/state/agentic-org/credentials`; nothing is created here. macOS has
/// no credential directory (a release host's secrets are in the Keychain) and refuses,
/// as every other system does.
pub fn credential_directory_base(home: &Path) -> Result<PathBuf, Refused> {
    native::credential_directory_base(home)
}

/// The command line's cache folder (completion candidates) of the account whose home is
/// `home`: a folder of this user's own, never a shared or system temporary one, so what it
/// lists (instance names, organizations, offices) is not readable by other users.
///
/// macOS: `<home>/Library/Caches/agentic.org/cli-cache`. GNU/Linux:
/// `<XDG_CACHE_HOME>/agentic-org/cli-cache`, or `<home>/.cache/agentic-org/cli-cache` (a
/// relative `XDG_CACHE_HOME` is ignored). Nothing is created here: the consumer creates it
/// owner-only ([`crate::fs::create_owner_only_dir_all`]). Every other system refuses.
pub fn cli_cache_dir(home: &Path) -> Result<PathBuf, Refused> {
    native::cli_cache_dir(home)
}

/// Why [`user_temp_base`] named no directory.
#[derive(Debug)]
pub enum UserTempError {
    /// This build target has no adapter.
    Unsupported(Refused),
    /// The system named no per-user temporary directory.
    Unavailable,
    /// The system named one that is not a usable path: not UTF-8, or it does not
    /// resolve.
    Invalid(io::Error),
}

impl fmt::Display for UserTempError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(refused) => refused.fmt(f),
            Self::Unavailable => write!(f, "the system names no per-user temporary directory"),
            Self::Invalid(error) => {
                write!(f, "the per-user temporary directory is unusable: {error}")
            }
        }
    }
}

impl std::error::Error for UserTempError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unsupported(refused) => Some(refused),
            Self::Unavailable => None,
            Self::Invalid(error) => Some(error),
        }
    }
}

/// The private per-user temporary base for the account whose home is `home`.
///
/// macOS asks the system for the user's own temporary directory and resolves it
/// (`home` is not used). GNU/Linux names a path under the user's cache directory and
/// creates nothing; the consumer creates and validates it (it must be on disk:
/// [`crate::fs::require_disk`]).
pub fn user_temp_base(home: &Path) -> Result<PathBuf, UserTempError> {
    native::user_temp_base(home)
}

/// How a session's environment spells the path `base` under the user temp base. On
/// macOS the environment carries the system's `/var/…` spelling of a resolved
/// `/private/var/…` path, which is shorter; elsewhere the path is unchanged. The
/// consumer keeps the resolved spelling for everything it records.
pub fn environment_spelling(base: &Path) -> Result<PathBuf, Refused> {
    native::environment_spelling(base)
}

/// The longest Unix socket path this system binds or connects, in bytes (`sun_path`
/// without its terminating NUL).
pub fn socket_path_limit() -> Result<usize, Refused> {
    native::socket_path_limit()
}

/// Locations for consumer tests (`test-support`).
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    use crate::Refused;
    use std::path::PathBuf;

    /// A shared temporary directory that is on disk, for tests of rules that refuse a
    /// memory filesystem (a Linux host's `/tmp` may be one).
    pub fn disk_backed_temp() -> Result<PathBuf, Refused> {
        super::native::disk_backed_temp()
    }

    /// A shared temporary directory that is in memory, for tests of rules that refuse a
    /// memory filesystem where data must persist. `None` on a system that mounts none by
    /// default: such a rule cannot be met there, and its test has nothing to run against.
    pub fn memory_backed_temp() -> Result<Option<PathBuf>, Refused> {
        super::native::memory_backed_temp()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn refusing_adapter_names_the_capability_and_the_system() {
        let refused = super::refused::application_base(std::path::Path::new("/home/x"));
        let crate::Refused::Unsupported { capability, os } = refused.unwrap_err();
        assert_eq!(capability, "instance locations");
        assert_eq!(os, std::env::consts::OS);
    }
}
