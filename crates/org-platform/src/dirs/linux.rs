use super::UserTempError;
use crate::Refused;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The folder of this product inside an XDG base directory.
const PRODUCT: &str = "agentic-org";

pub(super) fn application_base(home: &Path) -> Result<PathBuf, Refused> {
    Ok(application_base_from(
        std::env::var_os("XDG_DATA_HOME"),
        home,
    ))
}

pub(super) fn user_temp_base(home: &Path) -> Result<PathBuf, UserTempError> {
    Ok(user_temp_base_from(
        std::env::var_os("XDG_CACHE_HOME"),
        home,
    ))
}

pub(super) fn environment_spelling(base: &Path) -> Result<PathBuf, Refused> {
    Ok(base.to_owned())
}

/// `sun_path` is 108 bytes with its NUL.
pub(super) fn socket_path_limit() -> Result<usize, Refused> {
    Ok(107)
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn disk_backed_temp() -> Result<PathBuf, Refused> {
    Ok(PathBuf::from("/var/tmp"))
}

/// `<XDG_DATA_HOME>/agentic-org`, or `<home>/.local/share/agentic-org`.
fn application_base_from(data_home: Option<OsString>, home: &Path) -> PathBuf {
    xdg_base(data_home, home, ".local/share").join(PRODUCT)
}

/// `<XDG_CACHE_HOME>/agentic-org/tmp`, or `<home>/.cache/agentic-org/tmp`.
fn user_temp_base_from(cache_home: Option<OsString>, home: &Path) -> PathBuf {
    xdg_base(cache_home, home, ".cache")
        .join(PRODUCT)
        .join("tmp")
}

/// The XDG base directory rule: the variable's value when it is an absolute path, else
/// the default below the home folder. An unset, empty or relative value is ignored, as
/// the XDG Base Directory Specification requires.
fn xdg_base(value: Option<OsString>, home: &Path, default: &str) -> PathBuf {
    match value.map(PathBuf::from) {
        Some(base) if base.is_absolute() => base,
        _ => home.join(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/a";

    #[test]
    fn the_application_base_follows_xdg_data_home_when_it_is_absolute() {
        let from =
            |value: Option<&str>| application_base_from(value.map(OsString::from), Path::new(HOME));
        assert_eq!(from(None), Path::new("/home/a/.local/share/agentic-org"));
        assert_eq!(from(Some("/data/xdg")), Path::new("/data/xdg/agentic-org"));
        // Unset, empty and relative values are ignored.
        for ignored in ["", "data", "./data", "~/data"] {
            assert_eq!(
                from(Some(ignored)),
                Path::new("/home/a/.local/share/agentic-org"),
                "{ignored:?}"
            );
        }
    }

    #[test]
    fn the_user_temp_base_follows_xdg_cache_home_when_it_is_absolute() {
        let from =
            |value: Option<&str>| user_temp_base_from(value.map(OsString::from), Path::new(HOME));
        assert_eq!(from(None), Path::new("/home/a/.cache/agentic-org/tmp"));
        assert_eq!(
            from(Some("/cache/xdg")),
            Path::new("/cache/xdg/agentic-org/tmp")
        );
        for ignored in ["", "cache", "../cache"] {
            assert_eq!(
                from(Some(ignored)),
                Path::new("/home/a/.cache/agentic-org/tmp"),
                "{ignored:?}"
            );
        }
    }

    #[test]
    fn the_public_functions_name_a_path_and_create_nothing() {
        let home = tempfile::tempdir().unwrap();
        let base = application_base(home.path()).unwrap();
        let temp = user_temp_base(home.path()).unwrap();
        assert!(base.is_absolute() && base.ends_with("agentic-org"));
        assert!(temp.is_absolute() && temp.ends_with("agentic-org/tmp"));
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
    }

    #[test]
    fn the_environment_spelling_is_the_path_itself() {
        for path in ["/private/var/folders/example/T", "/home/a/.cache/x"] {
            assert_eq!(
                environment_spelling(Path::new(path)).unwrap(),
                Path::new(path)
            );
        }
    }

    #[test]
    fn a_socket_path_holds_107_bytes() {
        assert_eq!(socket_path_limit(), Ok(107));
        // SAFETY: an all-zero socket address is a valid value.
        let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        assert_eq!(address.sun_path.len() - 1, 107);
    }
}
