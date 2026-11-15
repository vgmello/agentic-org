//! BUG-100 initialization fence for the host process, which does not link org-office.
//! Keep in sync with crates/org-office/src/store.rs::sqlcipher_ready. If these
//! stores ever share a process, use one shared fence before either store opens.
//! Failure is cached for the process lifetime; restart after fixing its cause.

use super::{Connection, Error};
use std::sync::OnceLock;

static READY: OnceLock<Result<(), Failure>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    NoCipher,
    Sql(rusqlite::ErrorCode),
}
impl From<rusqlite::Error> for Failure {
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::SqliteFailure(error, _)
            | rusqlite::Error::SqlInputError { error, .. } => Self::Sql(error.code),
            _ => Self::Sql(rusqlite::ErrorCode::Unknown),
        }
    }
}
impl From<Failure> for Error {
    fn from(error: Failure) -> Self {
        match error {
            Failure::NoCipher => Self::NoCipher,
            Failure::Sql(code) => Self::Sql(code),
        }
    }
}

fn initialized(
    state: &OnceLock<Result<(), Failure>>,
    init: impl FnOnce() -> Result<(), Failure>,
) -> Result<(), Error> {
    (*state.get_or_init(init)).map_err(Error::from)
}

pub(super) fn ready() -> Result<(), Error> {
    let result = initialized(&READY, || {
        initialize(
            || {
                // SAFETY: SQLite initialization is thread-safe; the OnceLock waits
                // for SQLCipher's EXTRA_INIT before admitting any host opener.
                unsafe { rusqlite::ffi::sqlite3_initialize() }
            },
            || {
                #[allow(clippy::disallowed_methods)] // The only unfenced probe.
                let db = Connection::open_in_memory()?;
                keyed_cipher_version(&db)
            },
        )
    });
    #[cfg(test)]
    if let Some(error) = tests::take_failure() {
        return Err(error.into());
    }
    result
}

fn keyed_cipher_version(db: &Connection) -> Result<String, Failure> {
    // Public throwaway key: no user data or real key enters this probe.
    db.execute_batch(concat!(
        "PRAGMA key=\"x'",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "'\";"
    ))?;
    db.query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .map_err(cipher_version_failure)
}

fn cipher_version_failure(error: rusqlite::Error) -> Failure {
    match error {
        rusqlite::Error::QueryReturnedNoRows => Failure::NoCipher,
        other => other.into(),
    }
}

fn initialize(
    init: impl FnOnce() -> i32,
    keyed_probe: impl FnOnce() -> Result<String, Failure>,
) -> Result<(), Failure> {
    let code = init();
    if code != rusqlite::ffi::SQLITE_OK {
        return Err(Failure::Sql(rusqlite::ffi::Error::new(code).code));
    }
    if keyed_probe()?.is_empty() {
        return Err(Failure::NoCipher);
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests;
