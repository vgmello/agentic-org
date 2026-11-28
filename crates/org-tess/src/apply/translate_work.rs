//! Direct RPC translators for task, git repository/worktree, and process apply commands.

use org_proto::git::{
    RepoAttach, RepoAttachParams, RepoFetch, RepoFetchParams, RepoRegister, RepoRegisterParams,
    RepositoryKindWire, WorktreePrepare, WorktreePrepareParams,
};
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, ProcessScopeWire, ProcessWrite, ProcessWriteParams,
};
use org_proto::tasks::{
    PatchField, TaskPriority, TaskStatus, TaskView, TasksAddDependency, TasksAddDependencyParams,
    TasksAssign, TasksAssignParams, TasksComment, TasksCommentParams, TasksCommentResult,
    TasksCreate, TasksCreateParams, TasksMutationResult, TasksRemoveDependency,
    TasksRemoveDependencyParams, TasksTransition, TasksTransitionParams, TasksUnwatch,
    TasksUnwatchParams, TasksUpdate, TasksUpdateParams, TasksWatch, TasksWatchParams, WorkType,
};
use org_proto::{ApiError, Call};
use serde_json::{json, Map, Value};

use super::{
    invalid, is_set, opt_str_list, opt_string, opt_u64, require_rev, require_str, unreadable,
    OutputShape, PreparedRpc,
};

pub(super) fn try_prepare(
    next_id: &mut impl FnMut() -> String,
    resource: &str,
    verb: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Option<Result<PreparedRpc, ApiError>> {
    match (resource, verb) {
        ("task", "create") => Some(prep_task_create(next_id, input)),
        ("task", "update") => Some(prep_task_update(next_id, input, expected_revision)),
        ("task", "transition") => Some(prep_task_transition(next_id, input, expected_revision)),
        ("task", "assign") => Some(prep_task_assign(next_id, input, expected_revision)),
        ("task", "comment") => Some(prep_task_comment(next_id, input)),
        ("task", "watch") => Some(prep_task_watch(next_id, input, true)),
        ("task", "unwatch") => Some(prep_task_watch(next_id, input, false)),
        ("task", "dep-add") => Some(prep_task_dep(next_id, input, true)),
        ("task", "dep-remove") => Some(prep_task_dep(next_id, input, false)),
        ("repo", "register") => Some(prep_repo_register(next_id, input)),
        ("repo", "attach") => Some(prep_repo_attach(next_id, input)),
        ("repo", "fetch") => Some(prep_repo_fetch(input)),
        ("worktree", "prepare") => Some(prep_worktree_prepare(next_id, input)),
        ("process", "create") => Some(prep_process_write(next_id, input, expected_revision, false)),
        ("process", "update") => Some(prep_process_write(next_id, input, expected_revision, true)),
        _ => None,
    }
}

pub(super) fn shape_task_row(method: &str, raw: Value) -> Result<Value, ApiError> {
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(|_| unreadable(method))?;
    Ok(task_to_row(&res.task))
}

pub(super) fn shape_task_comment(method: &str, raw: Value) -> Result<Value, ApiError> {
    let res: TasksCommentResult = serde_json::from_value(raw).map_err(|_| unreadable(method))?;
    Ok(json!({
        "task_id": res.task.id,
        "key": res.task.key,
        "revision": res.task.revision,
        "comment": res.comment,
    }))
}

fn task_to_row(t: &TaskView) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(t.id));
    m.insert("key".into(), json!(t.key));
    m.insert("type".into(), json!(t.task_type));
    if let Some(v) = t.work_type {
        m.insert("work_type".into(), json!(v));
    }
    m.insert("title".into(), json!(t.title));
    m.insert("status".into(), json!(t.status));
    m.insert("priority".into(), json!(t.priority));
    if let Some(v) = t.effort {
        m.insert("effort".into(), json!(v));
    }
    if let Some(ref v) = t.severity {
        m.insert("severity".into(), json!(v));
    }
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

