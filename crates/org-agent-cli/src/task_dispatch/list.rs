//! `task list`: filters, sort, pages (`--limit` up to 200, `--cursor`, `meta.next_cursor`),
//! `meta.total`, and `--count`.

use org_command::{Answer, Refusal, Request};
use org_proto::tasks::{
    LabelMatchMode, TaskSortMode, TaskStatus, TasksList, TasksListParams, TasksListResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Map, Value};

use super::{decode_err, opt_bool, opt_str, opt_str_list, task_to_row};
use crate::transport::{parse_cursor, OfficeTransport};

fn invalid(message: impl Into<String>, hint: &str) -> Refusal {
    Refusal::new(ErrorCode::InvalidRequest, message, hint)
}

fn parse_status(raw: &str) -> Result<TaskStatus, Refusal> {
    TaskStatus::parse(raw).ok_or_else(|| {
        invalid(
            format!("invalid status `{raw}`"),
            "Use todo, in_progress, blocked, cancelled, or done.",
        )
    })
}

fn label_match(input: &Map<String, Value>) -> Result<LabelMatchMode, Refusal> {
    match opt_str(input, "label_match").as_deref() {
        None | Some("all") => Ok(LabelMatchMode::All),
        Some("any") => Ok(LabelMatchMode::Any),
        Some(other) => Err(invalid(
            format!("invalid label_match `{other}`"),
            "Use `all` or `any`.",
        )),
    }
}

fn sort(input: &Map<String, Value>) -> Result<TaskSortMode, Refusal> {
    match opt_str(input, "sort").as_deref() {
        None | Some("order") => Ok(TaskSortMode::Order),
        Some("number") => Ok(TaskSortMode::Number),
        Some("updated_desc") => Ok(TaskSortMode::UpdatedDesc),
        Some(other) => Err(invalid(
            format!("invalid sort `{other}`"),
            "Use `order`, `number`, or `updated_desc`.",
        )),
    }
}

/// The office's `tasks.list` parameters for one page: filters that contradict each other are
/// refused here, before anything is sent.
fn list_params(request: &Request, offset: usize) -> Result<TasksListParams, Refusal> {
    let i = &request.input;
    let status = opt_str(i, "status").map(|s| parse_status(&s)).transpose()?;
    let exclude_status = opt_str_list(i, "exclude_status")
        .unwrap_or_default()
        .iter()
        .map(|s| parse_status(s))
        .collect::<Result<Vec<_>, _>>()?;
    if status.is_some_and(|status| exclude_status.contains(&status)) {
        return Err(invalid(
            "--status and --exclude-status name the same status.",
            "Give a status to keep with --status, or statuses to leave out with --exclude-status.",
        ));
    }
    let unassigned = opt_bool(i, "unassigned");
    if unassigned && i.contains_key("assignee_id") {
        return Err(invalid(
            "--unassigned and --assignee-id cannot be given together.",
            "Use --unassigned for tasks nobody is assigned to, or --assignee-id for one employee's.",
        ));
    }
    let count = opt_bool(i, "count");
    if count && request.cursor.is_some() {
        return Err(invalid(
            "--count answers no page, so it takes no --cursor.",
            "Run `task list --count` with the filters alone.",
        ));
    }
    let limit = if count {
        0
    } else {
        request.limit.unwrap_or(org_command::DEFAULT_LIMIT)
    };
    Ok(TasksListParams {
        task_type: opt_str(i, "type"),
        status,
        team_id: opt_str(i, "team_id"),
        assigned_team_id: opt_str(i, "assigned_team_id"),
        assignee_id: opt_str(i, "assignee_id"),
        project_id: opt_str(i, "project_id"),
        labels: opt_str_list(i, "labels").unwrap_or_default(),
        label_match: label_match(i)?,
        depends_on: opt_str(i, "depends_on"),
        dependent_of: opt_str(i, "dependent_of"),
        query: opt_str(i, "query"),
        sort: sort(i)?,
        limit: Some(limit),
        exclude_status,
        unassigned,
        modified_file: opt_str(i, "modified_file"),
        offset: (offset > 0).then_some(offset as u64),
    })
}

pub(super) fn read_task_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let offset = parse_cursor(request.cursor.as_deref())?;
    let params = list_params(request, offset)?;
    let raw = transport.call_method(TasksList::METHOD, json!(params))?;
    let res: TasksListResult = serde_json::from_value(raw).map_err(decode_err)?;
    let limit = params.limit.unwrap_or(0) as usize;
    // The office answers at most the page; anything more is a fault, not more data to print.
    let items: Vec<Value> = res.tasks.iter().take(limit).map(task_to_row).collect();
    let returned = items.len();
    let next = offset.saturating_add(returned);
    let mut answer = Answer::completed(Value::Array(items));
    // An empty page never points on: its cursor would be its own, and a caller paging until
    // `next_cursor` is absent would loop (the total moved under it, or the office answered short).
    if limit > 0 && returned > 0 && (next as u64) < res.total {
        answer.next_cursor = Some(next.to_string());
    }
    answer.total = Some(res.total);
    Ok(answer)
}
