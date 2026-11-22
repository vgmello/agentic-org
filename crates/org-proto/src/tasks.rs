//! Unified v1 typed tasks, checklists, comments, watchers, immutable history,
//! dependencies hierarchy, searchable labels, reference/modified files, and ordering
//! (`TASK-49` B4/B5 + `TASK-77` B4b; `doc-77`, `doc-84`, `ADR-0019`, `ADR-0031`).
//!
//! Every write uses a canonical lowercase UUIDv7 `command_id` for idempotency and acts as the
//! authenticated active employee bound to the connection (`ADR-0031`, `TASKAUTH-1..3`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::Call;

/// Maximum task title length in Unicode scalar values.
pub const TASK_TITLE_MAX_CHARS: usize = 200;
/// Maximum Markdown body length in Unicode scalar values.
pub const TASK_BODY_MAX_CHARS: usize = 64_000;
/// Maximum comment body length in Unicode scalar values.
pub const TASK_COMMENT_MAX_CHARS: usize = 16_000;
/// Maximum checklist step text length in Unicode scalar values.
pub const TASK_STEP_TEXT_MAX_CHARS: usize = 500;
/// Maximum checklist steps per task.
pub const TASK_MAX_CHECKLIST_STEPS: usize = 200;
/// Maximum labels attached to one task.
pub const TASK_MAX_LABELS: usize = 64;
/// Maximum label length in ASCII bytes.
pub const TASK_LABEL_MAX_LEN: usize = 64;
/// Maximum reference or modified files per role on one task.
pub const TASK_MAX_FILES_PER_ROLE: usize = 256;
/// Maximum relative file path length in bytes.
pub const TASK_FILE_PATH_MAX_LEN: usize = 1024;
/// Maximum direct dependencies per task.
pub const TASK_MAX_DEPENDENCIES: usize = 128;

/// Unified task lifecycle status (`doc-77`: same closed list across `task`, `bug`, `request`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    Blocked,
    Cancelled,
    Done,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
            Self::Done => "done",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "todo" => Some(Self::Todo),
            "in_progress" => Some(Self::InProgress),
            "blocked" => Some(Self::Blocked),
            "cancelled" => Some(Self::Cancelled),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}

/// Task priority (`doc-77`: `urgent | high | normal | low`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Urgent,
    High,
    #[default]
    Normal,
    Low,
}

impl TaskPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Urgent => "urgent",
            Self::High => "high",
            Self::Normal => "normal",
            Self::Low => "low",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "urgent" => Some(Self::Urgent),
            "high" => Some(Self::High),
            "normal" => Some(Self::Normal),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

/// Optional Build/Run classification (`doc-77`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkType {
    Build,
    Run,
}

impl WorkType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Run => "run",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "build" => Some(Self::Build),
            "run" => Some(Self::Run),
            _ => None,
        }
    }
}

/// Multi-label filter match mode (`TASK-77`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelMatchMode {
    /// Task must carry ALL specified labels (AND).
    #[default]
    All,
    /// Task must carry AT LEAST ONE specified label (OR).
    Any,
}

/// List ordering mode (`TASK-77`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSortMode {
    /// Explicit `rank_order` ascending (nulls last), then `(office_code, number)` ascending.
    #[default]
    Order,
    /// Office-coded `(office_code, number)` ascending.
    Number,
    /// Most recently updated first.
    UpdatedDesc,
}

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

// ---------------------------------------------------------------------------
// RPC definitions
// ---------------------------------------------------------------------------

/// `tasks.create`: create a unified task (`task`, `bug`, `request`, ...), minting its final
/// office-coded `T-<CODE><N>` reference locally at commit (`ADR-0019`, `TASK-49`, `TASK-77`).
pub struct TasksCreate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksCreateParams {
    pub command_id: String,
    pub task_id: String,
    /// Owning team ID; optional when `project_id` is given (defaults to project's `owner_team_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    /// Defaults to `"task"`; may be `"bug"`, `"request"`, or any nonempty task type.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_type: Option<WorkType>,
    pub title: String,
    #[serde(default)]
    pub body_markdown: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<TaskPriority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank_order: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_files: Vec<String>,
    /// Initial upstream dependency task IDs or `T-<CODE><N>` references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<TaskDetailsWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksMutationResult {
    pub task: TaskView,
}

impl Call for TasksCreate {
    const METHOD: &'static str = "tasks.create";
    type Params = TasksCreateParams;
    type Result = TasksMutationResult;
}

