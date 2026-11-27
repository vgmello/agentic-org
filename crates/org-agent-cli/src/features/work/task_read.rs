//! The `task` reads: `task list` (filters, sort, pages up to 200 with `--cursor` and
//! `meta.next_cursor`, `meta.total`, and `--count`), `task inspect|history|deps|labels`.

use org_command::{Answer, Command, Example, Kind, Refusal, Request};
use org_proto::tasks::{
    LabelMatchMode, TaskDependenciesResult, TaskInspectResult, TaskSortMode, TaskStatus,
    TasksDependencies, TasksDependenciesParams, TasksHistory, TasksHistoryParams,
    TasksHistoryResult, TasksInspect, TasksInspectParams, TasksLabels, TasksLabelsParams,
    TasksLabelsResult, TasksList, TasksListParams, TasksListResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Map, Value};

use super::schema::{
    EVENT_FIELDS, READ_ERRORS, TASK_DEPS_OUTPUT, TASK_FIELDS, TASK_INSPECT_OUTPUT,
    TASK_LABELS_INPUT, TASK_LABEL_OUTPUT, TASK_LIST_INPUT, TASK_REF_INPUT,
};
use super::{task_to_row, unreadable};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{is_set, opt_str_list, opt_string, require_str};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// The `task` read rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "task",
        verb: "list",
        summary: "List and filter tasks",
        call: "tasks.list",
        kind: Kind::List,
        input: TASK_LIST_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"labels":["backend","security"],"label_match":"all","sort":"order"}"#,
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","type":"task","title":"Implement auth","status":"todo","priority":"high","team_id":"0193a5c0-0000-7000-8000-000000000010","rank_order":10,"labels":["backend","security"],"reference_files":["docs/auth.md"],"modified_files":["src/auth.rs"],"depends_on":[],"dependents":[],"revision":1,"checklist_total":0,"checklist_done":0,"comment_count":0,"watcher_count":0}]"#,
        },
    },
    Command {
        resource: "task",
        verb: "inspect",
        summary: "Inspect a task",
        call: "tasks.inspect",
        kind: Kind::Read,
        input: TASK_REF_INPUT,
        content_field: None,
        output: TASK_INSPECT_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: r#"{"task":{"id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","type":"task","title":"Implement auth","status":"todo","priority":"high","team_id":"0193a5c0-0000-7000-8000-000000000010","rank_order":10,"labels":["backend","security"],"reference_files":["docs/auth.md"],"modified_files":["src/auth.rs"],"depends_on":[],"dependents":[],"revision":1,"checklist_total":0,"checklist_done":0,"comment_count":0,"watcher_count":0},"body_markdown":"","checklist":[],"comments":[],"depends_on_tasks":[],"dependent_tasks":[],"history":[]}"#,
        },
    },
    Command {
        resource: "task",
        verb: "history",
        summary: "List task history",
        call: "tasks.history",
        kind: Kind::List,
        input: TASK_REF_INPUT,
        content_field: None,
        output: EVENT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: r#"[{"task_id":"0193a5c0-0000-7000-8000-000000000100","revision":1,"occurred_at":"2026-11-22T00:00:00.000Z","actor_kind":"employee","actor_id":"0193a5c0-0000-7000-8000-000000000020","payload_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}]"#,
        },
    },
    Command {
        resource: "task",
        verb: "deps",
        summary: "Show task dependency graph",
        call: "tasks.dependencies",
        kind: Kind::Read,
        input: TASK_REF_INPUT,
        content_field: None,
        output: TASK_DEPS_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"task":"T-A2"}"#,
            output: r#"{"task":{"id":"0193a5c0-0000-7000-8000-000000000101","key":"T-A2","title":"Adapter","type":"task","status":"todo","priority":"normal"},"direct_depends_on":[],"direct_dependents":[],"transitive_depends_on":[],"transitive_dependents":[],"edges":[]}"#,
        },
    },
    Command {
        resource: "task",
        verb: "labels",
        summary: "Search task labels",
        call: "tasks.labels",
        kind: Kind::List,
        input: TASK_LABELS_INPUT,
        content_field: None,
        output: TASK_LABEL_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"query":"sec"}"#,
            output: r#"[{"label":"security","task_count":2}]"#,
        },
    },
];

/// `Use when:` guidance of each `task` command here.
pub(super) const CUES: &[CommandCue] = &[
    ("task", "deps", "inspecting direct and transitive upstream/downstream dependency edges around a task."),
    ("task", "history", "auditing the immutable revision event log for a task."),
    ("task", "inspect", "reading a task's full details, Markdown body, checklist, comments, dependencies, and history."),
    ("task", "labels", "discovering existing normalized task labels and counts before filtering or tagging tasks."),
    ("task", "list", "querying, sorting and paging tasks by status (or `--exclude-status`), team, assignee (or `--unassigned`), project, labels, dependencies, `--modified-file` or search text; `--count` for the total only."),
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

/// The office's `tasks.list` parameters for one page: filters that contradict each other are
/// refused here, before anything is sent.
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
        status_scope: None,
        done_since: None,
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
    };
    let raw = transport.call_method(TasksInspect::METHOD, json!(params))?;
    let res: TaskInspectResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(Answer::completed(json!({
        "task": task_to_row(&res.task),
        "body_markdown": res.task.body_markdown,
        "checklist": res.checklist,
        "comments": res.comments,
        "depends_on_tasks": res.depends_on_tasks,
        "dependent_tasks": res.dependent_tasks,
        "history": res.history,
    })))
}

pub(super) fn read_task_history<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let params = TasksHistoryParams {
        task: require_str(&request.input, "task")?.to_owned(),
    };
    let raw = transport.call_method(TasksHistory::METHOD, json!(params))?;
    let res: TasksHistoryResult = serde_json::from_value(raw).map_err(unreadable)?;
    let items: Vec<Value> = res.events.into_iter().map(|e| json!(e)).collect();
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
