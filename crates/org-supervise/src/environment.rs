use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

use crate::invalid;

/// A captured search path with the verified release's helpers first. No ambient lookup.
#[derive(Debug)]
pub struct SearchPath(OsString);

impl SearchPath {
    /// Drop empty/relative, unavailable, and forbidden-root entries from the captured path.
    /// Roots must include the workspace and private home, resolved to their actual paths.
    /// The pinned helper directory must be absolute, existing and outside those roots.
    pub fn new(pinned_helpers: &Path, captured: &OsStr, forbidden: &[PathBuf]) -> io::Result<Self> {
        let roots = forbidden
            .iter()
            .map(|p| p.canonicalize())
            .collect::<io::Result<Vec<_>>>()?;
        if !pinned_helpers.is_absolute() {
            return Err(invalid("the pinned helper directory must be absolute"));
        }
        let pinned = pinned_helpers.canonicalize()?;
        if !pinned.is_dir() || roots.iter().any(|root| pinned.starts_with(root)) {
            return Err(invalid(
                "the helper directory overlaps a forbidden PATH root",
            ));
        }
        let mut paths = vec![pinned];
        for entry in std::env::split_paths(captured) {
            if !entry.is_absolute() {
                continue;
            }
            if let Ok(real) = entry.canonicalize() {
                if real.is_dir()
                    && !roots.iter().any(|root| real.starts_with(root))
                    && !paths.contains(&real)
                {
                    paths.push(real);
                }
            }
        }
        std::env::join_paths(paths)
            .map(Self)
            .map_err(|_| invalid("invalid search path"))
    }
}

/// An explicit environment. Debug never reveals values (which may hold run credentials).
pub struct Environment(pub(crate) BTreeMap<OsString, OsString>);

impl std::fmt::Debug for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Environment")
            .field("names", &self.0.keys())
            .finish()
    }
}

impl Environment {
    /// Build from captured login/service data only. No server environment is inherited.
    /// Only HOME, LANG, USER, LOGNAME, SHELL, TERM and TMPDIR are accepted here.
    pub fn new(path: SearchPath, captured: &[(OsString, OsString)]) -> io::Result<Self> {
        let mut result = Self(BTreeMap::new());
        result.0.insert("PATH".into(), path.0);
        for (name, value) in captured {
            if !matches!(
                name.to_str(),
                Some("HOME" | "LANG" | "USER" | "LOGNAME" | "SHELL" | "TERM" | "TMPDIR")
            ) {
                return Err(invalid("name is not in the captured environment allowlist"));
            }
            result.insert(name.clone(), value.clone())?;
        }
        Ok(result)
    }

    /// Add caller-owned context values. This crate validates names, never their domain meaning.
    /// ORG_* identity/binding values, GH_CONFIG_DIR and the precise git config mechanism are
    /// allowed. The session mark is installed only by the recorded-spawn path.
    pub fn with_context(mut self, context: &[(OsString, OsString)]) -> io::Result<Self> {
        for (name, value) in context {
            let name_text = name
                .to_str()
                .ok_or_else(|| invalid("invalid context variable name"))?;
            let git = matches!(
                name_text,
                "GIT_CONFIG_NOSYSTEM" | "GIT_CONFIG_GLOBAL" | "GIT_CONFIG_COUNT"
            ) || indexed(name_text, "GIT_CONFIG_KEY_")
                || indexed(name_text, "GIT_CONFIG_VALUE_");
            if !(name_text.starts_with("ORG_") || name_text == "GH_CONFIG_DIR" || git)
                || name_text == "ORG_SESSION_MARK"
            {
                return Err(invalid("name is not in the context environment allowlist"));
            }
            self.insert(name.clone(), value.clone())?;
        }
        Ok(self)
    }

    /// Add reviewed adapter switches. Any collision, injection name or unreviewed switch
    /// is an error, including a collision whose value happens to be equal.
    pub fn with_runtime_additions(
        mut self,
        additions: &[(OsString, OsString)],
    ) -> io::Result<Self> {
        for (name, value) in additions {
            if !matches!(
                name.to_str(),
                Some(
                    "DISABLE_AUTOUPDATER"
                        | "ENABLE_TOOL_SEARCH"
                        | "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS"
                )
            ) {
                return Err(invalid("unreviewed runtime environment switch"));
            }
            self.insert(name.clone(), value.clone())?;
        }
        Ok(self)
    }

    pub(crate) fn insert(&mut self, name: OsString, value: OsString) -> io::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let bytes = name.as_bytes();
        if bytes.is_empty()
            || !bytes
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || *c == b'_')
            || bytes[0].is_ascii_digit()
            || value.as_bytes().contains(&0)
        {
            return Err(invalid("invalid environment name or value"));
        }
        if self.0.contains_key(&name) {
            return Err(invalid("environment name collision"));
        }
        self.0.insert(name, value);
        Ok(())
    }
}

fn indexed(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|index| !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()))
}
