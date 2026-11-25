//! Runtime adapter registry: production adapters for `claude` (`ClaudeRuntime`), `codex`
//! (`CodexRuntime`), and `agy` (`AgyRuntime`), failing closed on any unknown runtime ID
//! (`AGENTS.md` Rule 1, `TASK-42`, `TASK-83`).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use org_agent_runtime::AgentRuntime;
use org_agent_runtime_agy::AgyRuntime;
use org_agent_runtime_claude::ClaudeRuntime;
use org_agent_runtime_codex::CodexRuntime;

/// Errors resolving a runtime adapter or its executable path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeRegistryError {
    UnbuiltAdapter(String),
    UnknownRuntime(String),
    BinaryNotFound(String),
}

impl fmt::Display for RuntimeRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnbuiltAdapter(id) => {
                write!(
                    f,
                    "runtime adapter `{id}` is not yet implemented on this office build"
                )
            }
            Self::UnknownRuntime(id) => write!(f, "unknown agent runtime `{id}`"),
            Self::BinaryNotFound(bin) => {
                write!(f, "runtime executable `{bin}` not found on this host")
            }
        }
    }
}

impl std::error::Error for RuntimeRegistryError {}

/// Registry of available [`AgentRuntime`] adapters and optional explicit executable overrides.
#[derive(Clone)]
pub struct RuntimeRegistry {
    adapters: BTreeMap<String, Arc<dyn AgentRuntime>>,
    binary_overrides: BTreeMap<String, PathBuf>,
}

impl Default for RuntimeRegistry {
    fn default() -> Self {
        Self::production()
    }
}

impl RuntimeRegistry {
    /// Production registry wiring all three native adapters (`claude`, `codex`, `agy`) behind
    /// the consumer-owned [`AgentRuntime`] contract. Any unknown runtime ID fails closed.
    pub fn production() -> Self {
        let mut adapters: BTreeMap<String, Arc<dyn AgentRuntime>> = BTreeMap::new();
        adapters.insert("claude".to_owned(), Arc::new(ClaudeRuntime::new()));
        adapters.insert("codex".to_owned(), Arc::new(CodexRuntime::new()));
        adapters.insert("agy".to_owned(), Arc::new(AgyRuntime::new()));
        Self {
            adapters,
            binary_overrides: BTreeMap::new(),
        }
    }

    /// Registers or replaces an adapter and optional explicit binary path (used by tests and extensions).
    pub fn register(&mut self, adapter: Arc<dyn AgentRuntime>, binary_override: Option<PathBuf>) {
        let key = adapter.id().as_str().to_owned();
        if let Some(path) = binary_override {
            self.binary_overrides.insert(key.clone(), path);
        }
        self.adapters.insert(key, adapter);
    }

    /// Resolves `runtime_id` (normalizing provider names like `"anthropic"` -> `"claude"`,
    /// `"openai"`/`"gpt"` -> `"codex"`, `"google"`/`"antigravity"` -> `"agy"`).
    pub fn resolve(&self, runtime_id: &str) -> Result<Arc<dyn AgentRuntime>, RuntimeRegistryError> {
        let key = normalize_runtime_key(runtime_id);
        self.adapters
            .get(key)
            .cloned()
            .ok_or_else(|| RuntimeRegistryError::UnknownRuntime(runtime_id.to_owned()))
    }

    /// Resolves the canonical absolute path to the runtime binary.
    ///
    /// Checks explicit overrides first, then `search_path` (or `PATH`), and when `search_path`
    /// is not overridden also probes standard user-local and Homebrew binary directories so
    /// background services (`systemd --user`, `launchd`) locate `claude`, `codex`, and `agy`.
    pub fn resolve_binary(
        &self,
        adapter: &dyn AgentRuntime,
        search_path: Option<&str>,
    ) -> Result<PathBuf, RuntimeRegistryError> {
        let runtime_id = adapter.id();
        let key = runtime_id.as_str();
        if let Some(explicit) = self.binary_overrides.get(key) {
            return explicit
                .canonicalize()
                .map_err(|_| RuntimeRegistryError::BinaryNotFound(explicit.display().to_string()));
        }
        let binary = adapter.binary_name();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        resolve_binary_in_dirs(
            binary,
            search_path,
            std::env::var_os("PATH").as_deref(),
            home.as_deref(),
        )
    }

    /// Resolves the pinned helper directory containing `agent-os`.
    ///
    /// Checks `explicit`, then `current_exe().parent()` (when it names an actual file holding a
    /// sibling `agent-os`), then `argv[0].parent()` (essential on GNU/Linux where admitted
    /// workers execute from `/memfd:... (deleted)` with their pinned path in `argv[0]`), then
    /// `PATH` and `$HOME/.local/bin`.
    pub fn resolve_helper_dir(explicit: Option<&Path>) -> PathBuf {
        let current_exe = std::env::current_exe().ok();
        let argv0 = std::env::args_os().next().map(PathBuf::from);
        let path_env = std::env::var_os("PATH");
        let home_dir = std::env::var_os("HOME").map(PathBuf::from);
        resolve_helper_dir_from(
            explicit,
            current_exe.as_deref(),
            argv0.as_deref(),
            path_env.as_deref(),
            home_dir.as_deref(),
        )
    }
}

