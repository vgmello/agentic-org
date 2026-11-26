//! Where the command line keeps its configuration (the saved context) and its cache.
//!
//! For each: the explicit override variable (an absolute path, else an error), then an
//! absolute `XDG_CONFIG_HOME` / `XDG_CACHE_HOME` joined with `agentic-org` (a relative one is
//! ignored, as the XDG base-directory specification says), then the platform directory the
//! composition root supplies (`org-platform`'s directories). An empty variable is unset.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::ShellError;

/// Overrides the configuration directory (absolute path).
pub const CONFIG_DIR_ENV: &str = "AGENTIC_ORG_CONFIG_DIR";
/// Overrides the cache directory (absolute path).
pub const CACHE_DIR_ENV: &str = "AGENTIC_ORG_CACHE_DIR";
/// The directory under an XDG base directory.
pub const XDG_SUBDIR: &str = "agentic-org";

/// The resolved directories. Nothing is created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellDirs {
    pub config: PathBuf,
    pub cache: PathBuf,
}

/// Resolves both directories (see the module documentation). `fallback_config` /
/// `fallback_cache` are asked only when no variable decides; a missing or relative fallback is
/// [`ShellError::InvalidConfig`].
pub fn resolve_dirs(
    env: &dyn Fn(&str) -> Option<OsString>,
    fallback_config: &dyn Fn() -> Option<PathBuf>,
    fallback_cache: &dyn Fn() -> Option<PathBuf>,
) -> Result<ShellDirs, ShellError> {
    Ok(ShellDirs {
        config: resolve_one(
            env,
            CONFIG_DIR_ENV,
            "XDG_CONFIG_HOME",
            fallback_config,
            "configuration",
        )?,
        cache: resolve_one(
            env,
            CACHE_DIR_ENV,
            "XDG_CACHE_HOME",
            fallback_cache,
            "cache",
        )?,
    })
}

fn resolve_one(
    env: &dyn Fn(&str) -> Option<OsString>,
    override_var: &str,
    xdg_var: &str,
    fallback: &dyn Fn() -> Option<PathBuf>,
    what: &str,
) -> Result<PathBuf, ShellError> {
    if let Some(value) = env(override_var).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(value);
        return if path.is_absolute() {
            Ok(path)
        } else {
            Err(ShellError::InvalidConfig {
                message: format!("{override_var} must be an absolute path"),
                help: Some(format!(
                    "Set {override_var} to an absolute path, or unset it to use the default {what} directory."
                )),
            })
        };
    }
    if let Some(value) = env(xdg_var).filter(|value| !value.is_empty()) {
        let base = PathBuf::from(value);
        if base.is_absolute() {
            return Ok(base.join(XDG_SUBDIR));
        }
    }
    fallback()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| ShellError::InvalidConfig {
            message: format!("the {what} directory could not be determined"),
            help: Some(format!("Set {override_var} to an absolute path.")),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{CliDiagnostic, EXIT_FAILURE};

    fn resolve(env: &[(&str, &str)]) -> Result<ShellDirs, ShellError> {
        let lookup = |name: &str| {
            env.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        };
        resolve_dirs(
            &lookup,
            &|| Some(PathBuf::from("/platform/config")),
            &|| Some(PathBuf::from("/platform/cache")),
        )
    }

    #[test]
    fn platform_fallback() {
        assert_eq!(
            resolve(&[]).unwrap(),
            ShellDirs {
                config: "/platform/config".into(),
                cache: "/platform/cache".into(),
            }
        );
    }

    #[test]
    fn overrides_win() {
        let dirs = resolve(&[
            (CONFIG_DIR_ENV, "/explicit/config"),
            (CACHE_DIR_ENV, "/explicit/cache"),
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_CACHE_HOME", "/xdg/cache"),
        ])
        .unwrap();
        assert_eq!(dirs.config, PathBuf::from("/explicit/config"));
        assert_eq!(dirs.cache, PathBuf::from("/explicit/cache"));
    }

    #[test]
    fn absolute_xdg_then_fallback() {
        let dirs = resolve(&[
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_CACHE_HOME", "relative"),
        ])
        .unwrap();
        assert_eq!(dirs.config, PathBuf::from("/xdg/config/agentic-org"));
        assert_eq!(
            dirs.cache,
            PathBuf::from("/platform/cache"),
            "relative XDG is ignored"
        );
    }

    #[test]
    fn empty_is_unset() {
        let dirs = resolve(&[(CONFIG_DIR_ENV, ""), ("XDG_CONFIG_HOME", "")]).unwrap();
        assert_eq!(dirs.config, PathBuf::from("/platform/config"));
    }

    #[test]
    fn relative_override_is_an_error() {
        let error = resolve(&[(CACHE_DIR_ENV, "cache")]).unwrap_err();
        assert!(matches!(error, ShellError::InvalidConfig { .. }));
        assert_eq!(error.code(), "cli::config::invalid");
        assert_eq!(error.exit_code(), EXIT_FAILURE);
        assert!(error.to_string().contains(CACHE_DIR_ENV));
    }

    #[test]
    fn no_directory_is_an_error() {
        let none = resolve_dirs(&|_| None, &|| None, &|| Some("/c".into())).unwrap_err();
        assert!(none.to_string().contains("configuration"), "{none}");
        let relative = resolve_dirs(&|_| None, &|| Some("/c".into()), &|| Some("rel".into()));
        assert!(matches!(relative, Err(ShellError::InvalidConfig { .. })));
    }
}
