//! `tasks.notes.list` against SQLite: the caller's visible active notes matching every filter,
//! newest first, one keyset page at a time (`TASK-111` ADEC-2, `ADR-0031`).

use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection};

use org_office::private_notes::{NotePage, NoteQuery};
use org_office::OfficeError;

use super::rows::{note_row, with_tags, NOTE_COLUMNS};

/// A `WHERE` clause built only from fixed fragments, with its bound values.
struct Filter {
    sql: String,
    values: Vec<Value>,
}

impl Filter {
    fn and(&mut self, clause: &str, value: &str) {
        self.sql.push_str(" AND ");
        self.sql.push_str(clause);
        self.values.push(Value::Text(value.to_owned()));
    }
}

pub(super) fn list(conn: &Connection, query: &NoteQuery<'_>) -> Result<NotePage, OfficeError> {
    let Some(author) = query.visible_author() else {
        return Ok(NotePage::default());
    };
    let mut filter = Filter {
        sql: "organization_id = ? AND deleted_at IS NULL".to_owned(),
        values: vec![Value::Text(query.org_id.to_owned())],
    };
    if let Some(author) = author {
        filter.and("author_id = ?", author);
    }
    if let Some(task_id) = query.task_id {
        filter.and("task_id = ?", task_id);
    }
    if let Some(project_id) = query.project_id {
        filter.and("project_id = ?", project_id);
    }
    if let Some(kind) = query.kind {
        filter.and("kind = ?", kind.as_str());
    }
    if let Some(since) = query.since {
        filter.and("updated_at >= ?", since);
    }
    if let Some(tag) = query.tag {
        filter.and(
            "EXISTS (SELECT 1 FROM private_note_tags t WHERE t.organization_id = \
             private_notes.organization_id AND t.note_id = private_notes.id AND t.tag = ?)",
            tag,
        );
    }
    let total: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM private_notes WHERE {}", filter.sql),
            params_from_iter(filter.values.iter()),
            |row| row.get(0),
        )
        .map_err(OfficeError::storage)?;
    if let Some(after) = query.after {
        filter
            .sql
            .push_str(" AND (updated_at < ? OR (updated_at = ? AND id < ?))");
        filter.values.extend([
            Value::Text(after.updated_at.clone()),
            Value::Text(after.updated_at.clone()),
            Value::Text(after.id.clone()),
        ]);
    }
    let fetch = i64::try_from(query.limit.saturating_add(1)).unwrap_or(i64::MAX);
    filter.values.push(Value::Integer(fetch));
    let mut statement = conn
        .prepare(&format!(
            "SELECT {NOTE_COLUMNS} FROM private_notes WHERE {}
             ORDER BY updated_at DESC, id DESC LIMIT ?",
            filter.sql
        ))
        .map_err(OfficeError::storage)?;
    let rows = statement
        .query_map(params_from_iter(filter.values.iter()), note_row)
        .map_err(OfficeError::storage)?;
    let mut candidates = Vec::new();
    for row in rows {
        candidates.push(with_tags(conn, row.map_err(OfficeError::storage)?)?);
    }
    Ok(NotePage::from_candidates(
        candidates,
        u64::try_from(total).unwrap_or(0),
        query.limit,
    ))
}
