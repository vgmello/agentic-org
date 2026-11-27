//! The organization family's two ports in SQL, on the data contract's tables
//! (`organizations`, `repositories`, `offices`, `office_codes`, `roles`, `teams`, `employees`,
//! `reference_counters`, `reference_allocations`, `record_refs`).
//!
//! No rule is here: the decisions and the projector's `validate` hold them. The tables'
//! constraints are the backstop, and one that fires is an engine failure, never a refusal.
//! Every row is scoped by the organization the engine was opened for, because a team's,
//! an office's and an employee's file do not name it.

use org_store::{EngineError, Id};
use rusqlite::{
    params_from_iter,
    types::{Type, Value},
    Connection, Row,
};

use super::{failed, SqliteSnapshot, SqliteTx};
use crate::organization::records::{By, Meta};

mod read;
mod rows;
#[cfg(test)]
mod tests;
mod write;

pub(super) use rows::default_dod;
pub(super) use write::default_dod_value;

/// The columns of document 08's fields, as every attributed table has them.
pub(super) const META: &str = "revision, created_at, created_by_kind, created_by_id, updated_at, \
                    updated_by_kind, updated_by_id";

/// The one counter of this family: team numbers, scoped by the organization.
const TEAM_SCOPE: &str = "scope_kind = 'organization' AND scope_id = ?1 AND entity_kind = 'team'";

/// A transaction's or a snapshot's connection, and the organization it is scoped to.
pub(super) trait View {
    fn view(&self) -> Result<(&Connection, &str), EngineError>;
}

fn unscoped() -> EngineError {
    EngineError::new("the projection was opened for no organization")
}

impl View for SqliteTx {
    fn view(&self) -> Result<(&Connection, &str), EngineError> {
        let organization = self.organization.as_ref().ok_or_else(unscoped)?;
        Ok((self.db(), organization.as_str()))
    }
}

impl View for SqliteSnapshot {
    fn view(&self) -> Result<(&Connection, &str), EngineError> {
        let organization = self.organization.as_ref().ok_or_else(unscoped)?;
        Ok((self.db(), organization.as_str()))
    }
}

// ---- reading ----

/// A stored value this code did not write.
fn foreign(column: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, what.into())
}

pub(super) fn id(row: &Row<'_>, column: usize) -> rusqlite::Result<Id> {
    let text: String = row.get(column)?;
    Id::parse(&text).map_err(|_| foreign(column, "not an ID"))
}

