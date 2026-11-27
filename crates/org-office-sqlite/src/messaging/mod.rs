//! SQLite adapter for [`MessagingRead`] and [`MessagingWrite`] backed exclusively by the
//! isolated `0700`/`0600` database `runtime/<office-id>/protected/messaging.db` using
//! `docs/design/organization-data/messaging-schema.sql` (`doc-90`, `TASK-10`, `ENG-1`).
//!
//! No messaging table ever enters `office.db`.
//!
//! [`MessagingRead`]: org_office::messaging::ports::MessagingRead
//! [`MessagingWrite`]: org_office::messaging::ports::MessagingWrite

use std::path::{Path, PathBuf};

use org_store::{Cursor, EngineError, Sha256};
use rusqlite::{Connection, OptionalExtension};

use super::{
    connect, connect_reader, connect_writer, failed, open_writer, refused, remove_database,
    SqliteSnapshot, SqliteTx,
};

mod files;
mod mirror;
mod read;
mod rows;
mod write;

/// Canonical messaging schema (`docs/design/organization-data/messaging-schema.sql`).
const MESSAGING_CONTRACT: &str =
    include_str!("../../../../docs/design/organization-data/messaging-schema.sql");

/// Adapter-only metadata tables inside `protected/messaging.db` (never in `office.db`).
const MESSAGING_ADAPTER: &str = "
CREATE TABLE messaging_schema (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  digest TEXT NOT NULL
) STRICT;
CREATE TABLE messaging_cursor (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  cursor TEXT NOT NULL
) STRICT;
CREATE TABLE messaging_local_meta (
  organization_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  local_seq INTEGER NOT NULL CHECK (local_seq > 0),
  mailbox_content_hash TEXT,
  PRIMARY KEY (organization_id, message_id)
) STRICT;
CREATE INDEX messaging_local_meta_seq ON messaging_local_meta(organization_id, local_seq);
CREATE TABLE messaging_pins (
  organization_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  pinned_by TEXT NOT NULL,
  pinned_at TEXT NOT NULL,
  PRIMARY KEY (organization_id, conversation_id, message_id)
) STRICT;
CREATE TABLE messaging_answers (
  organization_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision >= 1),
  accepted_by TEXT,
  accepted_at TEXT,
  reason TEXT,
  answer_json TEXT,
  PRIMARY KEY (organization_id, message_id, employee_id)
) STRICT;
CREATE INDEX messaging_message_files_by_id ON message_attachments(organization_id, id);
CREATE INDEX messaging_answer_files_by_id ON message_answer_attachments(organization_id, id);
";

const MESSAGING_ADAPTER_FORMAT: u32 = 2;

pub(super) fn messaging_db_path(office_db_path: &Path) -> PathBuf {
    let dir = office_db_path
        .parent()
        .expect("office.db sits inside <runtime>/<office-id>/");
    dir.join("protected").join("messaging.db")
}

/// The message search index's DDL (`search/messages.rs`), loaded after the adapter's.
fn messaging_search() -> &'static str {
    super::search::MESSAGING_SEARCH_DDL.as_str()
}

pub(super) fn messaging_schema_digest() -> String {
    let fmt = MESSAGING_ADAPTER_FORMAT.to_be_bytes();
    let len1 = (MESSAGING_CONTRACT.len() as u64).to_be_bytes();
    let len2 = (MESSAGING_ADAPTER.len() as u64).to_be_bytes();
    let len3 = (messaging_search().len() as u64).to_be_bytes();
    Sha256::of_parts(&[
        b"messaging-adapter",
        &fmt,
        &len1,
        MESSAGING_CONTRACT.as_bytes(),
        &len2,
        MESSAGING_ADAPTER.as_bytes(),
        &len3,
        messaging_search().as_bytes(),
    ])
    .to_hex()
}

pub(super) fn create_messaging_schema(db: &Connection) -> Result<(), EngineError> {
    let digest = messaging_schema_digest();
    let create = || -> rusqlite::Result<()> {
        db.execute_batch("BEGIN IMMEDIATE")?;
        db.execute_batch(MESSAGING_CONTRACT)?;
        db.execute_batch(MESSAGING_ADAPTER)?;
        db.execute_batch(messaging_search())?;
        db.execute(
            "INSERT INTO messaging_schema (id, digest) VALUES (1, ?1)",
            [digest],
        )?;
        db.execute_batch("COMMIT")
    };
    create().map_err(|error| failed(db, "creating the messaging schema", &error))
}

fn messaging_schema_matches(db: &Connection) -> Result<bool, EngineError> {
    let expected = messaging_schema_digest();
    let read = || -> rusqlite::Result<bool> {
        let count: i64 = db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'messaging_schema'",
            [],
            |row| row.get(0),
        )?;
        if count == 0 {
            return Ok(false);
        }
        let stored: Option<Option<String>> = db
            .query_row(
                "SELECT digest FROM messaging_schema WHERE id = 1",
                [],
                |row| Ok(row.get::<_, String>(0).ok()),
            )
            .optional()?;
        Ok(stored.flatten().is_some_and(|d| d == expected))
    };
    match read() {
        Err(error)
            if matches!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
            ) =>
        {
            Ok(false)
        }
        other => other.map_err(|error| failed(db, "reading messaging schema digest", &error)),
    }
}

