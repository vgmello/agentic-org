//! Isolated SQLite adapter for per-author private task notes (`TASK-50`, `L8`, `ADR-0016`,
//! `ADR-0031`, `doc-79` PN-01..06).
//!
//! Opens `<runtime-root>/<office-id>/private/task-notes.db` using `private-notes-schema.sql`
//! verbatim, completely outside `durable/`, `office.db`, and Git.

use std::path::Path;
use std::sync::Mutex;

use org_proto::{ApiError, ErrorCode};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::OfficeError;
use crate::private_fs::{absolute, private_dir, private_file, tighten_dir};
use crate::private_notes::{
    non_author_mutation_error, parse_receipt_error_code, BeginPublishCmd, CreateNoteCmd,
    DeleteNoteCmd, EditNoteCmd, PrivateNoteRecord, PrivateNotesStore, PublishAttempt,
};

/// Verbatim DDL for the isolated private task notes database (`doc-79`, `doc-93`).
const PRIVATE_NOTES_DDL: &str =
    include_str!("../../../../../docs/design/organization-data/private-notes-schema.sql");

/// SQLite-backed isolated private notes database behind a single serialized writer lock (`ADR-0002`).
pub struct SqlitePrivateNotesStore {
    conn: Mutex<Connection>,
}

impl SqlitePrivateNotesStore {
    /// Opens (or initializes) `<runtime-root>/<office-id>/private/task-notes.db` with owner-only
    /// directory and file permissions (`ADR-0016`, `PN-04`).
    pub fn open(runtime_root: &Path, office_id: &str) -> Result<Self, OfficeError> {
        let office_dir = runtime_root.join(office_id);
        private_dir(&office_dir).map_err(OfficeError::storage)?;
        let private_folder = office_dir.join("private");
        private_dir(&private_folder).map_err(OfficeError::storage)?;
        tighten_dir(&private_folder).map_err(OfficeError::storage)?;

        let db_path =
            absolute(private_folder.join("task-notes.db")).map_err(OfficeError::storage)?;
        // Pre-create with 0o600 before SQLite opens it (no 0o644 creation window).
        drop(private_file(&db_path).map_err(OfficeError::storage)?);
        let conn = Connection::open(&db_path).map_err(OfficeError::storage)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;
             PRAGMA recursive_triggers = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(OfficeError::storage)?;

        ensure_schema_initialized(&conn)?;
        tighten_db_and_sidecars(&db_path)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, OfficeError> {
        self.conn
            .lock()
            .map_err(|_| OfficeError::Invalid("private notes sqlite connection poisoned"))
    }
}

fn ensure_schema_initialized(conn: &Connection) -> Result<(), OfficeError> {
    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN \
             ('private_note_authors', 'private_note_tasks', 'private_task_notes', 'private_note_command_receipts')",
            [],
            |row| row.get(0),
        )
        .map_err(OfficeError::storage)?;
    let trigger_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger'",
            [],
            |row| row.get(0),
        )
        .map_err(OfficeError::storage)?;
    if table_count == 4 && trigger_count >= 10 {
        return Ok(());
    }
    if table_count != 0 || trigger_count != 0 {
        return Err(OfficeError::Invalid(
            "corrupt or partially initialized private task notes database",
        ));
    }
    conn.execute_batch("BEGIN IMMEDIATE;")
        .map_err(OfficeError::storage)?;
    if let Err(err) = conn.execute_batch(PRIVATE_NOTES_DDL) {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(OfficeError::storage(err));
    }
    conn.execute_batch("COMMIT;")
        .map_err(OfficeError::storage)?;
    Ok(())
}

#[cfg(unix)]
fn tighten_file(path: &Path) -> Result<(), OfficeError> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(OfficeError::storage)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten_file(_path: &Path) -> Result<(), OfficeError> {
    Err(OfficeError::Invalid(
        "private task notes owner-only file permissions are unsupported on this OS",
    ))
}

fn tighten_db_and_sidecars(db_path: &Path) -> Result<(), OfficeError> {
    tighten_file(db_path)?;
    for suffix in ["-wal", "-shm"] {
        let mut s = db_path.as_os_str().to_os_string();
        s.push(suffix);
        tighten_file(Path::new(&s))?;
    }
    Ok(())
}

