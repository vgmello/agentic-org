//! Dispatch for unified `task` and `checklist` commands over [`OfficeTransport`]
//! (`TASK-49` AC #3 `CLI-15` + `TASK-77` B4b).

use org_command::{Answer, Refusal, Request, Status};
use org_proto::tasks::{
    PatchField, TaskDependenciesResult, TaskInspectResult, TaskPriority, TaskStatus, TaskView,
    TasksAddDependency, TasksAddDependencyParams, TasksAssign, TasksAssignParams, TasksComment,
    TasksCommentParams, TasksCommentResult, TasksCreate, TasksCreateParams, TasksDependencies,
    TasksDependenciesParams, TasksHistory, TasksHistoryParams, TasksHistoryResult, TasksInspect,
    TasksInspectParams, TasksLabels, TasksLabelsParams, TasksLabelsResult, TasksMutationResult,
    TasksRemoveDependency, TasksRemoveDependencyParams, TasksTransition, TasksTransitionParams,
    TasksUnwatch, TasksUnwatchParams, TasksUpdate, TasksUpdateParams, TasksWatch, TasksWatchParams,
    WorkType,
};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Map, Value};

use crate::transport::{command_id, paginate, parse_cursor, write_ack, OfficeTransport};

mod checklist;
mod list;
mod note;

use checklist::try_checklist;
use list::read_task_list;
use note::try_task_note;

pub fn try_execute<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.resource {
        "task" => try_task(transport, request),
        "checklist" => try_checklist(transport, request),
        "task-note" => try_task_note(transport, request),
        _ => None,
    }
}

fn try_task<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "create" => Some(write_task_create(transport, request)),
        "list" => Some(read_task_list(transport, request)),
        "inspect" => Some(read_task_inspect(transport, request)),
        "update" => Some(write_task_update(transport, request)),
        "transition" => Some(write_task_transition(transport, request)),
        "assign" => Some(write_task_assign(transport, request)),
        "comment" => Some(write_task_comment(transport, request)),
        "watch" => Some(write_task_watch(transport, request, true)),
        "unwatch" => Some(write_task_watch(transport, request, false)),
        "history" => Some(read_task_history(transport, request)),
        "deps" => Some(read_task_deps(transport, request)),
        "dep-add" => Some(write_task_dep(transport, request, true)),
        "dep-remove" => Some(write_task_dep(transport, request, false)),
        "labels" => Some(read_task_labels(transport, request)),
        _ => None,
    }
}

fn cmd_id(request: &Request) -> Result<String, Refusal> {
    if request.dry_run {
        return Err(Refusal::new(
            ErrorCode::Unsupported,
            "`--dry-run` is not supported on task or checklist write commands",
            "Re-run without `--dry-run`.",
        ));
    }
    command_id(request, "task-command")
}

fn entity_id(request: &Request, domain: &str) -> Result<String, Refusal> {
    crate::transport::entity_id(request, None, domain)
}

/// The answer to a write, acknowledged in the class its row declares (`doc-79`, `ADR-0014`).
fn write_answer(request: &Request, data: Value, revision: Option<u64>) -> Answer {
    Answer {
        data,
        status: Status::Completed,
        operation_id: request.idempotency_key.clone(),
        ack: Some(write_ack(request)),
        revision,
        next_cursor: None,
        waiting_messages: None,
        total: None,
    }
}

fn req_str<'a>(input: &'a Map<String, Value>, key: &str) -> Result<&'a str, Refusal> {
    input.get(key).and_then(Value::as_str).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("`{key}` is required"),
            format!("Provide `{key}`."),
        )
    })
}

fn opt_str(input: &Map<String, Value>, key: &str) -> Option<String> {
    input.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn opt_u64(input: &Map<String, Value>, key: &str) -> Result<Option<u64>, Refusal> {
    let Some(val) = input.get(key) else {
        return Ok(None);
    };
    let Some(n) = val.as_i64() else {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("`{key}` must be a nonnegative integer"),
            format!("Pass a nonnegative integer for `{key}`."),
        ));
    };
    u64::try_from(n).map(Some).map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("`{key}` must be a nonnegative integer"),
            format!("Pass a nonnegative integer for `{key}`."),
        )
    })
}

fn opt_bool(input: &Map<String, Value>, key: &str) -> bool {
    input.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn opt_str_list(input: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    input.get(key).and_then(Value::as_array).map(|arr| {
        arr.iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    })
}

fn require_revision(request: &Request) -> Result<u64, Refusal> {
    request.if_match.ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "`--if-match` revision is required",
            "Pass `--if-match <revision>` from `task inspect`.",
        )
    })
}

/// An answer the office sent that this build cannot read: a fault between `agent-os` and the
/// office (`unavailable`, which a retry with the same key may cure), never the office's storage.
fn decode_err(e: serde_json::Error) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        format!("The office returned a task answer this agent-os cannot read: {e}."),
        "Retry the command; if it persists, the office and agent-os versions differ.",
    )
}

