//! SQLite adapter for [`TasksRead`] and [`TasksWrite`] over the verbatim `schema.sql` tables
//! `tasks`, `task_details`, `task_labels`, `task_files`, `office_task_counters`,
//! `checklist_steps`, `comments`, `watchers`, `links`, `task_events`, and `record_refs`
//! (`TASK-49`, `TASK-77`, `ENG-1`).
//!
//! [`TasksRead`]: crate::tasks::ports::TasksRead
//! [`TasksWrite`]: crate::tasks::ports::TasksWrite

use org_store::EngineError;
use rusqlite::{
    types::{Type, Value},
    Row,
};

use super::organization::{text, whole};

mod read;
mod rows;
mod write;

const TASK_COLS: &str = "id, office_code, number, team_id, assigned_team_id, project_id, \
    type, work_type, title, status, assignee_id, body_markdown, feature_id, milestone_id, \
    priority, effort, severity, resolution, blocked_reason, rank_order";

const DETAIL_COLS: &str = "definition_of_done, what_happened, expected, steps, environment, \
    severity_confirmed, reopen_count, duplicate_of_task_id, closed_reason, affects_feature_id, \
    request_kind, request_text, route_reason, routing_source, routed_by_employee_id, \
    merged_into_task_id, suggested_triager_id, suggested_verifier_id";

fn bad_col(col: usize, msg: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, Type::Text, msg.into())
}

fn opt_u64(row: &Row<'_>, col: usize) -> rusqlite::Result<Option<u64>> {
    let val: Option<i64> = row.get(col)?;
    val.map(|v| u64::try_from(v).map_err(|_| bad_col(col, "negative integer")))
        .transpose()
}

fn opt_text_val(val: Option<&str>) -> Value {
    val.map_or(Value::Null, text)
}

fn opt_u64_val(val: Option<u64>) -> Result<Value, EngineError> {
    match val {
        None => Ok(Value::Null),
        Some(n) => whole(n),
    }
}
