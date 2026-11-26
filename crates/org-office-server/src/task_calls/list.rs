//! `tasks.list` filters the office's own task query does not hold, and its pages (`TASK-85.3`):
//! `exclude_status`, `unassigned`, `modified_file` and `offset`. They are applied here, over the
//! office's sorted answer and without touching task storage, so `total` counts every match and
//! each page is a slice of the one order.

use org_office::{Office, OfficeError};
use org_proto::tasks::{TaskView, TasksListParams, TasksListResult};
use org_proto::{ApiError, ErrorCode};

/// Answers `tasks.list` with every filter of `params`, then the page `offset`..`offset + limit`.
pub(super) fn list_tasks(
    office: &Office,
    params: TasksListParams,
) -> Result<Result<TasksListResult, ApiError>, OfficeError> {
    let modified_file = match normalized_path(params.modified_file.as_deref()) {
        Ok(path) => path,
        Err(refusal) => return Ok(Err(refusal)),
    };
    if let Err(refusal) = check_filters(&params) {
        return Ok(Err(refusal));
    }
    let offset = usize::try_from(params.offset.unwrap_or(0)).unwrap_or(usize::MAX);
    let limit = params.limit.map(|limit| limit as usize);
    let filtered_here =
        !params.exclude_status.is_empty() || params.unassigned || modified_file.is_some();
    let mut base = TasksListParams {
        exclude_status: Vec::new(),
        unassigned: false,
        modified_file: None,
        offset: None,
        ..params.clone()
    };
    // Without a filter of its own here, the office's count is the total and it builds only the
    // tasks up to the end of the page. With one, every match is read and counted here.
    base.limit = match (filtered_here, limit) {
        (false, Some(limit)) => {
            Some(u32::try_from(offset.saturating_add(limit)).unwrap_or(u32::MAX))
        }
        _ => None,
    };
    let answered = match office.task_list(base)? {
        Ok(answered) => answered,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let (matched, total): (Vec<TaskView>, u64) = if filtered_here {
        let kept: Vec<TaskView> = answered
            .tasks
            .into_iter()
            .filter(|task| keeps(task, &params, modified_file.as_deref()))
            .collect();
        let total = kept.len() as u64;
        (kept, total)
    } else {
        (answered.tasks, answered.total)
    };
    let tasks = matched
        .into_iter()
        .skip(offset)
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    Ok(Ok(TasksListResult { tasks, total }))
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, message)
}

/// Filters that contradict each other are refused rather than answering an empty list.
fn check_filters(params: &TasksListParams) -> Result<(), ApiError> {
    if params.unassigned && params.assignee_id.is_some() {
        return Err(invalid(
            "`unassigned` and `assignee_id` cannot be given together",
        ));
    }
    if let Some(status) = params.status {
        if params.exclude_status.contains(&status) {
            return Err(invalid(
                "`status` and `exclude_status` name the same status",
            ));
        }
    }
    Ok(())
}

/// The path `modified_file` names, without `./` in front or `/` at the end. An empty path, or
/// one with a `..` component, is refused.
fn normalized_path(raw: Option<&str>) -> Result<Option<String>, ApiError> {
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

fn keeps(task: &TaskView, params: &TasksListParams, modified_file: Option<&str>) -> bool {
    if params.exclude_status.contains(&task.status) {
        return false;
    }
    if params.unassigned && task.assignee_id.is_some() {
        return false;
    }
    match modified_file {
        Some(path) => task.modified_files.iter().any(|file| names(file, path)),
        None => true,
    }
}

/// Whether the recorded `file` is `path`, or lies under it as a directory (component-wise:
/// `src/a` names `src/a/b.rs`, never `src/ab.rs`).
fn names(file: &str, path: &str) -> bool {
    let file = file.strip_prefix("./").unwrap_or(file);
    match file.strip_prefix(path) {
        Some("") => true,
        Some(rest) => rest.starts_with('/'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{names, normalized_path};

    #[test]
    fn a_path_names_itself_and_the_files_under_it_only() {
        assert!(names("src/a.rs", "src/a.rs"));
        assert!(names("src/a/b.rs", "src/a"));
        assert!(names("./src/a/b.rs", "src/a"));
        assert!(!names("src/ab.rs", "src/a"));
        assert!(!names("src", "src/a"));
    }

    #[test]
    fn a_modified_file_filter_is_normalized_or_refused() {
        assert_eq!(
            normalized_path(Some("./src/a/")).unwrap().as_deref(),
            Some("src/a")
        );
        assert_eq!(normalized_path(None).unwrap(), None);
        for bad in ["", "  ", "./", "/", "src/../etc"] {
            assert!(normalized_path(Some(bad)).is_err(), "{bad:?}");
        }
    }
}
