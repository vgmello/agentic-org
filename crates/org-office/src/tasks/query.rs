//! The read models a `tasks.list` page is built from, as every engine answers them
//! (`TASK-105`, `ENG-1`): one validated [`TaskListQuery`] whose filters, order, offset and
//! limit the engine applies itself, the [`TaskListPage`] it answers, the per-task
//! [`TaskRollup`] a list row shows, and the [`TaskHistoryOutline`] a rebuild checks.
//!
//! A page therefore costs a fixed number of reads, whether the organization holds 10 tasks
//! or 10,000; nothing here reads storage.

use org_proto::tasks::{LabelMatchMode, TaskSortMode, TaskStatus};
use org_store::Id;

use super::records::TaskRecord;

/// Every task status, in the order a status set lists them.
pub const STATUSES: [TaskStatus; 5] = [
    TaskStatus::Todo,
    TaskStatus::InProgress,
    TaskStatus::Blocked,
    TaskStatus::Cancelled,
    TaskStatus::Done,
];

/// A set of task statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskStatusSet(u8);

impl TaskStatusSet {
    /// Every status.
    pub const ALL: Self = Self(0b1_1111);
    /// `todo`, `in_progress` and `blocked`: the statuses of work not yet ended.
    pub const OPEN: Self = Self(0b0_0111);
    /// `cancelled` and `done`.
    pub const CLOSED: Self = Self(0b1_1000);
    pub const EMPTY: Self = Self(0);

    fn bit(status: TaskStatus) -> u8 {
        match status {
            TaskStatus::Todo => 1,
            TaskStatus::InProgress => 1 << 1,
            TaskStatus::Blocked => 1 << 2,
            TaskStatus::Cancelled => 1 << 3,
            TaskStatus::Done => 1 << 4,
        }
    }

    pub fn only(status: TaskStatus) -> Self {
        Self(Self::bit(status))
    }

    pub fn contains(self, status: TaskStatus) -> bool {
        self.0 & Self::bit(status) != 0
    }

    #[must_use]
    pub fn with(self, status: TaskStatus) -> Self {
        Self(self.0 | Self::bit(status))
    }

    #[must_use]
    pub fn without(self, status: TaskStatus) -> Self {
        Self(self.0 & !Self::bit(status))
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The statuses in the set, in [`STATUSES`] order.
    pub fn iter(self) -> impl Iterator<Item = TaskStatus> {
        STATUSES.into_iter().filter(move |s| self.contains(*s))
    }
}

impl Default for TaskStatusSet {
    fn default() -> Self {
        Self::ALL
    }
}

/// Labels a task must carry: all of them, or at least one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelFilter {
    /// Normalized, sorted and unique.
    pub labels: Vec<String>,
    pub mode: LabelMatchMode,
}

/// One validated `tasks.list` question: which tasks, in which order, and which page of them.
/// The features layer builds it from `TasksListParams` (and refuses what is invalid); an
/// engine answers it without further checks.
///
/// Every filter narrows the match: a task matches when it passes all that are set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskListQuery {
    /// The task type, compared ignoring ASCII case.
    pub task_type: Option<String>,
    /// The statuses a matching task may be in.
    pub statuses: TaskStatusSet,
    /// A `done` task matches only when its `updated_at` is an RFC 3339 instant at or after
    /// this one (Unix milliseconds). Times are compared as instants, never as text.
    pub done_since_ms: Option<u64>,
    pub team_id: Option<String>,
    pub assigned_team_id: Option<String>,
    pub assignee_id: Option<String>,
    pub project_id: Option<String>,
    /// Only tasks no employee is assigned to.
    pub unassigned: bool,
    pub labels: Option<LabelFilter>,
    /// Only tasks that directly depend on this task.
    pub depends_on: Option<Id>,
    /// Only tasks this task directly depends on.
    pub dependent_of: Option<Id>,
    /// Lowercase ASCII substring of the key, title, body, a label or a file path.
    pub text: Option<String>,
    /// A normalized relative path (no `./` in front, no `/` at the end, no `..`): only tasks
    /// whose `modified_files` hold it, or a file under it as a directory.
    pub modified_file: Option<String>,
    pub sort: TaskSortMode,
    /// Matches skipped, in `sort` order, before the page starts.
    pub offset: u64,
    /// At most this many tasks on the page; `None` is every match after `offset`.
    pub limit: Option<u64>,
}

/// One page of the tasks a [`TaskListQuery`] matches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskListPage {
    /// The page, in the query's order, each task whole (labels, files, repositories, details).
    pub tasks: Vec<TaskRecord>,
    /// How many tasks match in all, whatever the page.
    pub total: u64,
}

/// What a task's list row shows of its children and links.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskRollup {
    pub checklist_total: u64,
    pub checklist_done: u64,
    pub comment_count: u64,
    pub watcher_count: u64,
    /// Tasks this task directly depends on, sorted and unique.
    pub depends_on: Vec<Id>,
    /// Tasks that directly depend on this task, sorted and unique.
    pub dependents: Vec<Id>,
}

/// What a rebuild checks of one task's history (`doc-79`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskHistoryOutline {
    /// Every event's revision, ascending.
    pub revisions: Vec<u64>,
    /// The `change_json` of revision 1, when there is one.
    pub created_change_json: Option<String>,
}

/// Whether the recorded `file` is `path`, or lies under it as a directory (component-wise:
/// `src/a` names `src/a/b.rs`, never `src/ab.rs`). One `./` in front of `file` is ignored.
pub fn file_names_path(file: &str, path: &str) -> bool {
    let file = file.strip_prefix("./").unwrap_or(file);
    match file.strip_prefix(path) {
        Some("") => true,
        Some(rest) => rest.starts_with('/'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{file_names_path, TaskStatusSet, STATUSES};
    use org_proto::tasks::TaskStatus;

    #[test]
    fn a_path_names_itself_and_the_files_under_it_only() {
        assert!(file_names_path("src/a.rs", "src/a.rs"));
        assert!(file_names_path("src/a/b.rs", "src/a"));
        assert!(file_names_path("./src/a/b.rs", "src/a"));
        assert!(!file_names_path("src/ab.rs", "src/a"));
        assert!(!file_names_path("src", "src/a"));
        assert!(!file_names_path("././src/a", "src/a"));
    }

    #[test]
    fn status_sets_hold_exactly_their_statuses() {
        assert_eq!(TaskStatusSet::ALL.iter().collect::<Vec<_>>(), STATUSES);
        assert_eq!(
            TaskStatusSet::OPEN.iter().collect::<Vec<_>>(),
            [
                TaskStatus::Todo,
                TaskStatus::InProgress,
                TaskStatus::Blocked
            ]
        );
        assert_eq!(
            TaskStatusSet::CLOSED.iter().collect::<Vec<_>>(),
            [TaskStatus::Cancelled, TaskStatus::Done]
        );
        for status in STATUSES {
            assert_eq!(TaskStatusSet::OPEN.contains(status), !status.is_closed());
            assert_eq!(
                TaskStatusSet::only(status).iter().collect::<Vec<_>>(),
                [status]
            );
            assert!(!TaskStatusSet::ALL.without(status).contains(status));
            assert!(TaskStatusSet::EMPTY.with(status).contains(status));
        }
        assert!(TaskStatusSet::EMPTY.is_empty());
        assert_eq!(TaskStatusSet::default(), TaskStatusSet::ALL);
    }
}
