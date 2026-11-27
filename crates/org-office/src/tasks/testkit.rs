//! `MemoryEngine` `FamilyTable` implementation for [`TasksRead`] and [`TasksWrite`] (`ENG-1`).

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
};

use org_store::{
    testkit::{FamilyTable, MemorySnapshot, MemoryTx},
    EngineError, Id,
};

use super::{
    ports::{TasksRead, TasksWrite},
    query::{TaskHistoryOutline, TaskListPage, TaskListQuery, TaskRollup},
    records::{
        ChecklistStepRecord, OfficeTaskCounterRecord, TaskCommentRecord, TaskEventRecord,
        TaskLinkRecord, TaskRecord, TaskWatcherRecord,
    },
};

mod list;

#[derive(Clone, Default, Debug)]
pub struct TasksTables {
    tasks: BTreeMap<Id, TaskRecord>,
    /// Inverted index `label -> Set<task_id>` matching SQLite `task_labels_by_label` (`TASK-77`).
    label_index: BTreeMap<String, BTreeSet<Id>>,
    counters: BTreeMap<String, OfficeTaskCounterRecord>,
    checklists: BTreeMap<Id, Vec<ChecklistStepRecord>>,
    comments: BTreeMap<Id, BTreeMap<Id, TaskCommentRecord>>,
    watchers: BTreeMap<Id, Vec<TaskWatcherRecord>>,
    links: BTreeMap<Id, Vec<TaskLinkRecord>>,
    events: BTreeMap<Id, BTreeMap<u64, TaskEventRecord>>,
}

