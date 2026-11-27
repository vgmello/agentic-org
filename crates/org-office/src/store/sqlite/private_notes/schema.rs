//! The private notes database's schema: creating it from `private-notes-schema.sql`,
//! recognizing it, and the one-time upgrade of a `TASK-50` task-only notes database
//! (`private_task_notes`) to the `TASK-111` layout (`private_notes` + `private_note_tags`).

use std::collections::BTreeSet;

use rusqlite::{Connection, TransactionBehavior};

use crate::error::OfficeError;

/// Verbatim DDL for the isolated private notes database (`doc-79`, `doc-93`).
pub(crate) const DDL: &str =
    include_str!("../../../../../../docs/design/organization-data/private-notes-schema.sql");

/// Everything after this line of [`DDL`] creates triggers only.
const TRIGGERS_MARKER: &str = "-- Integrity triggers.";

const CURRENT_TABLES: [&str; 5] = [
    "private_note_authors",
    "private_note_command_receipts",
    "private_note_tags",
    "private_note_tasks",
    "private_notes",
];

const TASK50_TABLES: [&str; 4] = [
    "private_note_authors",
    "private_note_command_receipts",
    "private_note_tasks",
    "private_task_notes",
];

/// The triggers the `TASK-50` schema created, all of which a `TASK-50` database holds.
const TASK50_TRIGGERS: [&str; 10] = [
    "private_note_pending_publication",
    "private_note_tasks_no_undelete",
    "private_notes_initial_state",
    "private_notes_keep_tombstone",
    "private_notes_no_replace",
    "private_notes_update_contract",
    "private_receipt_matches_author",
    "private_receipts_finish_once",
    "private_receipts_no_delete",
    "private_receipts_no_replace",
];

/// Copies a `TASK-50` database's rows aside and drops its tables (children first, so the
/// implicit deletes of `DROP TABLE` never break a foreign key; triggers go with their table).
const TASK50_SAVE: &str = "
CREATE TEMP TABLE task50_note_authors AS
  SELECT organization_id, author_id FROM main.private_note_authors;
CREATE TEMP TABLE task50_note_tasks AS
  SELECT organization_id, task_id, is_deleted FROM main.private_note_tasks;
CREATE TEMP TABLE task50_notes AS
  SELECT organization_id, id, schema_version, revision, task_id, author_id, body_markdown,
         created_at, updated_at, deleted_at
  FROM main.private_task_notes;
CREATE TEMP TABLE task50_receipts AS
  SELECT organization_id, author_id, operation_id, request_sha256, command, note_id,
         result_revision, state, shared_comment_id, error_code, created_at, finished_at
  FROM main.private_note_command_receipts;
DROP TABLE main.private_note_command_receipts;
DROP TABLE main.private_task_notes;
DROP TABLE main.private_note_tasks;
DROP TABLE main.private_note_authors;";

/// Restores the saved rows into the new tables, before their triggers exist: a carried-over
/// note is a `note` with no title, why, alternatives or tags.
const TASK50_RESTORE: &str = "
INSERT INTO main.private_note_authors (organization_id, author_id)
  SELECT organization_id, author_id FROM temp.task50_note_authors;
INSERT INTO main.private_note_tasks (organization_id, task_id, is_deleted)
  SELECT organization_id, task_id, is_deleted FROM temp.task50_note_tasks;
INSERT INTO main.private_notes
  (organization_id, id, schema_version, revision, task_id, project_id, author_id, kind,
   title, why, alternatives, body_markdown, created_at, updated_at, deleted_at)
  SELECT organization_id, id, schema_version, revision, task_id, NULL, author_id, 'note',
         NULL, NULL, NULL, body_markdown, created_at, updated_at, deleted_at
  FROM temp.task50_notes;
INSERT INTO main.private_note_command_receipts
  (organization_id, author_id, operation_id, request_sha256, command, note_id,
   result_revision, state, shared_comment_id, error_code, created_at, finished_at)
  SELECT organization_id, author_id, operation_id, request_sha256, command, note_id,
         result_revision, state, shared_comment_id, error_code, created_at, finished_at
  FROM temp.task50_receipts;
