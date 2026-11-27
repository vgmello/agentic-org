//! [`TasksRead`] for the writer's transaction and for a snapshot.

use std::collections::BTreeMap;

use org_store::{EngineError, Id};

use super::super::{
    organization::{id, number, text, whole, View, META},
    SqliteSnapshot, SqliteTx,
};
use super::hydrate::{hydrate, Owners};
use super::list::{page, rollups};
use super::rows::{comment_row, counter_row, event_row, link_row, step_row, task_row, watcher_row};
use super::{fetch, fetch_one, TASK_COLS};
use crate::tasks::{
    ports::TasksRead,
    query::{TaskHistoryOutline, TaskListPage, TaskListQuery, TaskRollup},
    records::{
        ChecklistStepRecord, OfficeTaskCounterRecord, TaskCommentRecord, TaskEventRecord,
        TaskLinkRecord, TaskRecord, TaskWatcherRecord,
    },
};

/// One task row, whole.
fn hydrate_one(
    db: &rusqlite::Connection,
    scope: &str,
    row: Option<TaskRecord>,
) -> Result<Option<TaskRecord>, EngineError> {
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(hydrate(db, scope, vec![row], Owners::Listed)?.pop())
}

macro_rules! sqlite_tasks_read {
    ($($view:ty),+) => {$(
        impl TasksRead for $view {
            fn task(&self, task_id: &Id) -> Result<Option<TaskRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {TASK_COLS}, {META} FROM tasks \
                     WHERE organization_id = ?1 AND id = ?2"
                );
                let row = fetch_one(db, &sql, vec![text(scope), text(task_id.as_str())], task_row)?;
                hydrate_one(db, scope, row)
            }

            fn task_by_key(
                &self,
                office_code: &str,
                num: u64,
            ) -> Result<Option<TaskRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {TASK_COLS}, {META} FROM tasks \
                     WHERE organization_id = ?1 AND office_code = ?2 AND number = ?3"
                );
                let row = fetch_one(
                    db,
                    &sql,
                    vec![text(scope), text(office_code), whole(num)?],
                    task_row,
                )?;
                hydrate_one(db, scope, row)
            }

            fn tasks(&self) -> Result<Vec<TaskRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {TASK_COLS}, {META} FROM tasks \
                     WHERE organization_id = ?1 ORDER BY office_code, number, id"
                );
                let rows = fetch(db, &sql, vec![text(scope)], task_row)?;
                hydrate(db, scope, rows, Owners::Organization)
            }

            fn max_task_number(&self, office_code: &str) -> Result<u64, EngineError> {
                let (db, scope) = self.view()?;
                let max: Option<i64> = fetch_one(
                    db,
                    "SELECT MAX(number) FROM tasks WHERE organization_id = ?1 AND office_code = ?2",
                    vec![text(scope), text(office_code)],
                    |row| row.get(0),
                )?
                .flatten();
                Ok(max.and_then(|n| u64::try_from(n).ok()).unwrap_or(0))
            }

            fn task_ids_by_label(&self, label: &str) -> Result<Vec<Id>, EngineError> {
                let (db, scope) = self.view()?;
                fetch(
                    db,
                    "SELECT task_id FROM task_labels \
                     WHERE organization_id = ?1 AND label = ?2 ORDER BY task_id",
                    vec![text(scope), text(label)],
                    |r| id(r, 0),
                )
            }

            fn task_label_counts(&self) -> Result<Vec<(String, u64)>, EngineError> {
                let (db, scope) = self.view()?;
                fetch(
                    db,
                    "SELECT label, COUNT(*) FROM task_labels \
                     WHERE organization_id = ?1 GROUP BY label ORDER BY label",
                    vec![text(scope)],
                    |r| Ok((r.get::<_, String>(0)?, number(r, 1)?)),
                )
            }

            fn office_task_counter(
                &self,
                office_code: &str,
            ) -> Result<Option<OfficeTaskCounterRecord>, EngineError> {
                let (db, scope) = self.view()?;
                fetch_one(
                    db,
                    "SELECT office_code, last_number, reserved_through FROM office_task_counters \
                     WHERE organization_id = ?1 AND office_code = ?2",
                    vec![text(scope), text(office_code)],
                    counter_row,
                )
            }

            fn office_task_counters(&self) -> Result<Vec<OfficeTaskCounterRecord>, EngineError> {
                let (db, scope) = self.view()?;
                fetch(
                    db,
                    "SELECT office_code, last_number, reserved_through FROM office_task_counters \
                     WHERE organization_id = ?1 ORDER BY office_code",
                    vec![text(scope)],
                    counter_row,
                )
            }

            fn checklist_steps(&self, task_id: &Id) -> Result<Vec<ChecklistStepRecord>, EngineError> {
                let (db, scope) = self.view()?;
                fetch(
                    db,
                    "SELECT id, position, text, required, done, linked_task_id, \
                     checked_by_employee_id, checked_at, revision \
                     FROM checklist_steps \
                     WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2 \
                     ORDER BY position",
                    vec![text(scope), text(task_id.as_str())],
                    step_row,
                )
            }

            fn task_comments(&self, task_id: &Id) -> Result<Vec<TaskCommentRecord>, EngineError> {
                let (db, scope) = self.view()?;
                fetch(
                    db,
                    "SELECT target_id, id, position, author_id, created_at, body_markdown \
                     FROM comments \
                     WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2 \
                     ORDER BY position",
                    vec![text(scope), text(task_id.as_str())],
                    comment_row,
                )
            }

            fn task_watchers(&self, task_id: &Id) -> Result<Vec<TaskWatcherRecord>, EngineError> {
                let (db, scope) = self.view()?;
                fetch(
                    db,
                    "SELECT id, watcher_kind, watcher_id FROM watchers \
                     WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2 \
                     ORDER BY id",
                    vec![text(scope), text(task_id.as_str())],
                    watcher_row,
                )
            }

            fn task_links_from(&self, task_id: &Id) -> Result<Vec<TaskLinkRecord>, EngineError> {
                let (db, scope) = self.view()?;
                fetch(
                    db,
                    "SELECT source_id, id, kind, target_kind, target_id FROM links \
                     WHERE organization_id = ?1 AND source_kind = 'task' AND source_id = ?2 \
                     ORDER BY kind, target_id",
                    vec![text(scope), text(task_id.as_str())],
                    link_row,
                )
            }

            fn task_dependents(&self, task_id: &Id) -> Result<Vec<Id>, EngineError> {
                let (db, scope) = self.view()?;
                fetch(
                    db,
                    "SELECT source_id FROM links \
                     WHERE organization_id = ?1 AND source_kind = 'task' \
                       AND kind = 'depends_on' AND target_kind = 'task' AND target_id = ?2 \
                     ORDER BY source_id",
                    vec![text(scope), text(task_id.as_str())],
                    |r| id(r, 0),
                )
            }

            fn all_task_links(&self) -> Result<Vec<TaskLinkRecord>, EngineError> {
                let (db, scope) = self.view()?;
                fetch(
                    db,
                    "SELECT source_id, id, kind, target_kind, target_id FROM links \
                     WHERE organization_id = ?1 AND source_kind = 'task' \
                     ORDER BY source_id, kind, target_id",
                    vec![text(scope)],
                    link_row,
                )
            }

            fn task_events(&self, task_id: &Id) -> Result<Vec<TaskEventRecord>, EngineError> {
                let (db, scope) = self.view()?;
                fetch(
                    db,
                    "SELECT task_id, revision, schema_version, occurred_at, actor_kind, \
                     actor_id, operation_id, change_json, source_metadata_json, \
                     payload_sha256, resource_profile \
                     FROM task_events \
                     WHERE organization_id = ?1 AND task_id = ?2 ORDER BY revision",
                    vec![text(scope), text(task_id.as_str())],
                    event_row,
                )
            }

            fn task_page(&self, query: &TaskListQuery) -> Result<TaskListPage, EngineError> {
                let (db, scope) = self.view()?;
                page(db, scope, query)
            }

            fn task_rollups(&self, ids: &[Id]) -> Result<BTreeMap<Id, TaskRollup>, EngineError> {
                let (db, scope) = self.view()?;
                rollups(db, scope, ids)
            }

            fn task_history_outlines(
                &self,
            ) -> Result<BTreeMap<Id, TaskHistoryOutline>, EngineError> {
                let (db, scope) = self.view()?;
                let mut out: BTreeMap<Id, TaskHistoryOutline> = BTreeMap::new();
                let revisions = fetch(
                    db,
                    "SELECT task_id, revision FROM task_events \
                     WHERE organization_id = ?1 ORDER BY task_id, revision",
                    vec![text(scope)],
                    |r| Ok((id(r, 0)?, number(r, 1)?)),
                )?;
                for (task, revision) in revisions {
                    out.entry(task).or_default().revisions.push(revision);
                }
                let created = fetch(
                    db,
                    "SELECT task_id, change_json FROM task_events \
                     WHERE organization_id = ?1 AND revision = 1",
                    vec![text(scope)],
                    |r| Ok((id(r, 0)?, r.get::<_, String>(1)?)),
                )?;
                for (task, change) in created {
                    out.entry(task).or_default().created_change_json = Some(change);
                }
                Ok(out)
            }
        }
    )+};
}

sqlite_tasks_read!(SqliteTx, SqliteSnapshot);