fn parse_priority(raw: &str) -> Result<TaskPriority, ApiError> {
    TaskPriority::parse(raw).ok_or_else(|| invalid(format!("invalid priority `{raw}`")))
}

fn parse_work_type(raw: &str) -> Result<WorkType, ApiError> {
    WorkType::parse(raw).ok_or_else(|| invalid(format!("invalid work_type `{raw}`")))
}

fn reject_pair(both: bool, a: &str, b: &str) -> Result<(), ApiError> {
    if both {
        return Err(invalid(format!("cannot pass both `{a}` and `{b}`")));
    }
    Ok(())
}

fn patch_u64(
    input: &Map<String, Value>,
    set_key: &str,
    clear_key: &str,
) -> Result<PatchField<u64>, ApiError> {
    let val = opt_u64(input, set_key)?;
    let clear = is_set(input, clear_key);
    reject_pair(val.is_some() && clear, set_key, clear_key)?;
    Ok(if clear {
        PatchField::Set(None)
    } else if let Some(n) = val {
        PatchField::Set(Some(n))
    } else {
        PatchField::Unchanged
    })
}

fn patch_opt_str(
    input: &Map<String, Value>,
    set_key: &str,
    clear_key: &str,
) -> Result<PatchField<String>, ApiError> {
    let val = opt_string(input, set_key);
    let clear = is_set(input, clear_key);
    reject_pair(val.is_some() && clear, set_key, clear_key)?;
    Ok(if clear {
        PatchField::Set(None)
    } else if let Some(s) = val {
        PatchField::Set(Some(s))
    } else {
        PatchField::Unchanged
    })
}

fn prep_task_create(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let priority = opt_string(input, "priority")
        .map(|s| parse_priority(&s))
        .transpose()?;
    let work_type = opt_string(input, "work_type")
        .map(|s| parse_work_type(&s))
        .transpose()?;
    let params = TasksCreateParams {
        command_id: next_id(),
        task_id: next_id(),
        team_id: opt_string(input, "team_id"),
        project_id: opt_string(input, "project_id"),
        assigned_team_id: opt_string(input, "assigned_team_id"),
        assignee_id: opt_string(input, "assignee_id"),
        task_type: opt_string(input, "type"),
        work_type,
        title: require_str(input, "title")?.to_owned(),
        body_markdown: opt_string(input, "body").unwrap_or_default(),
        priority,
        effort: opt_u64(input, "effort")?,
        severity: opt_string(input, "severity"),
        rank_order: opt_u64(input, "order")?,
        labels: opt_str_list(input, "labels").unwrap_or_default(),
        reference_files: opt_str_list(input, "reference_files").unwrap_or_default(),
        modified_files: opt_str_list(input, "modified_files").unwrap_or_default(),
        depends_on: opt_str_list(input, "depends_on").unwrap_or_default(),
        attachment_ids: opt_str_list(input, "attachment_id").unwrap_or_default(),
        details: None,
        dod: None,
    };
    Ok(PreparedRpc {
        method: TasksCreate::METHOD,
        params: json!(params),
        shape: OutputShape::TaskRow,
    })
}

