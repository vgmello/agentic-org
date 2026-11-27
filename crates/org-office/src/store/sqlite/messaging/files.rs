//! The files of messages and ask answers inside `messaging.db` (`TASK-98.2`, ADR-0037):
//! `message_attachments` and `message_answer_attachments`, metadata only, rebuilt from the
//! message and answer files like every other messaging row.

use org_proto::attachments::ArtifactStatus;
use org_store::{EngineError, Id};
use rusqlite::{types::Value, Connection, Row};

use crate::messaging::ports::BoundMessageFile;
use crate::messaging::records::{MessageAttachmentRecord, MessageRecord, ParticipantAnswerFile};

use super::super::organization::{
    all, number, one, optional_id, optional_id_value, put, run, text, whole,
};
use super::mirror::ensure_employee;
use super::rows::foreign_col;

/// The metadata columns both tables share, after their keys.
const META: &str = "media_type, byte_count, content_sha256, filename, title, description, \
                    artifact_status, width, height, page_count, uploaded_by_employee_id";

fn optional_text(value: Option<&str>) -> Value {
    value.map_or(Value::Null, text)
}

fn dimension(value: Option<u32>) -> Value {
    value.map_or(Value::Null, |v| Value::Integer(i64::from(v)))
}

fn meta_values(file: &MessageAttachmentRecord) -> Result<Vec<Value>, EngineError> {
    Ok(vec![
        text(&file.media_type),
        whole(file.byte_count)?,
        text(&file.content_sha256),
        optional_text(file.filename.as_deref()),
        optional_text(file.title.as_deref()),
        optional_text(file.description.as_deref()),
        text(file.artifact_status.as_str()),
        dimension(file.width),
        dimension(file.height),
        dimension(file.page_count),
        optional_id_value(file.uploaded_by_employee_id.as_ref()),
    ])
}

