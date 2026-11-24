//! Read queries for unified tasks: filtered/ordered listing, inspection, dependency hierarchy
//! traversal, indexed label catalog, comments, and immutable history (`TASK-49`, `TASK-77`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use org_proto::tasks::{
    LabelMatchMode, TaskDependenciesResult, TaskDependencyEdge, TaskInspectResult,
    TaskLabelSummary, TaskRefSummary, TaskSortMode, TasksCommentsResult, TasksHistoryResult,
    TasksLabelsParams, TasksLabelsResult, TasksListParams, TasksListResult,
};
use org_store::{Id, Refusal};

use crate::tasks::ports::TasksRead;
use crate::tasks::records::{normalize_labels, TaskRecord};

use super::{
    build_ref_summary, build_task_view, comment_to_wire, event_to_wire, refuse, require_task,
    step_to_wire, storage_refusal, watcher_to_wire,
};

pub fn list_tasks<R: TasksRead>(
    view: &R,
    params: &TasksListParams,
) -> Result<TasksListResult, Refusal> {
    let label_filter_ids = resolve_label_filter(view, &params.labels, params.label_match)?;
    let depends_on_filter = match params.depends_on.as_deref() {
        Some(r) => {
            let target = require_task(view, r)?;
            Some(
                view.task_dependents(&target.id)
                    .map_err(storage_refusal)?
                    .into_iter()
                    .collect::<BTreeSet<Id>>(),
            )
        }
        None => None,
    };
    let dependent_of_filter = match params.dependent_of.as_deref() {
        Some(r) => {
            let source = require_task(view, r)?;
            Some(
                view.task_links_from(&source.id)
                    .map_err(storage_refusal)?
                    .into_iter()
                    .filter(|l| l.kind == "depends_on")
                    .map(|l| l.target_id)
                    .collect::<BTreeSet<Id>>(),
            )
        }
        None => None,
    };
    let query_lower = params
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(str::to_ascii_lowercase);

    let mut matched: Vec<TaskRecord> = view
        .tasks()
        .map_err(storage_refusal)?
        .into_iter()
        .filter(|t| {
            matches_task_filters(
                t,
                params,
                label_filter_ids.as_ref(),
                depends_on_filter.as_ref(),
                dependent_of_filter.as_ref(),
                query_lower.as_deref(),
            )
        })
        .collect();

    sort_tasks(&mut matched, params.sort);
    let total = matched.len() as u64;
    if let Some(limit) = params.limit {
        matched.truncate(limit as usize);
    }
    let mut views = Vec::with_capacity(matched.len());
    for task in &matched {
        views.push(build_task_view(view, task)?);
    }
    Ok(TasksListResult {
        tasks: views,
        total,
    })
}

fn resolve_label_filter<R: TasksRead>(
    view: &R,
    raw_labels: &[String],
    mode: LabelMatchMode,
) -> Result<Option<BTreeSet<Id>>, Refusal> {
    if raw_labels.is_empty() {
        return Ok(None);
    }
    let normalized = normalize_labels(raw_labels).map_err(|e| refuse(e.code, e.message))?;
    let mut acc: Option<BTreeSet<Id>> = None;
    for label in normalized {
        let ids: BTreeSet<Id> = view
            .task_ids_by_label(&label)
            .map_err(storage_refusal)?
            .into_iter()
            .collect();
        acc = Some(match (acc, mode) {
            (None, _) => ids,
            (Some(prev), LabelMatchMode::All) => prev.intersection(&ids).cloned().collect(),
            (Some(mut prev), LabelMatchMode::Any) => {
                prev.extend(ids);
                prev
            }
        });
    }
    Ok(Some(acc.unwrap_or_default()))
}

fn matches_scalar_filters(task: &TaskRecord, params: &TasksListParams) -> bool {
    let type_ok = params
        .task_type
        .as_deref()
        .is_none_or(|kind| task.task_type.eq_ignore_ascii_case(kind.trim()));
    let status_ok = params.status.is_none_or(|status| task.status == status);
    let team_ok = params
        .team_id
        .as_deref()
        .is_none_or(|tid| task.team_id.as_str() == tid.trim());
    let assigned_team_ok = params
        .assigned_team_id
        .as_deref()
        .is_none_or(|atid| task.assigned_team_id.as_ref().map(Id::as_str) == Some(atid.trim()));
    let assignee_ok = params
        .assignee_id
        .as_deref()
        .is_none_or(|aid| task.assignee_id.as_ref().map(Id::as_str) == Some(aid.trim()));
    let project_ok = params
        .project_id
        .as_deref()
        .is_none_or(|pid| task.project_id.as_ref().map(Id::as_str) == Some(pid.trim()));
    type_ok && status_ok && team_ok && assigned_team_ok && assignee_ok && project_ok
}

