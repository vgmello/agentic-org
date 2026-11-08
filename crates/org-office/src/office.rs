use std::fs::{self, File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::OfficeError;
use crate::key::{Key, KeyProvider};
use crate::model::{OfficeInfo, Organization};
use crate::name::OrganizationName;
use crate::private_fs::{
    absolute, private_dir, private_file, tighten_dir, tighten_file_if_present,
};
use crate::store::{Store, Undo};
use crate::ulid;

/// An office rooted at a home directory:
///
/// ```text
/// <home>/office.lock                      held while the office is open (single owner)
/// <home>/office.id                        the office's id, in the clear: the key is filed under it
/// <home>/office.db                        the encrypted registry of organizations (see `store`)
/// <home>/organizations/<id>/workspace/    an organization's workspace files
/// ```
///
/// One `Office` per home at a time: the lock makes the server the only writer of the
/// database, as ADR-0001 requires. Safe to share across threads. The office owns the rules
/// (names, the lock, workspace folders); `Store` owns the database engine.
pub struct Office {
    home: PathBuf,
    info: OfficeInfo,
    store: Store,
    _lock: File,
}

impl Office {
    /// Opens (creating on first use) the office at `home`, encrypting its database with the
    /// key `keys` holds for this office. On first use a new key is made and saved with
    /// `keys` *before* the database exists, so a crash can't leave data without its key.
    /// A database whose key is missing is refused, never replaced.
    pub fn open(home: impl Into<PathBuf>, keys: &dyn KeyProvider) -> Result<Self, OfficeError> {
        let home = absolute(home.into()).map_err(OfficeError::storage)?;
        private_dir(&home).map_err(OfficeError::storage)?;
        // The home may already exist (e.g. an override): make it owner-only regardless.
        tighten_dir(&home).map_err(OfficeError::storage)?;

        let lock = private_file(&home.join("office.lock")).map_err(OfficeError::storage)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(OfficeError::InUse),
            Err(TryLockError::Error(e)) => return Err(OfficeError::storage(e)),
        }

        let db_path = home.join("office.db");
        // An unreadable database is an error, not "no database": that would mint a key for it.
        let database_exists = holds_data(&db_path)? || holds_data(&home.join("office.db-wal"))?;
        let id = office_id(&home, database_exists)?;
        let key = office_key(keys, &id, database_exists)?;

        let (store, info) = Store::open(&db_path, &key, &id)?;
        Ok(Self {
            home,
            info,
            store,
            _lock: lock,
        })
    }

    pub fn info(&self) -> &OfficeInfo {
        &self.info
    }

    /// Creates an organization: a registry row and its workspace folder, or neither.
    pub fn create_organization(&self, name: &str) -> Result<Organization, OfficeError> {
        let name = OrganizationName::parse(name).map_err(OfficeError::InvalidName)?;

        // The folder is made before the commit, so a failure to make it registers nothing.
        // A crash between folder and commit leaves an unreferenced folder, which is harmless.
        let home = self.home.clone();
        self.store.create_organization(&name, move |organization| {
            let workspace = workspace_dir(&home, &organization.id);
            if let Err(cause) = private_dir(&workspace) {
                discard_organization_dir(&workspace);
                return Err(cause);
            }
            let undo: Undo = Box::new(move || discard_organization_dir(&workspace));
            Ok(undo)
        })
    }

    /// All organizations in creation order.
    pub fn list_organizations(&self) -> Result<Vec<Organization>, OfficeError> {
        self.store.list_organizations()
    }
}

fn workspace_dir(home: &Path, organization_id: &str) -> PathBuf {
    home.join("organizations")
        .join(organization_id)
        .join("workspace")
}

/// Whether `path` exists and is not empty. Anything but "not found" is an error: an
/// unreadable database must never be mistaken for no database, or a new key would be minted.
fn holds_data(path: &Path) -> Result<bool, OfficeError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len() > 0),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(OfficeError::storage(e)),
    }
}

