//! The work slice: `task` (14 verbs), `checklist` (6) and `note` (6, aliases
//! `task-note` / `task note`) (`TASK-49` `CLI-15`, `TASK-77` B4b, `TASK-90`, `TASK-111`),
//! with handlers and guidance in each file and declarative rows in `org_command_catalogue::work`.
//!
//! Each write declares the class the office answers it in (`doc-79`, `ADR-0014`): a create
//! inside the office's reserved number block, a comment, a watch or unwatch and a checklist
//! check are `committed`; a change that rewrites `task.md` (update, transition, assign, a
//! dependency) or the checklist's steps (add, edit, remove, move) is `durable`.

mod checklist;
mod task_note;
mod task_read;
mod task_write;

use org_command::{Answer, CommandTable, Request};
use org_proto::tasks::TaskView;
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use crate::runtime::domain::Domain;
use crate::runtime::input::command_id;
use crate::runtime::transport::{OfficeTransport, Refusal};

/// The work slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::work::ALL,
    resources: &[
        ("checklist", "listing, adding, checking, editing, removing, or moving steps on a task's checklist."),
        ("note", "recording private working notes, plans and decisions or publishing a copy as a shared comment (aliases: `task-note`, `task note`)."),
        ("task", "creating, filtering, inspecting, updating, transitioning, assigning, commenting on, or linking task dependencies."),
    ],
    cues: &[
        task_write::CUES,
        task_read::CUES,
        checklist::CUES,
        task_note::CUES,
    ],
    dispatch,
};

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match request.command.resource {
        "task" => dispatch_task(transport, request),
        "checklist" => checklist::try_checklist(transport, request),
        "note" => task_note::try_task_note(transport, request),
        _ => None,
    }
}

/// Rewrites legacy `note get ...` (from `task-note get` / `task note get`) to `note inspect ...`
/// after shorthand alias expansion, preserving backward compatibility without extra catalogue bytes.
pub(crate) fn rewrite_note_args(_table: &CommandTable, args: &[String]) -> Option<Vec<String>> {
    crate::runtime::input::rewrite_note_args(args)
}

fn dispatch_task(
    transport: &mut dyn OfficeTransport,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    Some(match request.command.verb {
        "create" => task_write::write_task_create(transport, request),
        "list" => task_read::read_task_list(transport, request),
        "inspect" => task_read::read_task_inspect(transport, request),
        "update" => task_write::write_task_update(transport, request),
        "transition" => task_write::write_task_transition(transport, request),
        "assign" => task_write::write_task_assign(transport, request),
        "comment" => task_write::write_task_comment(transport, request),
        "watch" => task_write::write_task_watch(transport, request, true),
        "unwatch" => task_write::write_task_watch(transport, request, false),
        "history" => task_read::read_task_history(transport, request),
        "deps" => task_read::read_task_deps(transport, request),
        "dep-add" => task_write::write_task_dep(transport, request, true),
        "dep-remove" => task_write::write_task_dep(transport, request, false),
        "labels" => task_read::read_task_labels(transport, request),
        _ => return None,
    })
}

/// The command ID of a task, checklist or note write ([`command_id`] in the `task-command`
/// domain): no dry run, and the same key always gives the same ID.
fn cmd_id(request: &Request) -> Result<String, Refusal> {
    command_id(request, "task-command")
}

/// The refusal of a task answer this build cannot read.
fn unreadable(e: serde_json::Error) -> Refusal {
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

#[cfg(test)]
mod tests;
