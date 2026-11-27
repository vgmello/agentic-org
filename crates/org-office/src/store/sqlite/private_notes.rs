//! Isolated SQLite adapter for per-author private notes, plans and decisions (`TASK-50`,
//! `TASK-111` ADEC-1/2, `L8`, `ADR-0016`, `ADR-0031`, `doc-79` PN-01..06, `TASK-97` ARCH-01).
//!
//! Opens HQ-authoritative `<durable-root>/private/task-notes.db` using `private-notes-schema.sql`
//! verbatim, completely outside `durable/state/`, `durable/journal/`, `office.db`, and Git.
//! It is NOT a disposable runtime cache (`doc-79` §Private Task Notes).

mod list;
mod rows;
pub(crate) mod schema;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use org_proto::{ApiError, ErrorCode};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::error::OfficeError;
use crate::private_fs::{
    absolute, full_sync, private_dir_synced, private_file, sync_dir, tighten_sqlite_sidecars,
};
use crate::private_notes::{
    check_author_mutation, check_receipt_reuse, monotonic_ts, next_revision,
    parse_receipt_error_code, replayed_note, require_publishable, Accepted, BeginPublishCmd,
    CreateNoteCmd, DeleteNoteCmd, EditNoteCmd, NotePage, NoteQuery, PrivateNoteRecord,
    PrivateNotesStore, PublishAttempt,
};
use crate::store::layout;

use rows::{
    has_pending_publish, insert_completed_receipt, insert_note, load_note, load_receipt, note_row,
    tombstone, update_content, with_tags, Expected, ReceiptRow, NOTE_COLUMNS,
};

/// SQLite-backed isolated private notes database behind a single serialized writer lock (`ADR-0002`).
pub struct SqlitePrivateNotesStore {
    db_path: PathBuf,
    conn: Mutex<Connection>,
}

