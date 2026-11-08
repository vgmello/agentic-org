use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, TransactionBehavior};

use crate::error::OfficeError;
use crate::name::OrganizationName;
use crate::{time, ulid};

const SCHEMA_VERSION: i64 = 1;
const DEFAULT_OFFICE_NAME: &str = "This Mac";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organization {
    /// Opaque ULID made by the office. Directory names use it, never the display name.
    pub id: String,
    pub name: String,
    /// RFC 3339 UTC with milliseconds.
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeInfo {
    pub id: String,
    pub name: String,
}

/// An office rooted at a home directory:
///
/// ```text
/// <home>/office.lock                      held while the office is open (single owner)
/// <home>/office.db                        SQLite (WAL): the registry of organizations
/// <home>/organizations/<id>/workspace/    an organization's workspace files
/// ```
///
/// One `Office` per home at a time: the lock makes the server the only writer of the
/// database, as ADR-0001 requires. Safe to share across threads.
pub struct Office {
    home: PathBuf,
    info: OfficeInfo,
    db: Mutex<Connection>,
    _lock: File,
}

impl Office {
    /// Opens (creating on first use) the office at `home`.
    pub fn open(home: impl Into<PathBuf>) -> Result<Self, OfficeError> {
        let home = absolute(home.into()).map_err(OfficeError::storage)?;
        private_dir(&home).map_err(OfficeError::storage)?;
        // The home may already exist (e.g. an override): make it owner-only regardless.
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700))
            .map_err(OfficeError::storage)?;

        let lock = private_file(&home.join("office.lock")).map_err(OfficeError::storage)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(OfficeError::InUse),
            Err(TryLockError::Error(e)) => return Err(OfficeError::storage(e)),
        }

        // SQLite gives new -wal/-shm files the main file's mode, so make that one 0600 first.
        let db_path = home.join("office.db");
        drop(private_file(&db_path).map_err(OfficeError::storage)?);
        // Leftover -wal/-shm files from an earlier run keep whatever mode they had.
        for name in ["office.db-wal", "office.db-shm"] {
            match fs::set_permissions(home.join(name), fs::Permissions::from_mode(0o600)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(OfficeError::storage(e));
                }
                _ => {}
            }
        }
        // `db_path` is absolute (see `absolute`), so SQLite can't read it as a `file:` URI.
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

        Ok(Self {
            home,
            info,
            db: Mutex::new(db),
            _lock: lock,
        })
    }

    pub fn info(&self) -> &OfficeInfo {
        &self.info
    }

    /// Creates an organization: a registry row and its workspace folder, or neither.
    pub fn create_organization(&self, name: &str) -> Result<Organization, OfficeError> {
        let name = OrganizationName::parse(name).map_err(OfficeError::InvalidName)?;

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
        let workspace = self.workspace_dir(&organization.id);
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

        // The folder is made before the commit: if it fails, dropping `tx` rolls the row back.
        // A crash between the two leaves an unreferenced folder, which is harmless.
        if let Err(cause) = private_dir(&workspace) {
            discard_organization_dir(&workspace);
            return Err(OfficeError::storage(cause));
        }
        if let Err(cause) = tx.commit() {
            discard_organization_dir(&workspace);
            return Err(OfficeError::storage(cause));
        }
        Ok(organization)
    }

    /// All organizations in creation order. Ordered by insertion (`rowid`), not by timestamp
    /// or id: two creates inside one millisecond have equal timestamps and random-suffixed ids.
    pub fn list_organizations(&self) -> Result<Vec<Organization>, OfficeError> {
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

    fn workspace_dir(&self, organization_id: &str) -> PathBuf {
        self.home
            .join("organizations")
            .join(organization_id)
            .join("workspace")
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the guard can't leave a half-applied change: SQLite rolls an
        // unfinished transaction back when its handle drops.
        self.db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Opens `path` for writing, creating it owner-only (0600) if missing; never truncates.
fn private_file(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)?;
    // `mode` only applies on creation; a file left by an earlier run may be looser.
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

/// Makes `path` absolute without touching the filesystem. Needed because the database path
/// goes to SQLite, whose bundled build reads any path starting with `file:` as a URI.
fn absolute(path: PathBuf) -> std::io::Result<PathBuf> {
    std::path::absolute(path)
}

/// Creates `path` and any missing parents as owner-only (0700) directories.
fn private_dir(path: &Path) -> std::io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(path)
}

/// Best-effort removal of `organizations/<id>/` after a failed create; `workspace` is its child.
fn discard_organization_dir(workspace: &Path) {
    if let Some(organization_dir) = workspace.parent() {
        let _ = fs::remove_dir_all(organization_dir);
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
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::*;
    use crate::name::NameError;

    fn open() -> (TempDir, Office) {
        let home = TempDir::new().unwrap();
        let office = Office::open(home.path().join("office")).unwrap();
        (home, office)
    }

    #[test]
    fn creates_and_lists_in_order() {
        let (_home, office) = open();
        assert!(office.list_organizations().unwrap().is_empty());

        let acme = office.create_organization("  Acme  ").unwrap();
        let globex = office.create_organization("Globex").unwrap();

        assert_eq!(acme.name, "Acme");
        assert_eq!(acme.id.len(), 26);
        assert_eq!(acme.created_at.len(), 24);
        assert_eq!(office.list_organizations().unwrap(), vec![acme, globex]);
    }

    #[test]
    fn creates_a_private_workspace_folder_keyed_by_id() {
        let (home, office) = open();
        let org = office.create_organization("Acme").unwrap();

        let workspace = home
            .path()
            .join("office/organizations")
            .join(&org.id)
            .join("workspace");
        assert!(workspace.is_dir());
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&workspace), 0o700);
        assert_eq!(mode(&home.path().join("office")), 0o700);
    }

    #[test]
    fn lists_in_creation_order_even_within_one_millisecond() {
        let (_home, office) = open();
        let created: Vec<_> = (0..50)
            .map(|n| office.create_organization(&format!("Org {n}")).unwrap())
            .collect();
        assert_eq!(office.list_organizations().unwrap(), created);
    }

    #[test]
    fn tightens_a_pre_existing_home_to_owner_only() {
        let parent = TempDir::new().unwrap();
        let path = parent.path().join("office");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

        let office = Office::open(&path).unwrap();
        office.create_organization("Acme").unwrap();

        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o700);
        for file in ["office.lock", "office.db", "office.db-wal"] {
            assert_eq!(mode(&path.join(file)), 0o600, "{file}");
        }
    }

    #[test]
    fn tightens_database_files_left_loose_by_an_earlier_run() {
        let parent = TempDir::new().unwrap();
        let path = parent.path().join("office");
        drop(Office::open(&path).unwrap());
        for file in ["office.lock", "office.db", "office.db-wal", "office.db-shm"] {
            let file = path.join(file);
            if !file.exists() {
                fs::write(&file, b"").unwrap();
            }
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        }

        // Assert while open: SQLite deletes -wal and -shm on a clean close.
        let office = Office::open(&path).unwrap();
        office.create_organization("Acme").unwrap();

        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        for file in ["office.lock", "office.db", "office.db-wal", "office.db-shm"] {
            assert_eq!(mode(&path.join(file)), 0o600, "{file}");
        }
    }

    #[test]
    fn a_relative_home_that_looks_like_a_sqlite_uri_becomes_a_plain_absolute_path() {
        // Checked on the helper: opening a relative home would write into the test's cwd.
        let path = absolute(PathBuf::from("file:office?cache=shared")).unwrap();
        assert!(path.is_absolute());
        assert!(!path.to_str().unwrap().starts_with("file:"));
        assert!(path.join("office.db").to_str().unwrap().starts_with('/'));
    }

    #[test]
    fn created_at_follows_insertion_order_under_concurrency() {
        let (_home, office) = open();
        let office = Arc::new(office);
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let office = Arc::clone(&office);
                std::thread::spawn(move || {
                    for n in 0..30 {
                        office.create_organization(&format!("Org {t}-{n}")).unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let listed = office.list_organizations().unwrap();
        assert_eq!(listed.len(), 240);
        assert!(listed
            .windows(2)
            .all(|w| w[0].created_at <= w[1].created_at));
    }

    #[test]
    fn rejects_invalid_names_without_side_effects() {
        let (home, office) = open();
        for (raw, reason) in [
            ("", NameError::Empty),
            ("   ", NameError::Empty),
            ("a\nb", NameError::ControlCharacter),
            (&"x".repeat(65), NameError::TooLong),
        ] {
            assert!(matches!(
                office.create_organization(raw),
                Err(OfficeError::InvalidName(r)) if r == reason
            ));
        }
        assert!(office.list_organizations().unwrap().is_empty());
        assert!(!home.path().join("office/organizations").exists());
    }

    #[test]
    fn names_are_unique_ignoring_case_and_outer_space() {
        let (_home, office) = open();
        office.create_organization("Acme").unwrap();
        assert!(matches!(
            office.create_organization("  aCME "),
            Err(OfficeError::AlreadyExists)
        ));
        assert_eq!(office.list_organizations().unwrap().len(), 1);
    }

    #[test]
    fn concurrent_creates_of_one_name_have_one_winner() {
        let (_home, office) = open();
        let office = Arc::new(office);
        let results: Vec<_> = (0..8)
            .map(|_| {
                let office = Arc::clone(&office);
                std::thread::spawn(move || office.create_organization("Acme"))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert!(results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(|e| matches!(e, OfficeError::AlreadyExists)));
    }

    #[test]
    fn storage_failure_leaves_no_row_and_hides_paths() {
        let (home, office) = open();
        // A file where the `organizations` folder should be makes folder creation fail.
        fs::write(home.path().join("office/organizations"), b"in the way").unwrap();

        let error = office.create_organization("Acme").unwrap_err();
        assert!(matches!(error, OfficeError::Storage(_)));
        // Exact text: the cause is only reachable through `source()`, never `Display`.
        assert_eq!(
            error.to_string(),
            "the office could not read or write its storage"
        );
        assert!(std::error::Error::source(&error).is_some());
        assert!(office.list_organizations().unwrap().is_empty());

        // The name was not consumed by the failed attempt.
        fs::remove_file(home.path().join("office/organizations")).unwrap();
        office.create_organization("Acme").unwrap();
    }

    #[test]
    fn data_and_identity_survive_reopening() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let (info, created) = {
            let office = Office::open(&path).unwrap();
            (
                office.info().clone(),
                office.create_organization("Acme").unwrap(),
            )
        };
        let office = Office::open(&path).unwrap();
        assert_eq!(office.info(), &info);
        assert_eq!(info.name, "This Mac");
        assert_eq!(office.list_organizations().unwrap(), vec![created]);
    }

    #[test]
    fn only_one_office_may_own_a_home() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let first = Office::open(&path).unwrap();
        assert!(matches!(Office::open(&path), Err(OfficeError::InUse)));
        drop(first);
        Office::open(&path).unwrap();
    }

    #[test]
    fn refuses_a_database_from_a_newer_server() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        drop(Office::open(&path).unwrap());
        Connection::open(path.join("office.db"))
            .unwrap()
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        assert!(matches!(Office::open(&path), Err(OfficeError::Storage(_))));
    }
}
