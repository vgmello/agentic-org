//! Row decoders for tasks and their children.

use org_proto::tasks::{TaskPriority, TaskStatus, WorkType};
use rusqlite::Row;

use super::super::organization::{id, meta, number, optional_id};
use super::{bad_col, opt_u64};
use crate::tasks::records::{
    ChecklistStepRecord, OfficeTaskCounterRecord, TaskCommentRecord, TaskDetailsRecord,
    TaskEventRecord, TaskLinkRecord, TaskMeta, TaskRecord, TaskWatcherRecord,
};

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
        rank_order: opt_u64(row, 19)?,
        labels: Vec::new(),
        reference_files: Vec::new(),
        modified_files: Vec::new(),
        repositories: Vec::new(),
        details: None,
        meta: TaskMeta::from_org_meta(meta(row, 20)?),
    })
}

pub(super) fn details_row(row: &Row<'_>) -> rusqlite::Result<TaskDetailsRecord> {
    let sev_int: i64 = row.get(5)?;
    Ok(TaskDetailsRecord {
        definition_of_done: row.get(0)?,
        what_happened: row.get(1)?,
        expected: row.get(2)?,
        steps: row.get(3)?,
        environment: row.get(4)?,
        severity_confirmed: sev_int != 0,
        reopen_count: number(row, 6)?,
        duplicate_of_task_id: optional_id(row, 7)?,
        closed_reason: row.get(8)?,
        affects_feature_id: optional_id(row, 9)?,
        request_kind: row.get(10)?,
        request_text: row.get(11)?,
        route_reason: row.get(12)?,
        routing_source: row.get(13)?,
        routed_by_employee_id: optional_id(row, 14)?,
        merged_into_task_id: optional_id(row, 15)?,
        suggested_triager_id: optional_id(row, 16)?,
        suggested_verifier_id: optional_id(row, 17)?,
    })
}

pub(super) fn counter_row(row: &Row<'_>) -> rusqlite::Result<OfficeTaskCounterRecord> {
    Ok(OfficeTaskCounterRecord {
        office_code: row.get(0)?,
        last_number: number(row, 1)?,
        reserved_through: number(row, 2)?,
    })
}

pub(super) fn step_row(row: &Row<'_>) -> rusqlite::Result<ChecklistStepRecord> {
    let req_i: i64 = row.get(3)?;
    let done_i: i64 = row.get(4)?;
    Ok(ChecklistStepRecord {
        id: id(row, 0)?,
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