/// `tasks.list`: query tasks with rich filtering (type, status, owning team, assigned team,
/// assignee, project, multi-label AND/OR, dependency relation, text search, and ordering).
pub struct TasksList;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksListParams {
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default)]
    pub label_match: LabelMatchMode,
    /// Optional task ref/ID: return only tasks that directly depend on this task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<String>,
    /// Optional task ref/ID: return only tasks that this task directly depends on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependent_of: Option<String>,
    /// Case-insensitive substring filter over key, title, body, labels, or file paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default)]
    pub sort: TaskSortMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksListResult {
    pub tasks: Vec<TaskView>,
    pub total: u64,
}

impl Call for TasksList {
    const METHOD: &'static str = "tasks.list";
    type Params = TasksListParams;
    type Result = TasksListResult;
}

/// `tasks.inspect`: inspect one task by UUIDv7 ID or `T-<CODE><N>` key.
pub struct TasksInspect;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksInspectParams {
    pub task: String,
}

impl Call for TasksInspect {
    const METHOD: &'static str = "tasks.inspect";
    type Params = TasksInspectParams;
    type Result = TaskInspectResult;
}

/// `tasks.update`: update task fields (title, body, type, priority, effort, severity, rank_order,
/// labels, reference_files, modified_files, details) at `expected_revision`.
pub struct TasksUpdate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksUpdateParams {
    pub command_id: String,
    pub task: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(default, skip_serializing_if = "PatchField::is_unchanged")]
    pub work_type: PatchField<WorkType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<TaskPriority>,
    #[serde(default, skip_serializing_if = "PatchField::is_unchanged")]
    pub effort: PatchField<u64>,
    #[serde(default, skip_serializing_if = "PatchField::is_unchanged")]
    pub severity: PatchField<String>,
    #[serde(default, skip_serializing_if = "PatchField::is_unchanged")]
    pub rank_order: PatchField<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_reference_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_reference_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_modified_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_modified_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<TaskDetailsWire>,
}

impl Call for TasksUpdate {
    const METHOD: &'static str = "tasks.update";
    type Params = TasksUpdateParams;
    type Result = TasksMutationResult;
}

/// `tasks.transition`: change a task's status (`todo`, `in_progress`, `blocked`, `cancelled`, `done`).
pub struct TasksTransition;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksTransitionParams {
    pub command_id: String,
    pub task: String,
    pub expected_revision: u64,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Call for TasksTransition {
    const METHOD: &'static str = "tasks.transition";
    type Params = TasksTransitionParams;
    type Result = TasksMutationResult;
}

/// `tasks.assign`: atomically set `assigned_team_id` and `assignee_id` (`doc-77`, `doc-84`).
pub struct TasksAssign;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksAssignParams {
    pub command_id: String,
    pub task: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "PatchField::is_unchanged")]
    pub assigned_team_id: PatchField<String>,
    #[serde(default, skip_serializing_if = "PatchField::is_unchanged")]
    pub assignee_id: PatchField<String>,
}

impl Call for TasksAssign {
    const METHOD: &'static str = "tasks.assign";
    type Params = TasksAssignParams;
    type Result = TasksMutationResult;
}

