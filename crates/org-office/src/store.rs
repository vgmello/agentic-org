//! The office's storage engine. **The only module that knows SQLite**: it owns the
//! connections, the encryption, the schema and every SQL statement, and speaks to the rest
//! of the crate in the plain values of `model` and `OfficeError`. Replacing the engine means
//! replacing this file; `Office` and its rules don't change.
//!
//! The database is SQLCipher (encrypted at rest) with exactly one writer: a dedicated
//! thread owns the only writing connection. It takes every request queued at that moment,
//! runs them in ONE transaction (each in its own SAVEPOINT, so a failure undoes only that
//! request), commits once, and only then answers each request. Readers use their own
//! connections (WAL lets them run beside the writer), so many concurrent callers never see
//! `SQLITE_BUSY`.

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::error::OfficeError;
use crate::key::Key;
use crate::model::{OfficeInfo, Organization};
use crate::name::OrganizationName;
use crate::private_fs::{private_file, tighten_file_if_present};
use crate::{time, ulid};

const SCHEMA_VERSION: i64 = 1;
const DEFAULT_OFFICE_NAME: &str = "This Mac";
/// Idle reader connections kept for reuse.
const MAX_IDLE_READERS: usize = 4;
/// Most requests committed together.
const MAX_BATCH: usize = 64;

/// Undoes what a request did outside the database (e.g. made a folder), when the batch it
/// was part of is not committed after all.
pub(crate) type Undo = Box<dyn FnOnce() + Send>;

/// One request for the writer. `attempt` may run more than once: if SQLite aborts the batch's
/// transaction, every other request in it is run again in a fresh one (after its `Undo`).
struct WriteJob {
    attempt: Box<Attempt>,
    /// Called once, after the batch's commit, with how this request ended.
    reply: Box<dyn FnOnce(Result<(), OfficeError>) + Send>,
}

/// One go at a request, inside its savepoint.
type Attempt = dyn FnMut(&Connection) -> Result<Option<Undo>, OfficeError> + Send;

/// Whether `Store::open` may make the database, or must find one already there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Opening {
    /// A new office: the database file and schema are created.
    New,
    /// An existing office: a missing file or one without our schema is refused, and nothing
    /// is created in its place.
    Existing,
}

pub(crate) struct Store {
    /// `None` only while dropping.
    jobs: Option<mpsc::Sender<WriteJob>>,
    writer: Option<JoinHandle<()>>,
    readers: Mutex<Vec<Connection>>,
    db_path: PathBuf,
    key: Key,
}

impl Store {
    /// Opens the encrypted registry at `db_path`, which must be absolute, with `key`, and
    /// returns it with this office's identity. `Opening::New` creates the file and schema;
    /// `Opening::Existing` never does. `office_id` is the id the office already holds; a
    /// database that belongs to another office is refused. The caller must hold the office
    /// lock: the store assumes it is the only process touching the database.
    pub(crate) fn open(
        db_path: &Path,
        key: &Key,
        office_id: &str,
        opening: Opening,
    ) -> Result<(Self, OfficeInfo), OfficeError> {
        if !db_path.is_absolute() {
            return Err(failure("the database path must be absolute"));
        }
        match opening {
            // SQLite gives new -wal/-shm files the main file's mode, so make that one 0600 first.
            Opening::New => drop(private_file(db_path).map_err(OfficeError::storage)?),
            Opening::Existing => {
                // SQLite deletes a WAL beside an empty main file on the first read, which
                // would destroy whatever that log still holds: don't let it look.
                let metadata = std::fs::metadata(db_path).map_err(OfficeError::storage)?;
                if !metadata.is_file() || metadata.len() == 0 {
                    return Err(failure("the database file is missing or empty"));
                }
                tighten_file_if_present(db_path).map_err(OfficeError::storage)?
            }
        }
        // Leftover -wal/-shm files from an earlier run keep whatever mode they had.
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = db_path.as_os_str().to_owned();
            sidecar.push(suffix);
            tighten_file_if_present(Path::new(&sidecar)).map_err(OfficeError::storage)?;
        }

