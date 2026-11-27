//! SQLite adapter of the documentation tree family on the data contract's tables: documents are
//! the `documents` rows with a `path`, media metadata the `documentation_media` rows. Rows are
//! removed by `(project_id, path)`, as files are, so removing a moved document's old file never
//! removes the row that moved.

use org_store::{EngineError, Id};
use rusqlite::{
    types::{Type, Value},
    Row,
};

use org_office::project_docs::{
    paths::{DocPath, MediaFile},
    ports::{ProjectDocsRead, ProjectDocsWrite},
    records::{DocEntry, DocMedia, DocStatus, MediaKind, ProjectDoc},
};

use super::{
    organization::{
        all, id, meta, meta_values, number, one, optional_id, optional_id_value, put, run, text,
        whole, View, META,
    },
    SqliteSnapshot, SqliteTx,
};

/// A document's columns but its body, then `body_markdown`: an entry reads the first, a whole
/// document both.
const ENTRY_COLS: &str = "id, project_id, path, kind, title, status, summary, owner_employee_id";
const DOC_COLS: &str =
    "id, project_id, path, kind, title, status, summary, owner_employee_id, body_markdown";
const MEDIA_COLS: &str = "id, project_id, path, type, media_type, bytes, sha256, title, summary";

fn foreign(column: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, what.into())
}

/// The [`ENTRY_COLS`] columns and, `body` columns later, the [`META`] ones.
fn entry_at(row: &Row<'_>, meta_at: usize) -> rusqlite::Result<DocEntry> {
    let path: String = row.get(2)?;
    let status: String = row.get(5)?;
    Ok(DocEntry {
        id: id(row, 0)?,
        project_id: id(row, 1)?,
        path: DocPath::parse(&path).map_err(|_| foreign(2, "not a document path"))?,
        kind: row.get(3)?,
        title: row.get(4)?,
        status: DocStatus::parse(&status).ok_or_else(|| foreign(5, "not a document status"))?,
        summary: row.get(6)?,
        owner_employee_id: optional_id(row, 7)?,
        meta: meta(row, meta_at)?,
    })
}

fn entry_row(row: &Row<'_>) -> rusqlite::Result<DocEntry> {
    entry_at(row, 8)
}

fn doc_row(row: &Row<'_>) -> rusqlite::Result<ProjectDoc> {
    let entry = entry_at(row, 9)?;
    Ok(ProjectDoc {
        id: entry.id,
        project_id: entry.project_id,
        path: entry.path,
        kind: entry.kind,
        title: entry.title,
        status: entry.status,
        summary: entry.summary,
        owner_employee_id: entry.owner_employee_id,
        body_markdown: row.get(8)?,
        meta: entry.meta,
    })
}

fn media_row(row: &Row<'_>) -> rusqlite::Result<DocMedia> {
    let path: String = row.get(2)?;
    let kind: String = row.get(3)?;
    Ok(DocMedia {
        id: id(row, 0)?,
        project_id: id(row, 1)?,
        file: MediaFile::from_rel_path(&path).ok_or_else(|| foreign(2, "not a media path"))?,
        kind: MediaKind::parse(&kind).ok_or_else(|| foreign(3, "not a media type"))?,
        media_type: row.get(4)?,
        bytes: number(row, 5)?,
        sha256: row.get(6)?,
        title: row.get(7)?,
        summary: row.get(8)?,
        meta: meta(row, 9)?,
    })
}

macro_rules! sqlite_read {
    ($($view:ty),+) => {$(
        impl ProjectDocsRead for $view {
            fn project_doc_entries(&self, project: &Id) -> Result<Vec<DocEntry>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {ENTRY_COLS}, {META} FROM documents WHERE organization_id = ?1 \
                     AND project_id = ?2 AND path IS NOT NULL ORDER BY path"
                );
                all(db, &sql, vec![text(scope), text(project.as_str())], entry_row)
            }

            fn project_doc(&self, doc: &Id) -> Result<Option<ProjectDoc>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {DOC_COLS}, {META} FROM documents WHERE organization_id = ?1 \
                     AND id = ?2 AND path IS NOT NULL"
                );
                one(db, &sql, vec![text(scope), text(doc.as_str())], doc_row)
            }

            fn project_doc_media(&self, project: &Id) -> Result<Vec<DocMedia>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {MEDIA_COLS}, {META} FROM documentation_media \
                     WHERE organization_id = ?1 AND project_id = ?2 ORDER BY path"
                );
                all(db, &sql, vec![text(scope), text(project.as_str())], media_row)
            }

            fn projects_with_docs(&self) -> Result<Vec<Id>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = "SELECT project_id FROM documents \
                           WHERE organization_id = ?1 AND path IS NOT NULL \
                           UNION SELECT project_id FROM documentation_media \
                           WHERE organization_id = ?1 ORDER BY 1";
                all(db, sql, vec![text(scope)], |row| id(row, 0))
            }
        }
    )+};
}

sqlite_read!(SqliteTx, SqliteSnapshot);

impl ProjectDocsWrite for SqliteTx {
    fn put_project_doc(&mut self, doc: &ProjectDoc) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(doc.id.as_str()),
            text(doc.project_id.as_str()),
            text(doc.path.as_str()),
            text(&doc.kind),
            text(&doc.title),
            text(doc.status.as_str()),
            doc.summary.as_deref().map_or(Value::Null, text),
            optional_id_value(doc.owner_employee_id.as_ref()),
            text(&doc.body_markdown),
        ];
        meta_values(&doc.meta, &mut values)?;
        let columns = format!("organization_id, {DOC_COLS}, schema_version, {META}");
        put(db, "documents", &columns, 2, values)
    }

    fn remove_project_doc_at(&mut self, project: &Id, path: &str) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM documents WHERE organization_id = ?1 AND project_id = ?2 AND path = ?3",
            vec![text(scope), text(project.as_str()), text(path)],
        )
    }

    fn put_doc_media(&mut self, media: &DocMedia) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(media.id.as_str()),
            text(media.project_id.as_str()),
            text(&media.file.rel_path()),
            text(media.kind.as_str()),
            text(&media.media_type),
            whole(media.bytes)?,
            text(&media.sha256),
            text(&media.title),
            text(&media.summary),
        ];
        meta_values(&media.meta, &mut values)?;
        let columns = format!("organization_id, {MEDIA_COLS}, schema_version, {META}");
        put(db, "documentation_media", &columns, 2, values)
    }

    fn remove_doc_media_at(&mut self, project: &Id, path: &str) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM documentation_media \
             WHERE organization_id = ?1 AND project_id = ?2 AND path = ?3",
            vec![text(scope), text(project.as_str()), text(path)],
        )
    }
}
