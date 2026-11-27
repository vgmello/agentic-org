//! Vendor-agnostic runtime adapter registry (`AGENTS.md` Rule 1, `TASK-42`, `TASK-83`,
//! `TASK-216`, `TASK-255`).
//!
//! Concrete vendor adapters (`ClaudeRuntime`, `CodexRuntime`, `AgyRuntime`, `GeminiRuntime`,
//! `KimiRuntime`, `PiRuntime`) are explicitly injected by the composition root
//! (`apps/agentic-org-office`) via [`RuntimeRegistry::with_adapters`]. Zero process-global state
//! is held; any unregistered or unknown runtime ID fails closed.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use org_agent_runtime::AgentRuntime;

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

/// Registry of available [`AgentRuntime`] adapters, their aliases, and optional explicit
/// executable overrides.
#[derive(Clone)]
pub struct RuntimeRegistry {
    adapters: BTreeMap<String, Arc<dyn AgentRuntime>>,
    aliases: BTreeMap<String, String>,
    binary_overrides: BTreeMap<String, PathBuf>,
    default_id: Option<String>,
}

impl RuntimeRegistry {
    /// Empty registry containing no runtime adapters; every lookup fails closed until adapters
    /// are registered with [`Self::register`].
    pub fn empty() -> Self {
        Self {
            adapters: BTreeMap::new(),
            aliases: BTreeMap::new(),
            binary_overrides: BTreeMap::new(),
            default_id: None,
        }
    }

    /// Builds a registry from an ordered sequence of adapters. The first adapter becomes the
    /// default adapter returned by [`Self::default_adapter`].
    pub fn with_adapters(adapters: impl IntoIterator<Item = Arc<dyn AgentRuntime>>) -> Self {
        let mut reg = Self::empty();
        for adapter in adapters {
            reg.register(adapter, None);
        }
        reg
    }

    /// Test-only helper returning a registry populated with all dev-dependency runtime adapters.
    /// Production callers MUST inject adapters explicitly via [`Self::with_adapters`].
    #[cfg(test)]
    pub fn production() -> Self {
        Self::with_adapters([
            Arc::new(org_agent_runtime_claude::ClaudeRuntime::new()) as Arc<dyn AgentRuntime>,
            Arc::new(org_agent_runtime_codex::CodexRuntime::new()) as Arc<dyn AgentRuntime>,
            Arc::new(org_agent_runtime_acp::AgyRuntime::new()) as Arc<dyn AgentRuntime>,
            Arc::new(org_agent_runtime_acp::GeminiRuntime::new()) as Arc<dyn AgentRuntime>,
            Arc::new(org_agent_runtime_acp::KimiRuntime::new()) as Arc<dyn AgentRuntime>,
            Arc::new(org_agent_runtime_acp::PiRuntime::new()) as Arc<dyn AgentRuntime>,
        ])
    }

    /// Registers or replaces an adapter and optional explicit binary path.
    ///
    /// Also indexes the adapter's canonical ID and every alias returned by
    /// [`AgentRuntime::aliases`] (ASCII case-insensitively). Canonical IDs are never clobbered
    /// by another adapter's alias. The first registered adapter becomes the default adapter for
    /// [`Self::default_adapter`].
    pub fn register(&mut self, adapter: Arc<dyn AgentRuntime>, binary_override: Option<PathBuf>) {
        let key = adapter.id().as_str().to_owned();
        let canonical_lower = key.to_ascii_lowercase();
        self.aliases.insert(canonical_lower, key.clone());
        for alias in adapter.aliases() {
            let trimmed = alias.trim();
            if !trimmed.is_empty() {
                let alias_lower = trimmed.to_ascii_lowercase();
                let collides_with_canonical = self
                    .adapters
                    .keys()
                    .any(|existing| existing.eq_ignore_ascii_case(&alias_lower));
                if !collides_with_canonical {
                    self.aliases.insert(alias_lower, key.clone());
                }
            }
        }
        if self.default_id.is_none() {
            self.default_id = Some(key.clone());
        }
        if let Some(path) = binary_override {
            self.binary_overrides.insert(key.clone(), path);
        }
        self.adapters.insert(key, adapter);
    }

