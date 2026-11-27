//! The `task` reads: `task list` (filters, sort, pages up to 200 with `--cursor` and
//! `meta.next_cursor`, `meta.total`, and `--count`), `task inspect|history|deps|labels`.

use org_command::{Answer, Refusal, Request};
use org_proto::tasks::{
    LabelMatchMode, TaskDependenciesResult, TaskInspectResult, TaskSortMode, TaskStatus,
    TaskStatusScope, TasksDependencies, TasksDependenciesParams, TasksHistory, TasksHistoryParams,
    TasksHistoryResult, TasksInspect, TasksInspectParams, TasksLabels, TasksLabelsParams,
    TasksLabelsResult, TasksList, TasksListParams, TasksListResult,
};
use org_proto::{parse_rfc3339_ms, Call, ErrorCode};
use serde_json::{json, Map, Value};

use super::{task_to_row, unreadable};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{is_set, opt_str_list, opt_string, require_str};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// `Use when:` guidance of each `task` command here.
pub(super) const CUES: &[CommandCue] = &[
    ("task", "deps", "inspecting direct and transitive upstream/downstream dependency edges around a task."),
    ("task", "history", "auditing the immutable revision event log for a task (filter with `--event-type` or `--blocks-only`)."),
    ("task", "inspect", "reading a task's details, Markdown body, checklist, comments, watchers, dependencies, and history (use `--fields` to prune unrequested sections server-side)."),
    ("task", "labels", "discovering existing normalized task labels and counts before filtering or tagging tasks."),
    ("task", "list", "querying, sorting and paging tasks (defaults to open tasks; pass `--all`, `--status-scope`, `--done-since`, `--ready`, `--dependency-blocked`, `--status`, `--exclude-status`, team, assignee, `--unassigned`, project, labels, dependencies, `--modified-file`, `--query`, or `--count`)."),
];

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
    match opt_string(input, "label_match").as_deref() {
        None | Some("all") => Ok(LabelMatchMode::All),
        Some("any") => Ok(LabelMatchMode::Any),
        Some(other) => Err(invalid(
            format!("invalid label_match `{other}`"),
            "Use `all` or `any`.",
        )),
    }
}

fn sort(input: &Map<String, Value>) -> Result<TaskSortMode, Refusal> {
    match opt_string(input, "sort").as_deref() {
        None | Some("order") => Ok(TaskSortMode::Order),
        Some("number") => Ok(TaskSortMode::Number),
        Some("updated_desc") => Ok(TaskSortMode::UpdatedDesc),
        Some(other) => Err(invalid(
            format!("invalid sort `{other}`"),
            "Use `order`, `number`, or `updated_desc`.",
        )),
    }
}

fn parse_status_scope(raw: &str) -> Result<TaskStatusScope, Refusal> {
    TaskStatusScope::parse(raw).ok_or_else(|| {
        invalid(
            format!("invalid status_scope `{raw}`"),
            "Use open, closed, or all.",
        )
    })
}

/// The office's `tasks.list` parameters for one page: filters that contradict each other are
/// refused here, before anything is sent. Defaults to `status_scope = open` unless `--status`,
/// `--all`, or `--status-scope` is given (`TASK-105`).
fn list_params(request: &Request, offset: usize) -> Result<TasksListParams, Refusal> {
    let i = &request.input;
    let status = opt_string(i, "status")
        .map(|s| parse_status(&s))
        .transpose()?;
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
    let all = is_set(i, "all");
    let explicit_scope = opt_string(i, "status_scope")
        .map(|s| parse_status_scope(&s))
        .transpose()?;
    if all && explicit_scope.is_some() {
        return Err(invalid(
            "--all and --status-scope cannot be given together.",
            "Pass either --all or --status-scope <open|closed|all>, not both.",
        ));
    }
    let status_scope = if all {
        Some(TaskStatusScope::All)
    } else if explicit_scope.is_some() {
        explicit_scope
    } else if status.is_some() {
        None
    } else {
        Some(TaskStatusScope::Open)
    };
    let done_since = opt_string(i, "done_since");
    if let Some(ref raw) = done_since {
        if parse_rfc3339_ms(raw.trim()).is_none() {
            return Err(invalid(
                format!("invalid --done-since timestamp `{raw}`."),
                "Pass an RFC 3339 timestamp such as 2026-11-26T00:00:00Z.",
            ));
        }
    }
    let ready = is_set(i, "ready");
    let dependency_blocked = is_set(i, "dependency_blocked");
    if ready && dependency_blocked {
        return Err(invalid(
            "--ready and --dependency-blocked cannot be given together.",
            "Use --ready for actionable tasks, or --dependency-blocked for tasks waiting on prerequisites.",
        ));
    }
    if ready && status_scope == Some(TaskStatusScope::Closed) {
        return Err(invalid(
            "--ready cannot be combined with --status-scope closed.",
            "Remove --status-scope closed or omit --ready.",
        ));
    }
    if ready && status.is_some_and(|s| s == TaskStatus::Blocked || s.is_closed()) {
        return Err(invalid(
            "--ready contradicts a blocked or closed --status filter.",
            "Use --ready with todo/in_progress or omit --status.",
        ));
    }
    if let (Some(scope), Some(st)) = (status_scope, status) {
        let admitted = scope.admits(st)
            || (scope == TaskStatusScope::Open && st == TaskStatus::Done && done_since.is_some());
        if !admitted {
            return Err(invalid(
                format!(
                    "--status {} is outside --status-scope {}.",
                    st.as_str(),
                    scope.as_str()
                ),
                "Pass --all or a matching --status-scope for that status.",
            ));
        }
    }
    let unassigned = is_set(i, "unassigned");
    if unassigned && i.contains_key("assignee_id") {
        return Err(invalid(
            "--unassigned and --assignee-id cannot be given together.",
            "Use --unassigned for tasks nobody is assigned to, or --assignee-id for one employee's.",
        ));
    }
    let count = is_set(i, "count");
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
        task_type: opt_string(i, "type"),
        status,
        team_id: opt_string(i, "team_id"),
        assigned_team_id: opt_string(i, "assigned_team_id"),
        assignee_id: opt_string(i, "assignee_id"),
        project_id: opt_string(i, "project_id"),
        labels: opt_str_list(i, "labels").unwrap_or_default(),
        label_match: label_match(i)?,
        depends_on: opt_string(i, "depends_on"),
        dependent_of: opt_string(i, "dependent_of"),
        query: opt_string(i, "query"),
        sort: sort(i)?,
        limit: Some(limit),
        exclude_status,
        unassigned,
        modified_file: opt_string(i, "modified_file"),
        offset: (offset > 0).then_some(offset as u64),
        status_scope,
        done_since,
        ready,
        dependency_blocked,
    })
}

