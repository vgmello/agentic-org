//! Atomic per-task checkpoint upsert and zero-N+1 batch latest-note lookup (`TASK-220`, `ADR-0031`).

use std::collections::BTreeMap;

use org_proto::private_notes::PRIVATE_NOTE_CHECKPOINT_TAG;
use org_proto::{ApiError, ErrorCode};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};

use org_office::private_notes::{
    check_receipt_reuse, monotonic_ts, next_revision, replayed_note, Accepted, CreateNoteCmd,
    PrivateNoteRecord,
};
use org_office::OfficeError;

use super::rows::{
    flush_authoritative_db, has_pending_publish, insert_completed_receipt, insert_note, load_note,
    load_receipt, note_row, update_content, with_tags, Expected, ReceiptRow, NOTE_COLUMNS,
};

fn find_active_checkpoint(
    tx: &Transaction<'_>,
    org_id: &str,
    author_id: &str,
    task_id: &str,
) -> Result<Option<PrivateNoteRecord>, OfficeError> {
    let row = tx
        .query_row(
            &format!(
                "SELECT {NOTE_COLUMNS} FROM private_notes n
                 WHERE n.organization_id = ?1 AND n.author_id = ?2 AND n.task_id = ?3
                   AND n.deleted_at IS NULL
                   AND EXISTS (
                     SELECT 1 FROM private_note_tags t
                     WHERE t.organization_id = n.organization_id
                       AND t.note_id = n.id
                       AND t.tag = ?4
                   )
                 ORDER BY n.updated_at DESC, n.id DESC
                 LIMIT 1"
            ),
            params![org_id, author_id, task_id, PRIVATE_NOTE_CHECKPOINT_TAG],
            note_row,
        )
        .optional()
        .map_err(OfficeError::storage)?;
    row.map(|r| with_tags(tx, r)).transpose()
}

fn replay_checkpoint(
    tx: Transaction<'_>,
    db_path: &std::path::Path,
    cmd: &CreateNoteCmd<'_>,
    receipt: &ReceiptRow,
) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
    if receipt.command != "create" && receipt.command != "edit" {
        return Ok(Err(ApiError::new(
            ErrorCode::OperationReused,
            "command_id reused with different private note request",
        )));
    }
    if let Err(e) = check_receipt_reuse(
        &receipt.command,
        &receipt.request_sha256,
        &receipt.command,
        cmd.request_sha256,
    ) {
        return Ok(Err(e));
    }
    let current = load_note(&tx, cmd.org_id, &receipt.note_id)?
        .ok_or(OfficeError::Invalid("replayed private note missing"))?;
    let replayed = replayed_note(
        &current,
        receipt.result_revision,
        receipt.time(),
        Accepted::Created(cmd.content),
    );
    drop(tx);
    flush_authoritative_db(db_path)?;
    Ok(Ok(replayed))
}

fn update_existing_checkpoint(
    tx: Transaction<'_>,
    db_path: &std::path::Path,
    cmd: &CreateNoteCmd<'_>,
    existing: &PrivateNoteRecord,
) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
    if has_pending_publish(&tx, cmd.org_id, &existing.id)? {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "resolve pending publication before updating checkpoint note",
        )));
    }
    let next_rev = match next_revision(existing.revision) {
        Ok(rev) => rev,
        Err(e) => return Ok(Err(e)),
    };
    let updated_at = monotonic_ts(cmd.now, &existing.updated_at);
    let edited = existing.with_content(cmd.content, next_rev, &updated_at);
    let expected = Expected {
        author_id: cmd.author_id,
        revision: existing.revision,
    };
    update_content(&tx, expected, &edited)?;
    let key = (
        cmd.org_id,
        cmd.author_id,
        cmd.operation_id,
        cmd.request_sha256,
    );
    insert_completed_receipt(&tx, key, "edit", &existing.id, next_rev, &updated_at)?;
    let saved = load_note(&tx, cmd.org_id, &existing.id)?
        .ok_or(OfficeError::Invalid("written private note missing"))?;
    tx.commit().map_err(OfficeError::storage)?;
    flush_authoritative_db(db_path)?;
    Ok(Ok(saved))
}

