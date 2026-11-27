//! Rows of the private notes database: notes with their tags, and command receipts.

use org_proto::private_notes::PrivateNoteKind;
use rusqlite::types::Type;
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::OfficeError;
use crate::private_notes::PrivateNoteRecord;

/// The `private_notes` columns [`note_row`] reads, in order.
pub(super) const NOTE_COLUMNS: &str = "organization_id, id, schema_version, revision, task_id, \
     project_id, author_id, kind, title, why, alternatives, body_markdown, created_at, \
     updated_at, deleted_at";

/// One `private_notes` row; its tags are loaded by [`with_tags`].
pub(super) fn note_row(row: &Row<'_>) -> rusqlite::Result<PrivateNoteRecord> {
    let schema_version: i64 = row.get(2)?;
    let revision: i64 = row.get(3)?;
    let kind: String = row.get(7)?;
    let kind = PrivateNoteKind::parse(&kind).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(7, Type::Text, "unknown private note kind".into())
    })?;
    Ok(PrivateNoteRecord {
        organization_id: row.get(0)?,
        id: row.get(1)?,
        schema_version: schema_version as u64,
        revision: revision as u64,
        task_id: row.get(4)?,
        project_id: row.get(5)?,
        author_id: row.get(6)?,
        kind,
        title: row.get(8)?,
        why: row.get(9)?,
        alternatives: row.get(10)?,
        tags: Vec::new(),
        body_markdown: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        deleted_at: row.get(14)?,
    })
}

/// `record` with its stored tags, sorted.
pub(super) fn with_tags(
    conn: &Connection,
    mut record: PrivateNoteRecord,
) -> Result<PrivateNoteRecord, OfficeError> {
    let mut statement = conn
        .prepare_cached(
            "SELECT tag FROM private_note_tags
             WHERE organization_id = ?1 AND note_id = ?2 ORDER BY tag",
        )
        .map_err(OfficeError::storage)?;
    record.tags = statement
        .query_map(params![record.organization_id, record.id], |row| row.get(0))
        .map_err(OfficeError::storage)?
        .collect::<Result<_, _>>()
        .map_err(OfficeError::storage)?;
    Ok(record)
}

/// The note with its tags, active or tombstoned.
pub(super) fn load_note(
    conn: &Connection,
    org_id: &str,
    note_id: &str,
) -> Result<Option<PrivateNoteRecord>, OfficeError> {
    let row = conn
        .query_row(
            &format!(
                "SELECT {NOTE_COLUMNS} FROM private_notes WHERE organization_id = ?1 AND id = ?2"
            ),
            params![org_id, note_id],
            note_row,
        )
        .optional()
        .map_err(OfficeError::storage)?;
    row.map(|record| with_tags(conn, record)).transpose()
}

/// Inserts a new active note and its tags.
pub(super) fn insert_note(conn: &Connection, note: &PrivateNoteRecord) -> Result<(), OfficeError> {
    conn.execute(
        &format!("INSERT INTO private_notes ({NOTE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"),
        params![
            note.organization_id,
            note.id,
            note.schema_version as i64,
            note.revision as i64,
            note.task_id,
            note.project_id,
            note.author_id,
            note.kind.as_str(),
            note.title,
            note.why,
            note.alternatives,
            note.body_markdown,
            note.created_at,
            note.updated_at,
            note.deleted_at,
        ],
    )
    .map_err(OfficeError::storage)?;
    insert_tags(conn, note)
}

fn insert_tags(conn: &Connection, note: &PrivateNoteRecord) -> Result<(), OfficeError> {
    let mut statement = conn
        .prepare_cached(
            "INSERT INTO private_note_tags (organization_id, note_id, tag) VALUES (?1, ?2, ?3)",
        )
        .map_err(OfficeError::storage)?;
    for tag in &note.tags {
        statement
            .execute(params![note.organization_id, note.id, tag])
            .map_err(OfficeError::storage)?;
    }
    Ok(())
}

