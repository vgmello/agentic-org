use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::error::OfficeError;
use crate::key::{Key, KeyProvider};
use crate::model::{OfficeInfo, Organization};
use crate::name::OrganizationName;
use crate::private_fs::{
    absolute, private_dir, private_file, tighten_dir, tighten_file_if_present,
};
use crate::store::{Opening, Store, Undo};
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
    /// Creates a new office at `home` (made if missing) and opens it. **The only path that
    /// makes a key** (ADR-0002, as amended: `org-office-server --create`): a new id is minted,
    /// a new key is made and saved with `keys` *before* the database exists, so a crash can't
    /// leave data without its key.
    ///
    /// Refuses with `OfficeExists` if `home` already holds an office id, any database file
    /// or an `organizations` folder, and with `KeyExists` if `keys` already holds a key for the
    /// new id. It never reuses or replaces anything. A create that fails once it has started
    /// writing removes its id and database files, so the home can be created again; a key it
    /// saved stays behind (`KeyProvider` can't delete one yet). A crash part-way leaves an
    /// unfinished home that `create` refuses (`OfficeExists`) and `open` can't use
    /// (`NotFound`, or a storage error if the database was half-made), for the caller to remove.
    pub fn create(home: impl Into<PathBuf>, keys: &dyn KeyProvider) -> Result<Self, OfficeError> {
        let home = absolute(home.into()).map_err(OfficeError::storage)?;
        private_dir(&home).map_err(OfficeError::storage)?;
        // The home may already exist (e.g. an override): make it owner-only regardless.
        tighten_dir(&home).map_err(OfficeError::storage)?;
        let lock = lock_home(&home)?;

        for entry in OFFICE_FILES.iter().chain(&["organizations"]) {
            if exists(&home.join(entry))? {
                return Err(OfficeError::OfficeExists);
            }
        }
        let id = ulid::generate().map_err(OfficeError::storage)?;
        if keys.load(&id).map_err(storage_from_key)?.is_some() {
            return Err(OfficeError::KeyExists);
        }
        let started = write_id_atomically(&home, &home.join("office.id"), &id)
            .and_then(|()| Key::generate())
            .map_err(OfficeError::storage)
            .and_then(|key| {
                keys.store(&id, &key).map_err(storage_from_key)?;
                Ok(key)
            })
            .and_then(|key| Store::open(&home.join("office.db"), &key, &id, Opening::New));
        match started {
            Ok((store, info)) => Ok(Self::new(home, info, store, lock)),
            Err(error) => {
                // Still under the lock, and none of these files existed before this create
                // (checked above), so everything removed is its own.
                discard_unfinished_office(&home);
                drop(lock);
                Err(error)
            }
        }
    }

    /// Opens the existing office at `home` with the key `keys` holds for it. **Never makes a
    /// key, an id or a database**: no office at `home` (no database) is `NotFound`, and a
    /// database whose key is missing is `KeyMissing` (fatal), never replaced.
    pub fn open(home: impl Into<PathBuf>, keys: &dyn KeyProvider) -> Result<Self, OfficeError> {
        let home = absolute(home.into()).map_err(OfficeError::storage)?;
        // Look before touching anything, so a path that holds no office is left as it was.
        match fs::metadata(&home) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(OfficeError::NotFound),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(OfficeError::NotFound)
            }
            Err(e) => return Err(OfficeError::storage(e)),
        }
        let (found_id, found_key) = find_office(&home, keys)?;
        tighten_dir(&home).map_err(OfficeError::storage)?;
        let lock = lock_home(&home)?;
        // Again under the lock: another process may have changed the home in between. The
        // key is looked up again only if the id changed (each lookup may be a Keychain prompt).
        if !database_exists(&home)? {
            return Err(OfficeError::NotFound);
        }
        let id = read_office_id(&home)?;
        let key = if id == found_id {
            found_key
        } else {
            find_office(&home, keys)?.1
        };
        tighten_file_if_present(&home.join("office.id")).map_err(OfficeError::storage)?;

        let (store, info) = Store::open(&home.join("office.db"), &key, &id, Opening::Existing)?;
        Ok(Self::new(home, info, store, lock))
    }

    fn new(home: PathBuf, info: OfficeInfo, store: Store, lock: File) -> Self {
        Self {
            home,
            info,
            store,
            _lock: lock,
        }
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

/// The id and key of the office at `home`, found without changing anything: `NotFound`
/// without a database, `KeyMissing` without its key.
fn find_office(home: &Path, keys: &dyn KeyProvider) -> Result<(String, Key), OfficeError> {
    if !database_exists(home)? {
        return Err(OfficeError::NotFound);
    }
    let id = read_office_id(home)?;
    let key = keys
        .load(&id)
        .map_err(storage_from_key)?
        .ok_or(OfficeError::KeyMissing)?;
    Ok((id, key))
}

/// Whether the office at `home` has a database: a non-empty `office.db` or WAL. An
/// unreadable one is an error, not "no database".
fn database_exists(home: &Path) -> Result<bool, OfficeError> {
    Ok(holds_data(&home.join("office.db"))? || holds_data(&home.join("office.db-wal"))?)
}

/// Best-effort removal of what a failed `create` wrote: its id and database files.
fn discard_unfinished_office(home: &Path) {
    for file in OFFICE_FILES {
        let _ = fs::remove_file(home.join(file));
    }
}

/// Whether `path` exists and is not empty. Anything but "not found" is an error: an
/// unreadable database must never be mistaken for no database (`NotFound`).
fn holds_data(path: &Path) -> Result<bool, OfficeError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len() > 0),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(OfficeError::storage(e)),
    }
}

