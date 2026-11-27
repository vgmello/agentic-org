//! The `MemoryEngine` answer to the batched task reads (`TASK-105`): a plain scan over every
//! task and the per-task reads. It is the reference the SQLite adapter's SQL must agree with
//! (`ENG-1`), so it stays as direct as the query's own documentation.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use org_proto::tasks::{BlockingDependencyWire, LabelMatchMode, TaskSortMode, TaskStatus};
use org_store::{EngineError, Id};

use crate::tasks::ports::TasksRead;
use crate::tasks::query::{file_names_path, LabelFilter, TaskListPage, TaskListQuery, TaskRollup};
use crate::tasks::records::TaskRecord;
use crate::time::parse_rfc3339_ms;

/// The tasks the dependency filters allow, when they are set.
struct Related {
    depends_on: Option<BTreeSet<Id>>,
    dependent_of: Option<BTreeSet<Id>>,
    dependency_blocked: Option<BTreeSet<Id>>,
}

pub(super) fn page<R: TasksRead>(
    view: &R,
    query: &TaskListQuery,
) -> Result<TaskListPage, EngineError> {
    let all_tasks = view.tasks()?;
    let depends_on = match &query.depends_on {
        Some(target) => Some(view.task_dependents(target)?.into_iter().collect()),
        None => None,
    };
    let dependent_of = match &query.dependent_of {
        Some(source) => Some(
            view.task_links_from(source)?
                .into_iter()
                .filter(|link| link.kind == "depends_on")
                .map(|link| link.target_id)
                .collect(),
        ),
        None => None,
    };
    let dependency_blocked = if query.dependency_blocked.is_some() {
        let status_by_id: BTreeMap<&Id, TaskStatus> =
            all_tasks.iter().map(|t| (&t.id, t.status)).collect();
        let mut blocked = BTreeSet::new();
        for link in view.all_task_links()? {
            if link.kind == "depends_on"
                && link.target_kind == "task"
                && status_by_id
                    .get(&link.target_id)
                    .is_some_and(|status| *status != TaskStatus::Done)
            {
                blocked.insert(link.source_id);
            }
        }
        Some(blocked)
    } else {
        None
    };
    let related = Related {
        depends_on,
        dependent_of,
        dependency_blocked,
    };
    let mut matched: Vec<TaskRecord> = all_tasks
        .into_iter()
        .filter(|task| matches(query, &related, task))
        .collect();
    matched.sort_by(|a, b| order(query.sort, a, b));
    let total = matched.len() as u64;
    let skip = usize::try_from(query.offset).unwrap_or(usize::MAX);
    let take = query.limit.map_or(usize::MAX, |limit| {
        usize::try_from(limit).unwrap_or(usize::MAX)
    });
    let tasks = matched.into_iter().skip(skip).take(take).collect();
    Ok(TaskListPage { tasks, total })
}

pub(super) fn rollups<R: TasksRead>(
    view: &R,
    ids: &[Id],
) -> Result<BTreeMap<Id, TaskRollup>, EngineError> {
    let mut out = BTreeMap::new();
    for id in ids {
        let steps = view.checklist_steps(id)?;
        let mut depends_on: Vec<Id> = view
            .task_links_from(id)?
            .into_iter()
            .filter(|link| link.kind == "depends_on" && link.target_kind == "task")
            .map(|link| link.target_id)
            .collect();
        depends_on.sort();
        depends_on.dedup();
        let mut unmet: Vec<(String, u64, Id, BlockingDependencyWire)> = Vec::new();
        for target_id in &depends_on {
            if let Some(dep) = view.task(target_id)? {
                if dep.status != TaskStatus::Done {
                    unmet.push((
                        dep.office_code.clone(),
                        dep.number,
                        dep.id.clone(),
                        BlockingDependencyWire {
                            id: dep.id.as_str().to_owned(),
                            key: dep.key(),
                            title: dep.title,
                            status: dep.status,
                            cancelled: dep.status == TaskStatus::Cancelled,
                        },
                    ));
                }
            }
        }
        unmet.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        let unmet_blocking_dependencies = unmet.into_iter().map(|(_, _, _, wire)| wire).collect();
        let mut dependents = view.task_dependents(id)?;
        dependents.sort();
        dependents.dedup();
        let mut rollup = TaskRollup {
            comment_count: view.task_comments(id)?.len() as u64,
            watcher_count: view.task_watchers(id)?.len() as u64,
            depends_on,
            dependents,
            unmet_blocking_dependencies,
            ..TaskRollup::default()
        };
        for step in &steps {
            rollup.add_steps(step.section, 1, u64::from(step.done));
        }
        out.insert(id.clone(), rollup);
    }
    Ok(out)
}

