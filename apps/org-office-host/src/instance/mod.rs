//! Private instance locator/reservation policy, separate from the account registry.
//!
//! Callers supply resolved absolute paths. There is no environment discovery, socket
//! selection, credential access or public host API here. A reservation owns the actual
//! root's inode lock until dropped. The locator's on-disk representation stays private.
//! NFS is unsupported: its emulation of flock does not provide these descriptor lifetimes.
//! Linux casefold directories (ext4/f2fs) are unsupported and refused along the root path.
//! Linux instance roots require case-sensitive ext4, XFS, btrfs, f2fs or overlayfs;
//! XFS geometry must positively exclude ASCII-CI. ZFS roots are unsupported in v1.
//! Ancestor mount types are unrestricted; descendants stay on the root device.
//! Locks serialize cooperating writers. Deliberate same-UID mutation outside the
//! locks is outside v1's threat model. The final metadata check before rename
//! narrows the replacement window; it is not an atomic inode compare-and-swap.

mod directory;
pub(crate) mod registry;
mod temp;

use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) enum Error {
    Invalid(&'static str),
    Unsafe(&'static str),
    Conflict(&'static str),
    RebindUnconfirmed,
    RebindRecoveryRequired {
        name: String,
        path: PathBuf,
    },
    RebindChanged {
        name: String,
        path: PathBuf,
    },
    IdentityClash {
        name: String,
        path: PathBuf,
        reserved_by: String,
        reserved_root: PathBuf,
    },
    RootUnavailable {
        name: String,
        path: PathBuf,
    },
    ReservedRoot {
        name: String,
        path: PathBuf,
    },
    #[cfg(target_os = "linux")]
    CaseInsensitive(PathBuf),
    #[cfg(target_os = "linux")]
    UnsupportedFilesystem {
        path: PathBuf,
        kind: u64,
    },
    #[cfg(target_os = "linux")]
    FilesystemProbe {
        path: PathBuf,
        probe: &'static str,
        source: io::Error,
    },
    Busy(PathBuf),
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(why) => write!(f, "invalid instance selection: {why}"),
            Self::Unsafe(why) => write!(f, "unsafe instance state: {why}; repair before retrying"),
            Self::Conflict(why) => write!(f, "instance conflict: {why}; use its registered root"),
            Self::RebindUnconfirmed => write!(f, "instance rebind was not confirmed"),
            Self::RebindRecoveryRequired { name, path } => write!(f,
                "instance {name} at {path:?}: recovery required; configuration has neither the recorded nor confirmed filesystem identity; a fresh explicit repair must verify the immutable instance ID"),
            Self::RebindChanged { name, path } => write!(f,
                "instance {name} at {path:?} changed during confirmation; nothing was changed; run rebind again"),
            Self::IdentityClash { name, path, reserved_by, reserved_root } => write!(f,
                "instance {name} root {path:?} has a filesystem identity reserved for instance {reserved_by} at {reserved_root:?}; registry unchanged; restore or rebind {reserved_by}, or choose another root"),
            Self::RootUnavailable { name, path } => write!(f,
                "instance {name} root {} is unavailable or changed; repair with agentic-org instance rebind {name}",
                path.display()),
            Self::ReservedRoot { name, path } => write!(f,
                "instance {name} reserves root {}; choose a disjoint root or repair that registration",
                path.display()),
            Self::Busy(path) => write!(f, "instance state is already locked: {}", path.display()),
            #[cfg(target_os = "linux")]
            Self::CaseInsensitive(path) => write!(f,
                "instance path {} uses a case-insensitive directory; case-insensitive directories aren't supported on Linux",
                path.display()),
            #[cfg(target_os = "linux")]
            Self::UnsupportedFilesystem { path, kind } => write!(f,
                "unsupported filesystem for the instance root at {} (type {kind:#x}); Linux requires a case-sensitive local filesystem",
                path.display()),
            #[cfg(target_os = "linux")]
            Self::FilesystemProbe { path, probe, source } => write!(f,
                "cannot validate instance path {} with {probe}: {source}", path.display()),
            Self::Io(error) => write!(f, "instance filesystem: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            #[cfg(target_os = "linux")]
            Self::FilesystemProbe { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct Name(String);

impl Name {
    pub(crate) fn parse(name: Option<&str>) -> Result<Self, Error> {
        let name = name.unwrap_or("default");
        if !(1..=32).contains(&name.len())
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || !name.as_bytes()[0].is_ascii_alphanumeric()
        {
            return Err(Error::Invalid(
                "name must be 1–32 lowercase ASCII letters, digits or hyphens, starting with a letter or digit",
            ));
        }
        Ok(Self(name.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Name {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Error> {
        Self::parse(Some(&value))
    }
}

impl From<Name> for String {
    fn from(value: Name) -> String {
        value.0
    }
}

/// Tests exercise disk policy even on Linux hosts whose /tmp is tmpfs.
#[cfg(test)]
fn test_directory() -> tempfile::TempDir {
    let fallback = if cfg!(target_os = "linux") {
        "/var/tmp"
    } else {
        "/private/tmp"
    };
    let base = option_env!("CARGO_TARGET_TMPDIR").unwrap_or(fallback);
    std::fs::create_dir_all(base).unwrap();
    tempfile::tempdir_in(base).unwrap()
}
