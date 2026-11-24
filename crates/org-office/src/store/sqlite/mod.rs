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

pub(crate) mod accounting;
pub(crate) mod budgets;
mod companion;
#[cfg(any(test, feature = "testkit"))]
mod dump;
mod messaging;
mod organization;
pub(crate) mod private_notes;
mod processes;
mod projects;
pub(crate) mod providers;
mod receipts;
mod schema;
#[cfg(test)]
mod spike;
mod tasks;
#[cfg(test)]
mod tests;

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
    time::Duration,
};

use org_store::{
    CorePort, CoreRead, Cursor, DurablePath, Engine, EngineError, Id, Mark, Receipt, Sha256,
    StoreError,
};
use rusqlite::{config::DbConfig, ffi, Connection, OpenFlags};

use schema::Schema;

/// How many read connections stay open between reads.
const MAX_IDLE_READERS: usize = 4;
/// How long a connection waits for another's lock before it fails.
const BUSY_WAIT: Duration = Duration::from_secs(5);
/// How long `replace` waits for open snapshots to end before it fails.
const DRAIN_WAIT: Duration = Duration::from_secs(30);

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

/// `path` with `suffix` after its file name: SQLite's `-wal` and `-shm`, and `.new`.
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Removes a database file and what SQLite keeps beside it.
fn remove_database(path: &Path) -> Result<(), EngineError> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        remove_if_present(&beside(path, suffix))
            .map_err(|error| refused("removing a projection file", error))?;
    }
    Ok(())
}

/// Opens a connection that never checkpoints when it closes (plan revision 2, F2).
fn connect(path: &Path, flags: OpenFlags) -> Result<Connection, EngineError> {
    open_connection(path, flags).map_err(|error| refused("opening the projection", error))
}

fn open_connection(path: &Path, flags: OpenFlags) -> rusqlite::Result<Connection> {
    let db = Connection::open_with_flags(path, flags | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    db.busy_timeout(BUSY_WAIT)?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
    Ok(db)
}

/// Why a write connection could not be opened. SQLite's own error is kept, so the caller
/// can tell a file that is no database from one that could not be reached.
enum WriterRefused {
    Sqlite(&'static str, rusqlite::Error),
    Engine(EngineError),
}

impl WriterRefused {
    /// Whether SQLite read the file and found it is not a database, or a damaged one. A
    /// busy file, a failed read or a refused permission is neither: the file may be whole.
    fn says_no_database(&self) -> bool {
        let Self::Sqlite(_, error) = self else {
            return false;
        };
        matches!(
            error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
        )
    }
}

impl From<WriterRefused> for EngineError {
    fn from(refused_as: WriterRefused) -> Self {
        match refused_as {
            WriterRefused::Sqlite(doing, error) => refused(doing, error),
            WriterRefused::Engine(error) => error,
        }
    }
}

/// The write connection: write-ahead log, no automatic checkpoint, references enforced.
fn connect_writer(path: &Path) -> Result<Connection, EngineError> {
    Ok(open_writer(path)?)
}

fn open_writer(path: &Path) -> Result<Connection, WriterRefused> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
    let db = open_connection(path, flags)
        .map_err(|error| WriterRefused::Sqlite("opening the projection", error))?;
    let mode = db
        .query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| WriterRefused::Sqlite("setting the journal mode", error))?;
    if !mode.eq_ignore_ascii_case("wal") {
        let error = refused("setting the journal mode", format!("it is {mode}"));
        return Err(WriterRefused::Engine(error));
    }
    db.execute_batch(
        "PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON; \
         PRAGMA recursive_triggers = ON; PRAGMA wal_autocheckpoint = 0;",
    )
    .map_err(|error| WriterRefused::Sqlite("configuring the write connection", error))?;
    Ok(db)
}