pub(super) fn optional_id(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<Id>> {
    let text: Option<String> = row.get(column)?;
    let parsed = text.map(|text| Id::parse(&text).map_err(|_| foreign(column, "not an ID")));
    parsed.transpose()
}

pub(super) fn number(row: &Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let stored: i64 = row.get(column)?;
    u64::try_from(stored).map_err(|_| foreign(column, "a negative number"))
}

/// The variant whose text is stored.
fn one_of<T: Copy>(
    row: &Row<'_>,
    column: usize,
    all: &[T],
    text: fn(T) -> &'static str,
) -> rusqlite::Result<T> {
    let stored: String = row.get(column)?;
    let found = all.iter().find(|variant| text(**variant) == stored);
    found
        .copied()
        .ok_or_else(|| foreign(column, "not a known value"))
}

fn by(row: &Row<'_>, column: usize) -> rusqlite::Result<By> {
    let kind: String = row.get(column)?;
    match (kind.as_str(), optional_id(row, column + 1)?) {
        ("employee", Some(id)) => Ok(By::Employee(id)),
        ("system", None) => Ok(By::System),
        ("unknown", None) => Ok(By::Unknown),
        _ => Err(foreign(column, "not an attribution")),
    }
}

/// The [`META`] columns, the first of them at `column`.
pub(super) fn meta(row: &Row<'_>, column: usize) -> rusqlite::Result<Meta> {
    Ok(Meta {
        revision: number(row, column)?,
        created_at: row.get(column + 1)?,
        created_by: by(row, column + 2)?,
        updated_at: row.get(column + 4)?,
        updated_by: by(row, column + 5)?,
    })
}

pub(super) fn all<T>(
    db: &Connection,
    sql: &str,
    values: Vec<Value>,
    read: fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, EngineError> {
    let query = || -> rusqlite::Result<Vec<T>> {
        let mut query = db.prepare_cached(sql)?;
        let rows = query.query_map(params_from_iter(values), read)?;
        let rows: rusqlite::Result<Vec<T>> = rows.collect();
        rows
    };
    query().map_err(|error| failed(db, "reading organization rows", &error))
}

pub(super) fn one<T>(
    db: &Connection,
    sql: &str,
    values: Vec<Value>,
    read: fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Option<T>, EngineError> {
    Ok(all(db, sql, values, read)?.pop())
}

pub(super) fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

pub(super) fn optional(value: Option<&str>) -> Value {
    value.map_or(Value::Null, text)
}

pub(super) fn whole(value: u64) -> Result<Value, EngineError> {
    let stored = i64::try_from(value);
    stored
        .map(Value::Integer)
        .map_err(|_| EngineError::new("a number is too large to store"))
}

const ORGANIZATION: &str =
    "id, slug, name, organization_repository_id, state_branch, default_dod_json";
const REPOSITORY: &str = "id, key, kind, remote_url";
const OFFICE: &str = "id, slug, name, role";
const ROLE: &str = "id, key, name, body_markdown";
const TEAM: &str = "id, name, number, parent_team_id, lead_employee_id, status, max_members";
const EMPLOYEE: &str = "id, handle, kind, name, home_team_id, role_id, reports_to_employee_id, \
                        status, contact, title, focus, role_text, title_in_team, offboarded_at, \
                        may_hire, builtin_key, body_markdown";
const HIRE_REQUEST: &str = "id, revision, state, requested_by_employee_id, home_team_id, role_id, \
                            proposal_json, reason, confirmed_at, closed_reason, employee_id, \
                            failed_step, created_at, updated_at";
const AGENT_EXEC: &str = "employee_id, model_profile_id, model_id, system_prompt_markdown, \
                          tools_config_json, permission_preset, enabled, effort";
const PROVIDER_CONN: &str = "id, key, name, provider_type, auth_mode, subscription_identity, \
                             runtime_driver, endpoint_url, enabled, archived, max_concurrent_turns";
const MODEL_PROFILE: &str =
    "id, key, name, description, is_auto_generated, generated_for_connection_id, archived";

/// Every row of `table` in the organization, or those `filter` (on `?2`) keeps.
fn select(columns: &str, table: &str, filter: &str, order: &str) -> String {
    format!(
        "SELECT {columns}, {META} FROM {table} WHERE organization_id = ?1{filter} ORDER BY {order}"
    )
}

// ---- writing ----

pub(super) fn run(db: &Connection, sql: &str, values: Vec<Value>) -> Result<(), EngineError> {
    db.prepare_cached(sql)
        .and_then(|mut write| write.execute(params_from_iter(values)))
        .map_err(|error| failed(db, "writing an organization row", &error))?;
    Ok(())
}

/// Inserts a row, or replaces every other column of the row with the same key: the first
/// `key` columns. A row is never deleted to be replaced, so what refers to it stays
/// resolved.
pub(super) fn put(
    db: &Connection,
    table: &str,
    columns: &str,
    key: usize,
    values: Vec<Value>,
) -> Result<(), EngineError> {
    let names: Vec<&str> = columns.split(',').map(str::trim).collect();
    debug_assert_eq!(names.len(), values.len(), "{table}");
    let places: Vec<String> = (1..=names.len()).map(|n| format!("?{n}")).collect();
    let rest: Vec<String> = names[key..]
        .iter()
        .map(|name| format!("{name} = excluded.{name}"))
        .collect();
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {}",
        names.join(", "),
        places.join(", "),
        names[..key].join(", "),
        rest.join(", "),
    );
    run(db, &sql, values)
}

/// Updates the non-key columns of an existing row matched by its first `key` columns.
/// Used for tables with `BEFORE INSERT` triggers (such as `provider_connections`) so updating
/// an existing row runs the `UPDATE` triggers instead of firing `BEFORE INSERT` on conflict.
pub(super) fn update(
    db: &Connection,
    table: &str,
    columns: &str,
    key: usize,
    values: Vec<Value>,
) -> Result<(), EngineError> {
    let names: Vec<&str> = columns.split(',').map(str::trim).collect();
    debug_assert_eq!(names.len(), values.len(), "{table}");
    let sets: Vec<String> = names[key..]
        .iter()
        .enumerate()
        .map(|(i, name)| format!("{name} = ?{}", key + i + 1))
        .collect();
    let predicate: Vec<String> = names[..key]
        .iter()
        .enumerate()
        .map(|(i, name)| format!("{name} = ?{}", i + 1))
        .collect();
    let sql = format!(
        "UPDATE {table} SET {} WHERE {}",
        sets.join(", "),
        predicate.join(" AND "),
    );
    let changed = db
        .prepare_cached(&sql)
        .and_then(|mut write| write.execute(params_from_iter(values)))
        .map_err(|error| failed(db, "updating an organization row", &error))?;
    if changed != 1 {
        return Err(EngineError::new(format!(
            "updating {table} affected {changed} rows (expected 1)"
        )));
    }
    Ok(())
}

fn by_values(by: &By, values: &mut Vec<Value>) {
    let (kind, id) = match by {
        By::Employee(id) => ("employee", Some(id.as_str())),
        By::System => ("system", None),
        By::Unknown => ("unknown", None),
    };
    values.push(text(kind));
    values.push(optional(id));
}

/// The values of `schema_version` and the [`META`] columns.
pub(super) fn meta_values(meta: &Meta, values: &mut Vec<Value>) -> Result<(), EngineError> {
    values.push(Value::Integer(1));
    values.push(whole(meta.revision)?);
    values.push(optional(meta.created_at.as_deref()));
    by_values(&meta.created_by, values);
    values.push(optional(meta.updated_at.as_deref()));
    by_values(&meta.updated_by, values);
    Ok(())
}

pub(super) fn optional_id_value(id: Option<&Id>) -> Value {
    optional(id.map(Id::as_str))
}

/// The row other families refer to a record of this one by.
pub(super) fn put_record_ref(
    db: &Connection,
    scope: &str,
    kind: &str,
    id: &Id,
) -> Result<(), EngineError> {
    let values = vec![
        text(scope),
        text(kind),
        text(id.as_str()),
        Value::Integer(0),
    ];
    put(
        db,
        "record_refs",
        "organization_id, kind, id, is_deleted",
        3,
        values,
    )
}