fn fallback_user_bin_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".bun/bin"));
        dirs.push(home.join(".cargo/bin"));
        dirs.push(home.join(".npm-global/bin"));
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs
}

fn find_binary_in_dirs(dirs: impl IntoIterator<Item = PathBuf>, binary: &str) -> Option<PathBuf> {
    dirs.into_iter()
        .map(|dir| dir.join(binary))
        .filter(|candidate| candidate.is_file())
        .find_map(|candidate| candidate.canonicalize().ok())
}

fn resolve_binary_in_dirs(
    binary: &str,
    explicit_search_path: Option<&str>,
    env_path: Option<&std::ffi::OsStr>,
    home: Option<&Path>,
) -> Result<PathBuf, RuntimeRegistryError> {
    let primary: Vec<PathBuf> = match explicit_search_path {
        Some(raw) => std::env::split_paths(raw).collect(),
        None => env_path
            .map(std::env::split_paths)
            .into_iter()
            .flatten()
            .collect(),
    };
    if let Some(found) = find_binary_in_dirs(primary, binary) {
        return Ok(found);
    }
    if explicit_search_path.is_none() {
        if let Some(found) = find_binary_in_dirs(fallback_user_bin_dirs(home), binary) {
            return Ok(found);
        }
    }
    Err(RuntimeRegistryError::BinaryNotFound(binary.to_owned()))
}

fn parent_with_agent_os(exe: Option<&Path>) -> Option<PathBuf> {
    let parent = exe
        .filter(|p| p.is_absolute() && p.is_file())
        .and_then(Path::parent)?;
    parent
        .join("agent-os")
        .is_file()
        .then(|| parent.canonicalize().ok())
        .flatten()
}

fn canonical_exe_parent(exe: Option<&Path>) -> Option<PathBuf> {
    exe.filter(|p| p.is_absolute() && p.is_file())
        .and_then(Path::parent)
        .and_then(|p| p.canonicalize().ok())
}

fn resolve_helper_dir_from(
    explicit: Option<&Path>,
    current_exe: Option<&Path>,
    argv0: Option<&Path>,
    path_env: Option<&std::ffi::OsStr>,
    home_dir: Option<&Path>,
) -> PathBuf {
    explicit
        .filter(|p| p.is_dir())
        .and_then(|p| p.canonicalize().ok())
        .or_else(|| parent_with_agent_os(current_exe))
        .or_else(|| parent_with_agent_os(argv0))
        .or_else(|| {
            resolve_binary_in_dirs("agent-os", None, path_env, home_dir)
                .ok()
                .and_then(|bin| bin.parent().map(Path::to_path_buf))
        })
        .or_else(|| canonical_exe_parent(current_exe))
        .or_else(|| canonical_exe_parent(argv0))
        .unwrap_or_else(|| PathBuf::from("/usr/bin"))
}

fn normalize_runtime_key(raw: &str) -> &str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "anthropic" | "claude" | "claude-code" => "claude",
        "openai" | "codex" | "codex-cli" | "gpt" => "codex",
        "google" | "agy" | "antigravity" | "antigravity-cli" | "gemini" => "agy",
        _ => raw.trim(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_helper_dir_falls_back_to_argv0_when_current_exe_is_memfd() {
        let dir = tempfile::tempdir().unwrap();
        let release = dir.path().join("release");
        fs::create_dir_all(&release).unwrap();
        let office_bin = release.join("agentic-org-office");
        let agent_os = release.join("agent-os");
        fs::write(&office_bin, b"office").unwrap();
        fs::write(&agent_os, b"agent-os").unwrap();

        let memfd = PathBuf::from("/memfd:agentic-org-office (deleted)");
        let resolved = resolve_helper_dir_from(None, Some(&memfd), Some(&office_bin), None, None);
        assert_eq!(resolved, release.canonicalize().unwrap());

        // Relative argv[0] must never resolve CWD.
        let rel = PathBuf::from("./agentic-org-office");
        assert!(parent_with_agent_os(Some(&rel)).is_none());
        assert!(canonical_exe_parent(Some(&rel)).is_none());
    }

    #[test]
    fn resolve_binary_discovers_user_local_bin_when_systemd_path_is_minimal() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let local_bin = home.join(".local/bin");
        fs::create_dir_all(&local_bin).unwrap();
        let claude = local_bin.join("claude");
        fs::write(&claude, b"#!/bin/sh\n").unwrap();

        let empty_bin = dir.path().join("empty");
        fs::create_dir_all(&empty_bin).unwrap();
        let resolved =
            resolve_binary_in_dirs("claude", None, Some(empty_bin.as_os_str()), Some(&home))
                .unwrap();
        assert_eq!(resolved, claude.canonicalize().unwrap());

        // Explicit search_path remains strict and does not scan HOME fallbacks.
        let strict = resolve_binary_in_dirs(
            "claude",
            Some(empty_bin.to_str().unwrap()),
            None,
            Some(&home),
        );
        assert!(matches!(
            strict,
            Err(RuntimeRegistryError::BinaryNotFound(_))
        ));
    }
}
