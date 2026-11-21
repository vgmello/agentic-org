//! Runtime adapter registry: `ClaudeRuntime` production adapter + fail-closed slots for
//! planned `codex` and `agy` adapters (`AGENTS.md` Rule 1, `TASK-42`).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use org_agent_runtime::{
    AgentRuntime, Capabilities, CostBasis, LaunchContext, LaunchPlan, PlanError, Probe,
    ProbeContext, ProbeOutput, ProbeResult, ProbeRun, RuntimeId, Session, SessionSpec, Unavailable,
    UsageScope, VersionRange,
};
use org_agent_runtime_claude::ClaudeRuntime;

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

/// Fail-closed placeholder adapter for planned runtimes (`codex`, `agy`) per Architecture Rule 1.
#[derive(Debug, Clone, Copy)]
struct RefusedRuntime {
    id: &'static str,
    binary: &'static str,
}

impl AgentRuntime for RefusedRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from(self.id).expect("static runtime id is valid")
    }

    fn binary_name(&self) -> &'static str {
        self.binary
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            steer: false,
            interrupt_in_band: false,
            idle_interrupt_ends_turn: false,
            approval_bridge: false,
            deny_message_reaches_model: false,
            runtime_cancels_approvals: false,
            unsolicited_turns: false,
            session_id_known_up_front: false,
            resume_ignores_cwd: false,
            resume_applies_instructions: false,
            reads_workspace_instruction_files: false,
            tools_detach_from_group: false,
            take_over: false,
            enforces_writable_roots: false,
            enforces_deny_write: false,
            enforces_deny_read: false,
            enforces_unix_socket_allowlist: false,
            enforces_tool_allowlist: false,
            enforces_deny_rules: false,
            usage_scope: UsageScope::TurnDelta,
            cost: CostBasis::ListPriceEstimate,
            tested_versions: VersionRange {
                min: "0.0.0".to_owned(),
                max: "0.0.0".to_owned(),
            },
        }
    }

    fn plan_session(
        &self,
        _spec: &SessionSpec,
        _ctx: &LaunchContext,
    ) -> Result<(LaunchPlan, Box<dyn Session>), PlanError> {
        Err(PlanError::Unsupported(
            "runtime adapter is not yet built in this release",
        ))
    }

    fn plan_take_over(
        &self,
        _spec: &SessionSpec,
        _ctx: &LaunchContext,
        _cols: u16,
        _rows: u16,
    ) -> Result<LaunchPlan, PlanError> {
        Err(PlanError::Unsupported(
            "runtime adapter is not yet built in this release",
        ))
    }

    fn plan_probe(&self, _probe: Probe, _ctx: &ProbeContext) -> Option<ProbeRun> {
        None
    }

    fn parse_probe(&self, _probe: Probe, _out: &ProbeOutput) -> ProbeResult {
        ProbeResult::Unavailable {
            reason: Unavailable::NotInstalled,
        }
    }
}

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
    /// Production registry: `claude` backed by [`ClaudeRuntime`]; `codex` and `agy` present
    /// behind the consumer-owned [`AgentRuntime`] contract and failing closed until implemented.
    pub fn production() -> Self {
        let mut adapters: BTreeMap<String, Arc<dyn AgentRuntime>> = BTreeMap::new();
        adapters.insert("claude".to_owned(), Arc::new(ClaudeRuntime::new()));
        adapters.insert(
            "codex".to_owned(),
            Arc::new(RefusedRuntime {
                id: "codex",
                binary: "codex",
            }),
        );
        adapters.insert(
            "agy".to_owned(),
            Arc::new(RefusedRuntime {
                id: "agy",
                binary: "agy",
            }),
        );
        Self {
            adapters,
            binary_overrides: BTreeMap::new(),
        }
    }

    /// Registers or replaces an adapter and optional explicit binary path (used by tests and future adapters).
    pub fn register(&mut self, adapter: Arc<dyn AgentRuntime>, binary_override: Option<PathBuf>) {
        let key = adapter.id().as_str().to_owned();
        if let Some(path) = binary_override {
            self.binary_overrides.insert(key.clone(), path);
        }
        self.adapters.insert(key, adapter);
    }

    /// Resolves `runtime_id` (normalizing provider names like `"anthropic"` -> `"claude"`).
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
        "openai" | "codex" | "codex-cli" => "codex",
        "google" | "agy" | "antigravity" => "agy",
        _ => raw.trim(),
    }
}