fn note_row(row: &Row<'_>) -> rusqlite::Result<PrivateNoteRecord> {
    let schema_version: i64 = row.get(2)?;
    let revision: i64 = row.get(3)?;
    Ok(PrivateNoteRecord {
        organization_id: row.get(0)?,
        id: row.get(1)?,
        schema_version: schema_version as u64,
        revision: revision as u64,
        task_id: row.get(4)?,
        author_id: row.get(5)?,
        body_markdown: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        deleted_at: row.get(9)?,
    })
}

struct ReceiptRow {
    request_sha256: String,
    command: String,
    note_id: String,
    result_revision: u64,
    state: String,
    shared_comment_id: Option<String>,
    error_code: Option<String>,
    created_at: String,
    finished_at: Option<String>,
}

fn load_receipt(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    author_id: &str,
    operation_id: &str,
) -> Result<Option<ReceiptRow>, OfficeError> {
    tx.query_row(
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

fn reconstruct_accepted_note(
    current: &PrivateNoteRecord,
    receipt: &ReceiptRow,
    accepted_body: Option<&str>,
) -> PrivateNoteRecord {
    let ts = receipt
        .finished_at
        .clone()
        .unwrap_or_else(|| receipt.created_at.clone());
    PrivateNoteRecord {
        organization_id: current.organization_id.clone(),
        id: current.id.clone(),
        schema_version: current.schema_version,
        revision: receipt.result_revision,
        task_id: current.task_id.clone(),
        author_id: current.author_id.clone(),
        body_markdown: accepted_body.map(str::to_owned),
        created_at: current.created_at.clone(),
        updated_at: ts.clone(),
        deleted_at: accepted_body.is_none().then_some(ts),
    }
}

fn load_note(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    note_id: &str,
) -> Result<Option<PrivateNoteRecord>, OfficeError> {
    tx.query_row(
        "SELECT organization_id, id, schema_version, revision, task_id, author_id,
                body_markdown, created_at, updated_at, deleted_at
         FROM private_task_notes
         WHERE organization_id = ?1 AND id = ?2",
        params![org_id, note_id],
        note_row,
    )
    .optional()
    .map_err(OfficeError::storage)
}

fn has_pending_publish(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    note_id: &str,
) -> Result<bool, OfficeError> {
    tx.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM private_note_command_receipts
            WHERE organization_id = ?1 AND note_id = ?2 AND state = 'pending'
         )",
        params![org_id, note_id],
        |row| row.get(0),
    )
    .map_err(OfficeError::storage)
}

fn check_receipt_reuse(
    existing: &ReceiptRow,
    expected_cmd: &str,
    request_sha256: &str,
) -> Result<(), ApiError> {
    if existing.command != expected_cmd || existing.request_sha256 != request_sha256 {
        return Err(ApiError::new(
            ErrorCode::OperationReused,
            "command_id reused with different private note request",
        ));
    }
    Ok(())
}

fn monotonic_ts(now: &str, prev: &str) -> String {
    if now >= prev {
        now.to_owned()
    } else {
        prev.to_owned()
    }
}

fn validate_author_mutation(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    note_id: &str,
    caller_id: &str,
    caller_is_person: bool,
    expected_revision: u64,
    pending_msg: &str,
) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
    let Some(existing) = load_note(tx, org_id, note_id)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "private note not found",
        )));
    };
    if existing.deleted_at.is_some() {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "private note not found",
        )));
    }
    if existing.author_id != caller_id {
        return Ok(Err(non_author_mutation_error(caller_is_person)));
    }
    if has_pending_publish(tx, org_id, note_id)? {
        return Ok(Err(ApiError::new(ErrorCode::Conflict, pending_msg)));
    }
    if existing.revision != expected_revision {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            format!(
                "stale expected_revision {expected_revision}: note is at revision {}",
                existing.revision
            ),
        )));
    }
    Ok(Ok(existing))
}