impl FamilyTable for TasksTables {
    fn name(&self) -> &'static str {
        "tasks"
    }

    fn clone_box(&self) -> Box<dyn FamilyTable> {
        Box::new(self.clone())
    }

    fn dump(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (code, c) in &self.counters {
            lines.push(format!(
                "task_counter {code} last={} reserved={}",
                c.last_number, c.reserved_through
            ));
        }
        for (id, t) in &self.tasks {
            lines.push(format!(
                "task {id} key={} rev={} status={} order={:?} labels={:?}",
                t.key(),
                t.meta.revision,
                t.status.as_str(),
                t.rank_order,
                t.labels
            ));
        }
        for (tid, steps) in &self.checklists {
            lines.push(format!("checklist {tid} count={}", steps.len()));
        }
        for (tid, map) in &self.comments {
            lines.push(format!("comments {tid} count={}", map.len()));
        }
        for (tid, list) in &self.links {
            lines.push(format!("links {tid} count={}", list.len()));
        }
        for (tid, map) in &self.events {
            lines.push(format!("events {tid} count={}", map.len()));
        }
        lines.sort();
        lines
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn read<R: Default>(
    tables: Option<&TasksTables>,
    query: impl FnOnce(&TasksTables) -> R,
) -> Result<R, EngineError> {
    Ok(tables.map(query).unwrap_or_default())
}

macro_rules! memory_tasks_read {
    ($($view:ty),+) => {$(
        impl TasksRead for $view {
            fn task(&self, id: &Id) -> Result<Option<TaskRecord>, EngineError> {
                read(self.table(), |t| t.tasks.get(id).cloned())
            }

            fn task_by_key(
                &self,
                office_code: &str,
                number: u64,
            ) -> Result<Option<TaskRecord>, EngineError> {
                read(self.table(), |t| {
                    t.tasks
                        .values()
                        .find(|r| r.office_code == office_code && r.number == number)
                        .cloned()
                })
            }

            fn max_task_number(&self, office_code: &str) -> Result<u64, EngineError> {
                read(self.table(), |t| {
                    t.tasks
                        .values()
                        .filter(|r| r.office_code == office_code)
                        .map(|r| r.number)
                        .max()
                        .unwrap_or(0)
                })
            }

            fn tasks(&self) -> Result<Vec<TaskRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<TaskRecord> = t.tasks.values().cloned().collect();
                    out.sort_by(|a, b| {
                        a.office_code
                            .cmp(&b.office_code)
                            .then_with(|| a.number.cmp(&b.number))
                            .then_with(|| a.id.cmp(&b.id))
                    });
                    out
                })
            }

            fn task_ids_by_label(&self, label: &str) -> Result<Vec<Id>, EngineError> {
                read(self.table(), |t| {
                    t.label_index
                        .get(label)
                        .map(|s| s.iter().cloned().collect())
                        .unwrap_or_default()
                })
            }

            fn task_label_counts(&self) -> Result<Vec<(String, u64)>, EngineError> {
                read(self.table(), |t| {
                    t.label_index
                        .iter()
                        .filter(|(_, ids)| !ids.is_empty())
                        .map(|(label, ids)| (label.clone(), ids.len() as u64))
                        .collect()
                })
            }

            fn office_task_counter(
                &self,
                office_code: &str,
            ) -> Result<Option<OfficeTaskCounterRecord>, EngineError> {
                read(self.table(), |t| t.counters.get(office_code).cloned())
            }

            fn office_task_counters(&self) -> Result<Vec<OfficeTaskCounterRecord>, EngineError> {
                read(self.table(), |t| t.counters.values().cloned().collect())
            }

            fn checklist_steps(&self, task_id: &Id) -> Result<Vec<ChecklistStepRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut list = t.checklists.get(task_id).cloned().unwrap_or_default();
                    list.sort_by_key(|s| s.position);
                    list
                })
            }

            fn task_comments(&self, task_id: &Id) -> Result<Vec<TaskCommentRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut list: Vec<_> = t
                        .comments
                        .get(task_id)
                        .map(|m| m.values().cloned().collect())
                        .unwrap_or_default();
                    list.sort_by_key(|c| c.position);
                    list
                })
            }

            fn task_watchers(&self, task_id: &Id) -> Result<Vec<TaskWatcherRecord>, EngineError> {
                read(self.table(), |t| {
                    t.watchers.get(task_id).cloned().unwrap_or_default()
                })
            }

            fn task_links_from(&self, task_id: &Id) -> Result<Vec<TaskLinkRecord>, EngineError> {
                read(self.table(), |t| {
                    t.links.get(task_id).cloned().unwrap_or_default()
                })
            }

            fn task_dependents(&self, task_id: &Id) -> Result<Vec<Id>, EngineError> {
                read(self.table(), |t| {
                    let mut out = BTreeSet::new();
                    for list in t.links.values() {
                        for link in list {
                            if link.kind == "depends_on" && &link.target_id == task_id {
                                out.insert(link.source_id.clone());
                            }
                        }
                    }
                    out.into_iter().collect()
                })
            }

            fn all_task_links(&self) -> Result<Vec<TaskLinkRecord>, EngineError> {
                read(self.table(), |t| {
                    t.links.values().flat_map(|v| v.iter().cloned()).collect()
                })
            }

            fn task_events(&self, task_id: &Id) -> Result<Vec<TaskEventRecord>, EngineError> {
                read(self.table(), |t| {
                    t.events
                        .get(task_id)
                        .map(|m| m.values().cloned().collect())
                        .unwrap_or_default()
                })
            }

            fn task_page(&self, query: &TaskListQuery) -> Result<TaskListPage, EngineError> {
                list::page(self, query)
            }

            fn task_rollups(&self, ids: &[Id]) -> Result<BTreeMap<Id, TaskRollup>, EngineError> {
                list::rollups(self, ids)
            }

            fn task_history_outlines(
                &self,
            ) -> Result<BTreeMap<Id, TaskHistoryOutline>, EngineError> {
                read(self.table(), |t| {
                    t.events
                        .iter()
                        .filter(|(_, by_rev)| !by_rev.is_empty())
                        .map(|(task_id, by_rev)| {
                            let outline = TaskHistoryOutline {
                                revisions: by_rev.keys().copied().collect(),
                                created_change_json: by_rev.get(&1).map(|e| e.change_json.clone()),
                            };
                            (task_id.clone(), outline)
                        })
                        .collect()
                })
            }
        }
    )+};
}

memory_tasks_read!(MemoryTx, MemorySnapshot);

