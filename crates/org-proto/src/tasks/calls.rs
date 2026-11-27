//! The task calls (`tasks.*`).

use serde::{Deserialize, Serialize};

use super::records::{
    ChecklistStepWire, PatchField, TaskCommentWire, TaskDependenciesResult, TaskDetailsWire,
    TaskEventWire, TaskInspectResult, TaskLabelSummary, TaskView,
};
use super::{
    ChecklistSection, LabelMatchMode, TaskPriority, TaskSortMode, TaskStatus, TaskStatusScope,
    WorkType,
};
use crate::calls::Call;

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
    /// The task's definition of done (`TASK-88`), copied into its checklist's `dod` section as
    /// required steps in the create's own commit. Absent: the project's `default_dod`, or else
    /// the organization's, as they stand at create; `[]`: none. Later changes to either
    /// default never change a task already created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dod: Option<Vec<String>>,
    /// Initial staged upload IDs or readable attachment IDs bound to the new task (`TASK-98.4`, max 20).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
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
///
/// The status filters combine as one set (`TASK-105`): start from `status_scope` (every status
/// when absent or `all`), keep only `status` when given, then drop `exclude_status`. A `status`
/// outside `status_scope`, or named in `exclude_status` too, is refused as `invalid_request`.
/// `done_since` then keeps a `done` task only when its `updated_at` is at or after that instant,
/// and with `status_scope = open` it also admits those recently done tasks (a board's open work
/// and what was just finished). Every other filter narrows the set further.
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
    /// At most this many tasks, after `offset`. `Some(0)` answers only `total`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Leave out tasks in any of these statuses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_status: Vec<TaskStatus>,
    /// Only tasks no employee is assigned to (`assignee_id` absent).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unassigned: bool,
    /// Only tasks whose `modified_files` name this path, or a file under it as a directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_file: Option<String>,
    /// Skip this many matching tasks, in the sort order, before the page starts. `total` still
    /// counts every match, so a client pages with `offset + tasks.len() < total`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// `open`, `closed` or `all` statuses; absent answers every status, as before `TASK-105`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_scope: Option<TaskStatusScope>,
    /// An RFC 3339 instant (`2026-11-26T00:00:00Z`): `done` tasks last updated before it are
    /// left out, and `status_scope = open` admits the `done` tasks updated since. Anything that
    /// is not an RFC 3339 timestamp is refused as `invalid_request`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_since: Option<String>,
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
    /// Optional top-level field/section allowlist for server-side pruning (`TASK-90`).
    /// When `None`, every section is fetched and returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
}

impl TasksInspectParams {
    pub fn new(task: impl Into<String>) -> Self {
        Self {
            task: task.into(),
            fields: None,
        }
    }
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
    /// Markdown fragment to append to the task body (mutually exclusive with `body_markdown`,
    /// `TASK-90`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_body: Option<String>,
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
    /// Canonical project feature this task implements (`TASK-55`, same project as the task).
    #[serde(default, skip_serializing_if = "PatchField::is_unchanged")]
    pub feature_id: PatchField<String>,
    /// Release milestone this task belongs to (`TASK-55`, same project as the task).
    #[serde(default, skip_serializing_if = "PatchField::is_unchanged")]
    pub milestone_id: PatchField<String>,
    /// Staged upload IDs or readable attachment IDs to bind to the task (`TASK-98.4`, max 20 total).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_attachments: Vec<String>,
    /// Task attachment IDs to detach from the task (`TASK-98.4`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_attachments: Vec<String>,
}

impl Call for TasksUpdate {
    const METHOD: &'static str = "tasks.update";
    type Params = TasksUpdateParams;
    type Result = TasksMutationResult;
}

/// `tasks.transition`: change a task's status (`todo`, `in_progress`, `blocked`, `cancelled`, `done`).
///
/// Completing a task (`done` with resolution `completed`, the default, or `fixed`) while a
/// required checklist step is unticked is refused as `checklist_incomplete`, naming the steps,
/// unless `done_anyway_reason` says why (`TASK-88`); the office then records the reason and
/// the steps on the task and in its `status_changed` event, and clears them when the task
/// leaves `done`. `cancelled` and other resolutions are never gated. `final_summary` sets the
/// task's attributed closing summary in the same commit, on a transition to `done` or
/// `cancelled` only.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_anyway_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_summary: Option<String>,
}

impl Call for TasksTransition {
    const METHOD: &'static str = "tasks.transition";
    type Params = TasksTransitionParams;
    type Result = TasksMutationResult;
}

/// `tasks.final_summary`: set a task's attributed closing summary at `expected_revision`
/// (`TASK-88`). Nothing else ever overwrites it: not comments, watches, checklist ticks nor
/// other edits.
pub struct TasksFinalSummary;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksFinalSummaryParams {
    pub command_id: String,
    pub task: String,
    pub expected_revision: u64,
    pub final_summary: String,
}