fn task_to_row(t: &TaskView) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(t.id));
    m.insert("key".into(), json!(t.key));
    m.insert("type".into(), json!(t.task_type));
    m.insert("title".into(), json!(t.title));
    m.insert("status".into(), json!(t.status));
    m.insert("priority".into(), json!(t.priority));
    m.insert("team_id".into(), json!(t.team_id));
    if let Some(ref v) = t.assigned_team_id {
        m.insert("assigned_team_id".into(), json!(v));
    }
    if let Some(ref v) = t.assignee_id {
        m.insert("assignee_id".into(), json!(v));
    }
    if let Some(ref v) = t.project_id {
        m.insert("project_id".into(), json!(v));
    }
    if let Some(v) = t.rank_order {
        m.insert("rank_order".into(), json!(v));
    }
    m.insert("labels".into(), json!(t.labels));
    m.insert("reference_files".into(), json!(t.reference_files));
    m.insert("modified_files".into(), json!(t.modified_files));
    m.insert("depends_on".into(), json!(t.depends_on));
    m.insert("dependents".into(), json!(t.dependents));
    m.insert("revision".into(), json!(t.revision));
    m.insert("checklist_total".into(), json!(t.checklist_total));
    m.insert("checklist_done".into(), json!(t.checklist_done));
    m.insert("comment_count".into(), json!(t.comment_count));
    m.insert("watcher_count".into(), json!(t.watcher_count));
    Value::Object(m)
}

fn write_task_create<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let priority = opt_str(i, "priority")
        .map(|s| {
            TaskPriority::parse(&s).ok_or_else(|| {
                Refusal::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid priority `{s}`"),
                    "Use urgent, high, normal, or low.",
                )
            })
        })
        .transpose()?;
    let work_type = opt_str(i, "work_type")
        .map(|s| {
            WorkType::parse(&s).ok_or_else(|| {
                Refusal::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid work_type `{s}`"),
                    "Use build or run.",
                )
            })
        })
        .transpose()?;
    let params = TasksCreateParams {
        command_id: cmd_id(request)?,
        task_id: entity_id(request, "task-create")?,
        team_id: opt_str(i, "team_id"),
        project_id: opt_str(i, "project_id"),
        assigned_team_id: opt_str(i, "assigned_team_id"),
        assignee_id: opt_str(i, "assignee_id"),
        task_type: opt_str(i, "type"),
        work_type,
        title: req_str(i, "title")?.to_owned(),
        body_markdown: opt_str(i, "body").unwrap_or_default(),
        priority,
        effort: opt_u64(i, "effort")?,
        severity: opt_str(i, "severity"),
        rank_order: opt_u64(i, "order")?,
        labels: opt_str_list(i, "labels").unwrap_or_default(),
        reference_files: opt_str_list(i, "reference_files").unwrap_or_default(),
        modified_files: opt_str_list(i, "modified_files").unwrap_or_default(),
        depends_on: opt_str_list(i, "depends_on").unwrap_or_default(),
        details: None,
    };
    let raw = transport.call_method(TasksCreate::METHOD, json!(params))?;
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(decode_err)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

fn read_task_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksInspectParams {
        task: req_str(&request.input, "task")?.to_owned(),
    };
    let raw = transport.call_method(TasksInspect::METHOD, json!(params))?;
    let res: TaskInspectResult = serde_json::from_value(raw).map_err(decode_err)?;
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

fn write_task_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let priority = opt_str(i, "priority")
        .map(|s| {
            TaskPriority::parse(&s).ok_or_else(|| {
                Refusal::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid priority `{s}`"),
                    "Use urgent, high, normal, or low.",
                )
            })
        })
        .transpose()?;
    let rank_order = if opt_bool(i, "clear_order") {
        PatchField::Set(None)
    } else if let Some(n) = opt_u64(i, "order")? {
        PatchField::Set(Some(n))
    } else {
        PatchField::Unchanged
    };
    let effort = match opt_u64(i, "effort")? {
        Some(n) => PatchField::Set(Some(n)),
        None => PatchField::Unchanged,
    };
    let severity = match opt_str(i, "severity") {
        Some(s) => PatchField::Set(Some(s)),
        None => PatchField::Unchanged,
    };
    let params = TasksUpdateParams {
        command_id: cmd_id(request)?,
        task: req_str(i, "task")?.to_owned(),
        expected_revision: require_revision(request)?,
        title: opt_str(i, "title"),
        body_markdown: opt_str(i, "body"),
        task_type: opt_str(i, "type"),
        work_type: PatchField::Unchanged,
        priority,
        effort,
        severity,
        rank_order,
        labels: opt_str_list(i, "labels"),
        add_labels: opt_str_list(i, "add_labels").unwrap_or_default(),
        remove_labels: opt_str_list(i, "remove_labels").unwrap_or_default(),
        reference_files: opt_str_list(i, "reference_files"),
        add_reference_files: opt_str_list(i, "add_reference_files").unwrap_or_default(),
        remove_reference_files: opt_str_list(i, "remove_reference_files").unwrap_or_default(),
        modified_files: opt_str_list(i, "modified_files"),
        add_modified_files: opt_str_list(i, "add_modified_files").unwrap_or_default(),
        remove_modified_files: opt_str_list(i, "remove_modified_files").unwrap_or_default(),
        details: None,
    };
    let raw = transport.call_method(TasksUpdate::METHOD, json!(params))?;
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(decode_err)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

