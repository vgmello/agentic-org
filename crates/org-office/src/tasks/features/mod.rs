//! Engine-independent domain features and read queries for unified typed tasks (`TASK-49`, `TASK-77`).

pub mod attachments;
pub mod checklist;
pub mod conflict;
pub mod core;
pub mod create;
pub mod list;
pub mod ops;
pub mod queries;
pub mod summary;
pub mod transition;

use std::collections::{BTreeSet, VecDeque};

use org_proto::attachments::AttachmentMeta;
use org_proto::tasks::{
    ChecklistSection, ChecklistStepWire, TaskCommentWire, TaskDetailsWire, TaskEventWire,
    TaskRefSummary, TaskView, TaskWatcherWire, TASK_BODY_MAX_CHARS, TASK_TITLE_MAX_CHARS,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Refusal};
use serde_json::json;

use crate::error::{api_refusal, decode_api_refusal, parse_uuid_v7_refusal};
use crate::organization::authority;
use crate::organization::records::{Employee, EmployeeStatus, TeamStatus};
pub(crate) use crate::tasks::ops::{
    require_local_task_write_authority, require_task, rewrite_task, storage_refusal,
    validate_i64_u64, TaskWriteAuthority,
};
use crate::tasks::ports::TasksRead;
use crate::tasks::query::TaskRollup;
use crate::tasks::records::{
    ChecklistStepRecord, FinalSummaryRecord, TaskCommentRecord, TaskDetailsRecord, TaskEventRecord,
    TaskRecord, TaskWatcherRecord,
};

pub use self::attachments::{
    AttachToTask, PreparedTaskFile, PreparedTaskFiles, UpdateTaskAttachmentMeta,
};
pub use self::checklist::{
    ChecklistAdd, ChecklistCheck, ChecklistEdit, ChecklistMove, ChecklistRemove,
};
pub use self::core::{AssignTask, UpdateTask};
pub use self::create::{CreateTask, CreatedTask, TaskNumbering, TASK_NUMBER_BLOCK};
pub use self::list::list_tasks;
pub use self::ops::{AddDependency, CommentTask, RemoveDependency, UnwatchTask, WatchTask};
pub use self::queries::{
    inspect_task, inspect_task_with_fields, list_comments, list_history, list_labels,
    task_dependencies,
};
pub use self::summary::SetFinalSummary;
pub use self::transition::TransitionTask;

/// Result metadata returned to `org-office-server` after a task mutation commits.
#[derive(Debug, Clone)]
pub struct TaskWriteOutcome {
    pub task: TaskView,
    /// Set when the mutation assigned or reassigned the task to an active agent (`AVAIL-2`).
    pub newly_assigned_agent: Option<Id>,
    /// True when the answer was served from an idempotency receipt rather than a fresh commit.
    pub replayed: bool,
}

pub(crate) fn refuse(code: ErrorCode, message: impl Into<String>) -> Refusal {
    api_refusal(code, message)
}

pub fn decode_refusal(refusal: &Refusal) -> ApiError {
    decode_api_refusal(refusal, "task operation refused")
}

pub(crate) fn parse_uuid_v7(raw: &str, field: &str) -> Result<Id, Refusal> {
    parse_uuid_v7_refusal(raw, field)
}

/// Enforces `ADR-0031` (`TASKAUTH-1..3`): any active employee (person or agent) may mutate tasks;
/// unknown, inactive, or paused employees are refused with `NotAuthorized`.
pub(crate) fn require_active_employee<R: TasksRead>(
    view: &R,
    member_id: &str,
) -> Result<Employee, Refusal> {
    authority::require_active_employee(
        view,
        member_id,
        "only active employees may change tasks (ADR-0031)",
    )
}

/// Resolves the active `office_code` for `office_id` so final `T-<CODE><N>` keys mint locally
/// at commit even while HQ is unreachable (`ADR-0019`).
pub(crate) fn require_office_code<R: TasksRead>(
    view: &R,
    office_id: &Id,
) -> Result<String, Refusal> {
    let code = view
        .office_code(office_id)
        .map_err(storage_refusal)?
        .ok_or_else(|| {
            refuse(
                ErrorCode::Unavailable,
                "this office does not have an active office code for task numbering",
            )
        })?;
    Ok(code.code)
}