/// The files an office (or a create in progress) keeps in its home. `create` refuses if any
/// exists, and removes them again if it fails.
const OFFICE_FILES: [&str; 6] = [
    "office.id",
    "office.id.tmp",
    "office.db",
    "office.db-wal",
    "office.db-shm",
    "office.db-journal",
];

/// Takes the home's lock, which makes this process the office's only owner.
fn lock_home(home: &Path) -> Result<File, OfficeError> {
    let lock = private_file(&home.join("office.lock")).map_err(OfficeError::storage)?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(TryLockError::WouldBlock) => Err(OfficeError::InUse),
        Err(TryLockError::Error(e)) => Err(OfficeError::storage(e)),
    }
}

/// Whether anything is at `path` (a dangling symlink counts). Anything but "not found" is an
/// error, so `create` never mistakes an unreadable file for a missing one.
fn exists(path: &Path) -> Result<bool, OfficeError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(OfficeError::storage(e)),
    }
}

/// The id of the existing office at `home`, from `office.id`, read without changing
/// anything. The id has to be readable before the database is, because the database key is
/// filed under it, so a database without a valid id is refused rather than guessed at.
fn read_office_id(home: &Path) -> Result<String, OfficeError> {
    let path = home.join("office.id");
    let saved = match fs::read_to_string(&path) {
        Ok(text) => Some(text.trim().to_owned()).filter(|id| is_valid_id(id)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(OfficeError::storage(e)),
    };
    saved.ok_or_else(|| {
        OfficeError::storage(std::io::Error::other(
            "office.id is missing or invalid but office.db exists",
        ))
    })
}

fn is_valid_id(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Writes `id` to `path` so that a crash leaves either no file or the whole file. The
/// temporary file must not exist yet (`create` checked): it is made fresh, never followed
/// through a link.
fn write_id_atomically(home: &Path, path: &Path, id: &str) -> std::io::Result<()> {
    let temp = home.join("office.id.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    file.write_all(id.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    // Make the rename itself durable.
    File::open(home)?.sync_all()
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

    /// A fresh office in a temporary home.
    fn open() -> (TempDir, Office) {
        let home = TempDir::new().unwrap();
        let office = Office::create(home.path().join("office"), &InMemoryKeys::new()).unwrap();
        (home, office)
    }

    /// The id `create` wrote, which the key is filed under.
    fn saved_id(path: &Path) -> String {
        fs::read_to_string(path.join("office.id"))
            .unwrap()
            .trim()
            .to_owned()
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

        let office = Office::create(&path, &InMemoryKeys::new()).unwrap();
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
        let keys = InMemoryKeys::new();
        drop(Office::create(&path, &keys).unwrap());
        for file in ["office.lock", "office.db", "office.db-wal", "office.db-shm"] {
            let file = path.join(file);
            if !file.exists() {
                fs::write(&file, b"").unwrap();
            }
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        }

        // Assert while open: SQLite deletes -wal and -shm on a clean close.
        let office = Office::open(&path, &keys).unwrap();
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

        let office = Office::create(&path, &provider).unwrap();
        let id = office.info().id.clone();
        let created = office.create_organization("Acme").unwrap();
        drop(office);

        assert!(provider.load(&id).unwrap().is_some(), "no key was saved");
        let reopened = Office::open(&path, &provider).unwrap();
        assert_eq!(reopened.list_organizations().unwrap(), vec![created]);
    }

    #[test]
    fn open_with_the_key_missing_is_key_missing_and_mints_nothing() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        drop(Office::create(&path, &InMemoryKeys::new()).unwrap());
        let db_before = fs::read(path.join("office.db")).unwrap();

        fs::remove_file(path.join("office.lock")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

        // A fresh provider has no key for this office.
        let fresh = InMemoryKeys::new();
        let error = Office::open(&path, &fresh)
            .err()
            .expect("opened without a key");

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "the home was changed");
        assert!(!path.join("office.lock").exists(), "a lock file was left");
        assert!(matches!(error, OfficeError::KeyMissing), "{error:?}");
        assert_eq!(error.to_string(), "the office's database key is missing");
        assert!(
            fresh.load(&saved_id(&path)).unwrap().is_none(),
            "a new key was minted"
        );
        assert_eq!(fs::read(path.join("office.db")).unwrap(), db_before);
    }

    #[test]
    fn a_database_without_its_office_id_is_refused() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let keys = InMemoryKeys::new();
        drop(Office::create(&path, &keys).unwrap());
        fs::remove_file(path.join("office.id")).unwrap();

        assert!(matches!(
            Office::open(&path, &keys),
            Err(OfficeError::Storage(_))
        ));
        assert!(!path.join("office.id").exists(), "a new id was minted");
    }

    #[test]
    fn a_wal_without_a_database_file_still_counts_as_data() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let provider = InMemoryKeys::new();
        drop(Office::create(&path, &provider).unwrap());
        // The main file lost its contents, but a write-ahead log survives.
        fs::write(path.join("office.db"), b"").unwrap();
        fs::write(path.join("office.db-wal"), b"not empty").unwrap();

        // Data exists, so a missing key is `KeyMissing`, not `NotFound`.
        let fresh = InMemoryKeys::new();
        let result = Office::open(&path, &fresh);

        assert!(matches!(result, Err(OfficeError::KeyMissing)));
        assert!(
            fresh.load(&saved_id(&path)).unwrap().is_none(),
            "a new key was minted"
        );
    }

    #[test]
    fn a_database_that_cannot_be_inspected_is_an_error_not_no_database() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let provider = InMemoryKeys::new();
        drop(Office::create(&path, &provider).unwrap());
        // `office.db` becomes a symlink to itself: looking at it fails with something other
        // than "not found".
        fs::remove_file(path.join("office.db")).unwrap();
        std::os::unix::fs::symlink(path.join("office.db"), path.join("office.db")).unwrap();

        let fresh = InMemoryKeys::new();
        let result = Office::open(&path, &fresh);

        assert!(matches!(result, Err(OfficeError::Storage(_))));
        assert!(
            fresh.load(&saved_id(&path)).unwrap().is_none(),
            "a new key was minted"
        );
    }

    #[test]
    fn data_and_identity_survive_reopening() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let keys = InMemoryKeys::new();
        let (info, created) = {
            let office = Office::create(&path, &keys).unwrap();
            (
                office.info().clone(),
                office.create_organization("Acme").unwrap(),
            )
        };
        let office = Office::open(&path, &keys).unwrap();
        assert_eq!(office.info(), &info);
        assert_eq!(info.name, "This Mac");
        assert_eq!(office.list_organizations().unwrap(), vec![created]);
    }

    #[test]
    fn only_one_office_may_own_a_home() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let keys = InMemoryKeys::new();
        let first = Office::create(&path, &keys).unwrap();
        assert!(matches!(
            Office::open(&path, &keys),
            Err(OfficeError::InUse)
        ));
        assert!(matches!(
            Office::create(&path, &keys),
            Err(OfficeError::InUse)
        ));
        drop(first);
        Office::open(&path, &keys).unwrap();
    }

    #[test]
    fn create_makes_a_fresh_office_with_a_new_id_and_a_saved_key() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let keys = InMemoryKeys::new();

        let office = Office::create(&path, &keys).unwrap();

        let id = saved_id(&path);
        assert!(is_valid_id(&id));
        assert_eq!(office.info().id, id);
        assert!(keys.load(&id).unwrap().is_some(), "no key was saved");
        assert!(path.join("office.db").is_file());
        assert!(office.list_organizations().unwrap().is_empty());
    }

    #[test]
    fn create_refuses_an_existing_office_and_changes_nothing() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let keys = InMemoryKeys::new();
        drop(Office::create(&path, &keys).unwrap());
        let id = saved_id(&path);
        let key_before = keys.load(&id).unwrap();
        let db_before = fs::read(path.join("office.db")).unwrap();

        assert!(matches!(
            Office::create(&path, &keys),
            Err(OfficeError::OfficeExists)
        ));

        assert_eq!(saved_id(&path), id, "the office id was replaced");
        assert_eq!(keys.load(&id).unwrap(), key_before, "the key was replaced");
        assert_eq!(fs::read(path.join("office.db")).unwrap(), db_before);
    }

    #[test]
    fn create_refuses_any_existing_database_file_even_without_an_id() {
        for file in OFFICE_FILES.iter().chain(&["organizations"]) {
            let home = TempDir::new().unwrap();
            let path = home.path().join("office");
            fs::create_dir(&path).unwrap();
            // Even an empty file: `create` starts only from a clean home.
            fs::write(path.join(file), b"").unwrap();

            assert!(
                matches!(
                    Office::create(&path, &InMemoryKeys::new()),
                    Err(OfficeError::OfficeExists)
                ),
                "{file}"
            );
            if *file != "office.id" {
                assert!(
                    !path.join("office.id").exists(),
                    "an id was minted ({file})"
                );
            }
        }
    }

    #[test]
    fn create_refuses_when_a_key_already_exists_for_the_office() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        // A provider that already holds a key for every office id.
        let keys = InMemoryKeys::fixed(Key::from_bytes([9; 32]));

        assert!(matches!(
            Office::create(&path, &keys),
            Err(OfficeError::KeyExists)
        ));

        for file in ["office.id", "office.db"] {
            assert!(!path.join(file).exists(), "{file} was written");
        }
    }

    #[test]
    fn open_reopens_a_created_office_with_its_saved_key() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let keys = InMemoryKeys::new();
        let created = {
            let office = Office::create(&path, &keys).unwrap();
            office.create_organization("Acme").unwrap()
        };

        let office = Office::open(&path, &keys).unwrap();

        assert_eq!(office.info().id, saved_id(&path));
        assert_eq!(office.list_organizations().unwrap(), vec![created]);
    }

    #[test]
    fn open_with_no_office_is_not_found_and_creates_nothing() {
        let home = TempDir::new().unwrap();

        // No home at all: nothing is made.
        let missing = home.path().join("missing");
        assert!(matches!(
            Office::open(&missing, &InMemoryKeys::new()),
            Err(OfficeError::NotFound)
        ));
        assert!(!missing.exists());

        // An empty home, even with a key on offer for every id: nothing is written or changed.
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let empty = home.path().join("empty");
        fs::create_dir(&empty).unwrap();
        fs::set_permissions(&empty, fs::Permissions::from_mode(0o755)).unwrap();
        let keys = InMemoryKeys::fixed(Key::from_bytes([9; 32]));
        assert!(matches!(
            Office::open(&empty, &keys),
            Err(OfficeError::NotFound)
        ));
        assert_eq!(
            fs::read_dir(&empty).unwrap().count(),
            0,
            "a file was written"
        );
        assert_eq!(mode(&empty), 0o755, "the folder's mode was changed");

        // A file where the home should be: not an office, and left as it was.
        let file = home.path().join("file");
        fs::write(&file, b"not an office").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            Office::open(&file, &keys),
            Err(OfficeError::NotFound)
        ));
        assert_eq!(mode(&file), 0o644, "the file's mode was changed");
    }

    #[test]
    fn open_never_replaces_a_lost_database_with_a_new_one() {
        // The key and id are there, and a WAL with no valid frames survives, but the
        // database itself is empty or gone: refused, not re-created empty.
        for emptied in [true, false] {
            let home = TempDir::new().unwrap();
            let path = home.path().join("office");
            let keys = InMemoryKeys::new();
            drop(Office::create(&path, &keys).unwrap());
            if emptied {
                fs::write(path.join("office.db"), b"").unwrap();
            } else {
                fs::remove_file(path.join("office.db")).unwrap();
            }
            fs::write(path.join("office.db-wal"), b"what survives of the log").unwrap();

            assert!(
                matches!(Office::open(&path, &keys), Err(OfficeError::Storage(_))),
                "emptied: {emptied}"
            );
            assert_eq!(
                fs::read(path.join("office.db-wal")).unwrap(),
                b"what survives of the log",
                "the surviving log was changed (emptied: {emptied})"
            );
            if emptied {
                assert_eq!(fs::metadata(path.join("office.db")).unwrap().len(), 0);
            } else {
                assert!(!path.join("office.db").exists(), "a database was created");
            }
        }
    }

    #[test]
    fn open_leaves_a_folder_with_a_database_but_no_office_id_as_it_was() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(path.join("office.db"), b"someone else's file").unwrap();

        assert!(matches!(
            Office::open(&path, &InMemoryKeys::new()),
            Err(OfficeError::Storage(_))
        ));
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "the folder's mode was changed");
        assert!(!path.join("office.lock").exists(), "a lock file was left");
    }

    #[test]
    fn create_never_writes_through_a_planted_temporary_id_file() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        fs::create_dir(&path).unwrap();
        let victim = home.path().join("victim");
        fs::write(&victim, b"precious").unwrap();
        std::os::unix::fs::symlink(&victim, path.join("office.id.tmp")).unwrap();

        assert!(matches!(
            Office::create(&path, &InMemoryKeys::new()),
            Err(OfficeError::OfficeExists)
        ));
        assert_eq!(fs::read(&victim).unwrap(), b"precious");
        assert!(
            path.join("office.id.tmp").is_symlink(),
            "a file it didn't make was removed"
        );
    }

    /// Saves keys, but puts something that isn't a database where the office's goes first,
    /// so `create` fails only once it opens the store.
    struct SpoilingKeys(PathBuf, InMemoryKeys);

    impl KeyProvider for SpoilingKeys {
        fn load(&self, office_id: &str) -> Result<Option<Key>, crate::key::KeyError> {
            self.1.load(office_id)
        }

        fn store(&self, office_id: &str, key: &Key) -> Result<(), crate::key::KeyError> {
            fs::write(self.0.join("office.db"), vec![0x55u8; 8192])?;
            self.1.store(office_id, key)
        }
    }

    #[test]
    fn a_create_whose_store_fails_to_open_cleans_up_its_files() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");

        assert!(matches!(
            Office::create(&path, &SpoilingKeys(path.clone(), InMemoryKeys::new())),
            Err(OfficeError::Storage(_))
        ));
        for file in OFFICE_FILES {
            assert!(!path.join(file).exists(), "{file} was left behind");
        }

        Office::create(&path, &InMemoryKeys::new()).unwrap();
    }

    /// Holds no keys and fails to save any.
    struct FailingKeys;

    impl KeyProvider for FailingKeys {
        fn load(&self, _office_id: &str) -> Result<Option<Key>, crate::key::KeyError> {
            Ok(None)
        }

        fn store(&self, _office_id: &str, _key: &Key) -> Result<(), crate::key::KeyError> {
            Err("the keychain refused".into())
        }
    }

    #[test]
    fn a_failed_create_cleans_up_so_the_home_can_be_created_again() {
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");

        assert!(matches!(
            Office::create(&path, &FailingKeys),
            Err(OfficeError::Storage(_))
        ));
        for file in ["office.id", "office.id.tmp", "office.db", "office.db-wal"] {
            assert!(!path.join(file).exists(), "{file} was left behind");
        }

        Office::create(&path, &InMemoryKeys::new()).unwrap();
    }

    #[test]
    fn open_after_a_create_that_stopped_before_the_database_is_not_found() {
        // `office.id` and the key were saved, but no database was made.
        let home = TempDir::new().unwrap();
        let path = home.path().join("office");
        let keys = InMemoryKeys::new();
        drop(Office::create(&path, &keys).unwrap());
        for file in ["office.db", "office.db-wal", "office.db-shm"] {
            let _ = fs::remove_file(path.join(file));
        }

        assert!(matches!(
            Office::open(&path, &keys),
            Err(OfficeError::NotFound)
        ));
        assert!(!path.join("office.db").exists(), "a database was created");
    }
}
