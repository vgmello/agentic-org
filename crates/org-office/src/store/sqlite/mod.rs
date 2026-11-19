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

#[cfg(any(test, feature = "testkit"))]
mod dump;
mod organization;
mod receipts;
mod schema;
#[cfg(test)]
mod spike;
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
    sync::{Arc, Condvar, Mutex, MutexGuard},
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

/// Where a transaction's connection goes when the transaction ends.
type Home = Arc<Mutex<Option<Connection>>>;

/// The idle read connections and how many snapshots are open.
#[derive(Default)]
struct Pool {
    idle: Vec<Connection>,
    open: usize,
}

#[derive(Default)]
struct Readers {
    pool: Mutex<Pool>,
    /// Signalled when a snapshot ends.
    ended: Condvar,
}

/// The projection in one SQLite file.
pub struct SqliteEngine {
    path: PathBuf,
    schema: Schema,
    /// The organization whose rows the organization family's ports read and write.
    organization: Option<Id>,
    writer: Home,
    readers: Arc<Readers>,
    /// The connection [`SqliteEngine::checkpoint`] uses: its own, so a checkpoint never
    /// waits for, or holds, the writer's (plan revision 2, F12).
    checkpointer: Mutex<Option<Connection>>,
}

/// The writer's open transaction. Dropped without `commit`, it rolls back.
pub struct SqliteTx {
    db: Option<Connection>,
    organization: Option<Id>,
    home: Home,
    /// The open savepoint, if the last mark has not been undone to.
    mark: Option<u64>,
    marks: u64,
}

/// One read transaction on a pooled connection.
pub struct SqliteSnapshot {
    db: Option<Connection>,
    organization: Option<Id>,
    readers: Arc<Readers>,
}

impl SqliteEngine {
    /// Opens the projection at `path`, creating it when it is missing. A file that was
    /// made from other DDL, was never finished, or is not a database is deleted and made
    /// anew, empty: the store then rebuilds it from the files and the journal. A file that
    /// could not be opened or read (busy, an input/output error, a refused permission) is
    /// left where it is and the error returned.
    pub fn open(path: &Path) -> Result<Self, EngineError> {
        Self::open_as(path, Schema::current())
    }

    fn open_as(path: &Path, schema: Schema) -> Result<Self, EngineError> {
        if !path.is_absolute() {
            return Err(EngineError::new("the projection's path must be absolute"));
        }
        // What an interrupted `replace` left behind.
        remove_database(&beside(path, ".new"))?;
        let writer = match Self::existing(path, &schema)? {
            Some(writer) => writer,
            None => {
                remove_database(path)?;
                let writer = connect_writer(path)?;
                schema.create(&writer)?;
                writer
            }
        };
        Ok(Self {
            path: path.to_owned(),
            schema,
            organization: None,
            writer: Arc::new(Mutex::new(Some(writer))),
            readers: Arc::default(),
            checkpointer: Mutex::new(None),
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
        let mut slot = lock(&self.checkpointer);
        let db = match slot.take() {
            Some(db) => db,
            None => {
                let db = connect(&self.path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
                db.execute_batch("PRAGMA wal_autocheckpoint = 0;")
                    .map_err(|error| refused("configuring the checkpoint connection", error))?;
                db
            }
        };
        let done = db.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()));
        *slot = Some(db);
        done.map_err(|error| refused("checkpointing", error))
    }

    fn transaction(&self, db: Connection, home: Home) -> Result<SqliteTx, EngineError> {
        // Deferring is issued after BEGIN and ends with the transaction.
        match db.execute_batch("BEGIN IMMEDIATE; PRAGMA defer_foreign_keys = ON;") {
            Ok(()) => Ok(SqliteTx {
                db: Some(db),
                organization: self.organization.clone(),
                home,
                mark: None,
                marks: 0,
            }),
            Err(error) => {
                let error = refused("beginning a transaction", error);
                let _ = db.execute_batch("ROLLBACK");
                *lock(&home) = Some(db);
                Err(error)
            }
        }
    }

    /// Builds the replacement at `new_path` and leaves it closed and synced.
    fn build_replacement(
        &self,
        new_path: &Path,
        build: &mut dyn FnMut(&mut SqliteTx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        let db = connect(new_path, flags)?;
        // On a rollback journal, synced at each commit: the file is whole, with no log
        // beside it, before it takes the projection's name.
        db.execute_batch(
            "PRAGMA journal_mode = DELETE; PRAGMA synchronous = FULL; \
             PRAGMA foreign_keys = ON; PRAGMA recursive_triggers = ON;",
        )
        .map_err(|error| refused("configuring the replacement", error))?;
        self.schema.create(&db)?;
        let mut tx = self.transaction(db, Home::default())?;
        let cursor = build(&mut tx)?;
        let db = tx.finish(&cursor)?;
        db.close()
            .map_err(|(_, error)| refused("closing the replacement", error))?;
        Ok(())
    }

    /// Puts the file at `new_path` in the projection's place. Called with no snapshot open
    /// and the pool held, so nothing reads the old file's log while it goes.
    ///
    /// Locks are taken in one order, pool, writer, checkpointer, and `checkpoint` takes
    /// only its own. The checkpointer's is held until the new writer is connected: a
    /// checkpoint that arrives meanwhile waits, then opens the file that has the name. Let
    /// in earlier, it would open the old file and keep checkpointing that one, unlinked.
    fn install(&self, new_path: &Path) -> Result<(), EngineError> {
        let mut writer = lock(&self.writer);
        let mut checkpointer = lock(&self.checkpointer);
        let Some(old) = writer.take() else {
            return Err(EngineError::new(
                "the projection cannot be replaced while a transaction is open",
            ));
        };
        *checkpointer = None;
        // Empty the old log first: a log left beside the new file would be replayed into
        // it. With the log empty, a crash before the rename leaves the old projection whole.
        let emptied = old.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            row.get::<_, i64>(0)
        });
        if !matches!(emptied, Ok(0)) {
            *writer = Some(old);
            return Err(EngineError::new(
                "the old projection's log could not be emptied",
            ));
        }
        if let Err((old, error)) = old.close() {
            *writer = Some(old);
            return Err(refused("closing the old projection", error));
        }
        let sync_folder = || match self.path.parent() {
            Some(folder) => fs::File::open(folder)?.sync_all(),
            None => Ok(()),
        };
        let swap = || -> std::io::Result<()> {
            remove_if_present(&beside(&self.path, "-wal"))?;
            remove_if_present(&beside(&self.path, "-shm"))?;
            // The old log is durably gone before the new file can have the name: a power
            // cut that kept the rename but not the unlinks would replay the old log into it.
            sync_folder()?;
            fs::rename(new_path, &self.path)?;
            sync_folder()
        };
        let swapped = swap().map_err(|error| refused("installing the replacement", error));
        // Whichever file now has the name, the writer goes on from it.
        *writer = Some(connect_writer(&self.path)?);
        drop(checkpointer);
        swapped
    }
}