pub(super) fn read_task_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let offset = parse_cursor(request.cursor.as_deref())?;
    let params = list_params(request, offset)?;
    let raw = transport.call_method(TasksList::METHOD, json!(params))?;
    let res: TasksListResult = serde_json::from_value(raw).map_err(unreadable)?;
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

pub(super) fn read_task_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksInspectParams {
        task: require_str(&request.input, "task")?.to_owned(),
        fields: request.fields.clone(),
    };
    let raw = transport.call_method(TasksInspect::METHOD, json!(params))?;
    let res: TaskInspectResult = serde_json::from_value(raw).map_err(unreadable)?;
    let include = |name: &str| {
        request
            .fields
            .as_ref()
            .is_none_or(|roots| roots.iter().any(|r| r == name))
    };
    let mut data = Map::new();
    data.insert("task".into(), task_to_row(&res.task));
    type SectionRow = (&'static str, fn(&TaskInspectResult) -> Value);
    let sections: [SectionRow; 8] = [
        ("body_markdown", |r| json!(r.task.body_markdown)),
        ("checklist", |r| json!(r.checklist)),
        ("comments", |r| json!(r.comments)),
        ("attachments", |r| json!(r.attachments)),
        ("watchers", |r| json!(r.watchers)),
        ("depends_on_tasks", |r| json!(r.depends_on_tasks)),
        ("dependent_tasks", |r| json!(r.dependent_tasks)),
        ("history", |r| json!(r.history)),
    ];
    for (name, build) in sections {
        if include(name) {
            data.insert(name.into(), build(&res));
        }
    }
    Ok(Answer::completed(Value::Object(data)))
}

pub(super) fn read_task_history<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let event_type = match opt_string(&request.input, "event_type") {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(invalid(
                    "--event-type cannot be empty.",
                    "Pass a change type such as --event-type status_changed.",
                ));
            }
            Some(trimmed.to_owned())
        }
        None => None,
    };
    let blocks_only = is_set(&request.input, "blocks_only");
    if blocks_only && event_type.as_deref().is_some_and(|t| t != "status_changed") {
        return Err(invalid(
            "--blocks-only matches only `status_changed` events.",
            "Remove --event-type or pass --event-type status_changed with --blocks-only.",
        ));
    }
    let params = TasksHistoryParams {
        task: require_str(&request.input, "task")?.to_owned(),
        event_type: event_type.clone(),
        blocks_only,
    };
    let raw = transport.call_method(TasksHistory::METHOD, json!(params))?;
    let res: TasksHistoryResult = serde_json::from_value(raw).map_err(unreadable)?;
    let items: Vec<Value> = res
        .events
        .into_iter()
        .filter(|e| {
            let kind = e.change.get("type").and_then(Value::as_str);
            if let Some(ref wanted) = event_type {
                if kind != Some(wanted.as_str()) {
                    return false;
                }
            }
            if blocks_only {
                let from = e.change.get("from").and_then(Value::as_str);
                let to = e.change.get("to").and_then(Value::as_str);
                return kind == Some("status_changed")
                    && (from == Some("blocked") || to == Some("blocked"));
            }
            true
        })
        .map(|e| json!(e))
        .collect();
    paginate(request, items)
}

pub(super) fn read_task_deps<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksDependenciesParams {
        task: require_str(&request.input, "task")?.to_owned(),
    };
    let raw = transport.call_method(TasksDependencies::METHOD, json!(params))?;
    let res: TaskDependenciesResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(Answer::completed(json!(res)))
}

pub(super) fn read_task_labels<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let params = TasksLabelsParams {
        query: opt_string(&request.input, "query"),
    };
    let raw = transport.call_method(TasksLabels::METHOD, json!(params))?;
    let res: TasksLabelsResult = serde_json::from_value(raw).map_err(unreadable)?;
    let items: Vec<Value> = res.labels.into_iter().map(|l| json!(l)).collect();
    paginate(request, items)
}