impl SqlitePrivateNotesStore {
    /// Opens (or initializes, or upgrades from the `TASK-50` layout) HQ-authoritative
    /// `<durable-root>/private/task-notes.db` with owner-only directory and file permissions
    /// (`doc-79`, `ADR-0016`, `PN-04`).
    pub fn open(durable_root: &Path) -> Result<Self, OfficeError> {
        let private_folder = layout::private_notes(durable_root);
        private_dir_synced(durable_root, &private_folder).map_err(OfficeError::storage)?;

        let db_path =
            absolute(private_folder.join("task-notes.db")).map_err(OfficeError::storage)?;
        // Pre-create with 0o600 before SQLite opens it (no 0o644 creation window).
        drop(private_file(&db_path).map_err(OfficeError::storage)?);
        let mut conn = Connection::open(&db_path).map_err(OfficeError::storage)?;
        // `fullfsync` + `checkpoint_fullfsync` request `F_FULLFSYNC` inside SQLite, and
        // `flush_authoritative_db` additionally invokes `org_platform::fs::full_sync` directly
        // on the WAL/DB file after every commit so macOS VFS fallback can never silently
        // acknowledge a commit without a verified device flush (`TASK-97` / Pass 2 Finding #6).
        // `temp_store = MEMORY` comes before the schema step: the upgrade's temporary copies
        // and every sort or temporary B-tree of note text stay in memory, never in a temporary
        // file outside the owner-only `durable/private/` folder.
        conn.execute_batch(
            "PRAGMA temp_store = MEMORY;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA fullfsync = ON;
             PRAGMA checkpoint_fullfsync = ON;
             PRAGMA foreign_keys = ON;
             PRAGMA recursive_triggers = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(OfficeError::storage)?;

        schema::ensure_schema(&mut conn)?;
        sync_dir(&private_folder).map_err(OfficeError::storage)?;
        tighten_sqlite_sidecars(&db_path).map_err(OfficeError::storage)?;
        flush_authoritative_db(&db_path)?;

        Ok(Self {
            db_path,
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, OfficeError> {
        self.conn
            .lock()
            .map_err(|_| OfficeError::Invalid("private notes sqlite connection poisoned"))
    }

    /// Answers an exact replay of `receipt` after flushing, or refuses a reused command.
    fn replay(
        &self,
        tx: Transaction<'_>,
        org_id: &str,
        receipt: &ReceiptRow,
        request: (&str, &str),
        accepted: Accepted<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        if let Err(e) = check_receipt_reuse(
            &receipt.command,
            &receipt.request_sha256,
            request.0,
            request.1,
        ) {
            return Ok(Err(e));
        }
        let current = load_note(&tx, org_id, &receipt.note_id)?
            .ok_or(OfficeError::Invalid("replayed private note missing"))?;
        let replayed = replayed_note(&current, receipt.result_revision, receipt.time(), accepted);
        drop(tx);
        flush_authoritative_db(&self.db_path)?;
        Ok(Ok(replayed))
    }

    /// Commits `tx`, flushes, and answers the stored note.
    fn commit_note(
        &self,
        tx: Transaction<'_>,
        org_id: &str,
        note_id: &str,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let record = load_note(&tx, org_id, note_id)?
            .ok_or(OfficeError::Invalid("written private note missing"))?;
        tx.commit().map_err(OfficeError::storage)?;
        flush_authoritative_db(&self.db_path)?;
        Ok(Ok(record))
    }
}

fn begin(conn: &mut Connection) -> Result<Transaction<'_>, OfficeError> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(OfficeError::storage)
}

/// Explicitly runs `org_platform::fs::full_sync` (`F_FULLFSYNC` on macOS, never downgraded)
/// on the WAL file (when present) and main DB file so an acknowledged private-note mutation
/// never relies on SQLite's VFS fallback to plain `fsync` (`TASK-97` / Pass 2 Finding #6).
fn flush_authoritative_db(db_path: &Path) -> Result<(), OfficeError> {
    let mut wal = db_path.as_os_str().to_os_string();
    wal.push("-wal");
    let wal_path = PathBuf::from(wal);
    match std::fs::File::open(&wal_path) {
        Ok(f) => full_sync(&f).map_err(OfficeError::storage)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(OfficeError::storage(err)),
    }
    let db_file = std::fs::File::open(db_path).map_err(OfficeError::storage)?;
    full_sync(&db_file).map_err(OfficeError::storage)
}

/// Validates an author mutation of the stored note inside `tx`.
fn author_mutation(
    tx: &Transaction<'_>,
    org_id: &str,
    note_id: &str,
    caller: (&str, bool),
    expected_revision: u64,
    pending_msg: &str,
) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
    check_author_mutation(
        load_note(tx, org_id, note_id)?,
        caller.0,
        caller.1,
        expected_revision,
        || has_pending_publish(tx, org_id, note_id),
        pending_msg,
    )
}

impl PrivateNotesStore for SqlitePrivateNotesStore {
    fn create_note(
        &self,
        cmd: CreateNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut conn = self.lock()?;
        let tx = begin(&mut conn)?;
        if let Some(receipt) = load_receipt(&tx, cmd.org_id, cmd.author_id, cmd.operation_id)? {
            let request = ("create", cmd.request_sha256);
            return self.replay(
                tx,
                cmd.org_id,
                &receipt,
                request,
                Accepted::Created(cmd.content),
            );
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
        if let Some(task_id) = cmd.task_id {
            tx.execute(
                "INSERT OR IGNORE INTO private_note_tasks (organization_id, task_id, is_deleted) VALUES (?1, ?2, 0)",
                params![cmd.org_id, task_id],
            )
            .map_err(OfficeError::storage)?;
        }
        insert_note(&tx, &PrivateNoteRecord::new_active(&cmd))?;
        let key = (
            cmd.org_id,
            cmd.author_id,
            cmd.operation_id,
            cmd.request_sha256,
        );
        insert_completed_receipt(&tx, key, "create", cmd.note_id, 1, cmd.now)?;
        self.commit_note(tx, cmd.org_id, cmd.note_id)
    }

    fn edit_note(
        &self,
        cmd: EditNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut conn = self.lock()?;
        let tx = begin(&mut conn)?;
        if let Some(receipt) = load_receipt(&tx, cmd.org_id, cmd.caller_id, cmd.operation_id)? {
            let request = ("edit", cmd.request_sha256);
            return self.replay(
                tx,
                cmd.org_id,
                &receipt,
                request,
                Accepted::Edited(cmd.patch),
            );
        }
        let existing = match author_mutation(
            &tx,
            cmd.org_id,
            cmd.note_id,
            (cmd.caller_id, cmd.caller_is_person),
            cmd.expected_revision,
            "resolve pending publication before editing private note",
        )? {
            Ok(note) => note,
            Err(e) => return Ok(Err(e)),
        };
        let next_rev = match next_revision(existing.revision) {
            Ok(rev) => rev,
            Err(e) => return Ok(Err(e)),
        };
        let content = cmd
            .patch
            .apply(&existing)
            .ok_or(OfficeError::Invalid("active private note without a body"))?;
        let updated_at = monotonic_ts(cmd.now, &existing.updated_at);
        let expected = Expected {
            author_id: cmd.caller_id,
            revision: cmd.expected_revision,
        };
        let edited = existing.with_content(&content, next_rev, &updated_at);
        update_content(&tx, expected, &edited)?;
        let key = (
            cmd.org_id,
            cmd.caller_id,
            cmd.operation_id,
            cmd.request_sha256,
        );
        insert_completed_receipt(&tx, key, "edit", cmd.note_id, next_rev, &updated_at)?;
        self.commit_note(tx, cmd.org_id, cmd.note_id)
    }

    fn delete_note(
        &self,
        cmd: DeleteNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut conn = self.lock()?;
        let tx = begin(&mut conn)?;
        if let Some(receipt) = load_receipt(&tx, cmd.org_id, cmd.caller_id, cmd.operation_id)? {
            let request = ("delete", cmd.request_sha256);
            return self.replay(tx, cmd.org_id, &receipt, request, Accepted::Deleted);
        }
        let existing = match author_mutation(
            &tx,
            cmd.org_id,
            cmd.note_id,
            (cmd.caller_id, cmd.caller_is_person),
            cmd.expected_revision,
            "resolve pending publication before deleting private note",
        )? {
            Ok(note) => note,
            Err(e) => return Ok(Err(e)),
        };
        let next_rev = match next_revision(existing.revision) {
            Ok(rev) => rev,
            Err(e) => return Ok(Err(e)),
        };
        let deleted_at = monotonic_ts(cmd.now, &existing.updated_at);
        let expected = Expected {
            author_id: cmd.caller_id,
            revision: cmd.expected_revision,
        };
        tombstone(
            &tx,
            (cmd.org_id, cmd.note_id),
            expected,
            next_rev,
            &deleted_at,
        )?;
        let key = (
            cmd.org_id,
            cmd.caller_id,
            cmd.operation_id,
            cmd.request_sha256,
        );
        insert_completed_receipt(&tx, key, "delete", cmd.note_id, next_rev, &deleted_at)?;
        self.commit_note(tx, cmd.org_id, cmd.note_id)
    }

    fn get_note(
        &self,
        org_id: &str,
        caller_id: &str,
        caller_is_person: bool,
        note_id: &str,
    ) -> Result<Option<PrivateNoteRecord>, OfficeError> {
        let conn = self.lock()?;
        // `ADR-0031`: an agent reads only its own notes; a person reads every author's.
        let row = conn
            .query_row(
                &format!(
                    "SELECT {NOTE_COLUMNS} FROM private_notes
                     WHERE organization_id = ?1 AND id = ?2 AND deleted_at IS NULL
                       AND (?3 OR author_id = ?4)"
                ),
                params![org_id, note_id, caller_is_person, caller_id],
                note_row,
            )
            .optional()
            .map_err(OfficeError::storage)?;
        row.map(|record| with_tags(&conn, record)).transpose()
    }

    fn list_notes(&self, query: &NoteQuery<'_>) -> Result<NotePage, OfficeError> {
        let conn = self.lock()?;
        list::list(&conn, query)
    }

    fn begin_publish(
        &self,
        cmd: BeginPublishCmd<'_>,
    ) -> Result<Result<PublishAttempt, ApiError>, OfficeError> {
        let mut conn = self.lock()?;
        let tx = begin(&mut conn)?;

        if let Some(existing) = load_receipt(&tx, cmd.org_id, cmd.caller_id, cmd.operation_id)? {
            if let Err(e) = check_receipt_reuse(
                &existing.command,
                &existing.request_sha256,
                "publish",
                cmd.request_sha256,
            ) {
                return Ok(Err(e));
            }
            let shared_comment_id = existing.shared_comment_id.unwrap_or_default();
            let note = load_note(&tx, cmd.org_id, &existing.note_id)?
                .ok_or(OfficeError::Invalid("replayed publish note missing"))?;
            drop(tx);
            flush_authoritative_db(&self.db_path)?;
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

        let note = match author_mutation(
            &tx,
            cmd.org_id,
            cmd.note_id,
            (cmd.caller_id, cmd.caller_is_person),
            cmd.expected_revision,
            "another publication of this private note is already pending",
        )? {
            Ok(n) => n,
            Err(e) => return Ok(Err(e)),
        };
        if let Err(e) = require_publishable(&note) {
            return Ok(Err(e));
        }

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
        flush_authoritative_db(&self.db_path)?;

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
        let tx = begin(&mut conn)?;
        let Some(existing) = load_receipt(&tx, org_id, author_id, operation_id)? else {
            return Ok(());
        };
        if existing.state != "pending" {
            drop(tx);
            flush_authoritative_db(&self.db_path)?;
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
        flush_authoritative_db(&self.db_path)?;
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
        flush_authoritative_db(&self.db_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