        let mut writer = connect(db_path, key, opening == Opening::New)?;
        if opening == Opening::Existing {
            // Before anything writes (switching to WAL would write a header): a file without
            // our schema is not an office, and must not silently become a new, empty one.
            let version: i64 = writer
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(OfficeError::storage)?;
            if version < 1 {
                return Err(failure("the database holds no office schema"));
            }
        }
        let journal_mode: String = writer
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
            .map_err(OfficeError::storage)?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(failure(format!(
                "SQLite refused WAL mode (stayed in {journal_mode})"
            )));
        }
        let info = migrate(&mut writer, office_id)?;

        let (jobs, queue) = mpsc::channel::<WriteJob>();
        let thread = std::thread::Builder::new()
            .name("org-office-writer".into())
            .spawn(move || writer_loop(writer, queue))
            .map_err(OfficeError::storage)?;

        let store = Self {
            jobs: Some(jobs),
            writer: Some(thread),
            readers: Mutex::new(Vec::new()),
            db_path: db_path.to_owned(),
            key: key.clone(),
        };
        Ok((store, info))
    }

    /// Registers an organization, or fails with `AlreadyExists`.
    ///
    /// `before_commit` runs after the row is inserted and before the batch commits (the
    /// office makes the workspace folder there). If it fails, nothing is registered, and it
    /// must have cleaned up after itself. If it succeeds and the batch is then not
    /// committed, the `Undo` it returned is run. It may run more than once, for a request
    /// that is re-run, so it must be repeatable.
    pub(crate) fn create_organization(
        &self,
        name: &OrganizationName,
        before_commit: impl Fn(&Organization) -> std::io::Result<Undo> + Send + 'static,
    ) -> Result<Organization, OfficeError> {
        let display_name = name.as_str().to_owned();
        let name_key = name.uniqueness_key();
        self.write(move |db| {
            // Stamped on the writer, so `created_at` never runs against insertion order
            // (barring a backwards clock step). Ids are random within a millisecond: don't
            // sort by id.
            let organization = Organization {
                id: ulid::generate().map_err(OfficeError::storage)?,
                name: display_name.clone(),
                created_at: time::now_rfc3339(),
            };
            // Only the writer inserts, so this check can't race. The UNIQUE constraint stays
            // as a backstop, but the answer doesn't depend on SQLite's error wording.
            let taken = db
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM organizations WHERE name_key = ?1)",
                    [&name_key],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(OfficeError::storage)?;
            if taken {
                return Err(OfficeError::AlreadyExists);
            }
            db.execute(
                "INSERT INTO organizations (id, name, name_key, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                (
                    &organization.id,
                    &organization.name,
                    &name_key,
                    &organization.created_at,
                ),
            )
            .map_err(|e| {
                if is_unique_violation(&e, "name_key") {
                    OfficeError::AlreadyExists
                } else {
                    OfficeError::storage(e)
                }
            })?;

            // A failure here rolls the request's savepoint back.
            let undo = before_commit(&organization).map_err(OfficeError::storage)?;
            Ok((organization, Some(undo)))
        })
    }

    /// All organizations in creation order. Ordered by insertion (`rowid`), not by timestamp
    /// or id: two creates inside one millisecond have equal timestamps and random-suffixed ids.
    pub(crate) fn list_organizations(&self) -> Result<Vec<Organization>, OfficeError> {
        self.read(|db| {
            let mut statement = db
                .prepare("SELECT id, name, created_at FROM organizations ORDER BY rowid")
                .map_err(OfficeError::storage)?;
            let rows = statement
                .query_map((), |row| {
                    Ok(Organization {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                })
                .map_err(OfficeError::storage)?;
            rows.collect::<Result<_, _>>().map_err(OfficeError::storage)
        })
    }

    /// Queues `work` for the writer and waits for it to be committed. `work` runs inside a
    /// savepoint and returns its value plus an optional `Undo` for effects outside the
    /// database. It may be run again (see [`WriteJob`]).
    fn write<R: Send + 'static>(
        &self,
        mut work: impl FnMut(&Connection) -> Result<(R, Option<Undo>), OfficeError> + Send + 'static,
    ) -> Result<R, OfficeError> {
        let (reply, answer) = mpsc::sync_channel(1);
        let slot: Arc<Mutex<Option<R>>> = Arc::new(Mutex::new(None));
        let attempt = {
            let slot = Arc::clone(&slot);
            move |db: &Connection| {
                let (value, undo) = work(db)?;
                *lock(&slot) = Some(value);
                Ok(undo)
            }
        };
        let job = WriteJob {
            attempt: Box::new(attempt),
            reply: Box::new(move |outcome| {
                let result = outcome.map(|()| {
                    lock(&slot)
                        .take()
                        .expect("a request that succeeded stored its value")
                });
                let _ = reply.send(result);
            }),
        };
        let stopped = || failure("the storage writer stopped");
        self.jobs
            .as_ref()
            .ok_or_else(stopped)?
            .send(job)
            .map_err(|_| stopped())?;
        answer.recv().map_err(|_| stopped())?
    }

    /// Runs `work` on a reader connection: an idle one if there is one, else a new one.
    fn read<R>(
        &self,
        work: impl FnOnce(&Connection) -> Result<R, OfficeError>,
    ) -> Result<R, OfficeError> {
        let idle = self.idle_readers().pop();
        let db = match idle {
            Some(db) => db,
            None => {
                let db = connect(&self.db_path, &self.key, false)?;
                db.pragma_update(None, "query_only", true)
                    .map_err(OfficeError::storage)?;
                db
            }
        };
        let result = work(&db);
        // A reader that hit an error is dropped, not reused.
        if result.is_ok() {
            let mut idle = self.idle_readers();
            if idle.len() < MAX_IDLE_READERS {
                idle.push(db);
            }
        }
        result
    }

    /// Test seam: runs raw SQL on the writer, to inject failures the engine can't otherwise
    /// be made to have.
    #[cfg(test)]
    pub(crate) fn run_sql_for_test(&self, sql: &'static str) {
        self.write(move |db| {
            db.execute_batch(sql)
                .map_err(OfficeError::storage)
                .map(|()| ((), None))
        })
        .unwrap();
    }

    fn idle_readers(&self) -> std::sync::MutexGuard<'_, Vec<Connection>> {
        self.readers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // Closing the queue ends the writer thread, which closes its connection. Wait for
        // that, so the office lock is only released after the database is closed.
        self.jobs.take();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

/// The writer thread: takes whatever is queued, commits it together, repeats.
fn writer_loop(mut db: Connection, queue: mpsc::Receiver<WriteJob>) {
    while let Ok(first) = queue.recv() {
        let mut batch = vec![first];
        while batch.len() < MAX_BATCH {
            match queue.try_recv() {
                Ok(job) => batch.push(job),
                Err(_) => break,
            }
        }
        run_batch(&mut db, batch);
    }
}

enum BatchResult {
    /// Every request in the batch has been answered.
    Finished,
    /// SQLite ended the transaction itself (e.g. disk full, an I/O error) while running the
    /// request at `culprit`: everything else in the batch was undone with it.
    Aborted { culprit: usize, error: OfficeError },
}

fn run_batch(db: &mut Connection, mut jobs: Vec<WriteJob>) {
    while !jobs.is_empty() {
        match attempt_batch(db, &mut jobs) {
            BatchResult::Finished => return,
            BatchResult::Aborted { culprit, error } => {
                // The culprit fails. Every other request, including those that had already
                // succeeded (their savepoints were undone too), runs again in a new transaction.
                let job = jobs.remove(culprit);
                (job.reply)(Err(error));
            }
        }
    }
}

fn attempt_batch(db: &mut Connection, jobs: &mut Vec<WriteJob>) -> BatchResult {
    // A previous batch that failed while cleaning up can leave its transaction open, which
    // would fail every later BEGIN. Put the connection back in autocommit first.
    if !db.is_autocommit() {
        let _ = db.execute_batch("ROLLBACK");
    }
    let mut tx = match db.transaction_with_behavior(TransactionBehavior::Immediate) {
        Ok(tx) => tx,
        Err(cause) => {
            let reason = cause.to_string();
            for job in jobs.drain(..) {
                (job.reply)(Err(failure(reason.clone())));
            }
            return BatchResult::Finished;
        }
    };

    let mut outcomes: Vec<Result<Option<Undo>, OfficeError>> = Vec::with_capacity(jobs.len());
    for (index, job) in jobs.iter_mut().enumerate() {
        let outcome = match tx.savepoint() {
            Ok(mut savepoint) => {
                let result = catch_unwind(AssertUnwindSafe(|| (job.attempt)(&savepoint)))
                    .unwrap_or_else(|panic| Err(panicked(panic)));
                match result {
                    Ok(undo) => match savepoint.commit() {
                        Ok(()) => Ok(undo),
                        Err(cause) => {
                            if let Some(undo) = undo {
                                run_undo(undo);
                            }
                            Err(OfficeError::storage(cause))
                        }
                    },
                    // Roll this request back, and only this one. If even that fails the
                    // batch can't be trusted: treat it like an aborted transaction.
                    Err(error) => match savepoint.rollback() {
                        Ok(()) => Err(error),
                        Err(_) => {
                            for undo in outcomes.drain(..).flatten().flatten() {
                                run_undo(undo);
                            }
                            return BatchResult::Aborted {
                                culprit: index,
                                error,
                            };
                        }
                    },
                }
            }
            Err(cause) => Err(OfficeError::storage(cause)),
        };
        match outcome {
            Err(error) if tx.is_autocommit() => {
                // SQLite ended the whole transaction: undo what the earlier requests did
                // outside it, and let the caller run the others again.
                for undo in outcomes.drain(..).flatten().flatten() {
                    run_undo(undo);
                }
                return BatchResult::Aborted {
                    culprit: index,
                    error,
                };
            }
            outcome => outcomes.push(outcome),
        }
    }

    match tx.commit() {
        Ok(()) => {
            for (job, outcome) in jobs.drain(..).zip(outcomes) {
                (job.reply)(outcome.map(|_undo| ()));
            }
        }
        Err(cause) => {
            // Nothing was committed: every request that thought it had succeeded did not.
            // (SQLite can't rule out that the commit reached the WAL before a later fsync
            // failed; after a crash, recovery could then bring rows back whose folders were
            // just undone. A registered organization with no workspace folder is repairable.)
            let reason = cause.to_string();
            for (job, outcome) in jobs.drain(..).zip(outcomes) {
                match outcome {
                    Ok(undo) => {
                        if let Some(undo) = undo {
                            run_undo(undo);
                        }
                        (job.reply)(Err(failure(reason.clone())));
                    }
                    Err(error) => (job.reply)(Err(error)),
                }
            }
        }
    }
    BatchResult::Finished
}

/// Runs `undo`, containing a panic so it can't take down the writer thread (and with it
/// every later write). A panicking undo leaves its effect behind. This log line leaves out
/// the panic's message, which could carry paths or data; the process's panic hook (the
/// server binary's to set) decides what the panic itself prints. Relies on unwinding: a
/// `panic = "abort"` profile would make this moot.
fn run_undo(undo: Undo) {
    if catch_unwind(AssertUnwindSafe(undo)).is_err() {
        eprintln!("org-office: an undo panicked; its effect may remain (continuing)");
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn failure(reason: impl Into<String>) -> OfficeError {
    OfficeError::storage(std::io::Error::other(reason.into()))
}

fn panicked(panic: Box<dyn Any + Send>) -> OfficeError {
    let reason = panic
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_owned());
    failure(format!("the storage writer panicked: {reason}"))
}

/// The statement that hands SQLCipher a raw key: `PRAGMA key = "x'<64 hex digits>'";`.
/// It holds the key, so its bytes are overwritten when it is dropped. It is built in one
/// allocation of exactly its size, so no partial copy is left behind by a reallocation.
/// No `Debug`: it must never be printed. Best effort, and only this copy is wiped: SQLite
/// keeps its own copy of the statement text while preparing it and frees it without zeroing
/// (SQLCipher's `cipher_memory_security` is off), and if preparing fails, rusqlite's error
/// carries a copy that is dropped unwiped.
struct KeyPragma(String);

impl KeyPragma {
    const PREFIX: &'static str = "PRAGMA key = \"x'";
    const SUFFIX: &'static str = "'\";";

    fn new(key: &Key) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let bytes = key.as_bytes();
        let mut text =
            String::with_capacity(Self::PREFIX.len() + bytes.len() * 2 + Self::SUFFIX.len());
        text.push_str(Self::PREFIX);
        for byte in bytes {
            text.push(char::from(HEX[usize::from(byte >> 4)]));
            text.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        text.push_str(Self::SUFFIX);
        Self(text)
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl Drop for KeyPragma {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// Overwrites `text`'s bytes with zeros in place (same buffer, same length). Safe code: the
/// buffer is taken out as bytes, zeroed, and put back (zeros are valid UTF-8). `black_box`
/// keeps the compiler from dropping the writes as dead stores (best effort, as its docs say).
fn wipe(text: &mut String) {
    let mut bytes = std::mem::take(text).into_bytes();
    bytes.fill(0);
    *text = String::from_utf8(bytes).expect("zero bytes are valid UTF-8");
    std::hint::black_box(&*text);
}

/// Opens a connection to the encrypted database and proves the key works. Only with
/// `create` may SQLite make the file if it is missing.
fn connect(db_path: &Path, key: &Key, create: bool) -> Result<Connection, OfficeError> {
    let mut flags = OpenFlags::default();
    if !create {
        flags.remove(OpenFlags::SQLITE_OPEN_CREATE);
    }
    // `db_path` is absolute, so SQLite can't read it as a `file:` URI.
    let db = Connection::open_with_flags(db_path, flags).map_err(OfficeError::storage)?;
    // A raw key skips the passphrase KDF, so opening a connection is cheap. If this fails,
    // say nothing about why: the statement holds the key and rusqlite's errors quote SQL.
    let pragma = KeyPragma::new(key);
    let applied = db.execute_batch(pragma.as_str());
    drop(pragma);
    applied.map_err(|_| failure("could not apply the database key"))?;
    // Before the first read, so a busy database makes it wait instead of failing.
    db.pragma_update(None, "busy_timeout", 5_000)
        .map_err(OfficeError::storage)?;
    // Without SQLCipher, `PRAGMA key` is silently ignored and the data would be written in
    // the clear. Refuse to run on such a build.
    let cipher_version: Option<String> = db
        .query_row("PRAGMA cipher_version", (), |row| row.get(0))
        .ok()
        .filter(|version: &String| !version.is_empty());
    if cipher_version.is_none() {
        return Err(failure("this SQLite build has no encryption (SQLCipher)"));
    }
    // The first real read fails here if the key is wrong or the file isn't ours.
    db.query_row("SELECT count(*) FROM sqlite_master", (), |row| {
        row.get::<_, i64>(0)
    })
    .map_err(OfficeError::storage)?;
    // A commit is on the drive before it is acknowledged. Plain fsync on macOS doesn't flush
    // the drive's cache; `fullfsync` does, and `checkpoint_fullfsync` does the same for the
    // checkpoints that copy the WAL into the database file. Set on every connection, readers
    // included: the last one to close runs the final checkpoint.
    db.execute_batch(
        "PRAGMA synchronous = FULL; PRAGMA fullfsync = ON; PRAGMA checkpoint_fullfsync = ON;",
    )
    .map_err(OfficeError::storage)?;
    // Foreign keys are off by default in SQLite.
    db.pragma_update(None, "foreign_keys", true)
        .map_err(OfficeError::storage)?;
    Ok(db)
}

fn is_unique_violation(error: &rusqlite::Error, column: &str) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, Some(message))
            if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
                && message.contains(column)
    )
}

/// Brings the database to `SCHEMA_VERSION` and returns this office's identity. A new
/// database records `office_id`; an existing one must agree with it.
fn migrate(db: &mut Connection, office_id: &str) -> Result<OfficeInfo, OfficeError> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(OfficeError::storage)?;
    let version: i64 = tx
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(OfficeError::storage)?;
    if version > SCHEMA_VERSION {
        return Err(failure(format!(
            "database schema {version} is newer than this server supports ({SCHEMA_VERSION})"
        )));
    }
    if version < 1 {
        tx.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
             CREATE TABLE organizations (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 name_key TEXT NOT NULL UNIQUE,
                 created_at TEXT NOT NULL
             ) STRICT;",
        )
        .map_err(OfficeError::storage)?;
        for (key, value) in [
            ("office_id", office_id),
            ("office_name", DEFAULT_OFFICE_NAME),
        ] {
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                (key, value),
            )
            .map_err(OfficeError::storage)?;
        }
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(OfficeError::storage)?;
    }
    let meta = |key: &str| -> Result<String, OfficeError> {
        tx.query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .map_err(OfficeError::storage)
    };
    let info = OfficeInfo {
        id: meta("office_id")?,
        name: meta("office_name")?,
    };
    if info.id != office_id {
        return Err(failure("the database belongs to a different office"));
    }
    tx.commit().map_err(OfficeError::storage)?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use tempfile::TempDir;

    use super::*;

    const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn key() -> Key {
        Key::from_bytes([7; 32])
    }

    fn open_in(dir: &TempDir) -> Store {
        Store::open(&dir.path().join("office.db"), &key(), ID, Opening::New)
            .unwrap()
            .0
    }

    fn no_undo() -> Undo {
        Box::new(|| {})
    }

    #[test]
    fn refuses_a_database_from_a_newer_server() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("office.db");
        drop(open_in(&dir));
        let raw = Connection::open(&path).unwrap();
        raw.execute_batch(KeyPragma::new(&key()).as_str()).unwrap();
        raw.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(raw);
        assert!(matches!(
            Store::open(&path, &key(), ID, Opening::New),
            Err(OfficeError::Storage(_))
        ));
    }

    #[test]
    fn opening_an_existing_database_never_creates_one() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("office.db");

        // No file: refused, and none is made.
        assert!(Store::open(&path, &key(), ID, Opening::Existing).is_err());
        assert!(!path.exists(), "a database file was created");

        // An empty file with a WAL beside it: refused, and neither file is touched.
        std::fs::write(&path, b"").unwrap();
        let wal = dir.path().join("office.db-wal");
        std::fs::write(&wal, b"whatever the log holds").unwrap();
        assert!(matches!(
            Store::open(&path, &key(), ID, Opening::Existing),
            Err(OfficeError::Storage(_))
        ));
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            0,
            "it was written to"
        );
        assert_eq!(std::fs::read(&wal).unwrap(), b"whatever the log holds");
    }

    #[test]
    fn opening_an_existing_database_refuses_one_without_the_schema() {
        // What a crash between switching to WAL and the first migration leaves: a keyed
        // database file with no schema.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("office.db");
        {
            let raw = Connection::open(&path).unwrap();
            raw.execute_batch(KeyPragma::new(&key()).as_str()).unwrap();
            let _: String = raw
                .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
                .unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        assert!(!before.is_empty());

        assert!(matches!(
            Store::open(&path, &key(), ID, Opening::Existing),
            Err(OfficeError::Storage(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before, "it was changed");
    }

    #[test]
    fn opening_an_existing_database_finds_its_data() {
        let dir = TempDir::new().unwrap();
        let store = open_in(&dir);
        let name = OrganizationName::parse("Acme").unwrap();
        store.create_organization(&name, |_| Ok(no_undo())).unwrap();
        drop(store);

        let (store, info) =
            Store::open(&dir.path().join("office.db"), &key(), ID, Opening::Existing).unwrap();
        assert_eq!(info.id, ID);
        assert_eq!(store.list_organizations().unwrap().len(), 1);
    }

    #[test]
    fn refuses_a_relative_database_path() {
        // A relative path starting with `file:` would be read by SQLite as a URI.
        let error = Store::open(
            Path::new("file:office.db?cache=shared"),
            &key(),
            ID,
            Opening::New,
        );
        assert!(matches!(error, Err(OfficeError::Storage(_))));
    }

    #[test]
    fn key_pragma_is_64_hex_digits_in_sqlcipher_form() {
        let mut bytes = [0xAB; 32];
        bytes[0] = 0x01;
        bytes[31] = 0xF0;
        assert_eq!(
            KeyPragma::new(&Key::from_bytes(bytes)).as_str(),
            format!("PRAGMA key = \"x'01{}f0'\";", "ab".repeat(30))
        );
    }

    #[test]
    fn key_pragma_fits_its_allocation_exactly() {
        let pragma = KeyPragma::new(&key());
        assert_eq!(pragma.0.len(), pragma.0.capacity(), "it was reallocated");
    }

    #[test]
    fn wipe_zeroes_the_same_buffer_in_place() {
        let mut text = KeyPragma::new(&key()).as_str().to_owned();
        let (pointer, len) = (text.as_ptr(), text.len());

        wipe(&mut text);

        assert_eq!(
            text.as_ptr(),
            pointer,
            "a copy was zeroed, not the original"
        );
        assert_eq!(text.len(), len);
        assert!(text.bytes().all(|b| b == 0));
    }

    #[test]
    fn opening_something_that_is_not_our_database_does_not_quote_the_key() {
        // Opening a file that isn't ours fails (on the first read, not on `PRAGMA key`);
        // whatever the error says, it must not carry the key or the statement.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("notadb");
        std::fs::write(&path, vec![0x55u8; 8192]).unwrap();
        let error = Store::open(&path, &key(), ID, Opening::New).err().unwrap();
        let text = format!("{error:?} {error} {:?}", std::error::Error::source(&error));
        assert!(
            !text.contains(&"07".repeat(32)),
            "the key is in the error: {text}"
        );
        assert!(
            !text.contains("PRAGMA key"),
            "the statement is in the error: {text}"
        );
    }

    #[test]
    fn data_is_encrypted_at_rest() {
        let dir = TempDir::new().unwrap();
        let store = open_in(&dir);
        let name = OrganizationName::parse("Distinctive Corporation Name").unwrap();
        store.create_organization(&name, |_| Ok(no_undo())).unwrap();

        // Read the files while the store is open: the data is still in the WAL.
        let mut all = Vec::new();
        for file in ["office.db", "office.db-wal"] {
            if let Ok(bytes) = std::fs::read(dir.path().join(file)) {
                all.extend(bytes);
            }
        }
        assert!(!all.is_empty());
        assert!(
            !all.windows(b"Distinctive".len())
                .any(|w| w == b"Distinctive"),
            "the organization name is readable in the database files"
        );
        assert!(
            !all.starts_with(b"SQLite format 3\0"),
            "the file header is plain SQLite"
        );
    }

    #[test]
    fn refuses_the_wrong_key_and_a_plain_database() {
        let dir = TempDir::new().unwrap();
        drop(open_in(&dir));
        let path = dir.path().join("office.db");
        assert!(matches!(
            Store::open(&path, &Key::from_bytes([8; 32]), ID, Opening::New),
            Err(OfficeError::Storage(_))
        ));

        let plain = dir.path().join("plain.db");
        Connection::open(&plain)
            .unwrap()
            .execute_batch("CREATE TABLE t (x)")
            .unwrap();
        assert!(matches!(
            Store::open(&plain, &key(), ID, Opening::New),
            Err(OfficeError::Storage(_))
        ));
    }

    #[test]
    fn refuses_a_database_that_belongs_to_another_office() {
        let dir = TempDir::new().unwrap();
        drop(open_in(&dir));
        let other = Store::open(
            &dir.path().join("office.db"),
            &key(),
            "SOMEONEELSESOFFICE000000",
            Opening::New,
        );
        assert!(matches!(other, Err(OfficeError::Storage(_))));
    }

    #[test]
    fn a_failed_before_commit_registers_nothing() {
        let dir = TempDir::new().unwrap();
        let store = open_in(&dir);
        let name = OrganizationName::parse("Acme").unwrap();

        let failed = store.create_organization(&name, |_| Err(std::io::Error::other("no")));
        assert!(matches!(failed, Err(OfficeError::Storage(_))));
        assert!(store.list_organizations().unwrap().is_empty());
        // The name stays free.
        store.create_organization(&name, |_| Ok(no_undo())).unwrap();
    }

    #[test]
    fn a_panicking_request_is_reported_and_leaves_the_writer_alive() {
        let dir = TempDir::new().unwrap();
        let store = open_in(&dir);
        let name = OrganizationName::parse("Acme").unwrap();

        let crashed = store.create_organization(&name, |_| panic!("boom"));
        assert!(matches!(crashed, Err(OfficeError::Storage(_))));
        assert!(store.list_organizations().unwrap().is_empty());
        store.create_organization(&name, |_| Ok(no_undo())).unwrap();
    }

    #[test]
    fn readers_run_while_the_writer_is_busy() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(open_in(&dir));
        let (started, wait_for_start) = mpsc::channel();
        let (release, wait_for_release) = mpsc::channel::<()>();

        let writer = {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                let name = OrganizationName::parse("Slow").unwrap();
                let wait_for_release = Mutex::new(wait_for_release);
                let started = Mutex::new(started);
                store.create_organization(&name, move |_| {
                    lock(&started).send(()).unwrap();
                    lock(&wait_for_release).recv().unwrap();
                    Ok(no_undo())
                })
            })
        };
        // The writer is now inside its transaction, holding the write lock.
        wait_for_start.recv().unwrap();
        assert!(store.list_organizations().unwrap().is_empty());
        release.send(()).unwrap();
        writer.join().unwrap().unwrap();
        assert_eq!(store.list_organizations().unwrap().len(), 1);
    }

    /// Holds the writer busy so that requests sent meanwhile queue up and are taken as one
    /// batch. Returns the function that lets the writer go on.
    fn hold_writer(store: &Arc<Store>) -> (JoinHandle<()>, mpsc::Sender<()>) {
        let (started, wait_for_start) = mpsc::channel();
        let (release, wait_for_release) = mpsc::channel::<()>();
        let store = Arc::clone(store);
        let gate = std::thread::spawn(move || {
            let (started, wait_for_release) = (Mutex::new(started), Mutex::new(wait_for_release));
            store
                .write(move |_| {
                    lock(&started).send(()).unwrap();
                    lock(&wait_for_release).recv().unwrap();
                    Ok(((), None))
                })
                .unwrap();
        });
        wait_for_start.recv().unwrap();
        (gate, release)
    }

    /// Stands in for the office's workspace folders: an id is "present" between a request's
    /// effect and its `Undo`.
    #[derive(Clone, Default)]
    struct Folders(Arc<Mutex<HashSet<String>>>);

    impl Folders {
        fn effect(&self) -> impl Fn(&Organization) -> std::io::Result<Undo> + Send + 'static {
            let folders = self.clone();
            move |organization| {
                lock(&folders.0).insert(organization.id.clone());
                let (folders, id) = (folders.clone(), organization.id.clone());
                Ok(Box::new(move || {
                    lock(&folders.0).remove(&id);
                }))
            }
        }

        fn ids(&self) -> HashSet<String> {
            lock(&self.0).clone()
        }
    }

    fn registered_ids(store: &Store) -> HashSet<String> {
        store
            .list_organizations()
            .unwrap()
            .into_iter()
            .map(|o| o.id)
            .collect()
    }

    #[test]
    fn requests_queued_together_all_commit_and_each_is_answered_after_the_commit() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(open_in(&dir));
        let folders = Folders::default();
        let (gate, release) = hold_writer(&store);

        let workers: Vec<_> = (0..10)
            .map(|n| {
                let (store, effect) = (Arc::clone(&store), folders.effect());
                std::thread::spawn(move || {
                    let name = OrganizationName::parse(&format!("Org {n}")).unwrap();
                    store.create_organization(&name, effect)
                })
            })
            .collect();
        std::thread::sleep(std::time::Duration::from_millis(200));
        release.send(()).unwrap();
        gate.join().unwrap();

        for worker in workers {
            let organization = worker.join().unwrap().unwrap();
            // A returned success is already visible to a reader: it was acknowledged after
            // the commit.
            assert!(registered_ids(&store).contains(&organization.id));
        }
        assert_eq!(registered_ids(&store), folders.ids());
        assert_eq!(registered_ids(&store).len(), 10);
    }

    #[test]
    fn one_failing_request_in_a_batch_fails_alone() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(open_in(&dir));
        let folders = Folders::default();
        let (gate, release) = hold_writer(&store);

        // "Dup" twice: whichever runs second hits AlreadyExists inside the same transaction.
        let names = ["Dup", "Dup", "Other"];
        let workers: Vec<_> = names
            .iter()
            .map(|name| {
                let (store, effect) = (Arc::clone(&store), folders.effect());
                let name = OrganizationName::parse(name).unwrap();
                std::thread::spawn(move || store.create_organization(&name, effect))
            })
            .collect();
        std::thread::sleep(std::time::Duration::from_millis(200));
        release.send(()).unwrap();
        gate.join().unwrap();

        let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 2);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(OfficeError::AlreadyExists)))
                .count(),
            1
        );
        assert_eq!(registered_ids(&store).len(), 2);
        assert_eq!(registered_ids(&store), folders.ids());
    }

    #[test]
    fn the_writer_waits_for_the_drive_before_acknowledging() {
        let dir = TempDir::new().unwrap();
        let store = open_in(&dir);
        // `synchronous` is not asserted: FULL is already the default here, so it proves
        // nothing. The other two are off by default.
        let settings = store
            .write(|db| {
                let pragma = |name: &str| -> Result<i64, OfficeError> {
                    db.query_row(&format!("PRAGMA {name}"), (), |row| row.get(0))
                        .map_err(OfficeError::storage)
                };
                Ok((
                    (pragma("fullfsync")?, pragma("checkpoint_fullfsync")?),
                    None,
                ))
            })
            .unwrap();
        assert_eq!(
            settings,
            (1, 1),
            "fullfsync and checkpoint_fullfsync are ON"
        );
    }

    #[test]
    fn a_failed_commit_fails_every_request_and_undoes_their_effects() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(open_in(&dir));
        // A deferred foreign key every new organization violates: the inserts and the
        // effects succeed, and only COMMIT fails.
        store.run_sql_for_test(
            "CREATE TABLE parent (id INTEGER PRIMARY KEY);
             CREATE TABLE child (
                 parent_id INTEGER REFERENCES parent (id) DEFERRABLE INITIALLY DEFERRED
             );
             CREATE TRIGGER violate AFTER INSERT ON organizations
             BEGIN INSERT INTO child VALUES (999); END;",
        );
        let folders = Folders::default();
        let (gate, release) = hold_writer(&store);

        let workers: Vec<_> = (0..4)
            .map(|n| {
                let (store, effect) = (Arc::clone(&store), folders.effect());
                std::thread::spawn(move || {
                    let name = OrganizationName::parse(&format!("Org {n}")).unwrap();
                    store.create_organization(&name, effect)
                })
            })
            .collect();
        std::thread::sleep(std::time::Duration::from_millis(200));
        release.send(()).unwrap();
        gate.join().unwrap();

        for worker in workers {
            assert!(matches!(
                worker.join().unwrap(),
                Err(OfficeError::Storage(_))
            ));
        }
        assert!(registered_ids(&store).is_empty());
        assert!(
            folders.ids().is_empty(),
            "effects outlived the failed commit"
        );

        // The writer is still usable once the cause is gone.
        store.run_sql_for_test("DROP TRIGGER violate;");
        let name = OrganizationName::parse("After").unwrap();
        store.create_organization(&name, folders.effect()).unwrap();
        assert_eq!(registered_ids(&store), folders.ids());
    }

    #[test]
    fn a_panicking_undo_after_a_failed_commit_does_not_stop_the_writer() {
        let dir = TempDir::new().unwrap();
        let store = open_in(&dir);
        // Only COMMIT fails (a deferred foreign key), so the request's undo runs.
        store.run_sql_for_test(
            "CREATE TABLE parent (id INTEGER PRIMARY KEY);
             CREATE TABLE child (
                 parent_id INTEGER REFERENCES parent (id) DEFERRABLE INITIALLY DEFERRED
             );
             CREATE TRIGGER violate AFTER INSERT ON organizations
             BEGIN INSERT INTO child VALUES (999); END;",
        );
        let name = OrganizationName::parse("Acme").unwrap();
        let panicking: fn(&Organization) -> std::io::Result<Undo> =
            |_| Ok(Box::new(|| panic!("undo failed")));

        assert!(matches!(
            store.create_organization(&name, panicking),
            Err(OfficeError::Storage(_))
        ));

        // The writer thread survived the panic and still takes writes.
        store.run_sql_for_test("DROP TRIGGER violate;");
        store.create_organization(&name, |_| Ok(no_undo())).unwrap();
        assert_eq!(registered_ids(&store).len(), 1);
    }

    #[test]
    fn a_request_whose_own_rollback_fails_is_never_committed_and_the_others_run_again() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(open_in(&dir));
        let folders = Folders::default();
        let (gate, release) = hold_writer(&store);

        let good = |n: usize| {
            let (store, effect) = (Arc::clone(&store), folders.effect());
            let name = OrganizationName::parse(&format!("Good {n}")).unwrap();
            std::thread::spawn(move || store.create_organization(&name, effect))
        };
        let first = good(1);
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Writes a row, releases its own savepoint (so the writer's `ROLLBACK TO` for it will
        // fail while the transaction is still open), then fails.
        let culprit = {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                store.write(|db| {
                    db.execute(
                        "INSERT INTO organizations (id, name, name_key, created_at)
                         VALUES ('HALF', 'Half', 'half', 'x')",
                        (),
                    )
                    .map_err(OfficeError::storage)?;
                    db.execute_batch("RELEASE _rusqlite_sp")
                        .map_err(OfficeError::storage)?;
                    Err::<((), Option<Undo>), _>(failure("failed after releasing its savepoint"))
                })
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        let last = good(2);
        std::thread::sleep(std::time::Duration::from_millis(100));
        release.send(()).unwrap();
        gate.join().unwrap();

        assert!(first.join().unwrap().is_ok());
        assert!(culprit.join().unwrap().is_err());
        assert!(last.join().unwrap().is_ok());
        let ids = registered_ids(&store);
        assert!(!ids.contains("HALF"), "a half-done request was committed");
        assert_eq!(ids.len(), 2);
        assert_eq!(ids, folders.ids());
    }

    #[test]
    fn when_sqlite_ends_the_transaction_the_other_requests_are_run_again() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(open_in(&dir));
        let folders = Folders::default();
        let (gate, release) = hold_writer(&store);

        // Three queued requests; the middle one makes SQLite end the whole transaction
        // (what a full disk or an I/O error does), then fails.
        let good = |n: usize| {
            let (store, effect) = (Arc::clone(&store), folders.effect());
            let name = OrganizationName::parse(&format!("Good {n}")).unwrap();
            std::thread::spawn(move || store.create_organization(&name, effect))
        };
        let first = good(1);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let culprit = {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                store.write(|db| {
                    db.execute_batch("ROLLBACK").map_err(OfficeError::storage)?;
                    Err::<((), Option<Undo>), _>(failure("the transaction was aborted"))
                })
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        let last = good(2);
        std::thread::sleep(std::time::Duration::from_millis(100));
        release.send(()).unwrap();
        gate.join().unwrap();

        assert!(first.join().unwrap().is_ok());
        assert!(culprit.join().unwrap().is_err());
        assert!(last.join().unwrap().is_ok());
        // Exactly the two good organizations exist, each with exactly one surviving effect,
        // whether or not they were re-run.
        assert_eq!(registered_ids(&store).len(), 2);
        assert_eq!(registered_ids(&store), folders.ids());
        // And the writer still works.
        let name = OrganizationName::parse("Later").unwrap();
        store.create_organization(&name, folders.effect()).unwrap();
        assert_eq!(registered_ids(&store), folders.ids());
    }
}