fn read_dimension(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<u32>> {
    let stored: Option<i64> = row.get(column)?;
    stored
        .map(|v| u32::try_from(v).map_err(|_| foreign_col(column, "dimension")))
        .transpose()
}

/// The [`META`] columns from `column` on, onto a record that has its ID and blob reference.
fn read_meta(
    row: &Row<'_>,
    column: usize,
    mut file: MessageAttachmentRecord,
) -> rusqlite::Result<MessageAttachmentRecord> {
    let status: String = row.get(column + 6)?;
    file.media_type = row.get(column)?;
    file.byte_count = number(row, column + 1)?;
    file.content_sha256 = row.get(column + 2)?;
    file.filename = row.get(column + 3)?;
    file.title = row.get(column + 4)?;
    file.description = row.get(column + 5)?;
    file.artifact_status =
        ArtifactStatus::parse(&status).ok_or_else(|| foreign_col(column + 6, "artifact_status"))?;
    file.width = read_dimension(row, column + 7)?;
    file.height = read_dimension(row, column + 8)?;
    file.page_count = read_dimension(row, column + 9)?;
    file.uploaded_by_employee_id = optional_id(row, column + 10)?;
    Ok(file)
}

/// Where the shared blob store files a digest (an answer's row keeps no reference).
fn content_ref(sha256: &str) -> String {
    if sha256.len() < 4 {
        return String::new();
    }
    format!("blobs/sha256/{}/{}/{sha256}", &sha256[0..2], &sha256[2..4])
}

/// `id, content_ref, META` of `message_attachments`.
fn message_row(row: &Row<'_>) -> rusqlite::Result<MessageAttachmentRecord> {
    let file = MessageAttachmentRecord {
        id: row.get(0)?,
        content_ref: row.get(1)?,
        ..MessageAttachmentRecord::default()
    };
    read_meta(row, 2, file)
}

/// `id, META, created_at` of `message_answer_attachments`.
fn answer_row(row: &Row<'_>) -> rusqlite::Result<MessageAttachmentRecord> {
    let file = MessageAttachmentRecord {
        id: row.get(0)?,
        created_at: row.get(12)?,
        ..MessageAttachmentRecord::default()
    };
    let mut file = read_meta(row, 1, file)?;
    file.content_ref = content_ref(&file.content_sha256);
    Ok(file)
}

/// Replaces a message's file rows with those its record names.
pub(super) fn put_message_files(
    db: &Connection,
    scope: &str,
    record: &MessageRecord,
) -> Result<(), EngineError> {
    remove_message_files(db, scope, &record.id)?;
    for file in &record.attachments {
        let mut values = vec![
            text(scope),
            text(&record.id),
            text(&file.id),
            text(&file.content_ref),
        ];
        values.extend(meta_values(file)?);
        put(
            db,
            "message_attachments",
            &format!("organization_id, message_id, id, content_ref, {META}"),
            3,
            values,
        )?;
    }
    Ok(())
}

pub(super) fn remove_message_files(
    db: &Connection,
    scope: &str,
    message_id: &str,
) -> Result<(), EngineError> {
    run(
        db,
        "DELETE FROM message_attachments WHERE organization_id = ?1 AND message_id = ?2",
        vec![text(scope), text(message_id)],
    )
}

/// A message's files in attachment-ID order (the order they were named); a file that names no
/// time has the message's.
pub(super) fn message_files(
    db: &Connection,
    scope: &str,
    message: &MessageRecord,
) -> Result<Vec<MessageAttachmentRecord>, EngineError> {
    let mut files = all(
        db,
        &format!(
            "SELECT id, content_ref, {META} FROM message_attachments \
             WHERE organization_id = ?1 AND message_id = ?2 ORDER BY id"
        ),
        vec![text(scope), text(&message.id)],
        message_row,
    )?;
    for file in &mut files {
        file.created_at = Some(message.created_at.clone());
    }
    Ok(files)
}

/// Attaches a message's projected file rows onto a decoded [`MessageRecord`].
pub(super) fn attach_attachments(
    db: &Connection,
    scope: &str,
    mut msg: MessageRecord,
) -> Result<MessageRecord, EngineError> {
    msg.attachments = message_files(db, scope, &msg)?;
    Ok(msg)
}

/// Replaces one participant's answer file rows with those its answer names.
pub(super) fn put_answer_files(
    db: &Connection,
    scope: &str,
    answer: &ParticipantAnswerFile,
) -> Result<(), EngineError> {
    remove_answer_files(db, scope, &answer.message_id, &answer.employee_id)?;
    if !answer.attachments.is_empty() {
        ensure_employee(db, scope, answer.employee_id.as_str())?;
    }
    for file in &answer.attachments {
        let mut values = vec![
            text(scope),
            text(&answer.message_id),
            text(answer.employee_id.as_str()),
            text(&file.id),
        ];
        values.extend(meta_values(file)?);
        let created_at = file.created_at.as_deref().or(answer.accepted_at.as_deref());
        values.push(text(created_at.unwrap_or_default()));
        put(
            db,
            "message_answer_attachments",
            &format!("organization_id, message_id, employee_id, id, {META}, created_at"),
            4,
            values,
        )?;
    }
    Ok(())
}

pub(super) fn remove_answer_files(
    db: &Connection,
    scope: &str,
    message_id: &str,
    employee_id: &Id,
) -> Result<(), EngineError> {
    run(
        db,
        "DELETE FROM message_answer_attachments \
         WHERE organization_id = ?1 AND message_id = ?2 AND employee_id = ?3",
        vec![text(scope), text(message_id), text(employee_id.as_str())],
    )
}

/// Every answer file row on a message, whoever answered (the message is going).
pub(super) fn remove_all_answer_files(
    db: &Connection,
    scope: &str,
    message_id: &str,
) -> Result<(), EngineError> {
    run(
        db,
        "DELETE FROM message_answer_attachments WHERE organization_id = ?1 AND message_id = ?2",
        vec![text(scope), text(message_id)],
    )
}

/// One participant's answer files in attachment-ID order.
pub(super) fn answer_files(
    db: &Connection,
    scope: &str,
    message_id: &str,
    employee_id: &Id,
) -> Result<Vec<MessageAttachmentRecord>, EngineError> {
    all(
        db,
        &format!(
            "SELECT id, {META}, created_at FROM message_answer_attachments \
             WHERE organization_id = ?1 AND message_id = ?2 AND employee_id = ?3 ORDER BY id"
        ),
        vec![text(scope), text(message_id), text(employee_id.as_str())],
        answer_row,
    )
}

/// The message or answer file with this attachment ID.
pub(super) fn message_file(
    db: &Connection,
    scope: &str,
    attachment_id: &str,
) -> Result<Option<BoundMessageFile>, EngineError> {
    fn on_message(row: &Row<'_>) -> rusqlite::Result<(String, String, MessageAttachmentRecord)> {
        let file = message_row(row)?;
        Ok((row.get(13)?, row.get(14)?, file))
    }
    let found = one(
        db,
        &format!(
            "SELECT a.id, a.content_ref, {}, a.message_id, m.created_at \
             FROM message_attachments a JOIN messages m \
               ON m.organization_id = a.organization_id AND m.id = a.message_id \
             WHERE a.organization_id = ?1 AND a.id = ?2",
            prefixed("a.")
        ),
        vec![text(scope), text(attachment_id)],
        on_message,
    )?;
    if let Some((message_id, created_at, mut file)) = found {
        file.created_at = Some(created_at);
        return Ok(Some(BoundMessageFile {
            message_id,
            answerer: None,
            file,
        }));
    }
    fn on_answer(row: &Row<'_>) -> rusqlite::Result<(String, String, MessageAttachmentRecord)> {
        Ok((row.get(13)?, row.get(14)?, answer_row(row)?))
    }
    let found = one(
        db,
        &format!(
            "SELECT id, {META}, created_at, message_id, employee_id \
             FROM message_answer_attachments WHERE organization_id = ?1 AND id = ?2"
        ),
        vec![text(scope), text(attachment_id)],
        on_answer,
    )?;
    Ok(match found {
        Some((message_id, answerer, file)) => Some(BoundMessageFile {
            message_id,
            answerer: Some(
                Id::parse(&answerer).map_err(|_| EngineError::new("a corrupt answer file row"))?,
            ),
            file,
        }),
        None => None,
    })
}

/// [`META`] with every column qualified by `prefix`.
fn prefixed(prefix: &str) -> String {
    META.split(',')
        .map(|column| format!("{prefix}{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}
