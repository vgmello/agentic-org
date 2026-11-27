//! Read queries for unified tasks: inspection, dependency hierarchy traversal, indexed label
//! catalog, comments, and immutable history (`TASK-49`, `TASK-77`). Listing is `list.rs`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use org_proto::attachments::{
    AttachmentMeta, AttachmentTargetKind, TASK_ATTACHMENTS_MAX, TASK_COMMENT_ATTACHMENTS_MAX,
};
use org_proto::tasks::{
    TaskCommentWire, TaskDependenciesResult, TaskDependencyEdge, TaskInspectResult,
    TaskLabelSummary, TaskRefSummary, TasksCommentsResult, TasksHistoryResult, TasksLabelsParams,
    TasksLabelsResult,
};
use org_store::{Id, Refusal};

use crate::attachments::records::AttachmentRecord;
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{TaskCommentRecord, TaskRecord};

use super::{
    build_ref_summary, build_task_view, comment_to_wire, event_to_wire, require_task, step_to_wire,
    storage_refusal, watcher_to_wire,
};

fn hydrate_comments<R: TasksRead>(
    view: &R,
    comments: &[TaskCommentRecord],
) -> Result<Vec<TaskCommentWire>, Refusal> {
    let mut out = Vec::with_capacity(comments.len());
    for comment in comments {
        let attachments: Vec<AttachmentMeta> = view
            .attachments_for_target(
                AttachmentTargetKind::TaskComment,
                Some(&comment.id),
                None,
                TASK_COMMENT_ATTACHMENTS_MAX,
            )
            .map_err(storage_refusal)?
            .iter()
            .map(AttachmentRecord::to_meta)
            .collect();
        out.push(comment_to_wire(comment, attachments));
    }
    Ok(out)
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
    let comment_records = view.task_comments(&task.id).map_err(storage_refusal)?;
    let comments = hydrate_comments(view, &comment_records)?;
    let attachments = view
        .attachments_for_target(
            AttachmentTargetKind::Task,
            Some(&task.id),
            None,
            TASK_ATTACHMENTS_MAX,
        )
        .map_err(storage_refusal)?
        .iter()
        .map(AttachmentRecord::to_meta)
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
        attachments,
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
    let records = view.task_comments(&task.id).map_err(storage_refusal)?;
    let comments = hydrate_comments(view, &records)?;
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
