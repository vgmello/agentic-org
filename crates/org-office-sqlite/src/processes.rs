//! SQLite adapter for [`ProcessesRead`] and [`ProcessesWrite`] over `structure_documents`.

use org_process::{DocumentKind, DocumentStatus};
use org_store::{EngineError, Id};
use rusqlite::{types::Value, Row};

use org_office::processes::{
    ports::{ProcessesRead, ProcessesWrite},
    records::StructureDocument,
};

use super::{
    organization::{
        all, id, meta, meta_values, one, optional_id, optional_id_value, put, run, text, View, META,
    },
    SqliteSnapshot, SqliteTx,
};

const COLS: &str =
    "id, key, kind, title, status, owner_employee_id, team_id, project_id, overridable, body_markdown";

fn doc_row(row: &Row<'_>) -> rusqlite::Result<StructureDocument> {
    let kind_raw: String = row.get(2)?;
    let kind = DocumentKind::parse(&kind_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            format!("invalid structure_document kind `{kind_raw}`").into(),
        )
    })?;
    let status_raw: String = row.get(4)?;
    let status = DocumentStatus::parse(&status_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            format!("invalid structure_document status `{status_raw}`").into(),
        )
    })?;
    let overridable_int: i64 = row.get(8)?;
    Ok(StructureDocument {
        id: id(row, 0)?,
        key: row.get(1)?,
        kind,
        title: row.get(3)?,
        status,
        owner_employee_id: optional_id(row, 5)?,
        team_id: optional_id(row, 6)?,
        project_id: optional_id(row, 7)?,
        overridable: overridable_int != 0,
        body_markdown: row.get(9)?,
        meta: meta(row, 10)?,
    })
}

macro_rules! sqlite_read {
    ($($view:ty),+) => {$(
        impl ProcessesRead for $view {
            fn structure_document(&self, doc_id: &Id) -> Result<Option<StructureDocument>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {COLS}, {META} FROM structure_documents \
                     WHERE organization_id = ?1 AND id = ?2"
                );
                one(db, &sql, vec![text(scope), text(doc_id.as_str())], doc_row)
            }

            fn structure_documents(&self) -> Result<Vec<StructureDocument>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {COLS}, {META} FROM structure_documents \
                     WHERE organization_id = ?1 ORDER BY id"
                );
                all(db, &sql, vec![text(scope)], doc_row)
            }
        }
    )+};
}

sqlite_read!(SqliteTx, SqliteSnapshot);

impl ProcessesWrite for SqliteTx {
    fn put_structure_document(&mut self, record: &StructureDocument) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(record.id.as_str()),
            record
                .key
                .as_deref()
                .map_or(Value::Null, |k| Value::Text(k.to_owned())),
            text(record.kind.as_str()),
            text(&record.title),
            text(record.status.as_str()),
            optional_id_value(record.owner_employee_id.as_ref()),
            optional_id_value(record.team_id.as_ref()),
            optional_id_value(record.project_id.as_ref()),
            Value::Integer(i64::from(record.overridable)),
            text(&record.body_markdown),
        ];
        meta_values(&record.meta, &mut values)?;
        let columns = format!("organization_id, {COLS}, schema_version, {META}");
        put(db, "structure_documents", &columns, 2, values)
    }

    fn remove_structure_document(&mut self, doc_id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM structure_documents WHERE organization_id = ?1 AND id = ?2",
            vec![text(scope), text(doc_id.as_str())],
        )
    }
}