impl Engine for SqliteEngine {
    type Tx = SqliteTx;
    type Snapshot = SqliteSnapshot;

    fn begin(&self) -> Result<SqliteTx, EngineError> {
        let Some(db) = lock(&self.writer).take() else {
            return Err(EngineError::new("a transaction is already open"));
        };
        self.transaction(db, Arc::clone(&self.writer))
    }

    fn commit(&self, tx: SqliteTx, cursor: &Cursor) -> Result<(), EngineError> {
        let db = tx.finish(cursor)?;
        *lock(&self.writer) = Some(db);
        Ok(())
    }

    fn snapshot(&self) -> Result<SqliteSnapshot, EngineError> {
        let pooled = {
            let mut pool = lock(&self.readers.pool);
            pool.open += 1;
            pool.idle.pop()
        };
        // From here the snapshot's drop gives the count back, with or without a connection.
        let mut snapshot = SqliteSnapshot {
            db: None,
            organization: self.organization.clone(),
            readers: Arc::clone(&self.readers),
        };
        let db = match pooled {
            Some(db) => db,
            None => connect_reader(&self.path)?,
        };
        // The first read fixes what the transaction sees.
        db.execute_batch("BEGIN; SELECT id FROM store_schema;")
            .map_err(|error| refused("beginning a read", error))?;
        snapshot.db = Some(db);
        Ok(snapshot)
    }

    /// Folds the log into the file after every round of the flusher, so the log does not
    /// grow for as long as the store runs: once it is all folded in and no read is on
    /// it, the writer's next transaction starts it from its beginning. A long read only
    /// delays that. A checkpoint that fails leaves the log for the next round; the
    /// orderly close reports its own.
    fn after_round(&self) {
        let _ = self.checkpoint();
    }

    fn replace(
        &self,
        build: &mut dyn FnMut(&mut SqliteTx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError> {
        let new_path = beside(&self.path, ".new");
        remove_database(&new_path)?;
        if let Err(error) = self.build_replacement(&new_path, build) {
            let _ = remove_database(&new_path);
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
            self.install(&new_path)
        };
        if installed.is_err() {
            let _ = remove_database(&new_path);
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

    /// Writes the cursor and commits, giving the connection back. On failure the
    /// transaction is rolled back and the connection goes home.
    fn finish(mut self, cursor: &Cursor) -> Result<Connection, EngineError> {
        let db = self.db();
        if db.is_autocommit() {
            return Err(EngineError::ended("the transaction has ended"));
        }
        receipts::put_cursor(db, cursor)?;
        // A commit with a dangling reference would fail and stay open: say why instead.
        check_references(db)?;
        db.execute_batch("COMMIT")
            .map_err(|error| refused("committing", error))?;
        Ok(self.db.take().expect("present until the transaction ends"))
    }
}

impl Drop for SqliteTx {
    fn drop(&mut self) {
        let Some(db) = self.db.take() else {
            return;
        };
        if !db.is_autocommit() {
            let _ = db.execute_batch("ROLLBACK");
        }
        // A connection that could not roll back is closed, not reused: SQLite rolls back
        // when it closes, and the engine then reports that no transaction can begin.
        if db.is_autocommit() {
            *lock(&self.home) = Some(db);
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
}

impl CorePort for SqliteTx {
    fn mark(&mut self) -> Result<Mark, EngineError> {
        let db = self.db();
        // Only the most recent mark can be undone to: the one before it is let go.
        let sql = match self.mark {
            Some(_) => "RELEASE mark; SAVEPOINT mark;",
            None => "SAVEPOINT mark;",
        };
        db.execute_batch(sql)
            .map_err(|error| failed(db, "marking", &error))?;
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
        db.execute_batch("ROLLBACK TO mark; RELEASE mark;")
            .map_err(|error| failed(db, "undoing to a mark", &error))?;
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
        if db.is_autocommit() {
            return Err(EngineError::ended("the transaction has ended"));
        }
        check_references(db)
    }
}

impl SqliteSnapshot {
    fn db(&self) -> &Connection {
        self.db
            .as_ref()
            .expect("present once the snapshot is returned")
    }
}

impl Drop for SqliteSnapshot {
    fn drop(&mut self) {
        let ended = self.db.take();
        let ended = ended.filter(|db| db.execute_batch("ROLLBACK").is_ok());
        let mut pool = lock(&self.readers.pool);
        pool.open -= 1;
        if let Some(db) = ended {
            if pool.idle.len() < MAX_IDLE_READERS {
                pool.idle.push(db);
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
}
