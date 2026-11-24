//! Office-owned sub-agents and cross-turn background task tracking & reaping (`ADR-0025`,
//! `SUB-1..5`, `SUB-7`).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use org_agent_runtime::{ToolName, Usage};
use org_proto::sessions::{BackgroundTaskView, SessionUsageSummary};
use serde::{Deserialize, Serialize};

/// Office-owned definition written for the general-purpose sub-agent before every start (`SUB-1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentDefinition {
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub tools: Option<Vec<String>>,
}

/// Kind of tracked background work item (`ADR-0025`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundKind {
    Subagent,
    BackgroundTask,
}

impl BackgroundKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subagent => "subagent",
            Self::BackgroundTask => "background_task",
        }
    }
}

/// Lifecycle state of a tracked background task or sub-agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskState {
    Running,
    Completed,
    Failed,
    Reaped,
}

impl BackgroundTaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Reaped => "reaped",
        }
    }
}

/// Wake event emitted when a tracked background task or sub-agent completes across turns (`ADR-0025` d6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundWake {
    pub task_id: String,
    pub label: String,
    pub summary: String,
}

#[derive(Debug, Clone)]
struct TrackedTask {
    task_id: String,
    kind: BackgroundKind,
    label: String,
    pid: Option<u32>,
    state: BackgroundTaskState,
    started_at: String,
    ended_at: Option<String>,
}

/// Computes the strict intersection of `requested` tools with the parent's tool allowlist (`SUB-2`).
/// A sub-agent can NEVER gain tools outside its parent's allowlist.
pub fn subset_tools(
    parent_allowlist: Option<&[ToolName]>,
    requested: Option<&[ToolName]>,
) -> Option<Vec<ToolName>> {
    match (parent_allowlist, requested) {
        (None, None) => None,
        (None, Some(req)) => Some(req.to_vec()),
        (Some(parent), None) => Some(parent.to_vec()),
        (Some(parent), Some(req)) => Some(
            req.iter()
                .filter(|tool| parent.iter().any(|p| p == *tool))
                .cloned()
                .collect(),
        ),
    }
}

/// Builds the office-owned general-purpose sub-agent definition (`SUB-1`, `SUB-2`).
pub fn build_general_subagent_definition(
    parent_tools: Option<&[ToolName]>,
    requested_tools: Option<&[ToolName]>,
) -> SubagentDefinition {
    let effective = subset_tools(parent_tools, requested_tools);
    SubagentDefinition {
        name: "general".to_owned(),
        description: "Office-owned general-purpose helper sub-agent".to_owned(),
        prompt: "You are a bounded sub-agent launched by the parent agent inside Agentic Org. Respect all organization boundaries, workspace permissions, and parent instructions.".to_owned(),
        tools: effective.map(|list| list.into_iter().map(|t| t.as_str().to_owned()).collect()),
    }
}

/// Writes the office-owned sub-agent definition file under `runtime_dir/agents/general.json`,
/// overwriting any prior file so user-edited workspace agent files never take precedence (`SUB-1`).
pub fn write_subagent_definition(
    runtime_dir: &Path,
    definition: &SubagentDefinition,
) -> io::Result<PathBuf> {
    let agents_dir = runtime_dir.join("agents");
    fs::create_dir_all(&agents_dir)?;
    let path = agents_dir.join(format!("{}.json", definition.name));
    let json = serde_json::to_vec_pretty(definition).map_err(io::Error::other)?;
    fs::write(&path, json)?;
    Ok(path)
}

/// Tracks active and completed background tasks and sub-agents for one agent session,
/// including cross-turn persistence, completion wakes, usage attribution (`SUB-4`), and
/// full reaping on pause/stop/offboard/stall (`SUB-5`, `SUB-7`).
#[derive(Debug, Default)]
pub struct BackgroundRegistry {
    tasks: BTreeMap<String, TrackedTask>,
    pending_wakes: Vec<BackgroundWake>,
    usage: SessionUsageSummary,
}