pub(super) fn ensure_protected_dir(msg_path: &Path) -> Result<(), EngineError> {
    let folder = msg_path.parent().expect("messaging.db sits in protected/");
    org_platform::fs::create_owner_only_dir_all(folder)
        .and_then(|()| org_platform::fs::set_owner_only_dir(folder))
        .map_err(|e| refused("creating protected messaging directory", e))
}

pub(super) fn protect_messaging_file(msg_path: &Path) -> Result<(), EngineError> {
    org_office::private_fs::tighten_sqlite_sidecars(msg_path)
        .map_err(|e| refused("protecting messaging.db permissions", e))
}

/// Opens or recreates `protected/messaging.db`. Returns `(writer_conn, recreated)`.
pub(super) fn open_messaging_writer(msg_path: &Path) -> Result<(Connection, bool), EngineError> {
    ensure_protected_dir(msg_path)?;
    remove_database(&super::beside(msg_path, ".new"))?;
    let present = msg_path
        .try_exists()
        .map_err(|e| refused("finding messaging.db", e))?;
    if present {
        match open_writer(msg_path) {
            Ok(conn) if messaging_schema_matches(&conn)? => {
                protect_messaging_file(msg_path)?;
                return Ok((conn, false));
            }
            Ok(conn) => {
                let _ = conn.close();
            }
            Err(error) if error.says_no_database() => {}
            Err(error) => return Err(error.into()),
        }
    }
    remove_database(msg_path)?;
    // Pre-create with 0o600 before SQLite opens it (no 0o644 creation window).
    drop(
        org_office::private_fs::private_file(msg_path)
            .map_err(|e| refused("pre-creating messaging.db", e))?,
    );
    let conn = connect_writer(msg_path)?;
    create_messaging_schema(&conn)?;
    protect_messaging_file(msg_path)?;
    Ok((conn, true))
}

pub(super) fn open_messaging_replacement(new_path: &Path) -> Result<Connection, EngineError> {
    ensure_protected_dir(new_path)?;
    // Pre-create with 0o600 before SQLite opens it (no 0o644 creation window).
    drop(
        org_office::private_fs::private_file(new_path)
            .map_err(|e| refused("pre-creating messaging replacement", e))?,
    );
    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE;
    let db = connect(new_path, flags)?;
    db.execute_batch(
        "PRAGMA journal_mode = DELETE; PRAGMA synchronous = FULL; \
         PRAGMA foreign_keys = ON; PRAGMA recursive_triggers = ON;",
    )
    .map_err(|error| refused("configuring messaging replacement", error))?;
    create_messaging_schema(&db)?;
    protect_messaging_file(new_path)?;
    Ok(db)
}

pub(super) fn open_messaging_reader(msg_path: &Path) -> Result<Connection, EngineError> {
    connect_reader(msg_path)
}

pub(super) fn put_messaging_cursor(db: &Connection, cursor: &Cursor) -> Result<(), EngineError> {
    let text = serde_json::to_string(cursor).expect("cursor serializes");
    db.prepare_cached(
        "INSERT INTO messaging_cursor (id, cursor) VALUES (1, ?1) \
         ON CONFLICT (id) DO UPDATE SET cursor = excluded.cursor",
    )
    .and_then(|mut w| w.execute([text]))
    .map_err(|e| failed(db, "writing messaging cursor", &e))?;
    Ok(())
}

pub(super) fn read_messaging_cursor(db: &Connection) -> Result<Option<Cursor>, EngineError> {
    let stored = db
        .prepare_cached("SELECT cursor FROM messaging_cursor WHERE id = 1")
        .and_then(|mut r| r.query_row([], |row| row.get::<_, String>(0)).optional())
        .map_err(|e| failed(db, "reading messaging cursor", &e))?;
    stored
        .map(|s| {
            serde_json::from_str(&s)
                .map_err(|_| EngineError::new("corrupt messaging cursor in messaging.db"))
        })
        .transpose()
}

trait MsgView {
    fn msg_view(&self) -> Result<(&Connection, &str), EngineError>;
}

fn unscoped() -> EngineError {
    EngineError::new("the projection was opened for no organization")
}

impl MsgView for SqliteTx {
    fn msg_view(&self) -> Result<(&Connection, &str), EngineError> {
        let org = self.organization.as_ref().ok_or_else(unscoped)?;
        Ok((self.msg_db(), org.as_str()))
    }
}

impl MsgView for SqliteSnapshot {
    fn msg_view(&self) -> Result<(&Connection, &str), EngineError> {
        let org = self.organization.as_ref().ok_or_else(unscoped)?;
        Ok((self.msg_db(), org.as_str()))
    }
}