fn write_task_transition<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let status_str = req_str(i, "status")?;
    let status = TaskStatus::parse(status_str).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("invalid status `{status_str}`"),
            "Use todo, in_progress, blocked, cancelled, or done.",
        )
    })?;
    let params = TasksTransitionParams {
        command_id: cmd_id(request)?,
        task: req_str(i, "task")?.to_owned(),
        expected_revision: require_revision(request)?,
        status,
        blocked_reason: opt_str(i, "blocked_reason"),
        resolution: opt_str(i, "resolution"),
        note: opt_str(i, "note"),
    };
    let raw = transport.call_method(TasksTransition::METHOD, json!(params))?;
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(decode_err)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

fn write_task_assign<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let assigned_team_id = if opt_bool(i, "clear_assigned_team") {
        PatchField::Set(None)
    } else if let Some(tid) = opt_str(i, "assigned_team_id") {
        PatchField::Set(Some(tid))
    } else {
        PatchField::Unchanged
    };
    let assignee_id = if opt_bool(i, "clear_assignee") {
        PatchField::Set(None)
    } else if let Some(eid) = opt_str(i, "assignee_id") {
        PatchField::Set(Some(eid))
    } else {
        PatchField::Unchanged
    };
    let params = TasksAssignParams {
        command_id: cmd_id(request)?,
        task: req_str(i, "task")?.to_owned(),
        expected_revision: require_revision(request)?,
        assigned_team_id,
        assignee_id,
    };
    let raw = transport.call_method(TasksAssign::METHOD, json!(params))?;
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(decode_err)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

fn write_task_comment<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksCommentParams {
        command_id: cmd_id(request)?,
        task: req_str(&request.input, "task")?.to_owned(),
        comment_id: entity_id(request, "task-comment")?,
        body_markdown: req_str(&request.input, "body")?.to_owned(),
    };
    let raw = transport.call_method(TasksComment::METHOD, json!(params))?;
    let res: TasksCommentResult = serde_json::from_value(raw).map_err(decode_err)?;
    Ok(write_answer(
        request,
        json!({
            "task_id": res.task.id,
            "key": res.task.key,
            "revision": res.task.revision,
            "comment": res.comment,
        }),
        Some(res.task.revision),
    ))
}

fn write_task_watch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    watch: bool,
) -> Result<Answer, Refusal> {
    let raw = if watch {
        let params = TasksWatchParams {
            command_id: cmd_id(request)?,
            task: req_str(&request.input, "task")?.to_owned(),
        };
        transport.call_method(TasksWatch::METHOD, json!(params))?
    } else {
        let params = TasksUnwatchParams {
            command_id: cmd_id(request)?,
            task: req_str(&request.input, "task")?.to_owned(),
        };
        transport.call_method(TasksUnwatch::METHOD, json!(params))?
    };
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(decode_err)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

fn read_task_history<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let params = TasksHistoryParams {
        task: req_str(&request.input, "task")?.to_owned(),
    };
    let raw = transport.call_method(TasksHistory::METHOD, json!(params))?;
    let res: TasksHistoryResult = serde_json::from_value(raw).map_err(decode_err)?;
    let items: Vec<Value> = res.events.into_iter().map(|e| json!(e)).collect();
    paginate(request, items)
}

fn read_task_deps<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksDependenciesParams {
        task: req_str(&request.input, "task")?.to_owned(),
    };
    let raw = transport.call_method(TasksDependencies::METHOD, json!(params))?;
    let res: TaskDependenciesResult = serde_json::from_value(raw).map_err(decode_err)?;
    Ok(Answer::completed(json!(res)))
}

fn write_task_dep<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    add: bool,
) -> Result<Answer, Refusal> {
    let raw = if add {
        let params = TasksAddDependencyParams {
            command_id: cmd_id(request)?,
            task: req_str(&request.input, "task")?.to_owned(),
            depends_on_task: req_str(&request.input, "depends_on")?.to_owned(),
        };
        transport.call_method(TasksAddDependency::METHOD, json!(params))?
    } else {
        let params = TasksRemoveDependencyParams {
            command_id: cmd_id(request)?,
            task: req_str(&request.input, "task")?.to_owned(),
            depends_on_task: req_str(&request.input, "depends_on")?.to_owned(),
        };
        transport.call_method(TasksRemoveDependency::METHOD, json!(params))?
    };
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(decode_err)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

fn read_task_labels<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let params = TasksLabelsParams {
        query: opt_str(&request.input, "query"),
    };
    let raw = transport.call_method(TasksLabels::METHOD, json!(params))?;
    let res: TasksLabelsResult = serde_json::from_value(raw).map_err(decode_err)?;
    let items: Vec<Value> = res.labels.into_iter().map(|l| json!(l)).collect();
    paginate(request, items)
}

#[cfg(test)]
mod tests;