    /// Iterates over all registered adapters in deterministic canonical ID order.
    pub fn adapters(&self) -> impl Iterator<Item = &Arc<dyn AgentRuntime>> {
        self.adapters.values()
    }

    /// Returns the default adapter (the first adapter registered), or `None` when empty.
    pub fn default_adapter(&self) -> Option<Arc<dyn AgentRuntime>> {
        self.default_id
            .as_deref()
            .and_then(|id| self.adapters.get(id))
            .or_else(|| self.adapters.values().next())
            .cloned()
    }

    /// Resolves `runtime_id` against canonical adapter IDs and adapter-declared aliases (ASCII
    /// case-insensitively). Fails closed with [`RuntimeRegistryError::UnknownRuntime`] if
    /// unregistered.
    pub fn resolve(&self, runtime_id: &str) -> Result<Arc<dyn AgentRuntime>, RuntimeRegistryError> {
        let trimmed = runtime_id.trim();
        if let Some(adapter) = self.adapters.get(trimmed) {
            return Ok(Arc::clone(adapter));
        }
        let lower = trimmed.to_ascii_lowercase();
        self.aliases
            .get(&lower)
            .and_then(|canonical| self.adapters.get(canonical))
            .cloned()
            .ok_or_else(|| RuntimeRegistryError::UnknownRuntime(runtime_id.to_owned()))
    }

    /// Resolves the canonical absolute path to the runtime binary.
    ///
    /// Checks explicit overrides first, then `search_path` (or `PATH`), and when `search_path`
    /// is not overridden also probes standard user-local and Homebrew binary directories so
    /// background services (`systemd --user`, `launchd`) locate registered runtime CLIs.
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
        .find_map(|candidate| candidate.canonicalize().ok().filter(|p| p.is_file()))
}

