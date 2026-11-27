//! Engine-independent read and write ports for the unified tasks family (`TASK-49`, `TASK-77`).

use std::collections::BTreeMap;

use org_store::{Engine, EngineError, Id};

use crate::attachments::ports::{AttachmentsRead, AttachmentsWrite};
use crate::messaging::ports::{MessagingRead, MessagingWrite};
use crate::processes::ports::{ProcessesRead, ProcessesWrite};

use super::query::{TaskHistoryOutline, TaskListPage, TaskListQuery, TaskRollup};
use super::records::{
    parse_task_key, ChecklistStepRecord, OfficeTaskCounterRecord, TaskCommentRecord,
    TaskEventRecord, TaskLinkRecord, TaskRecord, TaskWatcherRecord,
};

/// Read-only queries over the tasks projection (Memory and SQLite, `ENG-1`).
pub trait TasksRead: MessagingRead + ProcessesRead + AttachmentsRead {
    fn task(&self, id: &Id) -> Result<Option<TaskRecord>, EngineError>;
    fn task_by_key(
        &self,
        office_code: &str,
        number: u64,
    ) -> Result<Option<TaskRecord>, EngineError>;
    /// All tasks in this organization ordered by `(office_code, number, id)`.
    fn tasks(&self) -> Result<Vec<TaskRecord>, EngineError>;
    /// The highest task number issued under `office_code`; 0 when it has none (`doc-80`).
    fn max_task_number(&self, office_code: &str) -> Result<u64, EngineError>;
    /// Indexed lookup of task IDs carrying normalized `label` (`TASK-77`).
    fn task_ids_by_label(&self, label: &str) -> Result<Vec<Id>, EngineError>;
    /// Distinct normalized labels with task counts, ordered alphabetically (`TASK-77`).
    fn task_label_counts(&self) -> Result<Vec<(String, u64)>, EngineError>;
    fn office_task_counter(
        &self,
        office_code: &str,
    ) -> Result<Option<OfficeTaskCounterRecord>, EngineError>;
    fn office_task_counters(&self) -> Result<Vec<OfficeTaskCounterRecord>, EngineError>;
    fn checklist_steps(&self, task_id: &Id) -> Result<Vec<ChecklistStepRecord>, EngineError>;
    fn task_comments(&self, task_id: &Id) -> Result<Vec<TaskCommentRecord>, EngineError>;
    /// Resolves one task comment by its UUIDv7 ID (`TASK-98.4`).
    fn task_comment_by_id(&self, comment_id: &Id)
        -> Result<Option<TaskCommentRecord>, EngineError>;
    fn task_watchers(&self, task_id: &Id) -> Result<Vec<TaskWatcherRecord>, EngineError>;
    /// Outgoing typed links authored on `task_id` (including `kind = "depends_on"`).
    fn task_links_from(&self, task_id: &Id) -> Result<Vec<TaskLinkRecord>, EngineError>;
    /// Incoming `depends_on` task IDs: tasks that directly depend on `task_id` (`TASK-77`).
    fn task_dependents(&self, task_id: &Id) -> Result<Vec<Id>, EngineError>;
    /// All task links in the organization (for graph validation & rebuild checks).
    fn all_task_links(&self) -> Result<Vec<TaskLinkRecord>, EngineError>;
    /// Immutable task history events in ascending `revision` order (`doc-84`).
    fn task_events(&self, task_id: &Id) -> Result<Vec<TaskEventRecord>, EngineError>;
    /// The page of tasks `query` matches, in its order, and how many match in all
    /// (`TASK-105`). The engine filters, orders, counts and pages, so a page costs the same
    /// few reads however many tasks the organization holds.
    fn task_page(&self, query: &TaskListQuery) -> Result<TaskListPage, EngineError>;
    /// The list rollup of each of `ids`, read for all of them at once (`TASK-105`). An ID
    /// with no children and no links, or of no task, maps to the default rollup.
    fn task_rollups(&self, ids: &[Id]) -> Result<BTreeMap<Id, TaskRollup>, EngineError>;
    /// Every task's history outline, by task ID, read at once for a rebuild's checks
    /// (`TASK-105`, `doc-79`). Tasks with no history are absent.
    fn task_history_outlines(&self) -> Result<BTreeMap<Id, TaskHistoryOutline>, EngineError>;

    /// Resolves a task by canonical UUIDv7 ID or office-coded `T-<CODE><N>` reference.
    fn resolve_task(&self, reference: &str) -> Result<Option<TaskRecord>, EngineError> {
        let trimmed = reference.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if let Ok(id) = Id::parse(trimmed) {
            if let Some(found) = self.task(&id)? {
                return Ok(Some(found));
            }
        }
        if let Some((code, number)) = parse_task_key(trimmed) {
            return self.task_by_key(&code, number);
        }
        Ok(None)
    }
}

/// Write operations on the tasks projection inside a transaction.
pub trait TasksWrite: TasksRead + MessagingWrite + ProcessesWrite + AttachmentsWrite {
    fn put_task(&mut self, record: &TaskRecord) -> Result<(), EngineError>;
    fn remove_task(&mut self, id: &Id) -> Result<(), EngineError>;
    fn put_office_task_counter(
        &mut self,
        record: &OfficeTaskCounterRecord,
    ) -> Result<(), EngineError>;
    fn remove_office_task_counter(&mut self, office_code: &str) -> Result<(), EngineError>;
    fn put_checklist_steps(
        &mut self,
        task_id: &Id,
        steps: &[ChecklistStepRecord],
    ) -> Result<(), EngineError>;
    fn put_task_comment(&mut self, record: &TaskCommentRecord) -> Result<(), EngineError>;
    fn remove_task_comment(&mut self, task_id: &Id, comment_id: &Id) -> Result<(), EngineError>;
    fn put_task_watchers(
        &mut self,
        task_id: &Id,
        watchers: &[TaskWatcherRecord],
    ) -> Result<(), EngineError>;
    fn put_task_links(&mut self, task_id: &Id, links: &[TaskLinkRecord])
        -> Result<(), EngineError>;
    fn put_task_event(&mut self, record: &TaskEventRecord) -> Result<(), EngineError>;
}

pub trait TasksEngine: Engine<Tx: TasksWrite, Snapshot: TasksRead> {}

impl<E> TasksEngine for E where E: Engine<Tx: TasksWrite, Snapshot: TasksRead> {}