impl Call for TasksFinalSummary {
    const METHOD: &'static str = "tasks.final_summary";
    type Params = TasksFinalSummaryParams;
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
    #[serde(default)]
    pub body_markdown: String,
    /// Staged upload IDs or readable attachment IDs bound to this comment (`TASK-98.4`, max 10).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
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

/// One checklist step entry for batch `tasks.checklist.add` (`TASK-90`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistAddItem {
    pub step_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<ChecklistSection>,
}

/// `tasks.checklist.add`: append one or more steps to a task's checklist (`work` when absent).
/// `required` defaults to `true` for `acceptance` and `dod` and to `false` for `work`
/// (`TASK-88`, `TASK-90`).
pub struct TasksChecklistAdd;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistAddParams {
    pub command_id: String,
    pub task: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub step_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<ChecklistSection>,
    /// Optional batch of steps to add atomically in one revision (`TASK-90`).
    /// Mutually exclusive with single-step `step_id`/`text`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<ChecklistAddItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistResult {
    pub task: TaskView,
    pub checklist: Vec<ChecklistStepWire>,
    /// Canonical UUIDv7 IDs of the step(s) created or mutated by this call (`TASK-90`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mutated_step_ids: Vec<String>,
}

impl Call for TasksChecklistAdd {
    const METHOD: &'static str = "tasks.checklist.add";
    type Params = TasksChecklistAddParams;
    type Result = TasksChecklistResult;
}

/// `tasks.checklist.check`: mark one or more checklist steps done or undone, addressed by
/// UUIDv7 `step_id` or by 1-based `(section, indices)` with mandatory `expected_revision`
/// (`TASK-90`).
pub struct TasksChecklistCheck;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistCheckParams {
    pub command_id: String,
    pub task: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub step_id: String,
    pub done: bool,
    /// Section for 1-based ordinal addressing (`TASK-90`, paired with `indices` and
    /// `expected_revision`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<ChecklistSection>,
    /// 1-based ordinal index(es) within `section` (requires `expected_revision`, `TASK-90`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indices: Vec<u64>,
    /// Optimistic-concurrency guard; mandatory when addressing by `(section, indices)` and
    /// enforced whenever supplied (`TASK-90`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

impl Call for TasksChecklistCheck {
    const METHOD: &'static str = "tasks.checklist.check";
    type Params = TasksChecklistCheckParams;
    type Result = TasksChecklistResult;
}

/// `tasks.checklist.edit`: update a checklist step's text and/or required flag, addressed by
/// UUIDv7 `step_id` or by 1-based `(section, index)` with mandatory `expected_revision`
/// (`TASK-90`).
pub struct TasksChecklistEdit;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistEditParams {
    pub command_id: String,
    pub task: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub step_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Section for 1-based ordinal addressing (`TASK-90`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<ChecklistSection>,
    /// 1-based ordinal index within `section` (requires `expected_revision`, `TASK-90`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u64>,
    /// Optimistic-concurrency guard; mandatory when addressing by `(section, index)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

impl Call for TasksChecklistEdit {
    const METHOD: &'static str = "tasks.checklist.edit";
    type Params = TasksChecklistEditParams;
    type Result = TasksChecklistResult;
}

/// `tasks.checklist.remove`: remove a checklist step from a task, addressed by UUIDv7 `step_id`
/// or by 1-based `(section, index)` with mandatory `expected_revision` (`TASK-90`).
pub struct TasksChecklistRemove;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistRemoveParams {
    pub command_id: String,
    pub task: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub step_id: String,
    /// Section for 1-based ordinal addressing (`TASK-90`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<ChecklistSection>,
    /// 1-based ordinal index within `section` (requires `expected_revision`, `TASK-90`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u64>,
    /// Optimistic-concurrency guard; mandatory when addressing by `(section, index)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

impl Call for TasksChecklistRemove {
    const METHOD: &'static str = "tasks.checklist.remove";
    type Params = TasksChecklistRemoveParams;
    type Result = TasksChecklistResult;
}

/// `tasks.checklist.move`: move a checklist step to a zero-based `position` within destination
/// `section` (its own section when absent; `TASK-88`). Addressed by UUIDv7 `step_id` or by
/// 1-based `(from_section, index)` with mandatory `expected_revision` (`TASK-90`).
pub struct TasksChecklistMove;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksChecklistMoveParams {
    pub command_id: String,
    pub task: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub step_id: String,
    pub position: u64,
    /// Destination checklist section (stays in its current section when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<ChecklistSection>,
    /// Source section for 1-based ordinal addressing (`TASK-90`, paired with `index` and
    /// `expected_revision`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_section: Option<ChecklistSection>,
    /// 1-based ordinal index within `from_section` (requires `expected_revision`, `TASK-90`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u64>,
    /// Optimistic-concurrency guard; mandatory when addressing by `(from_section, index)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

impl Call for TasksChecklistMove {
    const METHOD: &'static str = "tasks.checklist.move";
    type Params = TasksChecklistMoveParams;
    type Result = TasksChecklistResult;
}