/// This office's id: read from `office.id`, or minted on first use. The id has to be
/// readable before the database is, because the database key is filed under it.
fn office_id(home: &Path, database_exists: bool) -> Result<String, OfficeError> {
    let path = home.join("office.id");
    let saved = match fs::read_to_string(&path) {
        Ok(text) => Some(text.trim().to_owned()).filter(|id| is_valid_id(id)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(OfficeError::storage(e)),
    };
    if let Some(id) = saved {
        tighten_file_if_present(&path).map_err(OfficeError::storage)?;
        return Ok(id);
    }
    if database_exists {
        // A database without its id can't be matched to a key: don't guess.
        return Err(OfficeError::storage(std::io::Error::other(
            "office.id is missing or invalid but office.db exists",
        )));
    }
    // No database yet, so there is nothing a new id could orphan (a leftover empty or
    // half-written id file from a crashed first run is simply replaced).
    let id = ulid::generate().map_err(OfficeError::storage)?;
    write_id_atomically(home, &path, &id).map_err(OfficeError::storage)?;
    Ok(id)
}

fn is_valid_id(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Writes `id` to `path` so that a crash leaves either no file or the whole file.
fn write_id_atomically(home: &Path, path: &Path, id: &str) -> std::io::Result<()> {
    let temp = home.join("office.id.tmp");
    let mut file = private_file(&temp)?;
    file.set_len(0)?;
    file.write_all(id.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    // Make the rename itself durable.
    File::open(home)?.sync_all()
}

/// The database key for `id`. A new database gets a key that is saved before use; an
/// existing one must find its key, or the office refuses to open.
fn office_key(keys: &dyn KeyProvider, id: &str, database_exists: bool) -> Result<Key, OfficeError> {
    let saved = keys.load(id).map_err(storage_from_key)?;
    match (saved, database_exists) {
        (Some(key), _) => Ok(key),
        (None, false) => {
            let key = Key::generate().map_err(OfficeError::storage)?;
            keys.store(id, &key).map_err(storage_from_key)?;
            Ok(key)
        }
        (None, true) => Err(OfficeError::storage(std::io::Error::other(
            "the database key is missing",
        ))),
    }
}

fn storage_from_key(cause: crate::key::KeyError) -> OfficeError {
    OfficeError::Storage(cause)
}

/// Best-effort removal of `organizations/<id>/` after a failed create; `workspace` is its child.
fn discard_organization_dir(workspace: &Path) {
    if let Some(organization_dir) = workspace.parent() {
        let _ = fs::remove_dir_all(organization_dir);
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::*;
    use crate::key::InMemoryKeys;
    use crate::name::NameError;

    /// The same keys every time, so a test can reopen what it created.
    fn keys() -> InMemoryKeys {
        InMemoryKeys::fixed(Key::from_bytes([9; 32]))
    }

    fn open() -> (TempDir, Office) {
        let home = TempDir::new().unwrap();
        let office = Office::open(home.path().join("office"), &keys()).unwrap();
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

        let office = Office::open(&path, &keys()).unwrap();
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
        drop(Office::open(&path, &keys()).unwrap());
        for file in ["office.lock", "office.db", "office.db-wal", "office.db-shm"] {
            let file = path.join(file);
            if !file.exists() {
                fs::write(&file, b"").unwrap();
            }
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        }

        // Assert while open: SQLite deletes -wal and -shm on a clean close.
        let office = Office::open(&path, &keys()).unwrap();
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
    fn a_commit_failure_leaves_no_row_and_no_folder() {
        let (home, office) = open();
        // A deferred foreign key that every new organization violates: the INSERT and the
        // folder both succeed, and only COMMIT fails.
        office.store.run_sql_for_test(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE parent (id INTEGER PRIMARY KEY);
             CREATE TABLE child (
                 parent_id INTEGER REFERENCES parent (id) DEFERRABLE INITIALLY DEFERRED
             );
             CREATE TRIGGER violate AFTER INSERT ON organizations
             BEGIN INSERT INTO child VALUES (999); END;",
        );

        let error = office.create_organization("Acme").unwrap_err();

        assert!(matches!(error, OfficeError::Storage(_)));
        assert!(office.list_organizations().unwrap().is_empty());
        let organizations = home.path().join("office/organizations");
        assert_eq!(
            fs::read_dir(organizations).unwrap().count(),
            0,
            "the failed create left a folder behind"
        );
    }

    #[test]
    fn the_key_is_saved_before_the_database_is_used_and_reused_after() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let provider = InMemoryKeys::new();

        let office = Office::open(&path, &provider).unwrap();
        let id = office.info().id.clone();
        let created = office.create_organization("Acme").unwrap();
        drop(office);

        assert!(provider.load(&id).unwrap().is_some(), "no key was saved");
        let reopened = Office::open(&path, &provider).unwrap();
        assert_eq!(reopened.list_organizations().unwrap(), vec![created]);
    }

    #[test]
    fn a_database_whose_key_is_gone_is_refused_not_replaced() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        drop(Office::open(&path, &InMemoryKeys::new()).unwrap());
        let db_before = fs::read(path.join("office.db")).unwrap();

        // A fresh provider has no key for this office.
        let fresh = InMemoryKeys::new();
        let result = Office::open(&path, &fresh);

        let error = result.err().expect("opened without a key");
        assert_eq!(
            std::error::Error::source(&error).unwrap().to_string(),
            "the database key is missing"
        );
        let id = fs::read_to_string(path.join("office.id")).unwrap();
        assert!(
            fresh.load(id.trim()).unwrap().is_none(),
            "a new key was minted"
        );
        assert_eq!(fs::read(path.join("office.db")).unwrap(), db_before);
    }

    #[test]
    fn a_database_without_its_office_id_is_refused() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        drop(Office::open(&path, &keys()).unwrap());
        fs::remove_file(path.join("office.id")).unwrap();

        assert!(matches!(
            Office::open(&path, &keys()),
            Err(OfficeError::Storage(_))
        ));
        assert!(!path.join("office.id").exists(), "a new id was minted");
    }

    #[test]
    fn a_wal_without_a_database_file_still_counts_as_data() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let provider = InMemoryKeys::new();
        drop(Office::open(&path, &provider).unwrap());
        // The main file lost its contents, but a write-ahead log survives.
        fs::write(path.join("office.db"), b"").unwrap();
        fs::write(path.join("office.db-wal"), b"not empty").unwrap();

        let fresh = InMemoryKeys::new();
        let result = Office::open(&path, &fresh);

        assert!(matches!(result, Err(OfficeError::Storage(_))));
        let id = fs::read_to_string(path.join("office.id")).unwrap();
        assert!(
            fresh.load(id.trim()).unwrap().is_none(),
            "a new key was minted"
        );
    }

    #[test]
    fn a_database_that_cannot_be_inspected_is_an_error_not_no_database() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let provider = InMemoryKeys::new();
        drop(Office::open(&path, &provider).unwrap());
        // `office.db` becomes a symlink to itself: looking at it fails with something other
        // than "not found".
        fs::remove_file(path.join("office.db")).unwrap();
        std::os::unix::fs::symlink(path.join("office.db"), path.join("office.db")).unwrap();

        let fresh = InMemoryKeys::new();
        let result = Office::open(&path, &fresh);

        assert!(matches!(result, Err(OfficeError::Storage(_))));
        let id = fs::read_to_string(path.join("office.id")).unwrap();
        assert!(
            fresh.load(id.trim()).unwrap().is_none(),
            "a new key was minted"
        );
    }

    #[test]
    fn data_and_identity_survive_reopening() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let (info, created) = {
            let office = Office::open(&path, &keys()).unwrap();
            (
                office.info().clone(),
                office.create_organization("Acme").unwrap(),
            )
        };
        let office = Office::open(&path, &keys()).unwrap();
        assert_eq!(office.info(), &info);
        assert_eq!(info.name, "This Mac");
        assert_eq!(office.list_organizations().unwrap(), vec![created]);
    }

    #[test]
    fn only_one_office_may_own_a_home() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let first = Office::open(&path, &keys()).unwrap();
        assert!(matches!(
            Office::open(&path, &keys()),
            Err(OfficeError::InUse)
        ));
        drop(first);
        Office::open(&path, &keys()).unwrap();
    }
}