fn matches_text_query(task: &TaskRecord, q: &str) -> bool {
    task.key().to_ascii_lowercase().contains(q)
        || task.title.to_ascii_lowercase().contains(q)
        || task.body_markdown.to_ascii_lowercase().contains(q)
        || task.labels.iter().any(|l| l.contains(q))
        || task
            .reference_files
            .iter()
            .any(|p| p.to_ascii_lowercase().contains(q))
        || task
            .modified_files
            .iter()
            .any(|p| p.to_ascii_lowercase().contains(q))
}

fn matches_task_filters(
    task: &TaskRecord,
    params: &TasksListParams,
    label_ids: Option<&BTreeSet<Id>>,
    depends_on_ids: Option<&BTreeSet<Id>>,
    dependent_of_ids: Option<&BTreeSet<Id>>,
    query_lower: Option<&str>,
) -> bool {
    matches_scalar_filters(task, params)
        && label_ids.is_none_or(|set| set.contains(&task.id))
        && depends_on_ids.is_none_or(|set| set.contains(&task.id))
        && dependent_of_ids.is_none_or(|set| set.contains(&task.id))
        && query_lower.is_none_or(|q| matches_text_query(task, q))
}

fn sort_tasks(tasks: &mut [TaskRecord], mode: TaskSortMode) {
    match mode {
        TaskSortMode::Order => tasks.sort_by(|a, b| {
            let a_null = a.rank_order.is_none();
            let b_null = b.rank_order.is_none();
            a_null
                .cmp(&b_null)
                .then_with(|| a.rank_order.cmp(&b.rank_order))
                .then_with(|| a.office_code.cmp(&b.office_code))
                .then_with(|| a.number.cmp(&b.number))
                .then_with(|| a.id.cmp(&b.id))
        }),
        TaskSortMode::Number => tasks.sort_by(|a, b| {
            a.office_code
                .cmp(&b.office_code)
                .then_with(|| a.number.cmp(&b.number))
                .then_with(|| a.id.cmp(&b.id))
        }),
        TaskSortMode::UpdatedDesc => tasks.sort_by(|a, b| {
            b.meta
                .updated_at
                .cmp(&a.meta.updated_at)
                .then_with(|| b.number.cmp(&a.number))
                .then_with(|| a.id.cmp(&b.id))
        }),
    }
}

pub fn inspect_task<R: TasksRead>(view: &R, reference: &str) -> Result<TaskInspectResult, Refusal> {
    let task = require_task(view, reference)?;
    let task_view = build_task_view(view, &task)?;
    let checklist = view
        .checklist_steps(&task.id)
        .map_err(storage_refusal)?
        .iter()
        .map(step_to_wire)
        .collect();
    let comments = view
        .task_comments(&task.id)
        .map_err(storage_refusal)?
        .iter()
        .map(comment_to_wire)
        .collect();
    let watchers = view
        .task_watchers(&task.id)
        .map_err(storage_refusal)?
        .iter()
        .map(watcher_to_wire)
        .collect();
    let history = view
        .task_events(&task.id)
        .map_err(storage_refusal)?
        .iter()
        .map(event_to_wire)
        .collect();

    let mut depends_on_tasks = Vec::new();
    for id_str in &task_view.depends_on {
        if let Ok(id) = Id::parse(id_str) {
            if let Some(dep) = view.task(&id).map_err(storage_refusal)? {
                depends_on_tasks.push(build_ref_summary(&dep));
            }
        }
    }
    let mut dependent_tasks = Vec::new();
    for id_str in &task_view.dependents {
        if let Ok(id) = Id::parse(id_str) {
            if let Some(dep) = view.task(&id).map_err(storage_refusal)? {
                dependent_tasks.push(build_ref_summary(&dep));
            }
        }
    }
    Ok(TaskInspectResult {
        task: task_view,
        checklist,
        comments,
        watchers,
        depends_on_tasks,
        dependent_tasks,
        history,
    })
}