fn prep_task_update(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let body_markdown = opt_string(input, "body");
    let append_body = opt_string(input, "append_body");
    reject_pair(
        body_markdown.is_some() && append_body.is_some(),
        "body",
        "append_body",
    )?;
    let priority = opt_string(input, "priority")
        .map(|s| parse_priority(&s))
        .transpose()?;
    let wt_val = opt_string(input, "work_type")
        .map(|s| parse_work_type(&s))
        .transpose()?;
    let clear_wt = is_set(input, "clear_work_type");
    reject_pair(wt_val.is_some() && clear_wt, "work_type", "clear_work_type")?;
    let work_type = if clear_wt {
        PatchField::Set(None)
    } else if let Some(wt) = wt_val {
        PatchField::Set(Some(wt))
    } else {
        PatchField::Unchanged
    };
    let params = TasksUpdateParams {
        command_id: next_id(),
        task: require_str(input, "task")?.to_owned(),
        expected_revision: require_rev(expected_revision)?,
        title: opt_string(input, "title"),
        body_markdown,
        append_body,
        task_type: opt_string(input, "type"),
        work_type,
        priority,
        effort: patch_u64(input, "effort", "clear_effort")?,
        severity: patch_opt_str(input, "severity", "clear_severity")?,
        rank_order: patch_u64(input, "order", "clear_order")?,
        labels: opt_str_list(input, "labels"),
        add_labels: opt_str_list(input, "add_labels").unwrap_or_default(),
        remove_labels: opt_str_list(input, "remove_labels").unwrap_or_default(),
        reference_files: opt_str_list(input, "reference_files"),
        add_reference_files: opt_str_list(input, "add_reference_files").unwrap_or_default(),
        remove_reference_files: opt_str_list(input, "remove_reference_files").unwrap_or_default(),
        modified_files: opt_str_list(input, "modified_files"),
        add_modified_files: opt_str_list(input, "add_modified_files").unwrap_or_default(),
        remove_modified_files: opt_str_list(input, "remove_modified_files").unwrap_or_default(),
        add_attachments: opt_str_list(input, "add_attachment").unwrap_or_default(),
        remove_attachments: opt_str_list(input, "remove_attachment").unwrap_or_default(),
        feature_id: PatchField::Unchanged,
        milestone_id: PatchField::Unchanged,
        details: None,
    };
    Ok(PreparedRpc {
        method: TasksUpdate::METHOD,
        params: json!(params),
        shape: OutputShape::TaskRow,
    })
}

fn parse_waiting_on(input: &Map<String, Value>) -> Result<Option<bool>, ApiError> {
    let waiting_on_enum = opt_string(input, "waiting_on");
    let waiting_on_bool = input.get("waiting_on_owner").and_then(Value::as_bool);
    reject_pair(
        waiting_on_enum.is_some() && waiting_on_bool.is_some(),
        "waiting_on",
        "waiting_on_owner",
    )?;
    match waiting_on_enum.as_deref() {
        Some("owner") => Ok(Some(true)),
        Some("none") => Ok(Some(false)),
        Some(other) => Err(invalid(format!("invalid waiting_on `{other}`"))),
        None => Ok(waiting_on_bool),
    }
}

struct TransitionText {
    reason: Option<String>,
    blocked_reason: Option<String>,
    note: Option<String>,
}

fn resolve_transition_fields(
    status: TaskStatus,
    input: &Map<String, Value>,
    waiting_on_owner: Option<bool>,
) -> Result<TransitionText, ApiError> {
    let reason = opt_string(input, "reason");
    let blocked_reason = opt_string(input, "blocked_reason");
    let note = opt_string(input, "note");
    if status == TaskStatus::Blocked {
        reject_pair(
            reason.is_some() && blocked_reason.is_some(),
            "reason",
            "blocked_reason",
        )?;
        let (effective_blocked, extra_note) = match (blocked_reason, reason, note) {
            (Some(b), None, n) => (Some(b), n),
            (None, Some(r), n) => (Some(r), n),
            // Align `--note` alias on `blocked` transitions with CLI and office.
            (None, None, Some(n)) => (Some(n), None),
            (None, None, None) => (None, None),
            (Some(_), Some(_), _) => unreachable!("rejected above"),
        };
        if effective_blocked
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
        {
            return Err(invalid(
                "transitioning to `blocked` requires a non-empty `blocked_reason`",
            ));
        }
        return Ok(TransitionText {
            reason: None,
            blocked_reason: effective_blocked,
            note: extra_note,
        });
    }
    if blocked_reason.is_some() {
        return Err(invalid(format!(
            "`blocked_reason` applies only when transitioning to `blocked`, not `{}`",
            status.as_str()
        )));
    }
    if waiting_on_owner.is_some() {
        return Err(invalid(
            "`waiting_on_owner` applies only when transitioning to `blocked`",
        ));
    }
    reject_pair(reason.is_some() && note.is_some(), "reason", "note")?;
    if reason.as_deref().is_some_and(|s| s.trim().is_empty()) {
        return Err(invalid("`reason` cannot be empty"));
    }
    Ok(TransitionText {
        reason,
        blocked_reason: None,
        note,
    })
}

