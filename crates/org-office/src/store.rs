//! The office's storage engine. **The only module that knows SQLite**: it owns the
//! connection, the schema and every SQL statement, and speaks to the rest of the crate in
//! the plain values of `model` and `OfficeError`. Replacing the engine (the user is weighing
//! Turso) means replacing this file; `Office` and its rules don't change.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, TransactionBehavior};

use crate::error::OfficeError;
use crate::model::{OfficeInfo, Organization};
use crate::name::OrganizationName;
use crate::private_fs::{private_file, tighten_file_if_present};
use crate::{time, ulid};

const SCHEMA_VERSION: i64 = 1;
const DEFAULT_OFFICE_NAME: &str = "This Mac";

pub(crate) struct Store {
    db: Mutex<Connection>,
}

impl Store {
    /// Opens (creating on first use) the registry at `db_path`, which must be absolute, and
    /// returns it with this office's identity. The caller must already hold the office lock:
    /// the store assumes it is the only writer.
    pub(crate) fn open(db_path: &Path) -> Result<(Self, OfficeInfo), OfficeError> {
        if !db_path.is_absolute() {
            return Err(OfficeError::storage(std::io::Error::other(
                "the database path must be absolute",
            )));
        }
        // SQLite gives new -wal/-shm files the main file's mode, so make that one 0600 first.
        drop(private_file(db_path).map_err(OfficeError::storage)?);
        // Leftover -wal/-shm files from an earlier run keep whatever mode they had.
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = db_path.as_os_str().to_owned();
            sidecar.push(suffix);
            tighten_file_if_present(Path::new(&sidecar)).map_err(OfficeError::storage)?;
        }

        // `db_path` is absolute, so SQLite can't read it as a `file:` URI.
        let mut db = Connection::open(db_path).map_err(OfficeError::storage)?;
        let journal_mode: String = db
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
            .map_err(OfficeError::storage)?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(OfficeError::storage(std::io::Error::other(format!(
                "SQLite refused WAL mode (stayed in {journal_mode})"
            ))));
        }
        db.pragma_update(None, "busy_timeout", 5_000)
            .map_err(OfficeError::storage)?;
        let info = migrate(&mut db)?;
        Ok((Self { db: Mutex::new(db) }, info))
    }

    /// Registers an organization under the write lock, or fails with `AlreadyExists`.
    ///
    /// `before_commit` runs after the row is inserted and before it is committed (the office
    /// makes the workspace folder there). If it fails, nothing is registered. The caller
    /// removes anything `before_commit` made when this returns an error.
    pub(crate) fn create_organization(
        &self,
        name: &OrganizationName,
        before_commit: impl FnOnce(&Organization) -> std::io::Result<()>,
    ) -> Result<Organization, OfficeError> {
        let mut db = self.db();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(OfficeError::storage)?;
        // Stamped under the write lock, so `created_at` never runs against insertion order
        // (barring a backwards clock step). Ids are random within a millisecond: don't sort by id.
        let organization = Organization {
            id: ulid::generate().map_err(OfficeError::storage)?,
            name: name.as_str().to_owned(),
            created_at: time::now_rfc3339(),
        };
        // The write lock is held, so this check can't race. The UNIQUE constraint stays as a
        // backstop, but the answer doesn't depend on the wording of SQLite's error text.
        let name_key = name.uniqueness_key();
        let taken = tx
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM organizations WHERE name_key = ?1)",
                [&name_key],
                |row| row.get::<_, bool>(0),
            )
            .map_err(OfficeError::storage)?;
        if taken {
            return Err(OfficeError::AlreadyExists);
        }
        tx.execute(
            "INSERT INTO organizations (id, name, name_key, created_at) VALUES (?1, ?2, ?3, ?4)",
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

        // A failure here drops `tx`, which rolls the row back.
        before_commit(&organization).map_err(OfficeError::storage)?;
        tx.commit().map_err(OfficeError::storage)?;
        Ok(organization)
    }

    /// All organizations in creation order. Ordered by insertion (`rowid`), not by timestamp
    /// or id: two creates inside one millisecond have equal timestamps and random-suffixed ids.
    pub(crate) fn list_organizations(&self) -> Result<Vec<Organization>, OfficeError> {
        let db = self.db();
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
    }

    /// Test seam: runs raw SQL, to inject failures the engine can't otherwise be made to have.
    #[cfg(test)]
    pub(crate) fn run_sql_for_test(&self, sql: &str) {
        self.db().execute_batch(sql).unwrap();
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the guard can't leave a half-applied change: SQLite rolls an
        // unfinished transaction back when its handle drops.
        self.db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn is_unique_violation(error: &rusqlite::Error, column: &str) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, Some(message))
            if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
                && message.contains(column)
    )
}

/// Brings the database to `SCHEMA_VERSION` and returns this office's identity, minting it
/// on first use.
fn migrate(db: &mut Connection) -> Result<OfficeInfo, OfficeError> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(OfficeError::storage)?;
    let version: i64 = tx
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(OfficeError::storage)?;
    if version > SCHEMA_VERSION {
        return Err(OfficeError::storage(std::io::Error::other(format!(
            "database schema {version} is newer than this server supports ({SCHEMA_VERSION})"
        ))));
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
            ("office_id", ulid::generate().map_err(OfficeError::storage)?),
            ("office_name", DEFAULT_OFFICE_NAME.to_owned()),
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
    tx.commit().map_err(OfficeError::storage)?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn refuses_a_database_from_a_newer_server() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("office.db");
        drop(Store::open(&path).unwrap());
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        assert!(matches!(Store::open(&path), Err(OfficeError::Storage(_))));
    }

    #[test]
    fn refuses_a_relative_database_path() {
        // A relative path starting with `file:` would be read by SQLite as a URI.
        let error = Store::open(Path::new("file:office.db?cache=shared"));
        assert!(matches!(error, Err(OfficeError::Storage(_))));
    }

    #[test]
    fn a_failed_before_commit_registers_nothing() {
        let dir = TempDir::new().unwrap();
        let (store, _) = Store::open(&dir.path().join("office.db")).unwrap();
        let name = OrganizationName::parse("Acme").unwrap();

        let failed = store.create_organization(&name, |_| Err(std::io::Error::other("no")));
        assert!(matches!(failed, Err(OfficeError::Storage(_))));
        assert!(store.list_organizations().unwrap().is_empty());
        // The name stays free.
        store.create_organization(&name, |_| Ok(())).unwrap();
    }
}
