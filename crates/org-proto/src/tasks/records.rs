//! Task wire records: details, views, checklist steps, comments, watchers, history,
//! dependencies, labels, and the patch field of updates.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

/// Optional structured bug/request/definition-of-done payload (`doc-77` `task_details`).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDetailsWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_of_done: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub what_happened: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default)]
    pub severity_confirmed: bool,
    #[serde(default)]
    pub reopen_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affects_feature_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routed_by_employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_triager_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_verifier_id: Option<String>,
}

/// Compact summary row for a dependency/dependent task reference (`TASK-77`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRefSummary {
    pub id: String,
    pub key: String,
    pub title: String,
    #[serde(rename = "type")]
    pub task_type: String,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank_order: Option<u64>,
}

/// One checklist step attached to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistStepWire {
    pub id: String,
    pub position: u64,
    pub text: String,
    pub required: bool,
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_by_employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    pub revision: u64,
}

/// One public shared comment on a task (`doc-79`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCommentWire {
    pub id: String,
    pub position: u64,
    pub author_id: String,
    pub created_at: String,
    pub body_markdown: String,
}

/// One watcher entry on a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskWatcherWire {
    pub id: String,
    pub watcher_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watcher_id: Option<String>,
}

/// One immutable task history event (`doc-79`, `doc-84`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEventWire {
    pub task_id: String,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
    pub actor_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub change: Value,
    pub payload_sha256: String,
}

/// Complete task summary returned by `tasks.list` and mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskView {
    pub id: String,
    pub office_code: String,
    pub number: u64,
    /// Human-readable office-coded reference (`T-A1`, `T-B37`, `ADR-0019`).
    pub key: String,
    pub revision: u64,
    /// Owning / accountable team (`doc-77`).
    pub team_id: String,
    /// Independent currently assigned team, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_team_id: Option<String>,
    /// Optional owning project (its `owner_team_id` matches `team_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(rename = "type")]
    pub task_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_type: Option<WorkType>,
    pub title: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    pub body_markdown: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone_id: Option<String>,
    pub priority: TaskPriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    /// First-class numeric rank/order index (`TASK-77`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank_order: Option<u64>,
    /// First-class normalized searchable labels (`TASK-77`).
    #[serde(default)]
    pub labels: Vec<String>,
    /// First-class reference file paths (`TASK-77`).
    #[serde(default)]
    pub reference_files: Vec<String>,
    /// First-class modified file paths (`TASK-77`).
    #[serde(default)]
    pub modified_files: Vec<String>,
    /// Direct upstream task IDs this task depends on (`TASK-77`).
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Direct downstream task IDs that depend on this task (`TASK-77`).
    #[serde(default)]
    pub dependents: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<TaskDetailsWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub created_by_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub updated_by_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by_id: Option<String>,
    pub checklist_total: u64,
    pub checklist_done: u64,
    pub comment_count: u64,
    pub watcher_count: u64,
}

/// Full inspected task with checklist, comments, watchers, history, and dependency summaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskInspectResult {
    pub task: TaskView,
    pub checklist: Vec<ChecklistStepWire>,
    pub comments: Vec<TaskCommentWire>,
    pub watchers: Vec<TaskWatcherWire>,
    pub depends_on_tasks: Vec<TaskRefSummary>,
    pub dependent_tasks: Vec<TaskRefSummary>,
    pub history: Vec<TaskEventWire>,
}

/// Directed edge in a task dependency hierarchy (`TASK-77`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDependencyEdge {
    /// Task that depends on `depends_on_task_id`.
    pub task_id: String,
    /// Upstream prerequisite task ID.
    pub depends_on_task_id: String,
}

/// Full upstream + downstream dependency hierarchy for one task (`TASK-77`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDependenciesResult {
    pub task: TaskRefSummary,
    /// Direct upstream tasks (`this` depends on them).
    pub direct_depends_on: Vec<TaskRefSummary>,
    /// Direct downstream tasks (they depend on `this`).
    pub direct_dependents: Vec<TaskRefSummary>,
    /// Transitive upstream ancestors in topological BFS depth order.
    pub transitive_depends_on: Vec<TaskRefSummary>,
    /// Transitive downstream descendants in topological BFS depth order.
    pub transitive_dependents: Vec<TaskRefSummary>,
    /// All edges in the induced dependency subgraph around `task`.
    pub edges: Vec<TaskDependencyEdge>,
}

/// Label summary with matching task count (`TASK-77`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskLabelSummary {
    pub label: String,
    pub task_count: u64,
}

/// Explicit double-optional patch field for `tasks.update` (absent = keep, `Some(None)` = clear,
/// `Some(Some(v))` = set).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum PatchField<T> {
    #[default]
    Unchanged,
    Set(Option<T>),
}

impl<T: Serialize> Serialize for PatchField<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Unchanged => serializer.serialize_none(),
            Self::Set(val) => val.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for PatchField<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::Set(Option::<T>::deserialize(deserializer)?))
    }
}

impl<T> PatchField<T> {
    pub fn is_unchanged(&self) -> bool {
        matches!(self, Self::Unchanged)
    }
}