impl PrivateNotesStore for SqlitePrivateNotesStore {
    fn create_note(
        &self,
        cmd: CreateNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(OfficeError::storage)?;

        if let Some(existing) = load_receipt(&tx, cmd.org_id, cmd.author_id, cmd.operation_id)? {
            if let Err(e) = check_receipt_reuse(&existing, "create", cmd.request_sha256) {
                return Ok(Err(e));
            }
            let current = load_note(&tx, cmd.org_id, &existing.note_id)?
                .ok_or_else(|| OfficeError::Invalid("replayed private note missing"))?;
            return Ok(Ok(reconstruct_accepted_note(
                &current,
                &existing,
                Some(cmd.body_markdown),
            )));
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
            params![cmd.org_id, cmd.task_id],
        )
        .map_err(OfficeError::storage)?;
        tx.execute(
            "INSERT INTO private_task_notes
             (organization_id, id, schema_version, revision, task_id, author_id,
              body_markdown, created_at, updated_at, deleted_at)
             VALUES (?1, ?2, 1, 1, ?3, ?4, ?5, ?6, ?6, NULL)",
            params![
                cmd.org_id,
                cmd.note_id,
                cmd.task_id,
                cmd.author_id,
                cmd.body_markdown,
                cmd.now,
            ],
        )
        .map_err(OfficeError::storage)?;
        tx.execute(
            "INSERT INTO private_note_command_receipts
             (organization_id, author_id, operation_id, request_sha256, command, note_id,
              result_revision, state, shared_comment_id, error_code, created_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, 'create', ?5, 1, 'completed', NULL, NULL, ?6, ?6)",
            params![
                cmd.org_id,
                cmd.author_id,
                cmd.operation_id,
                cmd.request_sha256,
                cmd.note_id,
                cmd.now,
            ],
        )
        .map_err(OfficeError::storage)?;

        let record = load_note(&tx, cmd.org_id, cmd.note_id)?
            .ok_or_else(|| OfficeError::Invalid("inserted private note missing"))?;
        tx.commit().map_err(OfficeError::storage)?;
        Ok(Ok(record))
    }

