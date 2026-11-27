//! SQLite adapter for [`AttachmentsRead`] and [`AttachmentsWrite`] over the data contract's
//! `attachments` table (`TASK-98.1`, ADR-0037): metadata only, never bytes.

use org_proto::attachments::{ArtifactStatus, AttachmentTargetKind};
use org_store::{EngineError, Id};
use rusqlite::{types::Value, Row};

use org_office::attachments::{
    blobs::ContentHash,
    ports::{AttachmentsRead, AttachmentsWrite},
    records::AttachmentRecord,
};

use super::{
    organization::{all, id, number, one, optional_id, put, run, text, whole, View},
    SqliteSnapshot, SqliteTx,
};

const COLUMNS: &str = "organization_id, id, target_kind, target_id, content_sha256, mime_type, \
                       byte_count, filename, title, description, artifact_status, width, height, \
                       page_count, uploaded_by_employee_id, created_at, updated_at";
/// [`COLUMNS`] without the organization, as rows are read.
const READ: &str = "id, target_kind, target_id, content_sha256, mime_type, byte_count, filename, \
                    title, description, artifact_status, width, height, page_count, \
                    uploaded_by_employee_id, created_at, updated_at";

fn bad(column: usize) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        "an attachment row this code did not write".into(),
    )
}

fn dimension(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<u32>> {
    let stored: Option<i64> = row.get(column)?;
    stored
        .map(|value| u32::try_from(value).map_err(|_| bad(column)))
        .transpose()
}

fn attachment_row(row: &Row<'_>) -> rusqlite::Result<AttachmentRecord> {
    let kind: String = row.get(1)?;
    let sha: String = row.get(3)?;
    let status: String = row.get(9)?;
    Ok(AttachmentRecord {
        id: id(row, 0)?,
        target_kind: AttachmentTargetKind::parse(&kind).ok_or_else(|| bad(1))?,
        target_id: optional_id(row, 2)?,
        content_sha256: ContentHash::parse(&sha).ok_or_else(|| bad(3))?,
        mime_type: row.get(4)?,
        byte_count: number(row, 5)?,
        filename: row.get(6)?,
        title: row.get(7)?,
        description: row.get(8)?,
        artifact_status: ArtifactStatus::parse(&status).ok_or_else(|| bad(9))?,
        width: dimension(row, 10)?,
        height: dimension(row, 11)?,
        page_count: dimension(row, 12)?,
        uploaded_by_employee_id: id(row, 13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

fn optional_number(value: Option<u32>) -> Value {
    value.map_or(Value::Null, |v| Value::Integer(i64::from(v)))
}

macro_rules! sqlite_attachments_read {
    ($($view:ty),+) => {$(
        impl AttachmentsRead for $view {
            fn attachment(&self, attachment: &Id) -> Result<Option<AttachmentRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {READ} FROM attachments WHERE organization_id = ?1 AND id = ?2"
                );
                one(db, &sql, vec![text(scope), text(attachment.as_str())], attachment_row)
            }

            fn attachments_for_target(
                &self,
                kind: AttachmentTargetKind,
                target_id: Option<&Id>,
                after: Option<&Id>,
                limit: usize,
            ) -> Result<Vec<AttachmentRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {READ} FROM attachments WHERE organization_id = ?1 \
                     AND target_kind = ?2 AND target_id IS ?3 AND id > ?4 ORDER BY id LIMIT ?5"
                );
                let target = target_id.map_or(Value::Null, |t| text(t.as_str()));
                let after = text(after.map_or("", Id::as_str));
                let limit = whole(limit as u64)?;
                all(
                    db,
                    &sql,
                    vec![text(scope), text(kind.as_str()), target, after, limit],
                    attachment_row,
                )
            }

            fn attachments(&self) -> Result<Vec<AttachmentRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {READ} FROM attachments WHERE organization_id = ?1 ORDER BY id"
                );
                all(db, &sql, vec![text(scope)], attachment_row)
            }
        }
    )+};
}

sqlite_attachments_read!(SqliteTx, SqliteSnapshot);

impl AttachmentsWrite for SqliteTx {
    fn put_attachment(&mut self, record: &AttachmentRecord) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let values = vec![
            text(scope),
            text(record.id.as_str()),
            text(record.target_kind.as_str()),
            record
                .target_id
                .as_ref()
                .map_or(Value::Null, |t| text(t.as_str())),
            text(record.content_sha256.as_str()),
            text(&record.mime_type),
            whole(record.byte_count)?,
            text(&record.filename),
            text(&record.title),
            record.description.as_deref().map_or(Value::Null, text),
            text(record.artifact_status.as_str()),
            optional_number(record.width),
            optional_number(record.height),
            optional_number(record.page_count),
            text(record.uploaded_by_employee_id.as_str()),
            text(&record.created_at),
            text(&record.updated_at),
        ];
        put(db, "attachments", COLUMNS, 2, values)
    }

    fn remove_attachment(&mut self, attachment: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM attachments WHERE organization_id = ?1 AND id = ?2",
            vec![text(scope), text(attachment.as_str())],
        )
    }
}
