//! SQLite adapter for the delivery family (`DeliveryRead` and `DeliveryWrite`) over
//! `schema.sql`'s canonical tables (`features`, `releases`, `release_repository_commits`,
//! `release_features`, `release_tasks`, `milestones`, `deployments`,
//! `deployment_repository_commits`, `feature_flags`, `release_flag_settings`,
//! `deployment_flag_settings`, `documents` scoped changelogs, `links`, `reference_counters`,
//! `reference_allocations`, and `record_refs`).

mod read;
mod write;

use org_docs::delivery::records::{ActorRef, RecordMeta};
use org_store::EngineError;
use rusqlite::{types::Type, types::Value, Row};

use super::organization::{number, optional, text, whole};

pub(super) fn foreign(col: usize, msg: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, Type::Text, msg.into())
}

pub(super) fn read_actor(
    row: &Row<'_>,
    kind_col: usize,
    id_col: usize,
) -> rusqlite::Result<ActorRef> {
    let kind: String = row.get(kind_col)?;
    let id: Option<String> = row.get(id_col)?;
    match (kind.as_str(), id) {
        ("employee", Some(emp)) => Ok(ActorRef::Employee(emp)),
        ("system", None) => Ok(ActorRef::System),
        ("unknown", None) => Ok(ActorRef::Unknown),
        _ => Err(foreign(kind_col, "invalid actor kind/id pair")),
    }
}

pub(super) fn read_meta(row: &Row<'_>, at: usize) -> rusqlite::Result<RecordMeta> {
    Ok(RecordMeta {
        schema_version: 1,
        revision: number(row, at)?,
        created_at: row.get(at + 1)?,
        created_by: read_actor(row, at + 2, at + 3)?,
        updated_at: row.get(at + 4)?,
        updated_by: read_actor(row, at + 5, at + 6)?,
    })
}

pub(super) fn push_meta_values(meta: &RecordMeta, out: &mut Vec<Value>) -> Result<(), EngineError> {
    out.push(Value::Integer(1));
    out.push(whole(meta.revision)?);
    out.push(optional(meta.created_at.as_deref()));
    out.push(text(meta.created_by.kind()));
    out.push(optional(meta.created_by.employee_id()));
    out.push(optional(meta.updated_at.as_deref()));
    out.push(text(meta.updated_by.kind()));
    out.push(optional(meta.updated_by.employee_id()));
    Ok(())
}