pub(super) fn upsert_checkpoint(
    tx: Transaction<'_>,
    db_path: &std::path::Path,
    cmd: CreateNoteCmd<'_>,
) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
    if let Some(receipt) = load_receipt(&tx, cmd.org_id, cmd.author_id, cmd.operation_id)? {
        return replay_checkpoint(tx, db_path, &cmd, &receipt);
    }
    let Some(task_id) = cmd.task_id else {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "a checkpoint note must belong to a task",
        )));
    };
    if let Some(existing) = find_active_checkpoint(&tx, cmd.org_id, cmd.author_id, task_id)? {
        return update_existing_checkpoint(tx, db_path, &cmd, &existing);
    }
    if load_note(&tx, cmd.org_id, cmd.note_id)?.is_some() {
        return Ok(Err(ApiError::new(
            ErrorCode::AlreadyExists,
            format!("private note `{}` already exists", cmd.note_id),
        )));
    }
    tx.execute(
        "INSERT OR IGNORE INTO private_note_authors (organization_id, author_id) VALUES (?1, ?2)",
        params![cmd.org_id, cmd.author_id],
    )
    .map_err(OfficeError::storage)?;
    tx.execute(
        "INSERT OR IGNORE INTO private_note_tasks (organization_id, task_id, is_deleted) VALUES (?1, ?2, 0)",
        params![cmd.org_id, task_id],
    )
    .map_err(OfficeError::storage)?;
    insert_note(&tx, &PrivateNoteRecord::new_active(&cmd))?;
    let key = (
        cmd.org_id,
        cmd.author_id,
        cmd.operation_id,
        cmd.request_sha256,
    );
    insert_completed_receipt(&tx, key, "create", cmd.note_id, 1, cmd.now)?;
    let saved = load_note(&tx, cmd.org_id, cmd.note_id)?
        .ok_or(OfficeError::Invalid("written private note missing"))?;
    tx.commit().map_err(OfficeError::storage)?;
    flush_authoritative_db(db_path)?;
    Ok(Ok(saved))
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Zero-N+1 batch lookup of `author_id`'s latest active private note for each task in `task_ids`.
/// Strictly filters `author_id = ?2` (`ADR-0031`) and fetches tags in one second batch query.
pub(super) fn latest_author_notes_for_tasks(
    conn: &Connection,
    org_id: &str,
    author_id: &str,
    task_ids: &[String],
) -> Result<BTreeMap<String, PrivateNoteRecord>, OfficeError> {
    if task_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut values = Vec::with_capacity(2 + task_ids.len());
    values.push(Value::Text(org_id.to_owned()));
    values.push(Value::Text(author_id.to_owned()));
    values.extend(task_ids.iter().cloned().map(Value::Text));
    let sql = format!(
        "SELECT {NOTE_COLUMNS} FROM (
            SELECT {NOTE_COLUMNS},
                   ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY updated_at DESC, id DESC) AS rn
            FROM private_notes
            WHERE organization_id = ? AND author_id = ? AND deleted_at IS NULL
              AND task_id IN ({})
         ) WHERE rn = 1",
        placeholders(task_ids.len())
    );
    let mut stmt = conn.prepare(&sql).map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map(params_from_iter(values.iter()), note_row)
        .map_err(OfficeError::storage)?;
    let mut by_note_id: BTreeMap<String, PrivateNoteRecord> = BTreeMap::new();
    for row in rows {
        let record = row.map_err(OfficeError::storage)?;
        by_note_id.insert(record.id.clone(), record);
    }
    if by_note_id.is_empty() {
        return Ok(BTreeMap::new());
    }
    attach_tags_batch(conn, org_id, &mut by_note_id)?;
    let mut by_task = BTreeMap::new();
    for (_, record) in by_note_id {
        if let Some(tid) = record.task_id.clone() {
            by_task.insert(tid, record);
        }
    }
    Ok(by_task)
}

fn attach_tags_batch(
    conn: &Connection,
    org_id: &str,
    notes: &mut BTreeMap<String, PrivateNoteRecord>,
) -> Result<(), OfficeError> {
    let mut values = Vec::with_capacity(1 + notes.len());
    values.push(Value::Text(org_id.to_owned()));
    values.extend(notes.keys().cloned().map(Value::Text));
    let sql = format!(
        "SELECT note_id, tag FROM private_note_tags
         WHERE organization_id = ? AND note_id IN ({})
         ORDER BY note_id, tag",
        placeholders(notes.len())
    );
    let mut stmt = conn.prepare(&sql).map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map(params_from_iter(values.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(OfficeError::storage)?;
    for row in rows {
        let (note_id, tag) = row.map_err(OfficeError::storage)?;
        if let Some(note) = notes.get_mut(&note_id) {
            note.tags.push(tag);
        }
    }
    Ok(())
}
