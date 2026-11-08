use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};

use crate::error::OfficeError;
use crate::model::{OfficeInfo, Organization};
use crate::name::OrganizationName;
use crate::private_fs::{absolute, private_dir, private_file, tighten_dir};
use crate::store::Store;

/// An office rooted at a home directory:
///
/// ```text
/// <home>/office.lock                      held while the office is open (single owner)
/// <home>/office.db                        the registry of organizations (see `store`)
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
    /// Opens (creating on first use) the office at `home`.
    pub fn open(home: impl Into<PathBuf>) -> Result<Self, OfficeError> {
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

        let (store, info) = Store::open(&home.join("office.db"))?;
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
        let mut made = None;
        let created = self.store.create_organization(&name, |organization| {
            let workspace = self.workspace_dir(&organization.id);
            made = Some(workspace.clone());
            private_dir(&workspace)
        });
        if created.is_err() {
            if let Some(workspace) = made {
                discard_organization_dir(&workspace);
            }
        }
        created
    }

    /// All organizations in creation order.
    pub fn list_organizations(&self) -> Result<Vec<Organization>, OfficeError> {
        self.store.list_organizations()
    }

    fn workspace_dir(&self, organization_id: &str) -> PathBuf {
        self.home
            .join("organizations")
            .join(organization_id)
            .join("workspace")
    }
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
}