impl BackgroundRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a newly started sub-agent or background task.
    pub fn register(
        &mut self,
        task_id: impl Into<String>,
        kind: BackgroundKind,
        label: impl Into<String>,
        pid: Option<u32>,
        started_at: impl Into<String>,
    ) {
        let task_id = task_id.into();
        if kind == BackgroundKind::Subagent {
            self.usage.subagent_invocations = self.usage.subagent_invocations.saturating_add(1);
        }
        self.tasks.insert(
            task_id.clone(),
            TrackedTask {
                task_id,
                kind,
                label: label.into(),
                pid,
                state: BackgroundTaskState::Running,
                started_at: started_at.into(),
                ended_at: None,
            },
        );
    }

    /// Attributes token usage from either the primary agent or a sub-agent into the session (`SUB-4`).
    pub fn record_usage(&mut self, usage: &Usage) {
        self.usage.input_tokens = self.usage.input_tokens.saturating_add(usage.input);
        self.usage.output_tokens = self.usage.output_tokens.saturating_add(usage.output);
        self.usage.cache_read_tokens = self
            .usage
            .cache_read_tokens
            .saturating_add(usage.cache_read);
    }

    /// Marks a background task/sub-agent completed or failed and queues a `BackgroundWake`
    /// so the session wakes across turns (`ADR-0025` d6).
    pub fn complete(
        &mut self,
        task_id: &str,
        succeeded: bool,
        summary: impl Into<String>,
        ended_at: impl Into<String>,
    ) -> Option<BackgroundWake> {
        let task = self.tasks.get_mut(task_id)?;
        if task.state != BackgroundTaskState::Running {
            return None;
        }
        task.state = if succeeded {
            BackgroundTaskState::Completed
        } else {
            BackgroundTaskState::Failed
        };
        task.ended_at = Some(ended_at.into());
        let wake = BackgroundWake {
            task_id: task.task_id.clone(),
            label: task.label.clone(),
            summary: summary.into(),
        };
        self.pending_wakes.push(wake.clone());
        Some(wake)
    }

    /// Drains queued background completion wakes.
    pub fn drain_wakes(&mut self) -> Vec<BackgroundWake> {
        std::mem::take(&mut self.pending_wakes)
    }

    /// Number of currently running background tasks / sub-agents (preserved across idle turns).
    pub fn active_count(&self) -> usize {
        self.tasks
            .values()
            .filter(|t| t.state == BackgroundTaskState::Running)
            .count()
    }

    /// Human-readable labels of all currently running background tasks/sub-agents (`ASK-7`).
    pub fn running_labels(&self) -> Vec<String> {
        self.tasks
            .values()
            .filter(|t| t.state == BackgroundTaskState::Running)
            .map(|t| format!("{} ({})", t.label, t.task_id))
            .collect()
    }

    /// Marks every still-running background task/sub-agent as `Reaped` and returns their IDs & PIDs
    /// for process cleanup on Pause, Stop, Offboard, or stall timeout (`SUB-5`, `SUB-7`).
    pub fn reap_all(&mut self, ended_at: impl Into<String>) -> Vec<(String, Option<u32>)> {
        let ended_at = ended_at.into();
        let mut reaped = Vec::new();
        for task in self.tasks.values_mut() {
            if task.state == BackgroundTaskState::Running {
                task.state = BackgroundTaskState::Reaped;
                task.ended_at = Some(ended_at.clone());
                reaped.push((task.task_id.clone(), task.pid));
            }
        }
        self.pending_wakes.clear();
        reaped
    }

    pub fn usage(&self) -> SessionUsageSummary {
        self.usage.clone()
    }

    pub fn views(&self) -> Vec<BackgroundTaskView> {
        self.tasks
            .values()
            .map(|task| BackgroundTaskView {
                task_id: task.task_id.clone(),
                kind: task.kind.as_str().to_owned(),
                label: task.label.clone(),
                state: task.state.as_str().to_owned(),
                pid: task.pid,
                started_at: task.started_at.clone(),
                ended_at: task.ended_at.clone(),
            })
            .collect()
    }
}
