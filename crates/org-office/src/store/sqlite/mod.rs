//! The SQLite engine of the office store (ADR-0018, `docs/design/office-store.md`): the
//! projection as one plain database file, `runtime/<office-id>/office.db`. It holds
//! nothing that is not in the files or the journal, so it is never migrated and never
//! encrypted: a file made from other DDL is deleted, and the store rebuilds it.
//!
//! - One write connection, handed to the writer's transaction and back. References are
//!   deferred in every transaction, so files may be applied in any order; `check` reads
//!   SQLite's count of unresolved references before anything is appended.
//! - Read connections are `query_only`, pooled, one read transaction per snapshot.
//! - No connection checkpoints on its own or when it closes: dropping the engine does no
//!   durable work. [`SqliteEngine::checkpoint`] is the one place the log is folded in:
//!   the flusher calls it after each round ([`Engine::after_round`]), on its own thread.
//!
//! This module and its children are the only code of the new office store that names
//! `rusqlite`.
//!
//! [`Engine::after_round`]: org_store::Engine::after_round

pub(crate) mod accounting;
pub(crate) mod budgets;
mod companion;
mod connection;
#[cfg(any(test, feature = "testkit"))]
mod dump;
mod engine;
mod messaging;
mod organization;
pub(crate) mod private_notes;
mod processes;
mod projects;
pub(crate) mod providers;
mod receipts;
mod references;
pub(crate) mod replication;
mod schema;
mod snapshot;
#[cfg(test)]
mod spike;
mod tasks;
#[cfg(test)]
mod tests;
mod tx;

/// One more index than this build's schema: the DDL of "another build", for the store
/// tests that check a projection made from other DDL is replaced (STORE-3). It lives here
/// because SQL is written only in this adapter (ENG-1, `tests/engine_boundary.rs`).
#[cfg(test)]
pub(crate) const ANOTHER_BUILDS_DDL: &str =
    "CREATE INDEX teams_by_name ON teams(organization_id, name);";

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, MutexGuard, RwLock,
    },
};

use org_store::{Cursor, EngineError, Id, StoreError};
#[cfg(test)]
use rusqlite::ffi;
use rusqlite::{Connection, OpenFlags};

use connection::{
    beside, connect, connect_reader, connect_writer, open_writer, remove_database,
    remove_if_present,
};
use references::check_references;
use schema::Schema;

/// How many read connections stay open between reads.
const MAX_IDLE_READERS: usize = 4;

/// An engine failure, saying whether SQLite ended the open transaction with it (a full
/// disk, an input/output error): then everything the batch applied is gone.
fn failed(db: &Connection, doing: &str, error: &rusqlite::Error) -> EngineError {
    let message = format!("{doing}: {error}");
    if db.is_autocommit() {
        return EngineError::ended(message);
    }
    EngineError::new(message)
}