    fn edit_note(
        &self,
        cmd: EditNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(OfficeError::storage)?;

        if let Some(existing) = load_receipt(&tx, cmd.org_id, cmd.caller_id, cmd.operation_id)? {
            if let Err(e) = check_receipt_reuse(&existing, "edit", cmd.request_sha256) {
                return Ok(Err(e));
            }
            let current = load_note(&tx, cmd.org_id, &existing.note_id)?
                .ok_or_else(|| OfficeError::Invalid("replayed private note missing"))?;
            return Ok(Ok(reconstruct_accepted_note(
                &current,
                &existing,
                Some(cmd.body_markdown),
            )));
        }

        let existing = match validate_author_mutation(
            &tx,
            cmd.org_id,
            cmd.note_id,
            cmd.caller_id,
            cmd.caller_is_person,
            cmd.expected_revision,
            "resolve pending publication before editing private note",
        )? {
            Ok(n) => n,
            Err(e) => return Ok(Err(e)),
        };
        if existing.revision >= i64::MAX as u64 {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "private note revision limit reached",
            )));
        }

        let updated_at = monotonic_ts(cmd.now, &existing.updated_at);
        let next_rev = existing.revision + 1;
        tx.execute(
            "UPDATE private_task_notes
             SET revision = ?3, body_markdown = ?4, updated_at = ?5
             WHERE organization_id = ?1 AND id = ?2",
            params![
                cmd.org_id,
                cmd.note_id,
                next_rev as i64,
                cmd.body_markdown,
                updated_at,
            ],
        )
        .map_err(OfficeError::storage)?;
        tx.execute(
            "INSERT INTO private_note_command_receipts
             (organization_id, author_id, operation_id, request_sha256, command, note_id,
              result_revision, state, shared_comment_id, error_code, created_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, 'edit', ?5, ?6, 'completed', NULL, NULL, ?7, ?7)",
            params![
                cmd.org_id,
                cmd.caller_id,
                cmd.operation_id,
                cmd.request_sha256,
                cmd.note_id,
                next_rev as i64,
                updated_at,
            ],
        )
        .map_err(OfficeError::storage)?;

        let record = load_note(&tx, cmd.org_id, cmd.note_id)?
            .ok_or_else(|| OfficeError::Invalid("updated private note missing"))?;
        tx.commit().map_err(OfficeError::storage)?;
        Ok(Ok(record))
    }

    fn delete_note(
        &self,
        cmd: DeleteNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(OfficeError::storage)?;

        if let Some(existing) = load_receipt(&tx, cmd.org_id, cmd.caller_id, cmd.operation_id)? {
            if let Err(e) = check_receipt_reuse(&existing, "delete", cmd.request_sha256) {
                return Ok(Err(e));
            }
            let current = load_note(&tx, cmd.org_id, &existing.note_id)?
                .ok_or_else(|| OfficeError::Invalid("replayed private note missing"))?;
            return Ok(Ok(reconstruct_accepted_note(&current, &existing, None)));
        }

        let existing = match validate_author_mutation(
            &tx,
            cmd.org_id,
            cmd.note_id,
            cmd.caller_id,
            cmd.caller_is_person,
            cmd.expected_revision,
            "resolve pending publication before deleting private note",
        )? {
            Ok(n) => n,
            Err(e) => return Ok(Err(e)),
        };
        if existing.revision >= i64::MAX as u64 {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "private note revision limit reached",
            )));
        }

        let deleted_at = monotonic_ts(cmd.now, &existing.updated_at);
        let next_rev = existing.revision + 1;
        tx.execute(
            "UPDATE private_task_notes
             SET revision = ?3, body_markdown = NULL, updated_at = ?4, deleted_at = ?4
             WHERE organization_id = ?1 AND id = ?2",
            params![cmd.org_id, cmd.note_id, next_rev as i64, deleted_at],
        )
        .map_err(OfficeError::storage)?;
        tx.execute(
            "INSERT INTO private_note_command_receipts
             (organization_id, author_id, operation_id, request_sha256, command, note_id,
              result_revision, state, shared_comment_id, error_code, created_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, 'delete', ?5, ?6, 'completed', NULL, NULL, ?7, ?7)",
            params![
                cmd.org_id,
                cmd.caller_id,
                cmd.operation_id,
                cmd.request_sha256,
                cmd.note_id,
                next_rev as i64,
                deleted_at,
            ],
        )
        .map_err(OfficeError::storage)?;

        let record = load_note(&tx, cmd.org_id, cmd.note_id)?
            .ok_or_else(|| OfficeError::Invalid("deleted private note tombstone missing"))?;
        tx.commit().map_err(OfficeError::storage)?;
        Ok(Ok(record))
    }

    fn get_note(
        &self,
        org_id: &str,
        caller_id: &str,
        caller_is_person: bool,
        note_id: &str,
    ) -> Result<Option<PrivateNoteRecord>, OfficeError> {
        let conn = self.lock()?;
        let row = if caller_is_person {
            conn.query_row(
                "SELECT organization_id, id, schema_version, revision, task_id, author_id,
                        body_markdown, created_at, updated_at, deleted_at
                 FROM private_task_notes
                 WHERE organization_id = ?1 AND id = ?2 AND deleted_at IS NULL",
                params![org_id, note_id],
                note_row,
            )
            .optional()
            .map_err(OfficeError::storage)?
        } else {
            conn.query_row(
                "SELECT organization_id, id, schema_version, revision, task_id, author_id,
                        body_markdown, created_at, updated_at, deleted_at
                 FROM private_task_notes
                 WHERE organization_id = ?1 AND id = ?2 AND author_id = ?3 AND deleted_at IS NULL",
                params![org_id, note_id, caller_id],
                note_row,
            )
            .optional()
            .map_err(OfficeError::storage)?
        };
        Ok(row)
    }

    fn list_notes(
        &self,
        org_id: &str,
        caller_id: &str,
        caller_is_person: bool,
        task_id: &str,
        author_filter: Option<&str>,
    ) -> Result<Vec<PrivateNoteRecord>, OfficeError> {
        if !caller_is_person {
            if let Some(filter) = author_filter {
                if filter != caller_id {
                    return Ok(Vec::new());
                }
            }
        }
        let conn = self.lock()?;
        let effective_author = if caller_is_person {
            author_filter
        } else {
            Some(caller_id)
        };
        let mut out = Vec::new();
        if let Some(author) = effective_author {
            let mut stmt = conn
                .prepare(
                    "SELECT organization_id, id, schema_version, revision, task_id, author_id,
                            body_markdown, created_at, updated_at, deleted_at
                     FROM private_task_notes
                     WHERE organization_id = ?1 AND task_id = ?2 AND author_id = ?3 AND deleted_at IS NULL
                     ORDER BY updated_at ASC, id ASC",
                )
                .map_err(OfficeError::storage)?;
            let rows = stmt
                .query_map(params![org_id, task_id, author], note_row)
                .map_err(OfficeError::storage)?;
            for item in rows {
                out.push(item.map_err(OfficeError::storage)?);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT organization_id, id, schema_version, revision, task_id, author_id,
                            body_markdown, created_at, updated_at, deleted_at
                     FROM private_task_notes
                     WHERE organization_id = ?1 AND task_id = ?2 AND deleted_at IS NULL
                     ORDER BY updated_at ASC, id ASC",
                )
                .map_err(OfficeError::storage)?;
            let rows = stmt
                .query_map(params![org_id, task_id], note_row)
                .map_err(OfficeError::storage)?;
            for item in rows {
                out.push(item.map_err(OfficeError::storage)?);
            }
        }
        Ok(out)
    }

    fn begin_publish(
        &self,
        cmd: BeginPublishCmd<'_>,
    ) -> Result<Result<PublishAttempt, ApiError>, OfficeError> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(OfficeError::storage)?;

        if let Some(existing) = load_receipt(&tx, cmd.org_id, cmd.caller_id, cmd.operation_id)? {
            if let Err(e) = check_receipt_reuse(&existing, "publish", cmd.request_sha256) {
                return Ok(Err(e));
            }
            let shared_comment_id = existing.shared_comment_id.unwrap_or_default();
            let note = load_note(&tx, cmd.org_id, &existing.note_id)?
                .ok_or_else(|| OfficeError::Invalid("replayed publish note missing"))?;
            return match existing.state.as_str() {
                "completed" => Ok(Ok(PublishAttempt::AlreadyCompleted {
                    note,
                    result_revision: existing.result_revision,
                    shared_comment_id,
                })),
                "pending" => Ok(Ok(PublishAttempt::Pending {
                    note,
                    shared_comment_id,
                })),
                _ => Ok(Err(ApiError::new(
                    parse_receipt_error_code(existing.error_code.as_deref()),
                    "private note publish was rejected",
                ))),
            };
        }

        let note = match validate_author_mutation(
            &tx,
            cmd.org_id,
            cmd.note_id,
            cmd.caller_id,
            cmd.caller_is_person,
            cmd.expected_revision,
            "another publication of this private note is already pending",
        )? {
            Ok(n) => n,
            Err(e) => return Ok(Err(e)),
        };

        let comment_used: bool = tx
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM private_note_command_receipts
                    WHERE organization_id = ?1 AND shared_comment_id = ?2
                 )",
                params![cmd.org_id, cmd.comment_id],
                |row| row.get(0),
            )
            .map_err(OfficeError::storage)?;
        if comment_used {
            return Ok(Err(ApiError::new(
                ErrorCode::AlreadyExists,
                format!("comment_id `{}` is already reserved", cmd.comment_id),
            )));
        }

        tx.execute(
            "INSERT INTO private_note_command_receipts
             (organization_id, author_id, operation_id, request_sha256, command, note_id,
              result_revision, state, shared_comment_id, error_code, created_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, 'publish', ?5, ?6, 'pending', ?7, NULL, ?8, NULL)",
            params![
                cmd.org_id,
                cmd.caller_id,
                cmd.operation_id,
                cmd.request_sha256,
                cmd.note_id,
                note.revision as i64,
                cmd.comment_id,
                cmd.now,
            ],
        )
        .map_err(OfficeError::storage)?;
        tx.commit().map_err(OfficeError::storage)?;

        Ok(Ok(PublishAttempt::Pending {
            note,
            shared_comment_id: cmd.comment_id.to_owned(),
        }))
    }

    fn finish_publish(
        &self,
        org_id: &str,
        author_id: &str,
        operation_id: &str,
        error_code: Option<&str>,
        now: &str,
    ) -> Result<(), OfficeError> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(OfficeError::storage)?;
        let Some(existing) = load_receipt(&tx, org_id, author_id, operation_id)? else {
            return Ok(());
        };
        if existing.state != "pending" {
            return Ok(());
        }
        let finished_at = monotonic_ts(now, &existing.created_at);
        let next_state = if error_code.is_some() {
            "rejected"
        } else {
            "completed"
        };
        tx.execute(
            "UPDATE private_note_command_receipts
             SET state = ?4, error_code = ?5, finished_at = ?6
             WHERE organization_id = ?1 AND author_id = ?2 AND operation_id = ?3",
            params![
                org_id,
                author_id,
                operation_id,
                next_state,
                error_code,
                finished_at,
            ],
        )
        .map_err(OfficeError::storage)?;
        tx.commit().map_err(OfficeError::storage)?;
        Ok(())
    }

    fn mark_task_deleted(&self, org_id: &str, task_id: &str) -> Result<(), OfficeError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO private_note_tasks (organization_id, task_id, is_deleted)
             VALUES (?1, ?2, 1)
             ON CONFLICT (organization_id, task_id) DO UPDATE SET is_deleted = 1",
            params![org_id, task_id],
        )
        .map_err(OfficeError::storage)?;
        Ok(())
    }
}