fn matches(query: &TaskListQuery, related: &Related, task: &TaskRecord) -> bool {
    let equal = |filter: &Option<String>, value: Option<&Id>| {
        filter
            .as_deref()
            .is_none_or(|wanted| value.map(Id::as_str) == Some(wanted))
    };
    query
        .task_type
        .as_deref()
        .is_none_or(|kind| task.task_type.eq_ignore_ascii_case(kind))
        && query.statuses.contains(task.status)
        && recent_enough(query.done_since_ms, task)
        && equal(&query.team_id, Some(&task.team_id))
        && equal(&query.assigned_team_id, task.assigned_team_id.as_ref())
        && equal(&query.assignee_id, task.assignee_id.as_ref())
        && equal(&query.project_id, task.project_id.as_ref())
        && (!query.unassigned || task.assignee_id.is_none())
        && matches_secondary(query, related, task)
}

fn matches_secondary(query: &TaskListQuery, related: &Related, task: &TaskRecord) -> bool {
    query
        .labels
        .as_ref()
        .is_none_or(|filter| carries(filter, task))
        && related
            .depends_on
            .as_ref()
            .is_none_or(|ids| ids.contains(&task.id))
        && related
            .dependent_of
            .as_ref()
            .is_none_or(|ids| ids.contains(&task.id))
        && query.dependency_blocked.is_none_or(|wanted| {
            related
                .dependency_blocked
                .as_ref()
                .is_some_and(|blocked| blocked.contains(&task.id) == wanted)
        })
        && query.text.as_deref().is_none_or(|q| mentions(task, q))
        && query.modified_file.as_deref().is_none_or(|path| {
            task.modified_files
                .iter()
                .any(|file| file_names_path(file, path))
        })
}

fn recent_enough(done_since_ms: Option<u64>, task: &TaskRecord) -> bool {
    let Some(since) = done_since_ms else {
        return true;
    };
    task.status != TaskStatus::Done
        || task
            .meta
            .updated_at
            .as_deref()
            .and_then(parse_rfc3339_ms)
            .is_some_and(|at| at >= since)
}

fn carries(filter: &LabelFilter, task: &TaskRecord) -> bool {
    let has = |label: &String| task.labels.contains(label);
    match filter.mode {
        LabelMatchMode::All => filter.labels.iter().all(has),
        LabelMatchMode::Any => filter.labels.iter().any(has),
    }
}

fn mentions(task: &TaskRecord, q: &str) -> bool {
    let lower_has = |text: &str| text.to_ascii_lowercase().contains(q);
    lower_has(&task.key())
        || lower_has(&task.title)
        || lower_has(&task.body_markdown)
        || task.blocked_reason.as_deref().is_some_and(lower_has)
        || task.labels.iter().any(|l| l.contains(q))
        || task.reference_files.iter().any(|p| lower_has(p))
        || task.modified_files.iter().any(|p| lower_has(p))
}

fn order(mode: TaskSortMode, a: &TaskRecord, b: &TaskRecord) -> Ordering {
    match mode {
        TaskSortMode::Order => (a.rank_order.is_none())
            .cmp(&b.rank_order.is_none())
            .then_with(|| a.rank_order.cmp(&b.rank_order))
            .then_with(|| a.office_code.cmp(&b.office_code))
            .then_with(|| a.number.cmp(&b.number))
            .then_with(|| a.id.cmp(&b.id)),
        TaskSortMode::Number => a
            .office_code
            .cmp(&b.office_code)
            .then_with(|| a.number.cmp(&b.number))
            .then_with(|| a.id.cmp(&b.id)),
        TaskSortMode::UpdatedDesc => b
            .meta
            .updated_at
            .cmp(&a.meta.updated_at)
            .then_with(|| b.number.cmp(&a.number))
            .then_with(|| a.id.cmp(&b.id)),
    }
}