pub(crate) fn validate_active_team<R: TasksRead>(
    view: &R,
    team_id: &str,
    field: &str,
) -> Result<Id, Refusal> {
    let id = parse_uuid_v7(team_id, field)?;
    let team = view
        .team(&id)
        .map_err(storage_refusal)?
        .ok_or_else(|| refuse(ErrorCode::NotFound, format!("team `{team_id}` not found")))?;
    if team.status != TeamStatus::Active {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            format!("team `{team_id}` is archived"),
        ));
    }
    Ok(id)
}

pub(crate) fn validate_active_employee<R: TasksRead>(
    view: &R,
    emp_id: &str,
    field: &str,
) -> Result<Employee, Refusal> {
    let id = parse_uuid_v7(emp_id, field)?;
    let emp = view
        .employee(&id)
        .map_err(storage_refusal)?
        .ok_or_else(|| {
            refuse(
                ErrorCode::NotFound,
                format!("employee `{emp_id}` not found"),
            )
        })?;
    if emp.status != EmployeeStatus::Active {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            format!("employee `{emp_id}` is not active"),
        ));
    }
    Ok(emp)
}

pub(crate) fn validate_title(raw: &str) -> Result<String, Refusal> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(refuse(ErrorCode::InvalidRequest, "title cannot be empty"));
    }
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "title must be a single line",
        ));
    }
    if trimmed.chars().count() > TASK_TITLE_MAX_CHARS {
        return Err(refuse(
            ErrorCode::TooLarge,
            format!("title exceeds {TASK_TITLE_MAX_CHARS} characters"),
        ));
    }
    Ok(trimmed.to_owned())
}

pub(crate) fn validate_body(raw: &str) -> Result<String, Refusal> {
    if raw.chars().count() > TASK_BODY_MAX_CHARS {
        return Err(refuse(
            ErrorCode::TooLarge,
            format!("body_markdown exceeds {TASK_BODY_MAX_CHARS} characters"),
        ));
    }
    Ok(raw.to_owned())
}

pub(crate) fn validate_task_type(raw: &str) -> Result<String, Refusal> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "task type must be 1..=64 characters",
        ));
    }
    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
    {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "task type must use lowercase ASCII letters, digits, `_`, or `-`",
        ));
    }
    Ok(trimmed)
}

pub(crate) fn parse_details<R: TasksRead>(
    view: &R,
    task_id: &Id,
    project_id: Option<&Id>,
    wire: &TaskDetailsWire,
) -> Result<TaskDetailsRecord, Refusal> {
    let opt_id = |raw: &Option<String>, field: &str| -> Result<Option<Id>, Refusal> {
        match raw.as_deref() {
            None => Ok(None),
            Some(v) => Ok(Some(parse_uuid_v7(v, field)?)),
        }
    };
    let duplicate_of = opt_id(&wire.duplicate_of_task_id, "duplicate_of_task_id")?;
    let merged_into = opt_id(&wire.merged_into_task_id, "merged_into_task_id")?;
    if duplicate_of.as_ref() == Some(task_id) || merged_into.as_ref() == Some(task_id) {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "a task cannot reference itself as duplicate or merge target",
        ));
    }
    for linked_task_id in [&duplicate_of, &merged_into].into_iter().flatten() {
        if view
            .task(linked_task_id)
            .map_err(storage_refusal)?
            .is_none()
        {
            return Err(refuse(
                ErrorCode::NotFound,
                format!("referenced task `{linked_task_id}` not found"),
            ));
        }
    }
    let affects_feature_id = match opt_id(&wire.affects_feature_id, "affects_feature_id")? {
        None => None,
        Some(fid) => {
            let Some(pid) = project_id else {
                return Err(refuse(
                    ErrorCode::InvalidRequest,
                    "affects_feature_id requires the task to belong to a project",
                ));
            };
            if !view
                .feature_in_project(pid, &fid)
                .map_err(storage_refusal)?
            {
                return Err(refuse(
                    ErrorCode::NotFound,
                    format!("referenced feature `{fid}` not found in project"),
                ));
            }
            Some(fid)
        }
    };
    validate_i64_u64(wire.reopen_count, "reopen_count")?;
    if let Some(ref src) = wire.routing_source {
        if !matches!(src.as_str(), "person" | "rule" | "copilot") {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "routing_source must be `person`, `rule`, or `copilot`",
            ));
        }
    }
    let routed_by = opt_id(&wire.routed_by_employee_id, "routed_by_employee_id")?;
    let triager = opt_id(&wire.suggested_triager_id, "suggested_triager_id")?;
    let verifier = opt_id(&wire.suggested_verifier_id, "suggested_verifier_id")?;
    for emp_id in [&routed_by, &triager, &verifier].into_iter().flatten() {
        if view.employee(emp_id).map_err(storage_refusal)?.is_none() {
            return Err(refuse(
                ErrorCode::NotFound,
                format!("referenced employee `{emp_id}` not found"),
            ));
        }
    }
    // `done_anyway` is the office's to set (`TASK-88`): a client's is ignored.
    Ok(TaskDetailsRecord {
        what_happened: wire.what_happened.clone(),
        expected: wire.expected.clone(),
        steps: wire.steps.clone(),
        environment: wire.environment.clone(),
        severity_confirmed: wire.severity_confirmed,
        reopen_count: wire.reopen_count,
        duplicate_of_task_id: duplicate_of,
        closed_reason: wire.closed_reason.clone(),
        affects_feature_id,
        request_kind: wire.request_kind.clone(),
        request_text: wire.request_text.clone(),
        route_reason: wire.route_reason.clone(),
        routing_source: wire.routing_source.clone(),
        routed_by_employee_id: routed_by,
        merged_into_task_id: merged_into,
        suggested_triager_id: triager,
        suggested_verifier_id: verifier,
        done_anyway: None,
    })
}

