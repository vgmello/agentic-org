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
        let path_env = search_path
            .map(ToOwned::to_owned)
            .or_else(|| std::env::var("PATH").ok())
            .unwrap_or_default();
        for dir in std::env::split_paths(&path_env) {
            let candidate = dir.join(binary);
            if candidate.is_file() {
                if let Ok(real) = candidate.canonicalize() {
                    return Ok(real);
                }
            }
        }
        Err(RuntimeRegistryError::BinaryNotFound(binary.to_owned()))
    }

    /// Resolves the pinned helper directory containing `agent-os` (falling back to current exe dir).
    pub fn resolve_helper_dir(explicit: Option<&Path>) -> PathBuf {
        if let Some(dir) = explicit.filter(|p| p.is_dir()) {
            if let Ok(real) = dir.canonicalize() {
                return real;
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                if let Ok(real) = parent.canonicalize() {
                    return real;
                }
            }
        }
        PathBuf::from("/usr/bin")
    }
}

fn normalize_runtime_key(raw: &str) -> &str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "anthropic" | "claude" | "claude-code" => "claude",
        "openai" | "codex" | "codex-cli" | "gpt" => "codex",
        "google" | "agy" | "antigravity" | "antigravity-cli" | "gemini" => "agy",
        _ => raw.trim(),
    }
}
