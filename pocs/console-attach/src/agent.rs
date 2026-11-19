use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_MAX_EVENTS: usize = 2_000;

/// User-friendly security & permission presets exposed under Advanced Settings.
/// Secure by default (`SafeAsk` with OS sandbox enabled).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPreset {
    /// Default: sandboxed workspace, ask before running commands or editing outside rules.
    #[default]
    SafeAsk,
    /// Read-only / planning: no edits or state-changing commands.
    ReadOnly,
    /// Auto-approve file edits inside the workspace, ask before shell commands.
    WorkspaceEdits,
    /// Run autonomously inside the OS workspace sandbox (no per-command prompts).
    AutonomousSandboxed,
    /// Unrestricted: disables sandbox and approval prompts (use only inside an external VM/container).
    Unrestricted,
}

/// Descriptive metadata for rendering friendly UI controls under Advanced Settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetInfo {
    pub id: PermissionPreset,
    pub label: String,
    pub description: String,
    pub sandbox_default: bool,
    pub dangerous: bool,
    pub claude_permission_mode: String,
    pub codex_flags: Vec<String>,
}

impl PermissionPreset {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "safe_ask" | "safe-ask" | "default" => Ok(Self::SafeAsk),
            "read_only" | "read-only" | "plan" => Ok(Self::ReadOnly),
            "workspace_edits" | "workspace-edits" | "accept_edits" | "acceptedits" => {
                Ok(Self::WorkspaceEdits)
            }
            "autonomous_sandboxed" | "autonomous-sandboxed" | "auto" => {
                Ok(Self::AutonomousSandboxed)
            }
            "unrestricted" | "bypass" => Ok(Self::Unrestricted),
            other => Err(format!(
                "unknown preset '{other}'. Valid presets: safe_ask, read_only, workspace_edits, autonomous_sandboxed, unrestricted"
            )),
        }
    }

    pub fn claude_permission_mode(self) -> &'static str {
        match self {
            Self::SafeAsk => "manual",
            Self::ReadOnly => "plan",
            Self::WorkspaceEdits => "acceptEdits",
            Self::AutonomousSandboxed => "auto",
            Self::Unrestricted => "bypassPermissions",
        }
    }

    pub fn default_sandbox_enabled(self) -> bool {
        !matches!(self, Self::Unrestricted)
    }

    /// Returns Claude `--settings` JSON enabling/disabling the built-in OS sandbox.
    pub fn claude_sandbox_settings(self, sandbox_enabled: bool) -> Option<String> {
        if !sandbox_enabled {
            return None;
        }
        let auto_allow_bash = matches!(self, Self::AutonomousSandboxed);
        Some(
            serde_json::json!({
                "sandbox": {
                    "enabled": true,
                    "autoAllowBashIfSandboxed": auto_allow_bash
                }
            })
            .to_string(),
        )
    }

    /// Returns equivalent Codex CLI flags for this preset.
    pub fn codex_flags(self, sandbox_enabled: bool) -> Vec<String> {
        if !sandbox_enabled && matches!(self, Self::Unrestricted) {
            return vec!["--dangerously-bypass-approvals-and-sandbox".to_string()];
        }
        match self {
            Self::SafeAsk => vec![
                "-s".into(),
                "workspace-write".into(),
                "-a".into(),
                "on-request".into(),
            ],
            Self::ReadOnly => vec![
                "-s".into(),
                "read-only".into(),
                "-a".into(),
                "on-request".into(),
            ],
            Self::WorkspaceEdits => vec![
                "-s".into(),
                "workspace-write".into(),
                "--approve-for-me".into(),
            ],
            Self::AutonomousSandboxed => vec![
                "-s".into(),
                "workspace-write".into(),
                "-a".into(),
                "never".into(),
            ],
            Self::Unrestricted => vec![
                "-s".into(),
                "danger-full-access".into(),
                "-a".into(),
                "never".into(),
            ],
        }
    }

    pub fn all_info() -> Vec<PresetInfo> {
        [
            (
                Self::SafeAsk,
                "Ask before actions (Recommended)",
                "Sandboxed to workspace; routes tool/command approvals to you before execution.",
                false,
            ),
            (
                Self::ReadOnly,
                "Read-only / Plan",
                "Inspect code and plan changes without editing files or running mutating commands.",
                false,
            ),
            (
                Self::WorkspaceEdits,
                "Auto-approve edits, ask for shell",
                "File edits inside the workspace run automatically; shell commands still ask approval.",
                false,
            ),
            (
                Self::AutonomousSandboxed,
                "Autonomous (sandboxed)",
                "Runs edits and sandboxed workspace commands automatically without pausing for prompts.",
                false,
            ),
            (
                Self::Unrestricted,
                "Unrestricted (no sandbox, no prompts)",
                "Disables OS sandbox and all approval prompts. Use only inside an external VM or container.",
                true,
            ),
        ]
        .into_iter()
        .map(|(id, label, description, dangerous)| {
            let sandbox_default = id.default_sandbox_enabled();
            PresetInfo {
                id,
                label: label.to_string(),
                description: description.to_string(),
                sandbox_default,
                dangerous,
                claude_permission_mode: id.claude_permission_mode().to_string(),
                codex_flags: id.codex_flags(sandbox_default),
            }
        })
        .collect()
    }
}

/// A pending tool-permission approval requested by the agent runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingApproval {
    pub request_id: String,
    pub tool_name: String,
    pub input: Value,
}

/// A structured event emitted by a long-lived agent session (Claude stream-json, Codex app-server, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    pub seq: u64,
    pub ts_ms: u64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub raw: Value,
}

