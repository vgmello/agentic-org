//! FTS5-backed `tasks.list --query` resolution (`TASK-91` AC #3) in `org-office-server`.
//!
//! Keeps `crates/org-office` non-test LOC unchanged while upgrading `tasks.list` text search
//! from literal substring scanning to the office's FTS5 `search.query` index (Porter stemming,
//! multi-word matching, and title/body/details/labels/files/comments/checklist coverage) and
//! preserving every structured `TasksListParams` filter and sort mode.

use std::collections::HashSet;

use org_office::{Office, OfficeError};
use org_proto::search::{SearchEntityType, SearchHitField, SearchQueryParams, SEARCH_MAX_LIMIT};
use org_proto::tasks::{TasksListParams, TasksListResult};
use org_proto::ApiError;

/// Upper bound on FTS5 pages walked for a single `tasks.list --query` call (5,000 hits).
const MAX_SEARCH_PAGES: usize = 50;

/// Executes `tasks.list` with a non-empty `query` via the office's FTS5 `search.query` index,
/// then applies all structured `TasksListParams` filters, ordering, and pagination.
pub(super) fn list_tasks_with_query(
    office: &Office,
    member: &str,
    params: TasksListParams,
) -> Result<Result<TasksListResult, ApiError>, OfficeError> {
    let query_text = params
        .query
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    if query_text.is_empty() {
        return office.task_list(params);
    }

    let matched_ids = match collect_fts_task_ids(office, member, &query_text, &params)? {
        Ok(ids) => ids,
        Err(refusal) => return Ok(Err(refusal)),
    };

    if matched_ids.is_empty() {
        // Validate contradictory/malformed structured filters identically to non-query `tasks.list`.
        let check = TasksListParams {
            query: None,
            limit: Some(0),
            offset: None,
            ..params
        };
        return match office.task_list(check)? {
            Ok(_) => Ok(Ok(TasksListResult {
                tasks: Vec::new(),
                total: 0,
            })),
            Err(refusal) => Ok(Err(refusal)),
        };
    }

    let offset = params.offset.unwrap_or(0) as usize;
    let limit = params.limit.map(|n| n as usize);
    let structured = TasksListParams {
        query: None,
        limit: None,
        offset: None,
        ..params
    };
    let mut listed = match office.task_list(structured)? {
        Ok(res) => res,
        Err(refusal) => return Ok(Err(refusal)),
    };
    listed.tasks.retain(|task| matched_ids.contains(&task.id));
    let total = listed.tasks.len() as u64;
    let tasks = match limit {
        Some(limit) => listed.tasks.into_iter().skip(offset).take(limit).collect(),
        None => listed.tasks.into_iter().skip(offset).collect(),
    };
    Ok(Ok(TasksListResult { tasks, total }))
}

fn collect_fts_task_ids(
    office: &Office,
    member: &str,
    query_text: &str,
    params: &TasksListParams,
) -> Result<Result<HashSet<String>, ApiError>, OfficeError> {
    let mut matched_ids = HashSet::new();
    let mut cursor: Option<String> = None;
    let status = params.status.map(|s| s.as_str().to_owned());

    for _ in 0..MAX_SEARCH_PAGES {
        let search_params = SearchQueryParams {
            q: query_text.to_owned(),
            types: vec![SearchEntityType::Task],
            status: status.clone(),
            limit: Some(SEARCH_MAX_LIMIT),
            cursor: cursor.take(),
            fields: vec![SearchHitField::Score],
            ..SearchQueryParams::default()
        };
        let page = match office.search(member, search_params)? {
            Ok(res) => res,
            Err(refusal) => return Ok(Err(refusal)),
        };
        for hit in page.hits {
            matched_ids.insert(hit.entity_id);
        }
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(Ok(matched_ids))
}
