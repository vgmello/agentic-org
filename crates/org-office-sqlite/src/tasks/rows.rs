//! Row decoders for tasks and their children.

use org_proto::tasks::{ChecklistSection, TaskPriority, TaskStatus, WorkType};
use rusqlite::Row;

use super::super::organization::{id, meta, number, optional_id};
use super::{bad_col, opt_u64, TASK_META_AT};
use org_office::tasks::records::{
    ChecklistStepRecord, DoneAnywayRecord, FinalSummaryRecord, OfficeTaskCounterRecord,
    TaskCommentRecord, TaskDetailsRecord, TaskEventRecord, TaskLinkRecord, TaskMeta, TaskRecord,
    TaskWatcherRecord,
};

/// Columns 20..=23 of a task row: its `final_summary*`, all null or all set (`TASK-88`).
fn final_summary(row: &Row<'_>) -> rusqlite::Result<Option<FinalSummaryRecord>> {
    let text: Option<String> = row.get(20)?;
    let at: Option<String> = row.get(21)?;
    let by_id = optional_id(row, 23)?;
    match (text, at, by_id) {
        (None, None, None) => Ok(None),
        (Some(text), Some(at), Some(by_id)) => Ok(Some(FinalSummaryRecord { text, at, by_id })),
        _ => Err(bad_col(20, "a final summary without its time or author")),
    }
}

pub(super) fn task_row(row: &Row<'_>) -> rusqlite::Result<TaskRecord> {
    let work_type_raw: Option<String> = row.get(7)?;
    let work_type = work_type_raw
        .as_deref()
        .map(|s| WorkType::parse(s).ok_or_else(|| bad_col(7, "invalid work_type")))
        .transpose()?;
    let status_raw: String = row.get(9)?;
    let status = TaskStatus::parse(&status_raw).ok_or_else(|| bad_col(9, "invalid status"))?;
    let prio_raw: String = row.get(14)?;
    let priority = TaskPriority::parse(&prio_raw).ok_or_else(|| bad_col(14, "invalid priority"))?;
    let waiting_on_owner_int: i64 = row.get(25)?;
    Ok(TaskRecord {
        id: id(row, 0)?,
        office_code: row.get(1)?,
        number: number(row, 2)?,
        team_id: id(row, 3)?,
        assigned_team_id: optional_id(row, 4)?,
        project_id: optional_id(row, 5)?,
        task_type: row.get(6)?,
        work_type,
        title: row.get(8)?,
        status,
        assignee_id: optional_id(row, 10)?,
        body_markdown: row.get(11)?,
        feature_id: optional_id(row, 12)?,
        milestone_id: optional_id(row, 13)?,
        priority,
        effort: opt_u64(row, 15)?,
        severity: row.get(16)?,
        resolution: row.get(17)?,
        blocked_reason: row.get(18)?,
        blocked_since: row.get(24)?,
        waiting_on_owner: waiting_on_owner_int != 0,
        rank_order: opt_u64(row, 19)?,
        labels: Vec::new(),
        reference_files: Vec::new(),
        modified_files: Vec::new(),
        repositories: Vec::new(),
        details: None,
        final_summary: final_summary(row)?,
        meta: TaskMeta::from_org_meta(meta(row, TASK_META_AT)?),
    })
}

pub(super) fn details_row(row: &Row<'_>) -> rusqlite::Result<TaskDetailsRecord> {
    let sev_int: i64 = row.get(4)?;
    let done_anyway: Option<String> = row.get(17)?;
    let done_anyway = done_anyway
        .map(|json| serde_json::from_str::<DoneAnywayRecord>(&json))
        .transpose()
        .map_err(|_| bad_col(17, "invalid done_anyway_json"))?;
    Ok(TaskDetailsRecord {
        what_happened: row.get(0)?,
        expected: row.get(1)?,
        steps: row.get(2)?,
        environment: row.get(3)?,
        severity_confirmed: sev_int != 0,
        reopen_count: number(row, 5)?,
        duplicate_of_task_id: optional_id(row, 6)?,
        closed_reason: row.get(7)?,
        affects_feature_id: optional_id(row, 8)?,
        request_kind: row.get(9)?,
        request_text: row.get(10)?,
        route_reason: row.get(11)?,
        routing_source: row.get(12)?,
        routed_by_employee_id: optional_id(row, 13)?,
        merged_into_task_id: optional_id(row, 14)?,
        suggested_triager_id: optional_id(row, 15)?,
        suggested_verifier_id: optional_id(row, 16)?,
        done_anyway,
    })
}

pub(super) fn counter_row(row: &Row<'_>) -> rusqlite::Result<OfficeTaskCounterRecord> {
    Ok(OfficeTaskCounterRecord {
        office_code: row.get(0)?,
        last_number: number(row, 1)?,
        reserved_through: number(row, 2)?,
    })
}

/// A step row: `id, position, text, required, done, linked_task_id, checked_by_employee_id,
/// checked_at, revision, section`.
pub(super) fn step_row(row: &Row<'_>) -> rusqlite::Result<ChecklistStepRecord> {
    let req_i: i64 = row.get(3)?;
    let done_i: i64 = row.get(4)?;
    let section: String = row.get(9)?;
    let section = ChecklistSection::parse(&section).ok_or_else(|| bad_col(9, "invalid section"))?;
    Ok(ChecklistStepRecord {
        id: id(row, 0)?,
        section,
        position: number(row, 1)?,
        text: row.get(2)?,
        required: req_i != 0,
        done: done_i != 0,
        linked_task_id: optional_id(row, 5)?,
        checked_by_employee_id: optional_id(row, 6)?,
        checked_at: row.get(7)?,
        revision: number(row, 8)?,
    })
}

pub(super) fn comment_row(row: &Row<'_>) -> rusqlite::Result<TaskCommentRecord> {
    Ok(TaskCommentRecord {
        task_id: id(row, 0)?,
        id: id(row, 1)?,
        position: number(row, 2)?,
        author_id: id(row, 3)?,
        created_at: row.get(4)?,
        body_markdown: row.get(5)?,
    })
}

pub(super) fn watcher_row(row: &Row<'_>) -> rusqlite::Result<TaskWatcherRecord> {
    Ok(TaskWatcherRecord {
        id: id(row, 0)?,
        watcher_kind: row.get(1)?,
        watcher_id: optional_id(row, 2)?,
    })
}

pub(super) fn link_row(row: &Row<'_>) -> rusqlite::Result<TaskLinkRecord> {
    Ok(TaskLinkRecord {
        source_id: id(row, 0)?,
        id: id(row, 1)?,
        kind: row.get(2)?,
        target_kind: row.get(3)?,
        target_id: id(row, 4)?,
    })
}

pub(super) fn event_row(row: &Row<'_>) -> rusqlite::Result<TaskEventRecord> {
    Ok(TaskEventRecord {
        task_id: id(row, 0)?,
        revision: number(row, 1)?,
        schema_version: number(row, 2)?,
        occurred_at: row.get(3)?,
        actor_kind: row.get(4)?,
        actor_id: optional_id(row, 5)?,
        operation_id: row.get(6)?,
        change_json: row.get(7)?,
        source_metadata_json: row.get(8)?,
        payload_sha256: row.get(9)?,
        resource_profile: row.get(10)?,
    })
}