DROP TABLE temp.task50_receipts;
DROP TABLE temp.task50_notes;
DROP TABLE temp.task50_note_tasks;
DROP TABLE temp.task50_note_authors;";

/// What a private notes database file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Nothing yet: a new file.
    Empty,
    /// This schema, complete.
    Current,
    /// The `TASK-50` task-only notes schema, upgraded when the store opens it.
    Task50,
}

/// The tables part and the triggers part of [`DDL`], split at the marker's comment line.
fn ddl_parts() -> Result<(&'static str, &'static str), OfficeError> {
    let (tables, rest) = DDL.split_once(TRIGGERS_MARKER).ok_or(OfficeError::Invalid(
        "the private notes schema lacks its trigger section",
    ))?;
    let triggers = rest.split_once('\n').map_or("", |(_, after)| after);
    Ok((tables, triggers))
}

/// The name of every trigger [`DDL`] creates.
fn current_triggers() -> Vec<&'static str> {
    DDL.split("CREATE TRIGGER ")
        .skip(1)
        .filter_map(|rest| rest.split_whitespace().next())
        .collect()
}

/// The names of the database's own objects of `kind` (`table`, `trigger` or `view`).
fn object_names(conn: &Connection, kind: &str) -> Result<BTreeSet<String>, OfficeError> {
    let mut statement = conn
        .prepare("SELECT name FROM main.sqlite_master WHERE type = ?1 AND name NOT GLOB 'sqlite_*'")
        .map_err(OfficeError::storage)?;
    let names = statement
        .query_map([kind], |row| row.get(0))
        .map_err(OfficeError::storage)?
        .collect::<Result<_, _>>()
        .map_err(OfficeError::storage)?;
    Ok(names)
}

/// Whether `found` holds exactly the `expected` names, no more and no fewer.
fn exactly(found: &BTreeSet<String>, expected: &[&str]) -> bool {
    found.len() == expected.len() && expected.iter().all(|name| found.contains(*name))
}

/// Recognizes the database's layout by exactly the tables and triggers each schema creates
/// (neither creates a view); `None` for anything else, such as a foreign, mixed or partially
/// created schema.
pub(crate) fn layout(conn: &Connection) -> Result<Option<Layout>, OfficeError> {
    if !object_names(conn, "view")?.is_empty() {
        return Ok(None);
    }
    let tables = object_names(conn, "table")?;
    let triggers = object_names(conn, "trigger")?;
    Ok(if tables.is_empty() && triggers.is_empty() {
        Some(Layout::Empty)
    } else if exactly(&tables, &CURRENT_TABLES) && exactly(&triggers, &current_triggers()) {
        Some(Layout::Current)
    } else if exactly(&tables, &TASK50_TABLES) && exactly(&triggers, &TASK50_TRIGGERS) {
        Some(Layout::Task50)
    } else {
        None
    })
}

/// Creates the schema in a new file, upgrades a `TASK-50` one, and accepts a current one, all
/// in one immediate transaction; anything else is refused.
pub(super) fn ensure_schema(conn: &mut Connection) -> Result<(), OfficeError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(OfficeError::storage)?;
    let (tables, triggers) = ddl_parts()?;
    match layout(&tx)? {
        Some(Layout::Current) => return Ok(()),
        Some(Layout::Empty) => {
            tx.execute_batch(tables).map_err(OfficeError::storage)?;
        }
        Some(Layout::Task50) => {
            tx.execute_batch(TASK50_SAVE)
                .map_err(OfficeError::storage)?;
            tx.execute_batch(tables).map_err(OfficeError::storage)?;
            tx.execute_batch(TASK50_RESTORE)
                .map_err(OfficeError::storage)?;
        }
        None => {
            return Err(OfficeError::Invalid(
                "corrupt or partially initialized private notes database",
            ))
        }
    }
    tx.execute_batch(triggers).map_err(OfficeError::storage)?;
    let dangling: i64 = tx
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(OfficeError::storage)?;
    if dangling != 0 || layout(&tx)? != Some(Layout::Current) {
        return Err(OfficeError::Invalid(
            "the private notes database did not reach its schema",
        ));
    }
    tx.commit().map_err(OfficeError::storage)
}