/// Computes the complete upstream + downstream dependency hierarchy for one task (`TASK-77`).
pub fn task_dependencies<R: TasksRead>(
    view: &R,
    reference: &str,
) -> Result<TaskDependenciesResult, Refusal> {
    let root = require_task(view, reference)?;
    let all_tasks: BTreeMap<Id, TaskRecord> = view
        .tasks()
        .map_err(storage_refusal)?
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect();
    let all_links = view.all_task_links().map_err(storage_refusal)?;
    let mut upstream: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    let mut downstream: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    for link in &all_links {
        if link.kind != "depends_on" {
            continue;
        }
        upstream
            .entry(link.source_id.clone())
            .or_default()
            .push(link.target_id.clone());
        downstream
            .entry(link.target_id.clone())
            .or_default()
            .push(link.source_id.clone());
    }

    let sort_ids = |ids: &mut Vec<Id>| {
        ids.sort_by(|a, b| {
            let ta = all_tasks.get(a);
            let tb = all_tasks.get(b);
            match (ta, tb) {
                (Some(ta), Some(tb)) => ta
                    .office_code
                    .cmp(&tb.office_code)
                    .then_with(|| ta.number.cmp(&tb.number))
                    .then_with(|| a.cmp(b)),
                _ => a.cmp(b),
            }
        });
        ids.dedup();
    };
    for neighbors in upstream.values_mut() {
        sort_ids(neighbors);
    }
    for neighbors in downstream.values_mut() {
        sort_ids(neighbors);
    }

    let direct_up_ids = upstream.get(&root.id).cloned().unwrap_or_default();
    let direct_down_ids = downstream.get(&root.id).cloned().unwrap_or_default();
    let trans_up_ids = bfs_walk(&root.id, &upstream);
    let trans_down_ids = bfs_walk(&root.id, &downstream);

    let mut subgraph_nodes: BTreeSet<Id> = BTreeSet::new();
    subgraph_nodes.insert(root.id.clone());
    subgraph_nodes.extend(trans_up_ids.iter().cloned());
    subgraph_nodes.extend(trans_down_ids.iter().cloned());

    let mut edges_set: BTreeSet<(Id, Id)> = BTreeSet::new();
    for link in &all_links {
        if link.kind == "depends_on"
            && subgraph_nodes.contains(&link.source_id)
            && subgraph_nodes.contains(&link.target_id)
        {
            edges_set.insert((link.source_id.clone(), link.target_id.clone()));
        }
    }

    let to_summaries = |ids: Vec<Id>| -> Vec<TaskRefSummary> {
        ids.into_iter()
            .filter_map(|id| all_tasks.get(&id).map(build_ref_summary))
            .collect()
    };
    let edges = edges_set
        .into_iter()
        .map(|(task_id, depends_on_task_id)| TaskDependencyEdge {
            task_id: task_id.as_str().to_owned(),
            depends_on_task_id: depends_on_task_id.as_str().to_owned(),
        })
        .collect();

    Ok(TaskDependenciesResult {
        task: build_ref_summary(&root),
        direct_depends_on: to_summaries(direct_up_ids),
        direct_dependents: to_summaries(direct_down_ids),
        transitive_depends_on: to_summaries(trans_up_ids),
        transitive_dependents: to_summaries(trans_down_ids),
        edges,
    })
}

fn bfs_walk(root: &Id, adj: &BTreeMap<Id, Vec<Id>>) -> Vec<Id> {
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    let mut queue = VecDeque::new();
    visited.insert(root.clone());
    queue.push_back(root.clone());
    while let Some(curr) = queue.pop_front() {
        let Some(neighbors) = adj.get(&curr) else {
            continue;
        };
        for next in neighbors {
            if visited.insert(next.clone()) {
                order.push(next.clone());
                queue.push_back(next.clone());
            }
        }
    }
    order
}

pub fn list_comments<R: TasksRead>(
    view: &R,
    reference: &str,
) -> Result<TasksCommentsResult, Refusal> {
    let task = require_task(view, reference)?;
    let comments = view
        .task_comments(&task.id)
        .map_err(storage_refusal)?
        .iter()
        .map(comment_to_wire)
        .collect();
    Ok(TasksCommentsResult {
        task_id: task.id.as_str().to_owned(),
        comments,
    })
}

pub fn list_history<R: TasksRead>(
    view: &R,
    reference: &str,
) -> Result<TasksHistoryResult, Refusal> {
    let task = require_task(view, reference)?;
    let events = view
        .task_events(&task.id)
        .map_err(storage_refusal)?
        .iter()
        .map(event_to_wire)
        .collect();
    Ok(TasksHistoryResult {
        task_id: task.id.as_str().to_owned(),
        events,
    })
}

pub fn list_labels<R: TasksRead>(
    view: &R,
    params: &TasksLabelsParams,
) -> Result<TasksLabelsResult, Refusal> {
    let query = params
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(str::to_ascii_lowercase);
    let labels = view
        .task_label_counts()
        .map_err(storage_refusal)?
        .into_iter()
        .filter(|(label, _)| query.as_deref().is_none_or(|q| label.contains(q)))
        .map(|(label, task_count)| TaskLabelSummary { label, task_count })
        .collect();
    Ok(TasksLabelsResult { labels })
}