/// Snapshot of a running structured agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub backend: String,
    pub pid: u32,
    pub alive: bool,
    pub session_id: Option<String>,
    pub state: String,
    pub turns_completed: u64,
    pub pending_approvals: Vec<PendingApproval>,
    pub last_seq: u64,
}

struct EventLogInner {
    events: VecDeque<AgentEvent>,
    max_events: usize,
    next_seq: u64,
    closed: bool,
}

/// Thread-safe bounded event log supporting cursor-based tailing and result waiting.
pub struct EventLog {
    started_at: Instant,
    inner: Mutex<EventLogInner>,
    changed: Condvar,
}

impl Default for EventLog {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_MAX_EVENTS)
    }
}

impl EventLog {
    pub fn with_capacity(max_events: usize) -> Self {
        Self {
            started_at: Instant::now(),
            inner: Mutex::new(EventLogInner {
                events: VecDeque::with_capacity(max_events.min(256)),
                max_events: max_events.max(32),
                next_seq: 1,
                closed: false,
            }),
            changed: Condvar::new(),
        }
    }

    /// Appends a new event and wakes any waiting readers.
    pub fn push(&self, kind: impl Into<String>, text: Option<String>, raw: Value) -> AgentEvent {
        let mut guard = self.inner.lock().expect("event log lock poisoned");
        let seq = guard.next_seq;
        guard.next_seq += 1;
        let event = AgentEvent {
            seq,
            ts_ms: self.started_at.elapsed().as_millis() as u64,
            kind: kind.into(),
            text,
            raw,
        };
        if guard.events.len() >= guard.max_events {
            guard.events.pop_front();
        }
        guard.events.push_back(event.clone());
        self.changed.notify_all();
        event
    }

    pub fn mark_closed(&self) {
        let mut guard = self.inner.lock().expect("event log lock poisoned");
        guard.closed = true;
        self.changed.notify_all();
    }

    pub fn last_seq(&self) -> u64 {
        let guard = self.inner.lock().expect("event log lock poisoned");
        guard.next_seq.saturating_sub(1)
    }

    pub fn since(&self, after_seq: u64) -> Vec<AgentEvent> {
        let guard = self.inner.lock().expect("event log lock poisoned");
        guard
            .events
            .iter()
            .filter(|ev| ev.seq > after_seq)
            .cloned()
            .collect()
    }

    /// Blocks until at least one event with `seq > after_seq` arrives, the log closes, or `timeout` elapses.
    pub fn wait_since(&self, after_seq: u64, timeout: Duration) -> Vec<AgentEvent> {
        let deadline = Instant::now() + timeout;
        let mut guard = self.inner.lock().expect("event log lock poisoned");
        loop {
            let matching: Vec<AgentEvent> = guard
                .events
                .iter()
                .filter(|ev| ev.seq > after_seq)
                .cloned()
                .collect();
            if !matching.is_empty() || guard.closed {
                return matching;
            }
            let now = Instant::now();
            if now >= deadline {
                return Vec::new();
            }
            let (next_guard, _) = self
                .changed
                .wait_timeout(guard, deadline - now)
                .expect("event log condvar poisoned");
            guard = next_guard;
        }
    }

    /// Waits for the first event with `seq > after_seq` and `kind == "result"`.
    pub fn wait_for_result(&self, after_seq: u64, timeout: Duration) -> Option<AgentEvent> {
        let deadline = Instant::now() + timeout;
        let mut guard = self.inner.lock().expect("event log lock poisoned");
        loop {
            if let Some(ev) = guard
                .events
                .iter()
                .find(|ev| ev.seq > after_seq && ev.kind == "result")
            {
                return Some(ev.clone());
            }
            if guard.closed {
                return None;
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (next_guard, _) = self
                .changed
                .wait_timeout(guard, deadline - now)
                .expect("event log condvar poisoned");
            guard = next_guard;
        }
    }
}

/// Formats an `AgentEvent` into a concise single- or multi-line human summary.
pub fn format_event_human(ev: &AgentEvent) -> String {
    match &ev.text {
        Some(text) if !text.is_empty() => {
            format!(
                "[#{:<3} {:>6}ms] {:<18} {}",
                ev.seq, ev.ts_ms, ev.kind, text
            )
        }
        _ => format!("[#{:<3} {:>6}ms] {}", ev.seq, ev.ts_ms, ev.kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preset_is_secure_and_sandboxed() {
        let preset = PermissionPreset::default();
        assert_eq!(preset, PermissionPreset::SafeAsk);
        assert!(preset.default_sandbox_enabled());
        assert_eq!(preset.claude_permission_mode(), "manual");
        let settings = preset.claude_sandbox_settings(true).unwrap();
        assert!(settings.contains("\"enabled\":true"));
        assert!(settings.contains("\"autoAllowBashIfSandboxed\":false"));
    }

    #[test]
    fn parses_friendly_presets_and_maps_codex_flags() {
        assert_eq!(
            PermissionPreset::parse("read-only").unwrap(),
            PermissionPreset::ReadOnly
        );
        assert_eq!(
            PermissionPreset::parse("workspace_edits").unwrap(),
            PermissionPreset::WorkspaceEdits
        );
        assert_eq!(
            PermissionPreset::parse("autonomous_sandboxed").unwrap(),
            PermissionPreset::AutonomousSandboxed
        );
        let unrestricted = PermissionPreset::parse("unrestricted").unwrap();
        assert!(!unrestricted.default_sandbox_enabled());
        assert_eq!(
            unrestricted.codex_flags(false),
            vec!["--dangerously-bypass-approvals-and-sandbox"]
        );
        assert_eq!(PermissionPreset::all_info().len(), 5);
    }
}
