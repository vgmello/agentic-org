//! Opening the projection's connections (write, read, checkpoint) and removing its files.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use org_store::EngineError;
use rusqlite::{config::DbConfig, Connection, OpenFlags};

use super::refused;

/// How long a connection waits for another's lock before it fails.
const BUSY_WAIT: Duration = Duration::from_secs(5);

/// `path` with `suffix` after its file name: SQLite's `-wal` and `-shm`, and `.new`.
pub(super) fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub(super) fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Removes a database file and what SQLite keeps beside it.
pub(super) fn remove_database(path: &Path) -> Result<(), EngineError> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        remove_if_present(&beside(path, suffix))
            .map_err(|error| refused("removing a projection file", error))?;
    }
    Ok(())
}

/// Opens a connection that never checkpoints when it closes (plan revision 2, F2).
pub(super) fn connect(path: &Path, flags: OpenFlags) -> Result<Connection, EngineError> {
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
pub(super) enum WriterRefused {
    Sqlite(&'static str, rusqlite::Error),
    Engine(EngineError),
}

impl WriterRefused {
    /// Whether SQLite read the file and found it is not a database, or a damaged one. A
    /// busy file, a failed read or a refused permission is neither: the file may be whole.
    pub(super) fn says_no_database(&self) -> bool {
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
pub(super) fn connect_writer(path: &Path) -> Result<Connection, EngineError> {
    Ok(open_writer(path)?)
}

pub(super) fn open_writer(path: &Path) -> Result<Connection, WriterRefused> {
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
pub(super) fn connect_reader(path: &Path) -> Result<Connection, EngineError> {
    let db = connect(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    db.execute_batch("PRAGMA query_only = ON;")
        .map_err(|error| refused("configuring a read connection", error))?;
    Ok(db)
}