/// Verifies that adding edge `source_id -> target_id` (`source` depends on `target`) introduces
/// neither a self-loop nor a transitive dependency cycle (`TASK-77`).
pub(crate) fn check_dependency_acyclic<R: TasksRead>(
    view: &R,
    source_id: &Id,
    target_id: &Id,
) -> Result<(), Refusal> {
    if source_id == target_id {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "a task cannot depend on itself",
        ));
    }
    // BFS upstream from `target_id`: if `source_id` is reachable, adding `source -> target` cycles.
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    visited.insert(target_id.clone());
    queue.push_back(target_id.clone());
    while let Some(curr) = queue.pop_front() {
        let links = view.task_links_from(&curr).map_err(storage_refusal)?;
        for link in links {
            if link.kind != "depends_on" {
                continue;
            }
            if &link.target_id == source_id {
                return Err(refuse(
                    ErrorCode::Conflict,
                    format!(
                        "dependency cycle detected: `{target_id}` already depends on `{source_id}`"
                    ),
                ));
            }
            if visited.insert(link.target_id.clone()) {
                queue.push_back(link.target_id);
            }
        }
    }
    Ok(())
}

pub(crate) fn build_ref_summary(task: &TaskRecord) -> TaskRefSummary {
    TaskRefSummary {
        id: task.id.as_str().to_owned(),
        key: task.key(),
        title: task.title.clone(),
        task_type: task.task_type.clone(),
        status: task.status,
        priority: task.priority,
        rank_order: task.rank_order,
    }
}

/// The view of one task, its rollup read on its own.
pub(crate) fn build_task_view<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
) -> Result<TaskView, Refusal> {
    let rollup = view
        .task_rollups(std::slice::from_ref(&task.id))
        .map_err(storage_refusal)?
        .remove(&task.id)
        .unwrap_or_default();
    Ok(task_view(task, rollup))
}