/// A read connection: it cannot write, whatever it is asked.
fn connect_reader(path: &Path) -> Result<Connection, EngineError> {
    let db = connect(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    db.execute_batch("PRAGMA query_only = ON;")
        .map_err(|error| refused("configuring a read connection", error))?;
    Ok(db)
}

/// Zero exactly when every reference of the open transaction is resolved, deferred or not.
fn unresolved_references(db: &Connection) -> Result<i32, EngineError> {
    let (mut current, mut high) = (0, 0);
    // SAFETY: `handle` is this open connection's own, `db` is borrowed for the whole call
    // so it cannot close meanwhile, and both out-pointers are valid for writes. rusqlite
    // 0.42 has no safe wrapper for `sqlite3_db_status`.
    let code = unsafe {
        ffi::sqlite3_db_status(
            db.handle(),
            ffi::SQLITE_DBSTATUS_DEFERRED_FKS,
            &mut current,
            &mut high,
            0,
        )
    };
    if code != ffi::SQLITE_OK {
        return Err(EngineError::new(format!(
            "reading the unresolved-reference count: code {code}"
        )));
    }
    Ok(current)
}

/// Fails when the transaction could not commit, naming the rows whose references dangle.
fn check_references(db: &Connection) -> Result<(), EngineError> {
    if unresolved_references(db)? == 0 {
        return Ok(());
    }
    let named = db
        .prepare("PRAGMA foreign_key_check")
        .and_then(|mut check| {
            let rows = check.query_map([], |row| {
                let (table, parent): (String, String) = (row.get(0)?, row.get(2)?);
                let rowid: Option<i64> = row.get(1)?;
                let rowid = rowid.map_or("?".to_owned(), |rowid| rowid.to_string());
                Ok(format!(
                    "{table} row {rowid} refers to a missing row of {parent}"
                ))
            })?;
            rows.take(8).collect::<rusqlite::Result<Vec<String>>>()
        })
        .map_err(|error| failed(db, "naming unresolved references", &error))?;
    Err(EngineError::new(format!(
        "unresolved references: {}",
        named.join("; ")
    )))
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

/// Consecutive failed or reader-blocked background WAL checkpoints before surfacing a persistent
/// upkeep warning through [`Engine::upkeep_error`] (`TASK-33` / B1c).
const CHECKPOINT_FAILURE_THRESHOLD: u32 = 3;

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
        // What an interrupted `replace` left behind.
        remove_database(&beside(path, ".new"))?;
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

    /// Builds both replacement projections (`office.db.new` and `protected/messaging.db.new`)
    /// and leaves them closed and synced.
    fn build_replacement(
        &self,
        new_path: &Path,
        new_msg_path: &Path,
        build: &mut dyn FnMut(&mut SqliteTx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError> {
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
            let sync_folder = || match dst.parent() {
                Some(folder) => crate::private_fs::sync_dir(folder),
                None => Ok(()),
            };
            remove_if_present(&beside(dst, "-wal"))?;
            remove_if_present(&beside(dst, "-shm"))?;
            sync_folder()?;
            fs::rename(src, dst)?;
            sync_folder()
        };
        let swapped = swap_one(new_path, &self.path)
            .and_then(|()| swap_one(new_msg_path, &self.msg_path))
            .map_err(|error| refused("installing the replacement", error))
            .and_then(|()| messaging::protect_messaging_file(&self.msg_path));
        let new_office = connect_writer(&self.path)?;
        let new_msg = connect_writer(&self.msg_path)?;
        *writer = Some((new_office, new_msg));
        drop(checkpointer);
        swapped
    }
}

impl Engine for SqliteEngine {
    type Tx = SqliteTx;
    type Snapshot = SqliteSnapshot;

    fn begin(&self) -> Result<SqliteTx, EngineError> {
        if self.diverged.load(Ordering::Acquire) {
            return Err(EngineError::new(
                "projection diverged after partial commit; reopen required",
            ));
        }
        let Some((db, msg_db)) = lock(&self.writer).take() else {
            return Err(EngineError::new("a transaction is already open"));
        };
        self.transaction(db, msg_db, Arc::clone(&self.writer))
    }

    fn commit(&self, tx: SqliteTx, cursor: &Cursor) -> Result<(), EngineError> {
        let (db, msg_db) = tx.finish(cursor)?;
        *lock(&self.writer) = Some((db, msg_db));
        Ok(())
    }

    fn snapshot(&self) -> Result<SqliteSnapshot, EngineError> {
        if self.diverged.load(Ordering::Acquire) {
            return Err(EngineError::new(
                "projection diverged after partial commit; reopen required",
            ));
        }
        let pooled = {
            let mut pool = lock(&self.readers.pool);
            pool.open += 1;
            pool.idle.pop()
        };
        // From here the snapshot's drop gives the count back, with or without a connection.
        let mut snapshot = SqliteSnapshot {
            db: None,
            msg_db: None,
            organization: self.organization.clone(),
            readers: Arc::clone(&self.readers),
        };
        let (db, msg_db) = match pooled {
            Some(pair) => pair,
            None => (
                connect_reader(&self.path)?,
                messaging::open_messaging_reader(&self.msg_path)?,
            ),
        };
        // Hold the leaf commit gate in shared mode ONLY across the two `BEGIN` statements
        // so both connections fix their read view at the exact same dual-DB commit cursor,
        // and refuse any read once a partial commit has marked the engine diverged (`TASK-97`).
        {
            let _gate = self.commit_gate.read().unwrap_or_else(|p| p.into_inner());
            if self.diverged.load(Ordering::Acquire) {
                return Err(EngineError::new(
                    "projection diverged after partial commit; reopen required",
                ));
            }
            db.execute_batch("BEGIN; SELECT id FROM store_schema;")
                .map_err(|error| refused("beginning a read", error))?;
            msg_db
                .execute_batch("BEGIN; SELECT id FROM messaging_schema;")
                .map_err(|error| {
                    let _ = db.execute_batch("ROLLBACK");
                    refused("beginning a messaging read", error)
                })?;
        }
        snapshot.db = Some(db);
        snapshot.msg_db = Some(msg_db);
        Ok(snapshot)
    }

    /// Folds the log into the file after every round of the flusher, so the log does not
    /// grow for as long as the store runs: once it is all folded in and no read is on
    /// it, the writer's next transaction starts it from its beginning. A long read only
    /// delays that. Repeated checkpoint failures or reader-blocked WAL folding
    /// (`busy != 0 || (log > 0 && checkpointed < log)`) are logged and surfaced via
    /// [`Engine::upkeep_error`] rather than silently dropped (`TASK-33` / B1c).
    fn after_round(&self) {
        let failure = match self.run_passive_checkpoint() {
            Ok((busy, log, checkpointed)) if busy != 0 || (log > 0 && checkpointed < log) => {
                Some(format!(
                    "WAL checkpoint incomplete (busy={busy}, {checkpointed}/{log} frames folded)"
                ))
            }
            Ok(_) => {
                self.record_checkpoint_ok();
                None
            }
            Err(error) => Some(error.message),
        };
        if let Some(detail) = failure {
            let count = self.record_checkpoint_failure(detail.clone());
            if count == 1 || count == CHECKPOINT_FAILURE_THRESHOLD || count.is_multiple_of(10) {
                eprintln!(
                    "agentic-org-office: sqlite WAL checkpoint warning ({count} consecutive): {detail}"
                );
            }
        }
    }

    fn upkeep_error(&self) -> Option<EngineError> {
        let health = lock(&self.checkpoint_health);
        if health.consecutive_failures < CHECKPOINT_FAILURE_THRESHOLD {
            return None;
        }
        let detail = health
            .last_error
            .as_deref()
            .unwrap_or("unknown checkpoint error");
        Some(EngineError::new(format!(
            "sqlite WAL checkpoint failed {} consecutive times: {detail}",
            health.consecutive_failures
        )))
    }

    fn replace(
        &self,
        build: &mut dyn FnMut(&mut SqliteTx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError> {
        let new_path = beside(&self.path, ".new");
        let new_msg_path = beside(&self.msg_path, ".new");
        remove_database(&new_path)?;
        remove_database(&new_msg_path)?;
        if let Err(error) = self.build_replacement(&new_path, &new_msg_path, build) {
            let _ = remove_database(&new_path);
            let _ = remove_database(&new_msg_path);
            return Err(error);
        }
        // Readers drained: wait for every open snapshot, and let none begin until the
        // new file has the name.
        let pool = lock(&self.readers.pool);
        let still_open = |pool: &mut Pool| pool.open > 0;
        let drained = self
            .readers
            .ended
            .wait_timeout_while(pool, DRAIN_WAIT, still_open);
        let (mut pool, waited) = drained.unwrap_or_else(|poisoned| poisoned.into_inner());
        let installed = if waited.timed_out() {
            Err(EngineError::new(
                "a read was still open when the projection was to be replaced",
            ))
        } else {
            pool.idle.clear();
            self.install(&new_path, &new_msg_path)
        };
        if installed.is_err() {
            let _ = remove_database(&new_path);
            let _ = remove_database(&new_msg_path);
        }
        installed.map_err(StoreError::from)
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

impl Drop for SqliteTx {
    fn drop(&mut self) {
        let (Some(db), Some(msg_db)) = (self.db.take(), self.msg_db.take()) else {
            return;
        };
        if !db.is_autocommit() {
            let _ = db.execute_batch("ROLLBACK");
        }
        if !msg_db.is_autocommit() {
            let _ = msg_db.execute_batch("ROLLBACK");
        }
        if db.is_autocommit() && msg_db.is_autocommit() {
            *lock(&self.home) = Some((db, msg_db));
        }
    }
}

impl CoreRead for SqliteTx {
    fn cursor(&self) -> Result<Option<Cursor>, EngineError> {
        receipts::cursor(self.db())
    }

    fn source_hash(&self, path: &DurablePath) -> Result<Option<Sha256>, EngineError> {
        receipts::source_hash(self.db(), path)
    }

    fn sources_under(&self, prefix: &DurablePath) -> Result<Vec<DurablePath>, EngineError> {
        receipts::sources_under(self.db(), prefix)
    }

    fn receipt(&self, id: &Id) -> Result<Option<Receipt>, EngineError> {
        receipts::receipt(self.db(), id)
    }

    fn list_receipts_by_prefix(&self, prefix: &str) -> Result<Vec<Receipt>, EngineError> {
        receipts::list_receipts_by_prefix(self.db(), prefix)
    }
}

impl CorePort for SqliteTx {
    fn mark(&mut self) -> Result<Mark, EngineError> {
        let db = self.db();
        let msg_db = self.msg_db();
        let sql = match self.mark {
            Some(_) => "RELEASE mark; SAVEPOINT mark;",
            None => "SAVEPOINT mark;",
        };
        db.execute_batch(sql)
            .map_err(|error| failed(db, "marking", &error))?;
        msg_db
            .execute_batch(sql)
            .map_err(|error| failed(msg_db, "marking messaging.db", &error))?;
        self.marks += 1;
        self.mark = Some(self.marks);
        Ok(Mark(self.marks))
    }

    fn undo_to(&mut self, mark: Mark) -> Result<(), EngineError> {
        if self.mark != Some(mark.0) {
            return Err(EngineError::new(
                "only the most recent mark can be undone to",
            ));
        }
        let db = self.db();
        let msg_db = self.msg_db();
        db.execute_batch("ROLLBACK TO mark; RELEASE mark;")
            .map_err(|error| failed(db, "undoing to a mark", &error))?;
        msg_db
            .execute_batch("ROLLBACK TO mark; RELEASE mark;")
            .map_err(|error| failed(msg_db, "undoing messaging.db to a mark", &error))?;
        self.mark = None;
        Ok(())
    }

    fn set_source(&mut self, path: &DurablePath, hash: Option<Sha256>) -> Result<(), EngineError> {
        receipts::set_source(self.db(), path, hash)
    }

    fn put_receipt(&mut self, receipt: &Receipt) -> Result<(), EngineError> {
        receipts::put_receipt(self.db(), receipt)
    }

    fn check(&mut self) -> Result<(), EngineError> {
        let db = self.db();
        let msg_db = self.msg_db();
        if db.is_autocommit() || msg_db.is_autocommit() {
            return Err(EngineError::ended("the transaction has ended"));
        }
        check_references(db)?;
        check_references(msg_db)
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

impl Drop for SqliteSnapshot {
    fn drop(&mut self) {
        let ended_office = self
            .db
            .take()
            .filter(|db| db.execute_batch("ROLLBACK").is_ok());
        let ended_msg = self
            .msg_db
            .take()
            .filter(|db| db.execute_batch("ROLLBACK").is_ok());
        let mut pool = lock(&self.readers.pool);
        pool.open -= 1;
        if let (Some(db), Some(msg_db)) = (ended_office, ended_msg) {
            if pool.idle.len() < MAX_IDLE_READERS {
                pool.idle.push((db, msg_db));
            }
        }
        drop(pool);
        self.readers.ended.notify_all();
    }
}

impl CoreRead for SqliteSnapshot {
    fn cursor(&self) -> Result<Option<Cursor>, EngineError> {
        receipts::cursor(self.db())
    }

    fn source_hash(&self, path: &DurablePath) -> Result<Option<Sha256>, EngineError> {
        receipts::source_hash(self.db(), path)
    }

    fn sources_under(&self, prefix: &DurablePath) -> Result<Vec<DurablePath>, EngineError> {
        receipts::sources_under(self.db(), prefix)
    }

    fn receipt(&self, id: &Id) -> Result<Option<Receipt>, EngineError> {
        receipts::receipt(self.db(), id)
    }

    fn list_receipts_by_prefix(&self, prefix: &str) -> Result<Vec<Receipt>, EngineError> {
        receipts::list_receipts_by_prefix(self.db(), prefix)
    }
}