fn prep_task_transition(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let status_str = require_str(input, "status")?;
    let status = TaskStatus::parse(status_str)
        .ok_or_else(|| invalid(format!("invalid status `{status_str}`")))?;
    let waiting_on_owner = parse_waiting_on(input)?;
    let text = resolve_transition_fields(status, input, waiting_on_owner)?;
    let params = TasksTransitionParams {
        command_id: next_id(),
        task: require_str(input, "task")?.to_owned(),
        expected_revision: require_rev(expected_revision)?,
        status,
        reason: text.reason,
        blocked_reason: text.blocked_reason,
        waiting_on_owner,
        resolution: opt_string(input, "resolution"),
        note: text.note,
        done_anyway_reason: None,
        final_summary: None,
    };
    Ok(PreparedRpc {
        method: TasksTransition::METHOD,
        params: json!(params),
        shape: OutputShape::TaskRow,
    })
}

fn prep_task_assign(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let assigned_team_id = if is_set(input, "clear_assigned_team") {
        PatchField::Set(None)
    } else if let Some(tid) = opt_string(input, "assigned_team_id") {
        PatchField::Set(Some(tid))
    } else {
        PatchField::Unchanged
    };
    let assignee_id = if is_set(input, "clear_assignee") {
        PatchField::Set(None)
    } else if let Some(eid) = opt_string(input, "assignee_id") {
        PatchField::Set(Some(eid))
    } else {
        PatchField::Unchanged
    };
    let params = TasksAssignParams {
        command_id: next_id(),
        task: require_str(input, "task")?.to_owned(),
        expected_revision: require_rev(expected_revision)?,
        assigned_team_id,
        assignee_id,
    };
    Ok(PreparedRpc {
        method: TasksAssign::METHOD,
        params: json!(params),
        shape: OutputShape::TaskRow,
    })
}

fn prep_task_comment(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let body_markdown = opt_string(input, "body").unwrap_or_default();
    let attachment_ids = opt_str_list(input, "attachment_id").unwrap_or_default();
    if body_markdown.trim().is_empty() && attachment_ids.is_empty() {
        return Err(invalid("`task comment` requires `body` or `attachment_id`"));
    }
    let params = TasksCommentParams {
        command_id: next_id(),
        task: require_str(input, "task")?.to_owned(),
        comment_id: next_id(),
        body_markdown,
        attachment_ids,
    };
    Ok(PreparedRpc {
        method: TasksComment::METHOD,
        params: json!(params),
        shape: OutputShape::TaskComment,
    })
}

fn prep_task_watch(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    watch: bool,
) -> Result<PreparedRpc, ApiError> {
    let task = require_str(input, "task")?.to_owned();
    let (method, params) = if watch {
        (
            TasksWatch::METHOD,
            json!(TasksWatchParams {
                command_id: next_id(),
                task,
            }),
        )
    } else {
        (
            TasksUnwatch::METHOD,
            json!(TasksUnwatchParams {
                command_id: next_id(),
                task,
            }),
        )
    };
    Ok(PreparedRpc {
        method,
        params,
        shape: OutputShape::TaskRow,
    })
}

fn prep_task_dep(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    add: bool,
) -> Result<PreparedRpc, ApiError> {
    let task = require_str(input, "task")?.to_owned();
    let depends_on_task = require_str(input, "depends_on")?.to_owned();
    let (method, params) = if add {
        (
            TasksAddDependency::METHOD,
            json!(TasksAddDependencyParams {
                command_id: next_id(),
                task,
                depends_on_task,
            }),
        )
    } else {
        (
            TasksRemoveDependency::METHOD,
            json!(TasksRemoveDependencyParams {
                command_id: next_id(),
                task,
                depends_on_task,
            }),
        )
    };
    Ok(PreparedRpc {
        method,
        params,
        shape: OutputShape::TaskRow,
    })
}

