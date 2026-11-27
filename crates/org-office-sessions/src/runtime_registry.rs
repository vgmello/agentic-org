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
    /// [`AgentRuntime::aliases`] (ASCII case-insensitively), pruning any stale aliases previously
    /// owned by this canonical ID. Canonical IDs are never clobbered by another adapter's alias.
    /// The first registered adapter becomes the default adapter for [`Self::default_adapter`].
    pub fn register(&mut self, adapter: Arc<dyn AgentRuntime>, binary_override: Option<PathBuf>) {
        let key = adapter.id().as_str().to_owned();
        let canonical_lower = key.to_ascii_lowercase();
        let adapter_aliases = adapter.aliases();
        self.aliases.retain(|_, canonical| canonical != &key);
        self.adapters.insert(key.clone(), adapter);
        self.aliases.insert(canonical_lower, key.clone());
        for alias in adapter_aliases {
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
        match binary_override {
            Some(path) => {
                self.binary_overrides.insert(key, path);
            }
            None => {
                self.binary_overrides.remove(&key);
            }
        }
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

    /// Resolves `runtime_id` against canonical adapter IDs (ASCII case-insensitively) before
    /// consulting adapter-declared aliases. Fails closed with
    /// [`RuntimeRegistryError::UnknownRuntime`] if unregistered.
    pub fn resolve(&self, runtime_id: &str) -> Result<Arc<dyn AgentRuntime>, RuntimeRegistryError> {
        let trimmed = runtime_id.trim();
        if let Some(adapter) = self.adapters.get(trimmed) {
            return Ok(Arc::clone(adapter));
        }
        if let Some((_, adapter)) = self
            .adapters
            .iter()
            .find(|(canonical, _)| canonical.eq_ignore_ascii_case(trimmed))
        {
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
    /// Checks explicit overrides first (failing closed without PATH fallback when an override is
    /// registered), then `search_path` (or `PATH`), and when `search_path` is not overridden also
    /// probes standard user-local and Homebrew binary directories so background services
    /// (`systemd --user`, `launchd`) locate registered runtime CLIs.
    pub fn resolve_binary(
        &self,
        adapter: &dyn AgentRuntime,
        search_path: Option<&str>,
    ) -> Result<PathBuf, RuntimeRegistryError> {
        let env_path = std::env::var_os("PATH");
        let home = std::env::var_os("HOME").map(PathBuf::from);
        self.resolve_binary_from(adapter, search_path, env_path.as_deref(), home.as_deref())
    }

    /// Pure testable variant of [`Self::resolve_binary`] using explicit `PATH` and `HOME` inputs
    /// without reading ambient process environment variables.
    pub(crate) fn resolve_binary_from(
        &self,
        adapter: &dyn AgentRuntime,
        search_path: Option<&str>,
        env_path: Option<&std::ffi::OsStr>,
        home: Option<&Path>,
    ) -> Result<PathBuf, RuntimeRegistryError> {
        let runtime_id = adapter.id();
        let key = runtime_id.as_str();
        if let Some(explicit) = self.binary_overrides.get(key) {
            if !explicit.is_absolute() {
                return Err(RuntimeRegistryError::BinaryNotFound(
                    explicit.display().to_string(),
                ));
            }
            return org_platform::fs::resolve_safe_executable_file(explicit).ok_or_else(|| {
                RuntimeRegistryError::BinaryNotFound(explicit.display().to_string())
            });
        }
        resolve_binary_in_dirs(adapter.binary_name(), search_path, env_path, home)
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
    if let Some(home) = home.filter(|h| h.is_absolute()) {
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
    org_platform::fs::find_safe_executable_in_dirs(dirs, binary)
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
        assert!(gemini.host_profile().host_config_subdir.is_none());
        assert!(gemini.host_profile().uses_host_oauth_token_injection);
        assert!(gemini.host_profile().config_credential_files.is_empty());
        assert!(!gemini.host_profile().host_credential_files.is_empty());
        assert_eq!(gemini.endpoint_env_var(), Some("GOOGLE_GEMINI_BASE_URL"));
        assert_eq!(gemini.auth_env_var(true, "tok"), Some("GEMINI_OAUTH_TOKEN"));
        assert_eq!(gemini.auth_env_var(false, "AIza"), Some("GEMINI_API_KEY"));
        assert_eq!(gemini.transcript_parser().unwrap().format_id(), "gemini");

        let kimi = reg.resolve("kimi").unwrap();
        assert!(kimi.host_profile().host_config_subdir.is_none());
        assert!(kimi.host_profile().uses_host_oauth_token_injection);
        assert!(kimi.host_profile().config_credential_files.is_empty());
        assert!(!kimi.host_profile().host_credential_files.is_empty());
        assert_eq!(kimi.endpoint_env_var(), Some("KIMI_BASE_URL"));
        assert_eq!(kimi.auth_env_var(true, "tok"), Some("KIMI_OAUTH_TOKEN"));
        assert_eq!(kimi.auth_env_var(false, "sk-kimi"), Some("KIMI_API_KEY"));
        assert_eq!(kimi.transcript_parser().unwrap().format_id(), "kimi");

        let pi = reg.resolve("pi").unwrap();
        assert!(pi.host_profile().host_config_subdir.is_none());
        assert!(pi.host_profile().uses_host_oauth_token_injection);
        assert!(pi.host_profile().config_credential_files.is_empty());
        assert!(!pi.host_profile().host_credential_files.is_empty());
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
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let local_bin = home.join(".local/bin");
        fs::create_dir_all(&local_bin).unwrap();
        let claude = local_bin.join("claude");
        fs::write(&claude, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();

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

    struct StubAdapter {
        id: &'static str,
        binary: &'static str,
        aliases: &'static [&'static str],
    }

    impl AgentRuntime for StubAdapter {
        fn id(&self) -> org_agent_runtime::RuntimeId {
            org_agent_runtime::RuntimeId::try_from(self.id).unwrap()
        }
        fn binary_name(&self) -> &'static str {
            self.binary
        }
        fn aliases(&self) -> &'static [&'static str] {
            self.aliases
        }
        fn capabilities(&self) -> org_agent_runtime::Capabilities {
            org_agent_runtime_acp::GeminiRuntime::new().capabilities()
        }
        fn plan_session(
            &self,
            _: &org_agent_runtime::SessionSpec,
            _: &org_agent_runtime::LaunchContext,
        ) -> Result<
            (
                org_agent_runtime::LaunchPlan,
                Box<dyn org_agent_runtime::Session>,
            ),
            org_agent_runtime::PlanError,
        > {
            Err(org_agent_runtime::PlanError::Unsupported("stub"))
        }
        fn plan_take_over(
            &self,
            _: &org_agent_runtime::SessionSpec,
            _: &org_agent_runtime::LaunchContext,
            _: u16,
            _: u16,
        ) -> Result<org_agent_runtime::LaunchPlan, org_agent_runtime::PlanError> {
            Err(org_agent_runtime::PlanError::Unsupported("stub"))
        }
        fn plan_probe(
            &self,
            _: org_agent_runtime::Probe,
            _: &org_agent_runtime::ProbeContext,
        ) -> Option<org_agent_runtime::ProbeRun> {
            None
        }
        fn parse_probe(
            &self,
            _: org_agent_runtime::Probe,
            _: &org_agent_runtime::ProbeOutput,
        ) -> org_agent_runtime::ProbeResult {
            org_agent_runtime::ProbeResult::Unavailable {
                reason: org_agent_runtime::Unavailable::NotInstalled,
            }
        }
    }

    #[test]
    fn canonical_ids_win_over_aliases_in_both_orders_and_reregister_prunes_stale_aliases() {
        let alpha = || -> Arc<dyn AgentRuntime> {
            Arc::new(StubAdapter {
                id: "alpha",
                binary: "alpha-bin",
                aliases: &["beta", "alpha-old"],
            })
        };
        let beta = || -> Arc<dyn AgentRuntime> {
            Arc::new(StubAdapter {
                id: "beta",
                binary: "beta-bin",
                aliases: &["alpha", "beta-alias"],
            })
        };

        // Order 1: alpha (with alias "beta") registered before beta.
        let mut r1 = RuntimeRegistry::empty();
        r1.register(alpha(), None);
        r1.register(beta(), None);
        assert_eq!(r1.resolve("alpha").unwrap().id().as_str(), "alpha");
        assert_eq!(r1.resolve("ALPHA").unwrap().id().as_str(), "alpha");
        assert_eq!(r1.resolve("beta").unwrap().id().as_str(), "beta");
        assert_eq!(r1.resolve("BETA").unwrap().id().as_str(), "beta");
        assert_eq!(r1.resolve("beta-alias").unwrap().id().as_str(), "beta");

        // Order 2: beta registered before alpha (with alias "beta").
        let mut r2 = RuntimeRegistry::empty();
        r2.register(beta(), None);
        r2.register(alpha(), None);
        assert_eq!(r2.resolve("alpha").unwrap().id().as_str(), "alpha");
        assert_eq!(r2.resolve("ALPHA").unwrap().id().as_str(), "alpha");
        assert_eq!(r2.resolve("beta").unwrap().id().as_str(), "beta");
        assert_eq!(r2.resolve("BETA").unwrap().id().as_str(), "beta");

        // Re-registering alpha with new aliases prunes stale `alpha-old`.
        assert_eq!(r1.resolve("alpha-old").unwrap().id().as_str(), "alpha");
        r1.register(
            Arc::new(StubAdapter {
                id: "alpha",
                binary: "alpha-bin",
                aliases: &["alpha-new"],
            }),
            None,
        );
        assert!(matches!(
            r1.resolve("alpha-old"),
            Err(RuntimeRegistryError::UnknownRuntime(_))
        ));
        assert_eq!(r1.resolve("alpha-new").unwrap().id().as_str(), "alpha");
    }

    #[test]
    fn resolve_binary_and_overrides_fail_closed_on_non_exec_dir_broken_symlink_setuid_and_bad_names(
    ) {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let non_exec = bin_dir.join("non-exec");
        fs::write(&non_exec, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&non_exec, fs::Permissions::from_mode(0o644)).unwrap();

        let subdir = bin_dir.join("a-dir");
        fs::create_dir(&subdir).unwrap();

        let broken = bin_dir.join("broken-link");
        org_platform::fs::create_symlink(&bin_dir.join("missing"), &broken).unwrap();

        let suid = bin_dir.join("suid-bin");
        fs::write(&suid, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&suid, fs::Permissions::from_mode(0o4755)).unwrap();

        let valid = bin_dir.join("valid-bin");
        fs::write(&valid, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&valid, fs::Permissions::from_mode(0o755)).unwrap();

        let adapter = Arc::new(StubAdapter {
            id: "alpha",
            binary: "valid-bin",
            aliases: &[],
        });

        // Invalid overrides MUST fail closed even when a valid executable exists on search_path.
        let mut bad_overrides = vec![
            non_exec.clone(),
            subdir.clone(),
            broken.clone(),
            PathBuf::from("./valid-bin"),
            bin_dir.join("does-not-exist"),
        ];
        if (fs::metadata(&suid).unwrap().permissions().mode() & 0o4000) != 0 {
            bad_overrides.push(suid.clone());
        }
        for bad in bad_overrides {
            let mut reg = RuntimeRegistry::empty();
            reg.register(
                Arc::clone(&adapter) as Arc<dyn AgentRuntime>,
                Some(bad.clone()),
            );
            assert!(
                reg.resolve_binary_from(
                    adapter.as_ref(),
                    Some(bin_dir.to_str().unwrap()),
                    None,
                    None
                )
                .is_err(),
                "invalid override `{}` must fail closed",
                bad.display()
            );
        }

        // Valid explicit override succeeds even with empty search_path.
        let mut ok_reg = RuntimeRegistry::empty();
        ok_reg.register(
            Arc::clone(&adapter) as Arc<dyn AgentRuntime>,
            Some(valid.clone()),
        );
        assert_eq!(
            ok_reg
                .resolve_binary_from(adapter.as_ref(), Some(""), None, None)
                .unwrap(),
            valid
        );

        // PATH discovery skips empty/relative segments and rejects bad binary names and 0644 files.
        assert!(
            resolve_binary_in_dirs("non-exec", Some(bin_dir.to_str().unwrap()), None, None)
                .is_err()
        );
        assert!(
            resolve_binary_in_dirs("a-dir", Some(bin_dir.to_str().unwrap()), None, None).is_err()
        );
        assert!(
            resolve_binary_in_dirs("broken-link", Some(bin_dir.to_str().unwrap()), None, None)
                .is_err()
        );
        assert!(resolve_binary_in_dirs("valid-bin", Some(":.:./bin"), None, None).is_err());
        assert_eq!(
            resolve_binary_in_dirs(
                "valid-bin",
                Some(&format!(":{}", bin_dir.display())),
                None,
                None
            )
            .unwrap(),
            valid
        );
        for bad_name in [
            "",
            ".",
            "..",
            "../valid-bin",
            "sub/valid-bin",
            r"sub\valid-bin",
        ] {
            assert!(
                resolve_binary_in_dirs(bad_name, Some(bin_dir.to_str().unwrap()), None, None)
                    .is_err(),
                "bad binary name `{bad_name}` must fail closed"
            );
        }
    }
}