/// A failure outside any transaction.
fn refused(doing: &str, error: impl std::fmt::Display) -> EngineError {
    EngineError::new(format!("{doing}: {error}"))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where a transaction's paired connections (`office.db`, `protected/messaging.db`) go when
/// the transaction ends.
type Home = Arc<Mutex<Option<(Connection, Connection)>>>;

/// The idle paired read connections and how many snapshots are open.
#[derive(Default)]
struct Pool {
    idle: Vec<(Connection, Connection)>,
    open: usize,
}

#[derive(Default)]
struct Readers {
    pool: Mutex<Pool>,
    /// Signalled when a snapshot ends.
    ended: Condvar,
}

#[derive(Default)]
struct CheckpointHealth {
    consecutive_failures: u32,
    last_error: Option<String>,
}

/// The projection in `office.db` plus its isolated `0700`/`0600` companion
/// `protected/messaging.db` (`doc-90`, `TASK-10`).
pub(crate) struct SqliteEngine {
    path: PathBuf,
    msg_path: PathBuf,
    schema: Schema,
    /// The organization whose rows the organization family's ports read and write.
    organization: Option<Id>,
    writer: Home,
    readers: Arc<Readers>,
    /// Leaf gate held in shared mode across a snapshot's paired `BEGIN` statements and in
    /// exclusive mode across `SqliteTx::finish`'s paired `COMMIT` statements so no snapshot
    /// ever observes `office.db` and `protected/messaging.db` at different cursors (`TASK-97`).
    commit_gate: Arc<RwLock<()>>,
    /// Set to `true` under `commit_gate.write()` if `office.db` COMMIT fails after
    /// `protected/messaging.db` already committed, refusing all subsequent reads and writes
    /// until the engine is reopened and rebuilt (`TASK-97` / Finding #2).
    diverged: Arc<AtomicBool>,
    /// The connections [`SqliteEngine::checkpoint`] uses: its own, so a checkpoint never
    /// waits for, or holds, the writer's (plan revision 2, F12).
    checkpointer: Mutex<Option<(Connection, Connection)>>,
    /// Atomic consecutive checkpoint failure/block count paired with its latest diagnostic.
    checkpoint_health: Mutex<CheckpointHealth>,
}

/// The writer's open transaction across `office.db` and `protected/messaging.db`.
/// Dropped without `commit`, it rolls back both.
pub(crate) struct SqliteTx {
    db: Option<Connection>,
    msg_db: Option<Connection>,
    organization: Option<Id>,
    home: Home,
    commit_gate: Arc<RwLock<()>>,
    diverged: Arc<AtomicBool>,
    /// The open savepoint, if the last mark has not been undone to.
    mark: Option<u64>,
    marks: u64,
    #[cfg(test)]
    fail_office_commit: bool,
}

/// One read transaction on paired pooled connections.
pub(crate) struct SqliteSnapshot {
    db: Option<Connection>,
    msg_db: Option<Connection>,
    organization: Option<Id>,
    readers: Arc<Readers>,
}

impl SqliteEngine {
    /// Opens the projection at `path` (and its isolated companion `protected/messaging.db`),
    /// creating either when missing or mismatched.
    pub fn open(path: &Path) -> Result<Self, EngineError> {
        Self::open_as(path, Schema::current())
    }

    fn open_as(path: &Path, schema: Schema) -> Result<Self, EngineError> {
        if !path.is_absolute() {
            return Err(EngineError::new("the projection's path must be absolute"));
        }
        let msg_path = messaging::messaging_db_path(path);
        let protected_office_new = msg_path
            .parent()
            .expect("messaging.db sits inside protected/")
            .join("office.db.new");
        // What an interrupted `replace` left behind (both current `protected/office.db.new`
        // and legacy sibling `office.db.new`).
        remove_database(&beside(path, ".new"))?;
        remove_database(&protected_office_new)?;
        remove_database(&beside(&msg_path, ".new"))?;
        let (writer, office_recreated) = match Self::existing(path, &schema)? {
            Some(writer) => (writer, false),
            None => {
                remove_database(path)?;
                let writer = connect_writer(path)?;
                schema.create(&writer)?;
                (writer, true)
            }
        };
        let (msg_writer, msg_recreated) = messaging::open_messaging_writer(&msg_path)?;
        let office_cursor = receipts::cursor(&writer)?;
        let msg_cursor = messaging::read_messaging_cursor(&msg_writer)?;
        if office_recreated || msg_recreated || office_cursor != msg_cursor {
            // Divergent cursors between office.db and protected/messaging.db (for example a
            // newly added messaging.db on an existing office or an interrupted commit) force
            // org-store to run `replace()` and rebuild both projections atomically.
            if office_cursor.is_some() {
                writer
                    .execute("DELETE FROM store_cursor", [])
                    .map_err(|e| refused("resetting divergent cursor", e))?;
            }
            if msg_cursor.is_some() {
                msg_writer
                    .execute("DELETE FROM messaging_cursor", [])
                    .map_err(|e| refused("resetting divergent messaging cursor", e))?;
            }
        }
        Ok(Self {
            path: path.to_owned(),
            msg_path,
            schema,
            organization: None,
            writer: Arc::new(Mutex::new(Some((writer, msg_writer)))),
            readers: Arc::default(),
            commit_gate: Arc::new(RwLock::new(())),
            diverged: Arc::new(AtomicBool::new(false)),
            checkpointer: Mutex::new(None),
            checkpoint_health: Mutex::new(CheckpointHealth::default()),
        })
    }

    /// This projection as the one of `organization`: the organization family's ports are
    /// scoped to it. Without it they refuse.
    pub fn for_organization(mut self, organization: &Id) -> Self {
        self.organization = Some(organization.clone());
        self
    }

    /// The write connection of the file at `path`, when it is a projection of `schema`.
    fn existing(path: &Path, schema: &Schema) -> Result<Option<Connection>, EngineError> {
        let present = path.try_exists();
        if !present.map_err(|error| refused("finding the projection", error))? {
            return Ok(None);
        }
        let writer = match open_writer(path) {
            Ok(writer) => writer,
            Err(error) if error.says_no_database() => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        // Another build's projection, or one never finished: "make it anew".
        Ok(schema.made(&writer)?.then_some(writer))
    }

    /// Folds as much of the write-ahead log into the database file as readers allow,
    /// without waiting for any of them. For the flusher, after it has materialized files,
    /// and for the orderly close.
    pub fn checkpoint(&self) -> Result<(), EngineError> {
        match self.run_passive_checkpoint() {
            Ok(_) => {
                self.record_checkpoint_ok();
                Ok(())
            }
            Err(error) => {
                self.record_checkpoint_failure(error.message.clone());
                Err(error)
            }
        }
    }

    /// Runs `PRAGMA wal_checkpoint(PASSIVE)` on both `office.db` and `protected/messaging.db`
    /// and returns `(busy, log_frames, checkpointed_frames)` (combining any incomplete round).
    fn run_passive_checkpoint(&self) -> Result<(i64, i64, i64), EngineError> {
        let mut slot = lock(&self.checkpointer);
        let (db, msg_db) = match slot.take() {
            Some(pair) => pair,
            None => {
                let db = connect(&self.path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
                db.execute_batch("PRAGMA wal_autocheckpoint = 0;")
                    .map_err(|error| refused("configuring the checkpoint connection", error))?;
                let msg_db = connect(&self.msg_path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
                msg_db
                    .execute_batch("PRAGMA wal_autocheckpoint = 0;")
                    .map_err(|error| {
                        refused("configuring the messaging checkpoint connection", error)
                    })?;
                (db, msg_db)
            }
        };
        let check_one = |conn: &Connection| {
            conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
        };
        let done_office = check_one(&db);
        let done_msg = check_one(&msg_db);
        *slot = Some((db, msg_db));
        let (b1, l1, c1) = done_office.map_err(|error| refused("checkpointing", error))?;
        let (b2, l2, c2) =
            done_msg.map_err(|error| refused("checkpointing messaging.db", error))?;
        Ok((b1.max(b2), l1.saturating_add(l2), c1.saturating_add(c2)))
    }

    fn record_checkpoint_ok(&self) {
        let mut health = lock(&self.checkpoint_health);
        health.consecutive_failures = 0;
        health.last_error = None;
    }

    fn record_checkpoint_failure(&self, detail: String) -> u32 {
        let mut health = lock(&self.checkpoint_health);
        health.consecutive_failures = health.consecutive_failures.saturating_add(1);
        health.last_error = Some(detail);
        health.consecutive_failures
    }

    #[cfg(test)]
    pub(crate) fn checkpoint_failures(&self) -> u32 {
        lock(&self.checkpoint_health).consecutive_failures
    }

    fn transaction(
        &self,
        db: Connection,
        msg_db: Connection,
        home: Home,
    ) -> Result<SqliteTx, EngineError> {
        let begin_both = || -> rusqlite::Result<()> {
            db.execute_batch("BEGIN IMMEDIATE; PRAGMA defer_foreign_keys = ON;")?;
            msg_db.execute_batch("BEGIN IMMEDIATE; PRAGMA defer_foreign_keys = ON;")?;
            Ok(())
        };
        match begin_both() {
            Ok(()) => Ok(SqliteTx {
                db: Some(db),
                msg_db: Some(msg_db),
                organization: self.organization.clone(),
                home,
                commit_gate: Arc::clone(&self.commit_gate),
                diverged: Arc::clone(&self.diverged),
                mark: None,
                marks: 0,
                #[cfg(test)]
                fail_office_commit: false,
            }),
            Err(error) => {
                let error = refused("beginning a transaction", error);
                if !db.is_autocommit() {
                    let _ = db.execute_batch("ROLLBACK");
                }
                if !msg_db.is_autocommit() {
                    let _ = msg_db.execute_batch("ROLLBACK");
                }
                *lock(&home) = Some((db, msg_db));
                Err(error)
            }
        }
    }

    /// Builds both replacement projections (`protected/office.db.new` and `protected/messaging.db.new`)
    /// and leaves them closed and synced.
    fn build_replacement(
        &self,
        new_path: &Path,
        new_msg_path: &Path,
        build: &mut dyn FnMut(&mut SqliteTx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError> {
        // Ensure `protected/` exists with `0700` BEFORE opening `protected/office.db.new`
        // so recovery still works even if `protected/` was deleted while open (`TASK-166`).
        messaging::ensure_protected_dir(new_msg_path)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        let db = connect(new_path, flags)?;
        db.execute_batch(
            "PRAGMA journal_mode = DELETE; PRAGMA synchronous = FULL; \
             PRAGMA foreign_keys = ON; PRAGMA recursive_triggers = ON;",
        )
        .map_err(|error| refused("configuring the replacement", error))?;
        self.schema.create(&db)?;
        let msg_db = messaging::open_messaging_replacement(new_msg_path)?;
        let mut tx = self.transaction(db, msg_db, Home::default())?;
        let cursor = build(&mut tx)?;
        let (db, msg_db) = tx.finish(&cursor)?;
        db.close()
            .map_err(|(_, error)| refused("closing the replacement", error))?;
        msg_db
            .close()
            .map_err(|(_, error)| refused("closing the messaging replacement", error))?;
        Ok(())
    }

    /// Puts `new_path` and `new_msg_path` in place of `office.db` and `protected/messaging.db`.
    fn install(&self, new_path: &Path, new_msg_path: &Path) -> Result<(), EngineError> {
        let mut writer = lock(&self.writer);
        let mut checkpointer = lock(&self.checkpointer);
        let Some((old, old_msg)) = writer.take() else {
            return Err(EngineError::new(
                "the projection cannot be replaced while a transaction is open",
            ));
        };
        *checkpointer = None;
        let emptied_office = old.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            row.get::<_, i64>(0)
        });
        let emptied_msg = old_msg.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            row.get::<_, i64>(0)
        });
        if !matches!(emptied_office, Ok(0)) || !matches!(emptied_msg, Ok(0)) {
            *writer = Some((old, old_msg));
            return Err(EngineError::new(
                "the old projection's log could not be emptied",
            ));
        }
        if let Err((old, error)) = old.close() {
            *writer = Some((old, old_msg));
            return Err(refused("closing the old projection", error));
        }
        if let Err((_, error)) = old_msg.close() {
            let re_office = connect_writer(&self.path)?;
            let re_msg = messaging::open_messaging_writer(&self.msg_path)?.0;
            *writer = Some((re_office, re_msg));
            return Err(refused("closing the old messaging projection", error));
        }
        let swap_one = |src: &Path, dst: &Path| -> std::io::Result<()> {
            let sync_folder = |dir: Option<&Path>| match dir {
                Some(folder) => crate::private_fs::sync_dir(folder),
                None => Ok(()),
            };
            remove_if_present(&beside(dst, "-wal"))?;
            remove_if_present(&beside(dst, "-shm"))?;
            sync_folder(dst.parent())?;
            fs::rename(src, dst)?;
            sync_folder(dst.parent())?;
            if src.parent() != dst.parent() {
                sync_folder(src.parent())?;
            }
            Ok(())
        };
        let swapped = swap_one(new_path, &self.path)
            .and_then(|()| swap_one(new_msg_path, &self.msg_path))
            .map_err(|error| refused("installing the replacement", error))
            .and_then(|()| messaging::protect_messaging_file(&self.msg_path));
        let new_office = connect_writer(&self.path)?;
        let _ = receipts::cursor(&new_office)?;
        let new_msg = connect_writer(&self.msg_path)?;
        let _ = messaging::read_messaging_cursor(&new_msg)?;
        *writer = Some((new_office, new_msg));
        drop(checkpointer);
        swapped
    }
}

impl SqliteTx {
    fn db(&self) -> &Connection {
        self.db
            .as_ref()
            .expect("present until the transaction ends")
    }

    fn msg_db(&self) -> &Connection {
        self.msg_db
            .as_ref()
            .expect("present until the transaction ends")
    }

    /// Writes the cursor and commits both `protected/messaging.db` and `office.db`, giving
    /// both connections back. If `office.db` fails after `protected/messaging.db` has already
    /// committed, the diverged pair is NOT returned to `self.home` (poisoning further writes
    /// until reopened/rebuilt) and `EngineError::ended` is returned (`TASK-97` / `ARCH-04`).
    fn finish(mut self, cursor: &Cursor) -> Result<(Connection, Connection), EngineError> {
        let db = self.db();
        let msg_db = self.msg_db();
        if db.is_autocommit() || msg_db.is_autocommit() {
            return Err(EngineError::ended("the transaction has ended"));
        }
        receipts::put_cursor(db, cursor)?;
        messaging::put_messaging_cursor(msg_db, cursor)?;
        check_references(db)?;
        check_references(msg_db)?;
        let _gate = self.commit_gate.write().unwrap_or_else(|p| p.into_inner());
        msg_db
            .execute_batch("COMMIT")
            .map_err(|error| refused("committing messaging.db", error))?;
        #[cfg(test)]
        let office_commit_result: rusqlite::Result<()> = if self.fail_office_commit {
            Err(rusqlite::Error::SqliteFailure(
                ffi::Error::new(ffi::SQLITE_IOERR),
                Some("injected office.db commit failure".into()),
            ))
        } else {
            db.execute_batch("COMMIT")
        };
        #[cfg(not(test))]
        let office_commit_result = db.execute_batch("COMMIT");
        if let Err(error) = office_commit_result {
            // `msg_db` is now ahead of `db`: mark the live engine diverged BEFORE releasing
            // `commit_gate` so no subsequent snapshot can observe the split state, then roll
            // back `db` and drop both connections WITHOUT returning them to `self.home`.
            self.diverged.store(true, Ordering::Release);
            if let Some(db) = self.db.take() {
                if !db.is_autocommit() {
                    let _ = db.execute_batch("ROLLBACK");
                }
            }
            let _ = self.msg_db.take();
            return Err(EngineError::ended(format!(
                "committing office.db after messaging.db committed (projection diverged): {error}"
            )));
        }
        Ok((
            self.db.take().expect("present until the transaction ends"),
            self.msg_db
                .take()
                .expect("present until the transaction ends"),
        ))
    }
}

impl SqliteSnapshot {
    fn db(&self) -> &Connection {
        self.db
            .as_ref()
            .expect("present once the snapshot is returned")
    }

    fn msg_db(&self) -> &Connection {
        self.msg_db
            .as_ref()
            .expect("present once the snapshot is returned")
    }
}