impl TasksWrite for MemoryTx {
    fn put_task(&mut self, record: &TaskRecord) -> Result<(), EngineError> {
        let tables: &mut TasksTables = self.table_mut()?;
        if let Some(old) = tables.tasks.get(&record.id) {
            for label in &old.labels {
                if let Some(set) = tables.label_index.get_mut(label) {
                    set.remove(&record.id);
                    if set.is_empty() {
                        tables.label_index.remove(label);
                    }
                }
            }
        }
        for label in &record.labels {
            tables
                .label_index
                .entry(label.clone())
                .or_default()
                .insert(record.id.clone());
        }
        let mut normalized = record.clone();
        normalized
            .repositories
            .sort_by(|a, b| a.repository_id.cmp(&b.repository_id));
        // Effective revision (`doc-79`): `max(task.md revision, latest event revision)`.
        let latest = tables
            .events
            .get(&record.id)
            .and_then(|by_rev| by_rev.keys().next_back().copied())
            .unwrap_or(0);
        normalized.meta.revision = normalized.meta.revision.max(latest);
        tables.tasks.insert(record.id.clone(), normalized);
        Ok(())
    }

    fn remove_task(&mut self, id: &Id) -> Result<(), EngineError> {
        let tables: &mut TasksTables = self.table_mut()?;
        if let Some(old) = tables.tasks.remove(id) {
            for label in &old.labels {
                if let Some(set) = tables.label_index.get_mut(label) {
                    set.remove(id);
                    if set.is_empty() {
                        tables.label_index.remove(label);
                    }
                }
            }
        }
        tables.checklists.remove(id);
        tables.comments.remove(id);
        tables.watchers.remove(id);
        tables.links.remove(id);
        Ok(())
    }

    fn put_office_task_counter(
        &mut self,
        record: &OfficeTaskCounterRecord,
    ) -> Result<(), EngineError> {
        let tables: &mut TasksTables = self.table_mut()?;
        tables
            .counters
            .insert(record.office_code.clone(), record.clone());
        Ok(())
    }

    fn remove_office_task_counter(&mut self, office_code: &str) -> Result<(), EngineError> {
        let tables: &mut TasksTables = self.table_mut()?;
        tables.counters.remove(office_code);
        Ok(())
    }

    fn put_checklist_steps(
        &mut self,
        task_id: &Id,
        steps: &[ChecklistStepRecord],
    ) -> Result<(), EngineError> {
        let tables: &mut TasksTables = self.table_mut()?;
        if steps.is_empty() {
            tables.checklists.remove(task_id);
        } else {
            tables.checklists.insert(task_id.clone(), steps.to_vec());
        }
        Ok(())
    }

    fn put_task_comment(&mut self, record: &TaskCommentRecord) -> Result<(), EngineError> {
        let tables: &mut TasksTables = self.table_mut()?;
        tables
            .comments
            .entry(record.task_id.clone())
            .or_default()
            .insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_task_comment(&mut self, task_id: &Id, comment_id: &Id) -> Result<(), EngineError> {
        let tables: &mut TasksTables = self.table_mut()?;
        if let Some(map) = tables.comments.get_mut(task_id) {
            map.remove(comment_id);
        }
        Ok(())
    }

    fn put_task_watchers(
        &mut self,
        task_id: &Id,
        watchers: &[TaskWatcherRecord],
    ) -> Result<(), EngineError> {
        let tables: &mut TasksTables = self.table_mut()?;
        if watchers.is_empty() {
            tables.watchers.remove(task_id);
        } else {
            tables.watchers.insert(task_id.clone(), watchers.to_vec());
        }
        Ok(())
    }

    fn put_task_links(
        &mut self,
        task_id: &Id,
        links: &[TaskLinkRecord],
    ) -> Result<(), EngineError> {
        let tables: &mut TasksTables = self.table_mut()?;
        if links.is_empty() {
            tables.links.remove(task_id);
        } else {
            tables.links.insert(task_id.clone(), links.to_vec());
        }
        Ok(())
    }

    fn put_task_event(&mut self, record: &TaskEventRecord) -> Result<(), EngineError> {
        let tables: &mut TasksTables = self.table_mut()?;
        let by_rev = tables.events.entry(record.task_id.clone()).or_default();
        if let Some(existing) = by_rev.get(&record.revision) {
            if existing.payload_sha256 == record.payload_sha256 {
                return Ok(());
            }
            return Err(EngineError::new(format!(
                "immutable task_event conflict for task {} revision {}",
                record.task_id, record.revision
            )));
        }
        by_rev.insert(record.revision, record.clone());
        // A child-only change (`doc-79`) advances the task's effective revision without
        // rewriting `task.md`.
        if let Some(task) = tables.tasks.get_mut(&record.task_id) {
            task.meta.revision = task.meta.revision.max(record.revision);
        }
        Ok(())
    }
}
