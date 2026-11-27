//! Bounded hybrid optimistic-concurrency conflict payloads (`TASK-90`, `TASK-98.4`).
//!
//! SECURITY: Callers MUST verify `require_active_employee` and
//! `require_local_task_write_authority` BEFORE calling [`stale_revision_refusal`] (ADR-0031).

use org_proto::tasks::ChecklistSection;
use org_proto::{ApiError, ConflictDetail, ConflictEvent, ConflictSummary};
use org_store::Refusal;
use serde_json::{json, Value};

use crate::tasks::ports::TasksRead;
use crate::tasks::records::TaskRecord;

/// Builds a structured `Conflict` refusal carrying bounded `events_since` (`<= 20` / `<= 8 KiB`),
/// `changed_sections`, `reread_recommended`, and compact `current_summary` (`TASK-90`).
pub(crate) fn stale_revision_refusal<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
    expected_revision: u64,
) -> Refusal {
    let raw_events: Vec<ConflictEvent> = view
        .task_events(&task.id)
        .unwrap_or_default()
        .into_iter()
        .filter(|ev| ev.revision > expected_revision)
        .map(|ev| ConflictEvent {
            revision: ev.revision,
            occurred_at: ev.occurred_at,
            actor_kind: ev.actor_kind,
            actor_id: ev.actor_id.map(|id| id.as_str().to_owned()),
            change: serde_json::from_str::<Value>(&ev.change_json).unwrap_or_else(|_| json!({})),
        })
        .collect();
    let detail = ConflictDetail::from_events(
        expected_revision,
        task.meta.revision,
        raw_events,
        Some(build_conflict_summary(view, task)),
    );
    let payload = ApiError::conflict(
        format!(
            "stale expected_revision {expected_revision}: task is at revision {}",
            task.meta.revision
        ),
        detail,
    );
    Refusal::rejected(serde_json::to_vec(&payload).expect("ApiError serializes"))
}

fn build_conflict_summary<R: TasksRead>(view: &R, task: &TaskRecord) -> ConflictSummary {
    let rollup = view
        .task_rollups(std::slice::from_ref(&task.id))
        .ok()
        .and_then(|mut map| map.remove(&task.id))
        .unwrap_or_default();
    let ac = rollup.section(ChecklistSection::Acceptance);
    let dod = rollup.section(ChecklistSection::Dod);
    let work = rollup.section(ChecklistSection::Work);
    ConflictSummary {
        key: Some(task.key()),
        title: task.title.clone(),
        status: task.status.as_str().to_owned(),
        priority: task.priority.as_str().to_owned(),
        assignee_id: task.assignee_id.as_ref().map(|id| id.as_str().to_owned()),
        assigned_team_id: task
            .assigned_team_id
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
