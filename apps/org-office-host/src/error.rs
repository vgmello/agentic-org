//! Why the host couldn't start.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Why the host couldn't start.
#[derive(Debug)]
pub enum HostError {
    /// The office home isn't an absolute path.
    RelativeHome { path: PathBuf },
    /// A socket in this office home would be longer than macOS allows.
    SocketPathTooLong { path: PathBuf, len: usize },
    /// The office home or one of its folders isn't a real directory owned by this user.
    UnsafeHome { path: PathBuf },
    /// Another host holds this office home's `host.lock`.
    AlreadyRunning { home: PathBuf },
    /// Something that isn't a socket is where `host.sock` goes.
    SocketInTheWay { path: PathBuf },
    /// The app's identity (bundle id, team id, development path) isn't well formed.
    InvalidAppIdentity { why: String },
    /// A debug build has no usable development key folder (`Config::dev_key_dir`, an absolute
    /// path outside the office home), so it has nowhere to keep the offices' keys. Debug builds
    /// never use the Keychain.
    NoDevKeyDir,
    /// A release build was compiled without its Keychain access group
    /// (`ORG_HOST_KEYCHAIN_GROUP`), so it has nowhere to keep the offices' keys.
    NoKeychainGroup,
    Io {
        context: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::RelativeHome { path } => {
                write!(
                    f,
                    "the office home {} isn't an absolute path",
                    path.display()
                )
            }
            HostError::SocketPathTooLong { path, len } => write!(
                f,
                "the office home is too long: the socket {} would be {len} bytes, and macOS \
                 allows at most {}; use a shorter office home",
                path.display(),
                org_proto::location::MAX_SOCKET_PATH_BYTES
            ),
            HostError::UnsafeHome { path } => write!(
                f,
                "{} isn't a folder owned by this user; refusing to use it",
                path.display()
            ),
            HostError::AlreadyRunning { home } => write!(
                f,
                "another org-office-host is already running for {}",
                home.display()
            ),
            HostError::SocketInTheWay { path } => write!(
                f,
                "{} exists and isn't a socket; refusing to replace it",
                path.display()
            ),
            HostError::InvalidAppIdentity { why } => write!(f, "{why}"),
            HostError::NoDevKeyDir => f.write_str(
                "a debug build keeps the offices' keys in a development folder, and none was \
                 given: set ORG_OFFICE_DEV_KEY_DIR to an absolute path outside the office home \
                 (debug builds never use the Keychain)",
            ),
            HostError::NoKeychainGroup => f.write_str(
                "this build has no Keychain access group for the offices' keys: build it with \
                 ORG_HOST_KEYCHAIN_GROUP set to the group in its keychain-access-groups \
                 entitlement",
            ),
            HostError::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for HostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            HostError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