fn prep_repo_register(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let kind_raw = require_str(input, "kind")?;
    let kind = RepositoryKindWire::parse(kind_raw)
        .ok_or_else(|| invalid("kind must be `remote` or `local`"))?;
    let repository_id = opt_string(input, "repository_id").unwrap_or_else(&mut *next_id);
    let params = RepoRegisterParams {
        command_id: next_id(),
        repository_id,
        key: opt_string(input, "key"),
        kind,
        remote_url: opt_string(input, "remote_url"),
        origin_path: opt_string(input, "origin_path"),
    };
    Ok(PreparedRpc {
        method: RepoRegister::METHOD,
        params: json!(params),
        shape: OutputShape::Passthrough,
    })
}

fn prep_repo_attach(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let params = RepoAttachParams {
        command_id: next_id(),
        project: require_str(input, "project")?.to_owned(),
        repository: require_str(input, "repo")?.to_owned(),
        integration_branch: require_str(input, "branch")?.to_owned(),
    };
    Ok(PreparedRpc {
        method: RepoAttach::METHOD,
        params: json!(params),
        shape: OutputShape::Passthrough,
    })
}

fn prep_repo_fetch(input: &Map<String, Value>) -> Result<PreparedRpc, ApiError> {
    let params = RepoFetchParams {
        repository: require_str(input, "repo")?.to_owned(),
    };
    Ok(PreparedRpc {
        method: RepoFetch::METHOD,
        params: json!(params),
        shape: OutputShape::Passthrough,
    })
}

fn prep_worktree_prepare(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let params = WorktreePrepareParams {
        command_id: next_id(),
        task: require_str(input, "task")?.to_owned(),
        repository: require_str(input, "repo")?.to_owned(),
        base_branch: opt_string(input, "base_branch"),
    };
    Ok(PreparedRpc {
        method: WorktreePrepare::METHOD,
        params: json!(params),
        shape: OutputShape::Passthrough,
    })
}

fn prep_process_write(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
    is_update: bool,
) -> Result<PreparedRpc, ApiError> {
    let document_id = if is_update {
        Some(require_str(input, "id")?.to_owned())
    } else {
        None
    };
    let kind = match require_str(input, "kind")? {
        "process" => DocumentKindWire::Process,
        "standard" => DocumentKindWire::Standard,
        other => return Err(invalid(format!("invalid kind `{other}`"))),
    };
    let status = match opt_string(input, "status").as_deref() {
        None | Some("current") => DocumentStatusWire::Current,
        Some("draft") => DocumentStatusWire::Draft,
        Some("deprecated") => DocumentStatusWire::Deprecated,
        Some(other) => return Err(invalid(format!("invalid status `{other}`"))),
    };
    let scope = match (opt_string(input, "project"), opt_string(input, "team")) {
        (Some(_), Some(_)) => return Err(invalid("provide at most one of `project` or `team`")),
        (Some(project_id), None) => ProcessScopeWire::Project { project_id },
        (None, Some(team_id)) => ProcessScopeWire::Team { team_id },
        (None, None) => ProcessScopeWire::Organization,
    };
    let params = ProcessWriteParams {
        command_id: next_id(),
        document_id,
        kind,
        scope,
        key: opt_string(input, "key"),
        title: require_str(input, "title")?.to_owned(),
        status,
        owner_employee_id: opt_string(input, "owner"),
        overridable: !is_set(input, "non_overridable"),
        body_markdown: require_str(input, "body")?.to_owned(),
        expected_revision,
    };
    Ok(PreparedRpc {
        method: ProcessWrite::METHOD,
        params: json!(params),
        shape: OutputShape::Passthrough,
    })
}