pub(crate) fn resolve_binary_in_dirs(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn empty_registry_fails_closed_and_adapter_aliases_resolve_case_insensitively() {
        let empty = RuntimeRegistry::empty();
        assert!(empty.default_adapter().is_none());
        assert!(matches!(
            empty.resolve("claude"),
            Err(RuntimeRegistryError::UnknownRuntime(_))
        ));

        let reg = RuntimeRegistry::production();
        assert_eq!(reg.default_adapter().unwrap().id().as_str(), "claude");
        assert_eq!(reg.resolve("ANTHROPIC").unwrap().id().as_str(), "claude");
        assert_eq!(reg.resolve("claude-code").unwrap().id().as_str(), "claude");
        assert_eq!(reg.resolve("openai").unwrap().id().as_str(), "codex");
        assert_eq!(reg.resolve("gpt").unwrap().id().as_str(), "codex");
        assert_eq!(reg.resolve("google").unwrap().id().as_str(), "agy");
        assert_eq!(reg.resolve("antigravity").unwrap().id().as_str(), "agy");
        assert_eq!(reg.resolve("gemini").unwrap().id().as_str(), "gemini");
        assert_eq!(reg.resolve("gemini-cli").unwrap().id().as_str(), "gemini");
        assert_eq!(reg.resolve("kimi").unwrap().id().as_str(), "kimi");
        assert_eq!(reg.resolve("moonshot").unwrap().id().as_str(), "kimi");
        assert_eq!(reg.resolve("pi").unwrap().id().as_str(), "pi");
        assert_eq!(reg.resolve("pi-acp").unwrap().id().as_str(), "pi");
        assert!(matches!(
            reg.resolve("unknown-vendor"),
            Err(RuntimeRegistryError::UnknownRuntime(_))
        ));

        let claude = reg.resolve("claude").unwrap();
        assert!(claude.host_profile().uses_host_oauth_token_injection);
        assert!(claude.host_profile().uses_mount_mask_sandbox);
        assert!(claude.host_profile().host_config_subdir.is_none());
        assert_eq!(claude.endpoint_env_var(), Some("ANTHROPIC_BASE_URL"));
        assert_eq!(
            claude.auth_env_var(false, "sk-ant-api-1"),
            Some("ANTHROPIC_API_KEY")
        );
        assert_eq!(
            claude.auth_env_var(false, "sk-ant-oat01-1"),
            Some("CLAUDE_CODE_OAUTH_TOKEN")
        );
        assert_eq!(claude.transcript_parser().unwrap().format_id(), "claude");

        let codex = reg.resolve("codex").unwrap();
        assert_eq!(codex.host_profile().host_config_subdir, Some(".codex"));
        assert!(codex.operator_rules_layout().is_some());
        assert_eq!(codex.endpoint_env_var(), Some("OPENAI_BASE_URL"));
        assert_eq!(codex.auth_env_var(true, "tok"), Some("CODEX_ACCESS_TOKEN"));
        assert_eq!(codex.auth_env_var(false, "sk-1"), Some("OPENAI_API_KEY"));
        assert_eq!(codex.transcript_parser().unwrap().format_id(), "codex");

        let agy = reg.resolve("agy").unwrap();
        assert_eq!(agy.host_profile().host_config_subdir, Some(".gemini"));
        assert!(agy.host_profile().needs_explicit_sandbox_ro_dirs);
        assert!(!agy.host_profile().config_credential_files.is_empty());
        assert_eq!(agy.endpoint_env_var(), Some("GOOGLE_GEMINI_BASE_URL"));
        assert_eq!(agy.auth_env_var(true, "tok"), Some("AGY_AUTH_TOKEN"));
        assert_eq!(agy.auth_env_var(false, "AIza"), Some("GEMINI_API_KEY"));
        assert_eq!(agy.transcript_parser().unwrap().format_id(), "agy");

        let gemini = reg.resolve("gemini").unwrap();
        assert_eq!(gemini.host_profile().host_config_subdir, Some(".gemini"));
        assert!(!gemini.host_profile().config_credential_files.is_empty());
        assert!(!gemini.host_profile().host_credential_files.is_empty());
        assert_eq!(gemini.endpoint_env_var(), Some("GOOGLE_GEMINI_BASE_URL"));
        assert_eq!(gemini.auth_env_var(true, "tok"), Some("GEMINI_OAUTH_TOKEN"));
        assert_eq!(gemini.auth_env_var(false, "AIza"), Some("GEMINI_API_KEY"));
        assert_eq!(gemini.transcript_parser().unwrap().format_id(), "gemini");

        let kimi = reg.resolve("kimi").unwrap();
        assert_eq!(kimi.host_profile().host_config_subdir, Some(".kimi"));
        assert!(!kimi.host_profile().config_credential_files.is_empty());
        assert_eq!(kimi.endpoint_env_var(), Some("KIMI_BASE_URL"));
        assert_eq!(kimi.auth_env_var(true, "tok"), Some("KIMI_OAUTH_TOKEN"));
        assert_eq!(kimi.auth_env_var(false, "sk-kimi"), Some("KIMI_API_KEY"));
        assert_eq!(kimi.transcript_parser().unwrap().format_id(), "kimi");

        let pi = reg.resolve("pi").unwrap();
        assert_eq!(pi.host_profile().host_config_subdir, Some(".pi"));
        assert!(!pi.host_profile().config_credential_files.is_empty());
        assert_eq!(pi.endpoint_env_var(), Some("PI_BASE_URL"));
        assert_eq!(pi.auth_env_var(true, "tok"), Some("PI_OAUTH_TOKEN"));
        assert_eq!(pi.auth_env_var(false, "sk-pi"), Some("PI_API_KEY"));
        assert_eq!(pi.transcript_parser().unwrap().format_id(), "pi");
    }

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