/// The wire view of `task`, with its list rollup.
pub(crate) fn task_view(task: &TaskRecord, rollup: TaskRollup) -> TaskView {
    let ids = |ids: Vec<Id>| -> Vec<String> {
        let mut out: Vec<String> = ids.into_iter().map(|id| id.as_str().to_owned()).collect();
        out.sort();
        out.dedup();
        out
    };
    let ac = rollup.section(ChecklistSection::Acceptance);
    let dod = rollup.section(ChecklistSection::Dod);
    let work = rollup.section(ChecklistSection::Work);
    let depends_on = ids(rollup.depends_on);
    let dependents = ids(rollup.dependents);
    TaskView {
        id: task.id.as_str().to_owned(),
        office_code: task.office_code.clone(),
        number: task.number,
        key: task.key(),
        revision: task.meta.revision,
        team_id: task.team_id.as_str().to_owned(),
        assigned_team_id: task
            .assigned_team_id
            .as_ref()
            .map(|id| id.as_str().to_owned()),
        project_id: task.project_id.as_ref().map(|id| id.as_str().to_owned()),
        task_type: task.task_type.clone(),
        work_type: task.work_type,
        title: task.title.clone(),
        status: task.status,
        assignee_id: task.assignee_id.as_ref().map(|id| id.as_str().to_owned()),
        body_markdown: task.body_markdown.clone(),
        feature_id: task.feature_id.as_ref().map(|id| id.as_str().to_owned()),
        milestone_id: task.milestone_id.as_ref().map(|id| id.as_str().to_owned()),
        priority: task.priority,
        effort: task.effort,
        severity: task.severity.clone(),
        resolution: task.resolution.clone(),
        blocked_reason: task.blocked_reason.clone(),
        rank_order: task.rank_order,
        labels: task.labels.clone(),
        reference_files: task.reference_files.clone(),
        modified_files: task.modified_files.clone(),
        depends_on,
        dependents,
        details: task.details.as_ref().map(TaskDetailsRecord::to_wire),
        final_summary: task.final_summary.as_ref().map(FinalSummaryRecord::to_wire),
        created_at: task.meta.created_at.clone(),
        created_by_kind: task.meta.created_by_kind.clone(),
        created_by_id: task
            .meta
            .created_by_id
            .as_ref()
            .map(|id| id.as_str().to_owned()),
        updated_at: task.meta.updated_at.clone(),
        updated_by_kind: task.meta.updated_by_kind.clone(),
        updated_by_id: task
            .meta
            .updated_by_id
            .as_ref()
            .map(|id| id.as_str().to_owned()),
        checklist_total: rollup.checklist_total,
        checklist_done: rollup.checklist_done,
        ac_total: ac.total,
        ac_done: ac.done,
        dod_total: dod.total,
        dod_done: dod.done,
        work_steps_total: work.total,
        work_steps_done: work.done,
        comment_count: rollup.comment_count,
        watcher_count: rollup.watcher_count,
    }
}

pub(crate) fn step_to_wire(step: &ChecklistStepRecord) -> ChecklistStepWire {
    ChecklistStepWire {
        id: step.id.as_str().to_owned(),
        section: step.section,
        position: step.position,
        text: step.text.clone(),
        required: step.required,
        done: step.done,
        linked_task_id: step
            .linked_task_id
            .as_ref()
            .map(|id| id.as_str().to_owned()),
        checked_by_employee_id: step
            .checked_by_employee_id
            .as_ref()
            .map(|id| id.as_str().to_owned()),
        checked_at: step.checked_at.clone(),
        revision: step.revision,
    }
}

pub(crate) fn comment_to_wire(
    comment: &TaskCommentRecord,
    attachments: Vec<AttachmentMeta>,
) -> TaskCommentWire {
    TaskCommentWire {
        id: comment.id.as_str().to_owned(),
        position: comment.position,
        author_id: comment.author_id.as_str().to_owned(),
        created_at: comment.created_at.clone(),
        body_markdown: comment.body_markdown.clone(),
        attachments,
    }
}

pub(crate) fn watcher_to_wire(w: &TaskWatcherRecord) -> TaskWatcherWire {
    TaskWatcherWire {
        id: w.id.as_str().to_owned(),
        watcher_kind: w.watcher_kind.clone(),
        watcher_id: w.watcher_id.as_ref().map(|id| id.as_str().to_owned()),
    }
}

pub(crate) fn event_to_wire(ev: &TaskEventRecord) -> TaskEventWire {
    let change = serde_json::from_str(&ev.change_json).unwrap_or_else(|_| json!({}));
    TaskEventWire {
        task_id: ev.task_id.as_str().to_owned(),
        revision: ev.revision,
        occurred_at: ev.occurred_at.clone(),
        actor_kind: ev.actor_kind.clone(),
        actor_id: ev.actor_id.as_ref().map(|id| id.as_str().to_owned()),
        operation_id: ev.operation_id.clone(),
        change,
        payload_sha256: ev.payload_sha256.clone(),
    }
}