/// `tasks.comment`: append an immutable shared public comment on a task (`doc-79`).
pub struct TasksComment;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksCommentParams {
    pub command_id: String,
    pub task: String,
    pub comment_id: String,
    pub body_markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksCommentResult {
    pub task: TaskView,
    pub comment: TaskCommentWire,
}

impl Call for TasksComment {
    const METHOD: &'static str = "tasks.comment";
    type Params = TasksCommentParams;
    type Result = TasksCommentResult;
}

/// `tasks.comments`: list public comments on a task in ascending position order.
pub struct TasksComments;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksCommentsParams {
    pub task: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksCommentsResult {
    pub task_id: String,
    pub comments: Vec<TaskCommentWire>,
}

impl Call for TasksComments {
    const METHOD: &'static str = "tasks.comments";
    type Params = TasksCommentsParams;
    type Result = TasksCommentsResult;
}

/// `tasks.watch`: add the bound employee as a watcher on a task.
pub struct TasksWatch;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksWatchParams {
    pub command_id: String,
    pub task: String,
}

impl Call for TasksWatch {
    const METHOD: &'static str = "tasks.watch";
    type Params = TasksWatchParams;
    type Result = TasksMutationResult;
}

/// `tasks.unwatch`: remove the bound employee from a task's watchers.
pub struct TasksUnwatch;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksUnwatchParams {
    pub command_id: String,
    pub task: String,
}

impl Call for TasksUnwatch {
    const METHOD: &'static str = "tasks.unwatch";
    type Params = TasksUnwatchParams;
    type Result = TasksMutationResult;
}

/// `tasks.history`: list immutable task events in ascending revision order (`doc-84`).
pub struct TasksHistory;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksHistoryParams {
    pub task: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksHistoryResult {
    pub task_id: String,
    pub events: Vec<TaskEventWire>,
}

impl Call for TasksHistory {
    const METHOD: &'static str = "tasks.history";
    type Params = TasksHistoryParams;
    type Result = TasksHistoryResult;
}

/// `tasks.dependencies`: query direct and transitive upstream dependencies and downstream
/// dependents for a task (`TASK-77`).
pub struct TasksDependencies;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksDependenciesParams {
    pub task: String,
}

impl Call for TasksDependencies {
    const METHOD: &'static str = "tasks.dependencies";
    type Params = TasksDependenciesParams;
    type Result = TaskDependenciesResult;
}

/// `tasks.add_dependency`: add a `depends_on` edge (`task` depends on `depends_on_task`),
/// rejecting self-loops and transitive cycles (`TASK-77`).
pub struct TasksAddDependency;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksAddDependencyParams {
    pub command_id: String,
    pub task: String,
    pub depends_on_task: String,
}

impl Call for TasksAddDependency {
    const METHOD: &'static str = "tasks.add_dependency";
    type Params = TasksAddDependencyParams;
    type Result = TasksMutationResult;
}

/// `tasks.remove_dependency`: remove a `depends_on` edge between two tasks (`TASK-77`).
pub struct TasksRemoveDependency;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksRemoveDependencyParams {
    pub command_id: String,
    pub task: String,
    pub depends_on_task: String,
}

impl Call for TasksRemoveDependency {
    const METHOD: &'static str = "tasks.remove_dependency";
    type Params = TasksRemoveDependencyParams;
    type Result = TasksMutationResult;
}

/// `tasks.labels`: search and list distinct normalized labels with task counts (`TASK-77`).
pub struct TasksLabels;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksLabelsParams {
    /// Optional substring or prefix search over normalized labels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksLabelsResult {
    pub labels: Vec<TaskLabelSummary>,
}

impl Call for TasksLabels {
    const METHOD: &'static str = "tasks.labels";
    type Params = TasksLabelsParams;
    type Result = TasksLabelsResult;
}

/// `tasks.checklist.add`: append a checklist step to a task.
pub struct TasksChecklistAdd;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistAddParams {
    pub command_id: String,
    pub task: String,
    pub step_id: String,
    pub text: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistResult {
    pub task: TaskView,
    pub checklist: Vec<ChecklistStepWire>,
}

impl Call for TasksChecklistAdd {
    const METHOD: &'static str = "tasks.checklist.add";
    type Params = TasksChecklistAddParams;
    type Result = TasksChecklistResult;
}

/// `tasks.checklist.check`: mark a checklist step done or undone.
pub struct TasksChecklistCheck;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistCheckParams {
    pub command_id: String,
    pub task: String,
    pub step_id: String,
    pub done: bool,
}

impl Call for TasksChecklistCheck {
    const METHOD: &'static str = "tasks.checklist.check";
    type Params = TasksChecklistCheckParams;
    type Result = TasksChecklistResult;
}

/// `tasks.checklist.edit`: update a checklist step's text and/or required flag.
pub struct TasksChecklistEdit;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistEditParams {
    pub command_id: String,
    pub task: String,
    pub step_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

impl Call for TasksChecklistEdit {
    const METHOD: &'static str = "tasks.checklist.edit";
    type Params = TasksChecklistEditParams;
    type Result = TasksChecklistResult;
}

/// `tasks.checklist.remove`: remove a checklist step from a task.
pub struct TasksChecklistRemove;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistRemoveParams {
    pub command_id: String,
    pub task: String,
    pub step_id: String,
}

impl Call for TasksChecklistRemove {
    const METHOD: &'static str = "tasks.checklist.remove";
    type Params = TasksChecklistRemoveParams;
    type Result = TasksChecklistResult;
}

/// `tasks.checklist.move`: reorder a checklist step to a zero-based `position`.
pub struct TasksChecklistMove;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistMoveParams {
    pub command_id: String,
    pub task: String,
    pub step_id: String,
    pub position: u64,
}

impl Call for TasksChecklistMove {
    const METHOD: &'static str = "tasks.checklist.move";
    type Params = TasksChecklistMoveParams;
    type Result = TasksChecklistResult;
}
