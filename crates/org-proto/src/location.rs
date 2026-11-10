//! Where this machine's host and its office servers live, so a client can find their sockets.
//!
//! The rule, in one place for the server and every client:
//! - The office home is `$ORG_OFFICE_HOME` when set to an absolute path (tests,
//!   development). Otherwise it is `~/Library/Application Support/agentic.org` (macOS).
//! - Relative paths are ignored, for `ORG_OFFICE_HOME` and `HOME` alike. The server runs with
//!   `/` as its working directory, so a relative path would point it at a different socket
//!   from the client's.
//! - The host (`org-office-host`, the login service) listens on `<office home>/host.sock`.
//! - Each office server listens on `<office home>/run/<handle>.sock`, where the handle is the
//!   last 12 characters of the office id (its random part). The host checks handles are unique,
//!   **ignoring case**, because macOS file systems usually are case-insensitive.
//!
//! macOS limits a Unix socket path to [`MAX_SOCKET_PATH_BYTES`] bytes, and binding or
//! connecting fails beyond that. Check with [`check_socket_path`] and report it clearly.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The environment variable that overrides the office home.
pub const OFFICE_HOME_ENV: &str = "ORG_OFFICE_HOME";

/// Host-selected private socket directory, independent of the data root (launch v2).
pub const OFFICE_RUNTIME_DIR_ENV: &str = "ORG_OFFICE_RUNTIME_DIR";

/// The host's socket file name inside the office home.
pub const HOST_SOCKET_FILE_NAME: &str = "host.sock";

/// The longest Unix socket path macOS accepts, in bytes (`sun_path` is 104 with its NUL).
pub const MAX_SOCKET_PATH_BYTES: usize = 103;

/// How many trailing characters of an office id name its socket.
pub const OFFICE_SOCKET_HANDLE_CHARS: usize = 12;

/// The longest office id this module takes, in characters. Ids stay opaque (a ULID today is
/// 26); the cap only bounds what arrives over the wire.
const MAX_OFFICE_ID_CHARS: usize = 64;

/// The office home on this machine, or `None` if neither `ORG_OFFICE_HOME` nor `HOME` is set
/// to an absolute path.
pub fn default_office_home() -> Option<PathBuf> {
    office_home_from(std::env::var_os(OFFICE_HOME_ENV), std::env::var_os("HOME"))
}

/// The host's socket path inside an office home.
pub fn host_socket_path(office_home: &Path) -> PathBuf {
    office_home.join(HOST_SOCKET_FILE_NAME)
}

/// An office server's socket path inside an office home, or `None` if the office id isn't a
/// plain id: 1 to 64 ASCII letters and digits. Ids stay opaque, so any length in that range
/// passes, not just a ULID's 26. Office ids arrive over the wire, so this never lets one point
/// outside `run/`.
pub fn office_socket_path(office_home: &Path, office_id: &str) -> Option<PathBuf> {
    office_socket_in(&office_home.join("run"), office_id)
}

/// An office socket in an explicitly selected runtime directory. This validates the
/// opaque ID; the caller owns runtime-directory authorization and lifetime.
pub fn office_socket_in(runtime_dir: &Path, office_id: &str) -> Option<PathBuf> {
    if !is_plain_office_id(office_id) {
        return None;
    }
    let start = office_id.len().saturating_sub(OFFICE_SOCKET_HANDLE_CHARS);
    let handle = &office_id[start..];
    Some(runtime_dir.join(format!("{handle}.sock")))
}

/// Whether an office id is plain: 1..=[`MAX_OFFICE_ID_CHARS`] ASCII letters and digits. It
/// checks the shape only, not that the office exists, and doesn't require a ULID.
fn is_plain_office_id(office_id: &str) -> bool {
    (1..=MAX_OFFICE_ID_CHARS).contains(&office_id.len())
        && office_id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// A socket path longer than macOS allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketPathTooLong {
    pub len: usize,
}

impl std::fmt::Display for SocketPathTooLong {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "socket path is {} bytes; macOS allows at most {MAX_SOCKET_PATH_BYTES} (set a shorter ORG_OFFICE_HOME)",
            self.len
        )
    }
}

impl std::error::Error for SocketPathTooLong {}

