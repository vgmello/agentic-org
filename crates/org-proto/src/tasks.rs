//! Unified v1 typed tasks, checklists, comments, watchers, immutable history,
//! dependencies hierarchy, searchable labels, reference/modified files, and ordering
//! (`TASK-49` B4/B5 + `TASK-77` B4b; `doc-77`, `doc-84`, `ADR-0019`, `ADR-0031`).
//!
//! Every write uses a canonical lowercase UUIDv7 `command_id` for idempotency and acts as the
//! authenticated active employee bound to the connection (`ADR-0031`, `TASKAUTH-1..3`).

mod calls;
mod records;

pub use calls::{
    TasksAddDependency, TasksAddDependencyParams, TasksAssign, TasksAssignParams,
    TasksChecklistAdd, TasksChecklistAddParams, TasksChecklistCheck, TasksChecklistCheckParams,
    TasksChecklistEdit, TasksChecklistEditParams, TasksChecklistMove, TasksChecklistMoveParams,
    TasksChecklistRemove, TasksChecklistRemoveParams, TasksChecklistResult, TasksComment,
    TasksCommentParams, TasksCommentResult, TasksComments, TasksCommentsParams,
    TasksCommentsResult, TasksCreate, TasksCreateParams, TasksDependencies,
    TasksDependenciesParams, TasksHistory, TasksHistoryParams, TasksHistoryResult, TasksInspect,
    TasksInspectParams, TasksLabels, TasksLabelsParams, TasksLabelsResult, TasksList,
    TasksListParams, TasksListResult, TasksMutationResult, TasksRemoveDependency,
    TasksRemoveDependencyParams, TasksTransition, TasksTransitionParams, TasksUnwatch,
    TasksUnwatchParams, TasksUpdate, TasksUpdateParams, TasksWatch, TasksWatchParams,
};
pub use records::{
    ChecklistStepWire, PatchField, TaskCommentWire, TaskDependenciesResult, TaskDependencyEdge,
    TaskDetailsWire, TaskEventWire, TaskInspectResult, TaskLabelSummary, TaskRefSummary, TaskView,
    TaskWatcherWire,
};

use serde::{Deserialize, Serialize};

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