/// Stores an author edit: the next revision's content, replacing every tag.
pub(super) fn update_content(
    conn: &Connection,
    note: &PrivateNoteRecord,
) -> Result<(), OfficeError> {
    conn.execute(
        "UPDATE private_notes
         SET revision = ?3, kind = ?4, title = ?5, why = ?6, alternatives = ?7,
             body_markdown = ?8, updated_at = ?9
         WHERE organization_id = ?1 AND id = ?2",
        params![
            note.organization_id,
            note.id,
            note.revision as i64,
            note.kind.as_str(),
            note.title,
            note.why,
            note.alternatives,
            note.body_markdown,
            note.updated_at,
        ],
    )
    .map_err(OfficeError::storage)?;
    conn.execute(
        "DELETE FROM private_note_tags WHERE organization_id = ?1 AND note_id = ?2",
        params![note.organization_id, note.id],
    )
    .map_err(OfficeError::storage)?;
    insert_tags(conn, note)
}

/// Stores a revisioned tombstone (`PN-06`): every private text field is cleared, and the
/// schema's `private_note_tombstone_clears_tags` trigger deletes the tags in the same statement.
pub(super) fn tombstone(
    conn: &Connection,
    org_id: &str,
    note_id: &str,
    revision: u64,
    deleted_at: &str,
) -> Result<(), OfficeError> {
    conn.execute(
        "UPDATE private_notes
         SET revision = ?3, title = NULL, why = NULL, alternatives = NULL,
             body_markdown = NULL, updated_at = ?4, deleted_at = ?4
         WHERE organization_id = ?1 AND id = ?2",
        params![org_id, note_id, revision as i64, deleted_at],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) struct ReceiptRow {
    pub request_sha256: String,
    pub command: String,
    pub note_id: String,
    pub result_revision: u64,
    pub state: String,
    pub shared_comment_id: Option<String>,
    pub error_code: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

impl ReceiptRow {
    /// When the command's result was accepted.
    pub fn time(&self) -> &str {
        self.finished_at.as_deref().unwrap_or(&self.created_at)
    }
}

pub(super) fn load_receipt(
    conn: &Connection,
    org_id: &str,
    author_id: &str,
    operation_id: &str,
) -> Result<Option<ReceiptRow>, OfficeError> {
    conn.query_row(
        "SELECT request_sha256, command, note_id, result_revision, state,
                shared_comment_id, error_code, created_at, finished_at
         FROM private_note_command_receipts
         WHERE organization_id = ?1 AND author_id = ?2 AND operation_id = ?3",
        params![org_id, author_id, operation_id],
        |row| {
            let result_revision: i64 = row.get(3)?;
            Ok(ReceiptRow {
                request_sha256: row.get(0)?,
                command: row.get(1)?,
                note_id: row.get(2)?,
                result_revision: result_revision as u64,
                state: row.get(4)?,
                shared_comment_id: row.get(5)?,
                error_code: row.get(6)?,
                created_at: row.get(7)?,
                finished_at: row.get(8)?,
            })
        },
    )
    .optional()
    .map_err(OfficeError::storage)
}

/// The identity of one author command: `(org_id, author_id, operation_id, request_sha256)`.
pub(super) type CommandKey<'a> = (&'a str, &'a str, &'a str, &'a str);

/// Records a completed `create`, `edit` or `delete` receipt.
pub(super) fn insert_completed_receipt(
    conn: &Connection,
    key: CommandKey<'_>,
    command: &str,
    note_id: &str,
    result_revision: u64,
    at: &str,
) -> Result<(), OfficeError> {
    conn.execute(
        "INSERT INTO private_note_command_receipts
         (organization_id, author_id, operation_id, request_sha256, command, note_id,
          result_revision, state, shared_comment_id, error_code, created_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'completed', NULL, NULL, ?8, ?8)",
        params![
            key.0,
            key.1,
            key.2,
            key.3,
            command,
            note_id,
            result_revision as i64,
            at
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) fn has_pending_publish(
    conn: &Connection,
    org_id: &str,
    note_id: &str,
) -> Result<bool, OfficeError> {
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM private_note_command_receipts
            WHERE organization_id = ?1 AND note_id = ?2 AND state = 'pending'
         )",
        params![org_id, note_id],
        |row| row.get(0),
    )
    .map_err(OfficeError::storage)
}
