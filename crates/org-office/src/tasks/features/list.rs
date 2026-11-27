//! `tasks.list`: every filter, the order and the page in one validated [`TaskListQuery`]
//! the engine answers itself, then each row's rollup read for the whole page at once
//! (`TASK-49`, `TASK-77`, `TASK-85.3`, `TASK-105`).
//!
//! The status filters form one set: `status_scope` (every status when absent), narrowed to
//! `status` when given, less `exclude_status`. `done_since` keeps a `done` task only when it
//! was last updated at or after that instant; with `status_scope = open` it also admits
//! those recently done tasks. Filters that contradict each other are refused rather than
//! answered with an empty list.

use org_proto::tasks::{TaskStatus, TaskStatusScope, TasksListParams, TasksListResult};
use org_proto::ErrorCode;
use org_store::{Id, Refusal};

use crate::tasks::ports::TasksRead;
use crate::tasks::query::{LabelFilter, TaskListQuery, TaskStatusSet};
use crate::tasks::records::normalize_labels;
use crate::time::parse_rfc3339_ms;

use super::{refuse, require_task, storage_refusal, task_view};

/// Answers `tasks.list`: the page `offset`..`offset + limit` of every match, in `sort`
/// order, and how many match in all.
pub fn list_tasks<R: TasksRead>(
    view: &R,
    params: &TasksListParams,
) -> Result<TasksListResult, Refusal> {
    let query = list_query(view, params)?;
    let page = view.task_page(&query).map_err(storage_refusal)?;
    let ids: Vec<Id> = page.tasks.iter().map(|task| task.id.clone()).collect();
    let mut rollups = view.task_rollups(&ids).map_err(storage_refusal)?;
    let tasks = page
        .tasks
        .iter()
        .map(|task| task_view(task, rollups.remove(&task.id).unwrap_or_default()))
        .collect();
    Ok(TasksListResult {
        tasks,
        total: page.total,
    })
}

fn invalid(message: impl Into<String>) -> Refusal {
    refuse(ErrorCode::InvalidRequest, message)
}

/// The query `params` ask, or the refusal of what they ask wrongly. Task references in the
/// dependency filters must name a task (`not_found` otherwise).
pub(crate) fn list_query<R: TasksRead>(
    view: &R,
    params: &TasksListParams,
) -> Result<TaskListQuery, Refusal> {
    let modified_file = normalized_path(params.modified_file.as_deref())?;
    if params.unassigned && params.assignee_id.is_some() {
        return Err(invalid(
            "`unassigned` and `assignee_id` cannot be given together",
        ));
    }
    if params.ready && params.dependency_blocked {
        return Err(invalid(
            "`ready` and `dependency_blocked` cannot be given together",
        ));
    }
    let mut statuses = statuses(params)?;
    if params.ready {
        if params.status_scope == Some(TaskStatusScope::Closed) {
            return Err(invalid(
                "`ready` cannot be combined with `status_scope` closed",
            ));
        }
        if let Some(status) = params.status {
            if status == TaskStatus::Blocked || status.is_closed() {
                return Err(invalid(format!(
                    "`ready` contradicts `status` {}",
                    status.as_str()
                )));
            }
        }
        statuses = statuses
            .without(TaskStatus::Blocked)
            .without(TaskStatus::Done)
            .without(TaskStatus::Cancelled);
    }
    let dependency_blocked = if params.dependency_blocked {
        Some(true)
    } else if params.ready {
        Some(false)
    } else {
        None
    };
    let done_since_ms = done_since(params.done_since.as_deref())?;
    let labels = if params.labels.is_empty() {
        None
    } else {
        let labels = normalize_labels(&params.labels).map_err(|e| refuse(e.code, e.message))?;
        Some(LabelFilter {
            labels,
            mode: params.label_match,
        })
    };
    let depends_on = match params.depends_on.as_deref() {
        Some(reference) => Some(require_task(view, reference)?.id),
        None => None,
    };
    let dependent_of = match params.dependent_of.as_deref() {
        Some(reference) => Some(require_task(view, reference)?.id),
        None => None,
    };
    let trimmed = |value: &Option<String>| value.as_deref().map(|v| v.trim().to_owned());
    Ok(TaskListQuery {
        task_type: trimmed(&params.task_type),
        statuses,
        done_since_ms,
        team_id: trimmed(&params.team_id),
        assigned_team_id: trimmed(&params.assigned_team_id),
        assignee_id: trimmed(&params.assignee_id),
        project_id: trimmed(&params.project_id),
        unassigned: params.unassigned,
        labels,
        depends_on,
        dependent_of,
        dependency_blocked,
        text: params
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(str::to_ascii_lowercase),
        modified_file,
        sort: params.sort,
        offset: params.offset.unwrap_or(0),
        limit: params.limit.map(u64::from),
    })
}

/// The statuses `status_scope`, `status` and `exclude_status` leave, with `done` admitted
/// back into an open scope when `done_since` asks for the recently done.
fn statuses(params: &TasksListParams) -> Result<TaskStatusSet, Refusal> {
    let mut set = match params.status_scope {
        None | Some(TaskStatusScope::All) => TaskStatusSet::ALL,
        Some(TaskStatusScope::Open) => TaskStatusSet::OPEN,
        Some(TaskStatusScope::Closed) => TaskStatusSet::CLOSED,
    };
    if params.status_scope == Some(TaskStatusScope::Open) && params.done_since.is_some() {
        set = set.with(TaskStatus::Done);
    }
    if let Some(status) = params.status {
        if params.exclude_status.contains(&status) {
            return Err(invalid(
                "`status` and `exclude_status` name the same status",
            ));
        }
        if !set.contains(status) {
            return Err(invalid(format!(
                "`status` {} is outside `status_scope` {}",
                status.as_str(),
                params.status_scope.map_or("all", TaskStatusScope::as_str)
            )));
        }
        set = TaskStatusSet::only(status);
    }
    for excluded in &params.exclude_status {
        set = set.without(*excluded);
    }
    Ok(set)
}

/// `done_since` as Unix milliseconds. Only an RFC 3339 timestamp is accepted.
fn done_since(raw: Option<&str>) -> Result<Option<u64>, Refusal> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    parse_rfc3339_ms(raw.trim()).map(Some).ok_or_else(|| {
        invalid("`done_since` must be an RFC 3339 timestamp such as 2026-11-26T00:00:00Z")
    })
}

/// The path `modified_file` names, without `./` in front or `/` at the end. An empty path, or
/// one with a `..` component, is refused.
fn normalized_path(raw: Option<&str>) -> Result<Option<String>, Refusal> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let mut path = raw.trim();
    while let Some(rest) = path.strip_prefix("./") {
        path = rest;
    }
    let path = path.trim_end_matches('/');
    if path.is_empty() || path == "." {
        return Err(invalid("`modified_file` names no path"));
    }
    if path.split('/').any(|part| part == "..") {
        return Err(invalid("`modified_file` may not hold a `..` component"));
    }
    Ok(Some(path.to_owned()))
}

#[cfg(test)]
#[path = "list_tests.rs"]
mod tests;
