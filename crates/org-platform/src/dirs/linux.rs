use super::UserTempError;
use crate::Refused;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The folder of this product inside an XDG base directory.
const PRODUCT: &str = "agentic-org";

/// The XDG base directory variable of each location.
const DATA_HOME: &str = "XDG_DATA_HOME";
const CACHE_HOME: &str = "XDG_CACHE_HOME";
const STATE_HOME: &str = "XDG_STATE_HOME";

pub(super) fn application_base(home: &Path) -> Result<PathBuf, Refused> {
    Ok(application_base_from(std::env::var_os(DATA_HOME), home))
}

pub(super) fn account_application_base(home: &Path) -> Result<PathBuf, Refused> {
    Ok(application_base_from(None, home))
}

pub(super) fn application_base_variables() -> Result<&'static [&'static str], Refused> {
    Ok(&[DATA_HOME])
}

pub(super) fn location_variables() -> Result<&'static [&'static str], Refused> {
    Ok(&[DATA_HOME, CACHE_HOME, STATE_HOME])
}

pub(super) fn location_overrides(home: &Path) -> Result<Vec<(&'static str, OsString)>, Refused> {
    Ok(overrides_from(|name| std::env::var_os(name), home))
}

pub(super) fn user_temp_base(home: &Path) -> Result<PathBuf, UserTempError> {
    Ok(user_temp_base_from(std::env::var_os(CACHE_HOME), home))
}

pub(super) fn credential_directory_base(home: &Path) -> Result<PathBuf, Refused> {
    Ok(credential_directory_base_from(
        std::env::var_os(STATE_HOME),
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

/// The POSIX shared-memory directory, a `tmpfs` wherever glibc's `shm_open` works.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn memory_backed_temp() -> Result<Option<PathBuf>, Refused> {
    Ok(Some(PathBuf::from("/dev/shm")))
}

/// Each XDG base directory variable with the default it replaces.
const DEFAULTS: [(&str, &str); 3] = [
    (DATA_HOME, ".local/share"),
    (CACHE_HOME, ".cache"),
    (STATE_HOME, ".local/state"),
];

/// The variables `value` gives that move their base away from its default below `home`:
/// absolute, and naming another folder than the default, by its spelling (a repeated
/// separator, a `.` component and a trailing separator do not count) and, where both
/// exist, by what they resolve to.
fn overrides_from(
    value: impl Fn(&str) -> Option<OsString>,
    home: &Path,
) -> Vec<(&'static str, OsString)> {
    DEFAULTS
        .iter()
        .filter_map(|(name, default)| {
            let set = value(name).filter(|v| Path::new(v).is_absolute())?;
            let default = home.join(default);
            let same = Path::new(&set) == default
                || matches!(
                    (std::fs::canonicalize(&set), std::fs::canonicalize(&default)),
                    (Ok(a), Ok(b)) if a == b
                );
            (!same).then_some((*name, set))
        })
        .collect()
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

/// `<XDG_STATE_HOME>/agentic-org/credentials`, or
/// `<home>/.local/state/agentic-org/credentials`.
fn credential_directory_base_from(state_home: Option<OsString>, home: &Path) -> PathBuf {
    xdg_base(state_home, home, ".local/state")
        .join(PRODUCT)
        .join("credentials")
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
    fn the_account_base_ignores_the_environment() {
        // Whatever XDG_DATA_HOME is in this process, the account's base is the default.
        assert_eq!(
            account_application_base(Path::new(HOME)).unwrap(),
            Path::new("/home/a/.local/share/agentic-org")
        );
        assert_eq!(application_base_variables().unwrap(), ["XDG_DATA_HOME"]);
    }

    /// The variables that move the three locations, the application base's among them.
    #[test]
    fn the_location_variables_are_the_three_xdg_bases() {
        assert_eq!(
            location_variables().unwrap(),
            ["XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"]
        );
        for name in application_base_variables().unwrap() {
            assert!(location_variables().unwrap().contains(name));
        }
    }

    /// X5 review F8: two environments that resolve the same locations give the same
    /// overrides, so a service unit rendered from either is the same.
    #[test]
    fn only_a_variable_that_moves_a_location_is_an_override() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join(".local/share")).unwrap();
        std::os::unix::fs::symlink(home.join(".local/share"), temp.path().join("linked")).unwrap();
        let from = |pairs: &[(&str, String)]| {
            let pairs: Vec<(String, String)> = pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect();
            overrides_from(
                |name| {
                    pairs
                        .iter()
                        .find(|(k, _)| k == name)
                        .map(|(_, v)| OsString::from(v))
                },
                &home,
            )
        };
        let shown = |p: &Path| p.display().to_string();
        assert!(from(&[]).is_empty());
        for same in [
            shown(&home.join(".local/share")),
            format!("{}/", shown(&home.join(".local/share"))),
            format!("{}//./.local/share", shown(&home)),
            shown(&temp.path().join("linked")),
        ] {
            assert!(from(&[(DATA_HOME, same.clone())]).is_empty(), "{same}");
        }
        assert!(from(&[(CACHE_HOME, "relative".into())]).is_empty());
        assert!(from(&[(STATE_HOME, shown(&home.join(".local/state")))]).is_empty());
        let elsewhere = shown(&temp.path().join("xdg"));
        assert_eq!(
            from(&[
                (CACHE_HOME, elsewhere.clone()),
                (DATA_HOME, shown(&home.join(".local/share")))
            ]),
            [(CACHE_HOME, OsString::from(&elsewhere))]
        );
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
    fn the_credential_directory_base_follows_xdg_state_home_when_it_is_absolute() {
        let from = |value: Option<&str>| {
            credential_directory_base_from(value.map(OsString::from), Path::new(HOME))
        };
        assert_eq!(
            from(None),
            Path::new("/home/a/.local/state/agentic-org/credentials")
        );
        assert_eq!(
            from(Some("/state/xdg")),
            Path::new("/state/xdg/agentic-org/credentials")
        );
        for ignored in ["", "state", "./state", "../state", "~/state"] {
            assert_eq!(
                from(Some(ignored)),
                Path::new("/home/a/.local/state/agentic-org/credentials"),
                "{ignored:?}"
            );
        }
        // It is not below the application base or the temp base, whatever the three
        // variables are when they are unset.
        let state = from(None);
        assert!(!state.starts_with(application_base_from(None, Path::new(HOME))));
        assert!(!state.starts_with(user_temp_base_from(None, Path::new(HOME))));
    }

    #[test]
    fn the_public_functions_name_a_path_and_create_nothing() {
        let home = tempfile::tempdir().unwrap();
        let base = application_base(home.path()).unwrap();
        let temp = user_temp_base(home.path()).unwrap();
        let credentials = credential_directory_base(home.path()).unwrap();
        assert!(base.is_absolute() && base.ends_with("agentic-org"));
        assert!(temp.is_absolute() && temp.ends_with("agentic-org/tmp"));
        assert!(credentials.is_absolute() && credentials.ends_with("agentic-org/credentials"));
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
