//! Where an office server lives on this machine, so a client can find its socket.
//!
//! The rule, in one place for the server and every client:
//! - The office home is `$ORG_OFFICE_HOME` when set to an absolute path (tests,
//!   development). Otherwise it is `~/Library/Application Support/agentic.org` (macOS).
//! - Relative paths are ignored, for `ORG_OFFICE_HOME` and `HOME` alike. The server runs with
//!   `/` as its working directory, so a relative path would point it at a different socket
//!   from the client's.
//! - The server listens on `<office home>/office.sock`.
//!
//! macOS limits a Unix socket path to 103 bytes, and binding or connecting fails beyond that.
//! Keep `ORG_OFFICE_HOME` reasonably short.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The environment variable that overrides the office home.
pub const OFFICE_HOME_ENV: &str = "ORG_OFFICE_HOME";

/// The socket's file name inside the office home.
pub const SOCKET_FILE_NAME: &str = "office.sock";

/// The office home on this machine, or `None` if neither `ORG_OFFICE_HOME` nor `HOME` is set
/// to an absolute path.
pub fn default_office_home() -> Option<PathBuf> {
    office_home_from(std::env::var_os(OFFICE_HOME_ENV), std::env::var_os("HOME"))
}

/// The socket path inside an office home.
pub fn socket_path(office_home: &Path) -> PathBuf {
    office_home.join(SOCKET_FILE_NAME)
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
    fn socket_lives_in_the_office_home() {
        assert_eq!(
            socket_path(Path::new("/tmp/o1")),
            PathBuf::from("/tmp/o1/office.sock")
        );
        assert_eq!(OFFICE_HOME_ENV, "ORG_OFFICE_HOME");
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
