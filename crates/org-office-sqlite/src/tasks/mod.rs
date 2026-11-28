//! SQLite adapter for [`TasksRead`] and [`TasksWrite`] over the verbatim `schema.sql` tables
//! `tasks`, `task_details`, `task_labels`, `task_files`, `office_task_counters`,
//! `checklist_steps`, `comments`, `watchers`, `links`, `task_events`, and `record_refs`
//! (`TASK-49`, `TASK-77`, `ENG-1`).
//!
//! Every read goes through [`fetch`], and a set of tasks is read with one statement per
//! table, its IDs bound [`CHUNK`] at a time, never one statement per task (`TASK-105`).
//!
//! [`TasksRead`]: org_office::tasks::ports::TasksRead
//! [`TasksWrite`]: org_office::tasks::ports::TasksWrite

use org_store::{EngineError, Id};
use rusqlite::{
    types::{Type, Value},
    Connection, Row,
};

use super::organization::{all, text, whole};

mod hydrate;
mod list;
mod read;
mod rows;
mod write;

/// The most task IDs one statement binds: far below SQLite's limit on bound parameters.
const CHUNK: usize = 500;

/// Runs one read statement of this adapter. It is the one place they run, so a test can
/// count the statements a read costs.
fn fetch<T>(
    db: &Connection,
    sql: &str,
    values: Vec<Value>,
    read: fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, EngineError> {
    #[cfg(any(test, feature = "testkit"))]
    statements::count();
    all(db, sql, values, read)
}

fn fetch_one<T>(
    db: &Connection,
    sql: &str,
    values: Vec<Value>,
    read: fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Option<T>, EngineError> {
    Ok(fetch(db, sql, values, read)?.pop())
}

/// `(?n, ?n+1, ...)` for `ids`, bound after the `values` already there. `ids` is never
/// empty and never longer than [`CHUNK`].
fn id_list(ids: &[Id], values: &mut Vec<Value>) -> String {
    let mut marks = Vec::with_capacity(ids.len());
    for id in ids {
        values.push(text(id.as_str()));
        marks.push(format!("?{}", values.len()));
    }
    format!("({})", marks.join(", "))
}

/// How many statements this adapter has run on this thread: the cost of a read, for tests.
#[cfg(any(test, feature = "testkit"))]
pub(crate) mod statements {
    use std::cell::Cell;

    thread_local! {
        static RUN: Cell<u64> = const { Cell::new(0) };
    }

    pub(super) fn count() {
        RUN.with(|run| run.set(run.get() + 1));
    }

    pub fn run() -> u64 {
        RUN.with(Cell::get)
    }
}

#[cfg(any(test, feature = "testkit"))]
pub(crate) fn explain_list_plan(
    db: &Connection,
    scope: &str,
    query: &org_office::tasks::query::TaskListQuery,
) -> Result<String, EngineError> {
    let clause = list::filter(scope, query)?;
    let (sql, values) = list::page_statement(clause, query);
    let mut explain = db
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .map_err(|e| EngineError::new(format!("explain query plan: {e}")))?;
    let rows = explain
        .query_map(rusqlite::params_from_iter(values), |row| {
            row.get::<_, String>(3)
        })
        .map_err(|e| EngineError::new(format!("explain query plan: {e}")))?;
    let lines = rows
        .collect::<rusqlite::Result<Vec<String>>>()
        .map_err(|e| EngineError::new(format!("explain query plan: {e}")))?;
    Ok(lines.join("\n"))
}

const TASK_COLS: &str = "id, office_code, number, team_id, assigned_team_id, project_id, \
    type, work_type, title, status, assignee_id, body_markdown, feature_id, milestone_id, \
    priority, effort, severity, resolution, blocked_reason, rank_order, final_summary, \
    final_summary_at, final_summary_by_kind, final_summary_by_id, blocked_since, waiting_on_owner";

/// The column `task_row` reads the task's `META` from: right after [`TASK_COLS`].
const TASK_META_AT: usize = 26;

const DETAIL_COLS: &str = "what_happened, expected, steps, environment, \
    severity_confirmed, reopen_count, duplicate_of_task_id, closed_reason, affects_feature_id, \
    request_kind, request_text, route_reason, routing_source, routed_by_employee_id, \
    merged_into_task_id, suggested_triager_id, suggested_verifier_id, done_anyway_json";

/// The checklist's order (`TASK-88`): section by section, then position within one.
const STEP_ORDER: &str = "CASE section WHEN 'acceptance' THEN 0 WHEN 'dod' THEN 1 ELSE 2 END, \
    position";

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

#[cfg(test)]
mod tests;