/// Check that a socket path fits macOS's limit.
pub fn check_socket_path(path: &Path) -> Result<(), SocketPathTooLong> {
    let len = path.as_os_str().len();
    if len > MAX_SOCKET_PATH_BYTES {
        Err(SocketPathTooLong { len })
    } else {
        Ok(())
    }
}

fn office_home_from(override_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    match override_home {
        Some(dir) if Path::new(&dir).is_absolute() => Some(PathBuf::from(dir)),
        _ => home
            .filter(|h| Path::new(h).is_absolute())
            .map(|h| PathBuf::from(h).join("Library/Application Support/agentic.org")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins_when_set() {
        let got = office_home_from(Some("/tmp/o1".into()), Some("/Users/me".into()));
        assert_eq!(got, Some(PathBuf::from("/tmp/o1")));
    }

    #[test]
    fn empty_override_falls_back_to_home() {
        let got = office_home_from(Some("".into()), Some("/Users/me".into()));
        assert_eq!(
            got,
            Some(PathBuf::from(
                "/Users/me/Library/Application Support/agentic.org"
            ))
        );
    }

    #[test]
    fn no_override_and_no_home_is_none() {
        assert_eq!(office_home_from(None, None), None);
        assert_eq!(office_home_from(None, Some("".into())), None);
    }

    #[test]
    fn sockets_live_in_the_office_home() {
        assert_eq!(
            host_socket_path(Path::new("/tmp/o1")),
            PathBuf::from("/tmp/o1/host.sock")
        );
        assert_eq!(
            office_socket_path(Path::new("/tmp/o1"), "01M6WGY1WJCC503RXF2NS0SH83"),
            Some(PathBuf::from("/tmp/o1/run/3RXF2NS0SH83.sock"))
        );
        assert_eq!(OFFICE_HOME_ENV, "ORG_OFFICE_HOME");
    }

    #[test]
    fn short_office_ids_use_the_whole_id() {
        assert_eq!(
            office_socket_path(Path::new("/h"), "abc"),
            Some(PathBuf::from("/h/run/abc.sock"))
        );
    }

    #[test]
    fn hostile_or_empty_office_ids_get_no_path() {
        for id in [
            "",
            "/etc/x",
            "../../x",
            "a/b",
            "a.b",
            "é1234567890",
            "abc def",
        ] {
            assert_eq!(
                office_socket_path(Path::new("/h"), id),
                None,
                "accepted {id:?}"
            );
        }
    }

    #[test]
    fn office_ids_are_capped_at_64_characters() {
        let at_cap = "a".repeat(MAX_OFFICE_ID_CHARS);
        assert_eq!(MAX_OFFICE_ID_CHARS, 64);
        assert!(is_plain_office_id(&at_cap));
        assert_eq!(
            office_socket_path(Path::new("/h"), &at_cap),
            Some(PathBuf::from("/h/run/aaaaaaaaaaaa.sock"))
        );
        let over = "a".repeat(MAX_OFFICE_ID_CHARS + 1);
        assert!(!is_plain_office_id(&over));
        assert_eq!(office_socket_path(Path::new("/h"), &over), None);
        assert!(!is_plain_office_id(""));
        assert_eq!(office_socket_path(Path::new("/h"), ""), None);
        // Any length up to the cap passes; a ULID's 26 isn't required.
        assert!(is_plain_office_id("x"));
        assert!(is_plain_office_id("01M6WGY1WJCC503RXF2NS0SH83"));
    }

    #[test]
    fn socket_path_limit_is_103_bytes() {
        let ok = PathBuf::from(format!("/{}", "a".repeat(102)));
        assert!(check_socket_path(&ok).is_ok());
        let long = PathBuf::from(format!("/{}", "a".repeat(103)));
        assert_eq!(
            check_socket_path(&long),
            Err(SocketPathTooLong { len: 104 })
        );
    }

    #[test]
    fn relative_paths_are_ignored() {
        let got = office_home_from(Some("tmp/o1".into()), Some("/Users/me".into()));
        assert_eq!(
            got,
            Some(PathBuf::from(
                "/Users/me/Library/Application Support/agentic.org"
            ))
        );
        assert_eq!(office_home_from(None, Some("me".into())), None);
        assert_eq!(office_home_from(Some("o1".into()), Some("me".into())), None);
    }
}
