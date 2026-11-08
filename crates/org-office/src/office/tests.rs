use std::os::unix::fs::PermissionsExt;

use tempfile::TempDir;

use super::*;
use crate::key::{InMemoryKeys, KeyError};
use crate::model::OfficeRole;

const ID: &str = "OFFICE1";
const SHORT: Duration = Duration::from_millis(100);

fn new_office(id: &str) -> NewOffice {
    NewOffice {
        office_id: id.into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        org_id: format!("ORG{id}"),
        org_name: "Acme".into(),
    }
}

fn folder_of(dir: &TempDir, id: &str) -> PathBuf {
    dir.path().join("offices").join(id)
}

fn made() -> (TempDir, InMemoryKeys, Office) {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let office = Office::create(folder_of(&dir, ID), &new_office(ID), &keys).unwrap();
    (dir, keys, office)
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// A key store that cannot delete.
struct CannotDelete(InMemoryKeys);

impl KeyProvider for CannotDelete {
    fn load(&self, id: &str) -> Result<Option<Key>, KeyError> {
        self.0.load(id)
    }
    fn store(&self, id: &str, key: &Key) -> Result<(), KeyError> {
        self.0.store(id, key)
    }
    fn delete(&self, _id: &str) -> Result<(), KeyError> {
        Err("the keychain refuses".into())
    }
}

/// A key store whose `delete` tries to take the folder's lock: it records whether it found it taken.
struct ChecksLock {
    inner: InMemoryKeys,
    folder: PathBuf,
    lock_was_held: std::sync::atomic::AtomicBool,
}
impl KeyProvider for ChecksLock {
    fn load(&self, id: &str) -> Result<Option<Key>, KeyError> {
        self.inner.load(id)
    }
    fn store(&self, id: &str, key: &Key) -> Result<(), KeyError> {
        self.inner.store(id, key)
    }
    fn delete(&self, id: &str) -> Result<(), KeyError> {
        let probe = private_file(&self.folder.join(LOCK_FILE))?;
        let held = matches!(probe.try_lock(), Err(TryLockError::WouldBlock));
        self.lock_was_held
            .store(held, std::sync::atomic::Ordering::SeqCst);
        self.inner.delete(id)
    }
}

// ---- create ---------------------------------------------------------------------------------

#[test]
fn create_makes_the_whole_folder_with_a_saved_key_and_office_json() {
    let (dir, keys, office) = made();
    let folder = folder_of(&dir, ID);

    assert_eq!(
        names_in(&folder),
        [
            "office.db",
            "office.db-shm",
            "office.db-wal",
            "office.json",
            "office.lock",
            "org.db",
            "org.db-shm",
            "org.db-wal",
            "workspace"
        ]
    );
    assert_eq!(office.folder(), folder);
    let info = office.info();
    assert_eq!(
        (
            info.office_id.as_str(),
            info.office_name.as_str(),
            info.org_name.as_str()
        ),
        (ID, "HQ", "Acme")
    );
    assert_eq!(info.role, OfficeRole::Hq);
    assert_eq!(office_file::read(&folder).unwrap().as_ref(), Some(info));
    assert!(
        keys.load(ID).unwrap().is_some(),
        "the key was saved under the office id"
    );

    // Owner-only everywhere.
    assert_eq!(mode(&folder), 0o700);
    assert_eq!(mode(&folder.join("workspace")), 0o700);
    for file in ["office.json", "office.lock", "org.db", "office.db"] {
        assert_eq!(mode(&folder.join(file)), 0o600, "{file}");
    }
}

#[test]
fn both_databases_are_encrypted_with_the_one_key() {
    let (dir, keys, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    for file in ["org.db", "office.db"] {
        let bytes = fs::read(folder.join(file)).unwrap();
        assert!(
            !bytes.starts_with(b"SQLite format 3"),
            "{file} is in the clear"
        );
        assert!(
            !bytes.windows(4).any(|w| w == b"Acme" || w == b"meta"),
            "{file} shows its contents"
        );
    }
    // The same key opens both.
    let key = keys.load(ID).unwrap().unwrap();
    for file in ["org.db", "office.db"] {
        Store::open(&folder.join(file), &key, Opening::Existing, &ORG_SCHEMA).unwrap();
    }
}

#[test]
fn a_failed_create_leaves_nothing_but_the_lock_file_and_no_key_and_can_be_repeated() {
    for step in ["databases", "workspace", "organization", "office_json"] {
        let dir = TempDir::new().unwrap();
        let keys = InMemoryKeys::new();
        let folder = folder_of(&dir, ID);

        FAIL_AT.with(|f| f.set(Some(step)));
        let failed = Office::create(&folder, &new_office(ID), &keys);
        FAIL_AT.with(|f| f.set(None));

        assert!(matches!(failed, Err(OfficeError::Storage(_))), "{step}");
        assert_eq!(names_in(&folder), ["office.lock"], "{step}");
        assert!(
            keys.load(ID).unwrap().is_none(),
            "{step}: the key it made is gone"
        );
        // The id can be created again.
        Office::create(&folder, &new_office(ID), &keys).unwrap();
    }
}

#[test]
fn office_json_is_written_after_every_other_step() {
    REACHED.with(|reached| reached.borrow_mut().clear());
    let (dir, _keys, _office) = made();
    let steps = REACHED.with(|reached| reached.borrow().clone());
    assert_eq!(
        steps,
        [
            ("databases", false),
            ("workspace", false),
            ("organization", false),
            ("office_json", false)
        ],
        "office.json must not exist before the last step"
    );
    assert!(folder_of(&dir, ID).join("office.json").exists());
}

#[test]
fn an_office_without_its_office_json_is_unfinished() {
    // A create that stops just before it leaves a folder the host sees as unfinished: no
    // `office.json`, though both databases hold the organization.
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let folder = folder_of(&dir, ID);
    let office = Office::create(&folder, &new_office(ID), &keys).unwrap();
    drop(office);
    fs::remove_file(folder.join("office.json")).unwrap();
    // ...and `open` does not treat it as an office.
    assert!(matches!(
        Office::open(&folder, &keys),
        Err(OfficeError::NotFound)
    ));
}

#[test]
fn create_refuses_anything_already_there_and_changes_none_of_it() {
    for planted in [
        "office.json",
        "office.json.tmp",
        "org.db",
        "office.db-wal",
        "workspace",
    ] {
        let dir = TempDir::new().unwrap();
        let keys = InMemoryKeys::new();
        let folder = folder_of(&dir, ID);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(planted), b"precious").unwrap();
        let before = names_in(&folder);

        let result = Office::create(&folder, &new_office(ID), &keys);

        assert!(
            matches!(result, Err(OfficeError::OfficeExists)),
            "{planted}"
        );
        let after = names_in(&folder);
        assert!(
            before.iter().all(|n| after.contains(n)),
            "{planted} vanished"
        );
        assert!(
            keys.load(ID).unwrap().is_none(),
            "{planted}: a key was made"
        );
        assert_eq!(fs::read(folder.join(planted)).unwrap(), b"precious");
    }
}

#[test]
fn create_refuses_an_id_that_already_has_a_key_and_keeps_that_key() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let old = Key::from_bytes([9; 32]);
    keys.store(ID, &old).unwrap();

    let result = Office::create(folder_of(&dir, ID), &new_office(ID), &keys);

    assert!(matches!(result, Err(OfficeError::KeyExists)));
    assert_eq!(
        keys.load(ID).unwrap(),
        Some(old),
        "the existing key was replaced or deleted"
    );
    assert_eq!(names_in(&folder_of(&dir, ID)), ["office.lock"]);
}

#[test]
fn create_checks_its_arguments_before_touching_anything() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let make = |change: &dyn Fn(&mut NewOffice)| {
        let mut new = new_office(ID);
        change(&mut new);
        Office::create(folder_of(&dir, ID), &new, &keys)
    };
    // A bad id is `Invalid`.
    for result in [
        make(&|n| n.office_id = "has-dash".into()),
        make(&|n| n.office_id = String::new()),
        make(&|n| n.office_id = "x".repeat(65)),
        make(&|n| n.org_id = "../up".into()),
    ] {
        assert!(matches!(result, Err(OfficeError::Invalid(_))), "{result:?}");
    }
    // A bad name is `InvalidName` (the host answers `invalid_name`), by the contract's rule: a
    // format character such as U+200B is refused too, which a control-character check misses.
    for name in ["   ", "bad\u{7}name", "zero\u{200B}width"] {
        let result = make(&|n| n.org_name = name.into());
        assert!(
            matches!(result, Err(OfficeError::InvalidName(_))),
            "{name:?}: {result:?}"
        );
        let result = make(&|n| n.office_name = name.into());
        assert!(
            matches!(result, Err(OfficeError::InvalidName(_))),
            "{name:?}: {result:?}"
        );
    }
    // A folder that is not named by the office id.
    let wrong = Office::create(
        dir.path().join("offices").join("OTHER"),
        &new_office(ID),
        &keys,
    );
    assert!(matches!(wrong, Err(OfficeError::Invalid(_))));
    assert!(!dir.path().join("offices").exists(), "something was made");
    assert!(keys.load(ID).unwrap().is_none());
}

#[test]
fn names_are_trimmed_into_office_json() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let mut new = new_office(ID);
    new.org_name = "  Acme Corp ".into();
    let office = Office::create(folder_of(&dir, ID), &new, &keys).unwrap();
    assert_eq!(office.info().org_name, "Acme Corp");
}

// ---- open -----------------------------------------------------------------------------------

#[test]
fn open_reopens_what_create_made() {
    let (dir, keys, office) = made();
    let info = office.info().clone();
    drop(office);

    let again = Office::open(folder_of(&dir, ID), &keys).unwrap();

    assert_eq!(again.info(), &info);
}

#[test]
fn one_process_at_a_time_owns_an_office_and_a_new_one_waits_for_the_old() {
    let (dir, keys, office) = made();
    let folder = folder_of(&dir, ID);

    assert!(matches!(
        Office::open_waiting(&folder, &keys, SHORT),
        Err(OfficeError::InUse)
    ));
    assert!(matches!(
        Office::create_waiting(&folder, &new_office(ID), &keys, SHORT),
        Err(OfficeError::InUse)
    ));
    assert!(matches!(
        Office::discard_waiting(&folder, ID, &keys, SHORT),
        Err(OfficeError::InUse)
    ));

    // A replacement that starts while the old one is still shutting down waits for it.
    let waiter = {
        let (folder, keys) = (folder.clone(), keys);
        std::thread::spawn(move || {
            Office::open_waiting(&folder, &keys, Duration::from_secs(10)).map(|_| ())
        })
    };
    std::thread::sleep(Duration::from_millis(200));
    drop(office);
    waiter.join().unwrap().unwrap();
}

#[test]
fn open_makes_nothing() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let folder = folder_of(&dir, ID);

    // No folder.
    assert!(matches!(
        Office::open(&folder, &keys),
        Err(OfficeError::NotFound)
    ));
    assert!(!dir.path().join("offices").exists());

    // A folder with no office.json: an unfinished create, not an office.
    fs::create_dir_all(&folder).unwrap();
    assert!(matches!(
        Office::open(&folder, &keys),
        Err(OfficeError::NotFound)
    ));
    assert_eq!(names_in(&folder), ["office.lock"]);
    assert!(keys.load(ID).unwrap().is_none());
}

#[test]
fn open_with_the_key_gone_is_key_missing_and_mints_nothing() {
    let (dir, keys, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    keys.delete(ID).unwrap();
    let before = names_in(&folder);

    assert!(matches!(
        Office::open(&folder, &keys),
        Err(OfficeError::KeyMissing)
    ));

    assert!(keys.load(ID).unwrap().is_none(), "a key was made");
    assert_eq!(names_in(&folder), before);
}

#[test]
fn open_with_the_wrong_key_is_an_error_that_leaves_the_files_alone() {
    let (dir, keys, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    keys.delete(ID).unwrap();
    keys.store(ID, &Key::from_bytes([1; 32])).unwrap();
    let before = fs::read(folder.join("org.db")).unwrap();

    assert!(matches!(
        Office::open(&folder, &keys),
        Err(OfficeError::Storage(_))
    ));

    assert_eq!(fs::read(folder.join("org.db")).unwrap(), before);
}

#[test]
fn open_refuses_an_office_json_that_names_another_office_or_organization() {
    let (dir, keys, office) = made();
    let folder = folder_of(&dir, ID);
    let info = office.info().clone();
    drop(office);

    // Another office id than the folder's name.
    let mut other = info.clone();
    other.office_id = "ELSEWHERE".into();
    fs::remove_file(folder.join("office.json")).unwrap();
    office_file::write(&folder, &other).unwrap();
    assert!(matches!(
        Office::open(&folder, &keys),
        Err(OfficeError::Invalid(_))
    ));

    // Another organization than the databases hold.
    let mut other = info;
    other.org_id = "SOMEONEELSE".into();
    fs::remove_file(folder.join("office.json")).unwrap();
    office_file::write(&folder, &other).unwrap();
    assert!(matches!(
        Office::open(&folder, &keys),
        Err(OfficeError::Invalid(_))
    ));
}

#[test]
fn open_refuses_a_missing_database_and_does_not_make_one() {
    let (dir, keys, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    for name in ["office.db", "office.db-wal", "office.db-shm"] {
        let _ = fs::remove_file(folder.join(name));
    }

    assert!(matches!(
        Office::open(&folder, &keys),
        Err(OfficeError::Storage(_))
    ));
    assert!(!folder.join("office.db").exists());
}

// ---- discard --------------------------------------------------------------------------------

#[test]
fn discard_removes_an_unfinished_create_and_its_key() {
    // (a) a folder with only the lock; (b) databases made but the organization not committed.
    for with_schema_only_database in [false, true] {
        let dir = TempDir::new().unwrap();
        let keys = InMemoryKeys::new();
        let folder = folder_of(&dir, ID);
        fs::create_dir_all(&folder).unwrap();
        let key = Key::generate().unwrap();
        keys.store(ID, &key).unwrap();
        if with_schema_only_database {
            drop(Store::open(&folder.join("org.db"), &key, Opening::New, &ORG_SCHEMA).unwrap());
            drop(
                Store::open(
                    &folder.join("office.db"),
                    &key,
                    Opening::New,
                    &OFFICE_SCHEMA,
                )
                .unwrap(),
            );
            fs::create_dir(folder.join("workspace")).unwrap();
        }

        Office::discard(&folder, ID, &keys).unwrap();

        assert!(!folder.exists(), "the folder is still there");
        assert!(keys.load(ID).unwrap().is_none(), "the key is still there");
    }
}

#[test]
fn discard_never_removes_a_finished_office() {
    let (dir, keys, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    let before = names_in(&folder);

    assert!(matches!(
        Office::discard(&folder, ID, &keys),
        Err(OfficeError::OfficeExists)
    ));

    assert_eq!(names_in(&folder), before);
    assert!(keys.load(ID).unwrap().is_some());
    Office::open(&folder, &keys).unwrap();
}

#[test]
fn discard_never_removes_an_office_that_lost_its_office_json() {
    let (dir, keys, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    fs::remove_file(folder.join("office.json")).unwrap();

    assert!(matches!(
        Office::discard(&folder, ID, &keys),
        Err(OfficeError::OfficeExists)
    ));

    assert!(folder.join("org.db").exists());
    assert!(keys.load(ID).unwrap().is_some());
}

#[test]
fn discard_refuses_databases_it_cannot_check() {
    let (dir, keys, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    fs::remove_file(folder.join("office.json")).unwrap();

    // The key is gone.
    let kept = keys.load(ID).unwrap().unwrap();
    keys.delete(ID).unwrap();
    assert!(matches!(
        Office::discard(&folder, ID, &keys),
        Err(OfficeError::Storage(_))
    ));
    assert!(folder.join("org.db").exists());

    // The key is wrong.
    keys.store(ID, &Key::from_bytes([3; 32])).unwrap();
    assert!(matches!(
        Office::discard(&folder, ID, &keys),
        Err(OfficeError::Storage(_))
    ));
    assert!(folder.join("org.db").exists());
    keys.delete(ID).unwrap();
    keys.store(ID, &kept).unwrap();
}

#[test]
fn discard_whose_keychain_refuses_fails_and_a_retry_finishes_the_job() {
    // Databases made, organization not committed; the keychain refuses to delete.
    let dir = TempDir::new().unwrap();
    let inner = InMemoryKeys::new();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(&folder).unwrap();
    let key = Key::generate().unwrap();
    inner.store(ID, &key).unwrap();
    drop(Store::open(&folder.join("org.db"), &key, Opening::New, &ORG_SCHEMA).unwrap());
    let keys = CannotDelete(inner);

    assert!(matches!(
        Office::discard(&folder, ID, &keys),
        Err(OfficeError::Storage(_))
    ));

    // The folder holds nothing the key was for, so a retry is not blocked by databases whose
    // key is gone.
    assert_eq!(names_in(&folder), ["office.lock"]);
    Office::discard(&folder, ID, &keys.0).unwrap();
    assert!(!folder.exists());
    assert!(keys.0.load(ID).unwrap().is_none());
}

#[test]
fn discard_of_a_folder_that_is_not_there_still_removes_its_orphan_key() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    keys.store(ID, &Key::from_bytes([5; 32])).unwrap();

    Office::discard(folder_of(&dir, ID), ID, &keys).unwrap();

    assert!(keys.load(ID).unwrap().is_none());
    assert!(!folder_of(&dir, ID).exists(), "it left a folder");
}

#[test]
fn discard_checks_the_id_and_the_folder_name() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(&folder).unwrap();

    for (path, id) in [(&folder, "has-dash"), (&folder, "OTHER"), (&folder, "")] {
        assert!(
            matches!(
                Office::discard(path, id, &keys),
                Err(OfficeError::Invalid(_))
            ),
            "{id:?}"
        );
    }
    assert!(folder.exists());
}

#[test]
fn discard_without_a_folder_removes_the_key_and_with_a_held_lock_gives_up() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let folder = folder_of(&dir, ID);
    keys.store(ID, &Key::from_bytes([6; 32])).unwrap();

    // With the folder absent the discard takes no lock and removes the orphan key.
    Office::discard(&folder, ID, &keys).unwrap();
    assert!(keys.load(ID).unwrap().is_none());

    // While someone holds the folder's lock (a create of the same id) it waits, then gives
    // up without touching the key.
    fs::create_dir_all(&folder).unwrap();
    keys.store(ID, &Key::from_bytes([6; 32])).unwrap();
    let _held = lock_folder(&folder, SHORT).unwrap();
    assert!(matches!(
        Office::discard_waiting(&folder, ID, &keys, SHORT),
        Err(OfficeError::InUse)
    ));
    assert!(keys.load(ID).unwrap().is_some());
}

#[test]
fn a_lock_taken_on_a_file_that_was_unlinked_does_not_count() {
    let dir = TempDir::new().unwrap();
    let folder = dir.path().join("f");
    fs::create_dir_all(&folder).unwrap();
    let first = lock_folder(&folder, SHORT).unwrap();
    // A waiter opened the lock file; then the owner removed it and made a new one.
    let stale = private_file(&folder.join(LOCK_FILE)).unwrap();
    fs::remove_file(folder.join(LOCK_FILE)).unwrap();
    drop(first);
    stale.try_lock().unwrap();
    assert!(!is_the_file_at(&stale, &folder.join(LOCK_FILE)));
    let fresh = lock_folder(&folder, SHORT).unwrap();
    assert!(is_the_file_at(&fresh, &folder.join(LOCK_FILE)));
}

#[test]
fn the_cleanup_of_a_failed_create_happens_while_the_lock_is_held() {
    let dir = TempDir::new().unwrap();
    let folder = folder_of(&dir, ID);
    let keys = ChecksLock {
        inner: InMemoryKeys::new(),
        folder: folder.clone(),
        lock_was_held: false.into(),
    };

    FAIL_AT.with(|f| f.set(Some("workspace")));
    let failed = Office::create(&folder, &new_office(ID), &keys);
    FAIL_AT.with(|f| f.set(None));

    assert!(failed.is_err());
    assert!(keys.lock_was_held.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn a_key_that_cannot_be_saved_fails_the_create_and_leaves_nothing() {
    struct CannotStore;
    impl KeyProvider for CannotStore {
        fn load(&self, _id: &str) -> Result<Option<Key>, KeyError> {
            Ok(None)
        }
        fn store(&self, _id: &str, _key: &Key) -> Result<(), KeyError> {
            Err("the keychain refuses".into())
        }
        fn delete(&self, _id: &str) -> Result<(), KeyError> {
            Ok(())
        }
    }
    let dir = TempDir::new().unwrap();
    let folder = folder_of(&dir, ID);

    let failed = Office::create(&folder, &new_office(ID), &CannotStore);

    assert!(matches!(failed, Err(OfficeError::Storage(_))));
    assert_eq!(names_in(&folder), ["office.lock"]);
}

#[test]
fn a_loose_folder_is_tightened_by_create() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(&folder).unwrap();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();

    Office::create(&folder, &new_office(ID), &keys).unwrap();

    assert_eq!(mode(&folder), 0o700);
}

#[test]
fn open_checks_that_each_database_belongs_to_the_office() {
    for file in ["org.db", "office.db"] {
        let (dir, keys, office) = made();
        let folder = folder_of(&dir, ID);
        drop(office);
        let key = keys.load(ID).unwrap().unwrap();
        let store = Store::open(&folder.join(file), &key, Opening::Existing, &ORG_SCHEMA).unwrap();
        store
            .write(|db| {
                db.execute(
                    "UPDATE meta SET value = 'SOMEONEELSE' WHERE key = 'office_id'",
                    (),
                )
                .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        drop(store);

        assert!(
            matches!(Office::open(&folder, &keys), Err(OfficeError::Invalid(_))),
            "{file}"
        );
    }
}

#[test]
fn discard_looks_at_office_db_alone_too_and_refuses_what_it_cannot_check() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(&folder).unwrap();
    let key = Key::generate().unwrap();
    drop(
        Store::open(
            &folder.join("office.db"),
            &key,
            Opening::New,
            &OFFICE_SCHEMA,
        )
        .unwrap(),
    );
    // Data in office.db and no key: refused, as for org.db.
    assert!(matches!(
        Office::discard(&folder, ID, &keys),
        Err(OfficeError::Storage(_))
    ));
    assert!(folder.join("office.db").exists());

    // An empty org.db beside a log that is not empty: not looked at (SQLite would delete
    // the log), refused.
    keys.store(ID, &key).unwrap();
    fs::write(folder.join("org.db-wal"), b"frames").unwrap();
    assert!(matches!(
        Office::discard(&folder, ID, &keys),
        Err(OfficeError::Storage(_))
    ));
    assert_eq!(fs::read(folder.join("org.db-wal")).unwrap(), b"frames");
}

#[test]
fn discard_of_a_missing_folder_takes_no_lock_makes_nothing_and_removes_the_orphan_key() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    keys.store(ID, &Key::from_bytes([2; 32])).unwrap();

    Office::discard(folder_of(&dir, ID), ID, &keys).unwrap();

    assert!(keys.load(ID).unwrap().is_none());
    assert!(!dir.path().join("offices").exists(), "it made a folder");
}

#[test]
fn a_lock_whose_file_is_replaced_before_it_is_taken_is_not_returned() {
    use std::cell::Cell;
    let dir = TempDir::new().unwrap();
    let folder = dir.path().join("f");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(LOCK_FILE);
    let calls = Cell::new(0);
    // On the first go, between opening the file and locking it, the file is replaced (as a
    // discard followed by a create would).
    let swap = || {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            fs::remove_file(&path).unwrap();
            fs::write(&path, b"").unwrap();
        }
    };

    let lock = lock_folder_with(&folder, SHORT, &swap).unwrap();

    assert!(
        is_the_file_at(&lock, &path),
        "it locked a file nobody else can see"
    );
    assert_eq!(calls.get(), 2, "it did not try again");
}

#[test]
fn a_folder_that_is_gone_is_not_found_not_a_storage_error() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let folder = folder_of(&dir, ID);
    assert!(matches!(
        lock_folder(&folder, SHORT),
        Err(OfficeError::NotFound)
    ));
    assert!(matches!(
        Office::open(&folder, &keys),
        Err(OfficeError::NotFound)
    ));
}

#[test]
fn a_failed_create_whose_cleanup_fails_keeps_the_key_with_its_databases_and_discard_finishes() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let folder = folder_of(&dir, ID);

    FAIL_AT.with(|f| f.set(Some("workspace")));
    FAIL_CLEANUP.with(|f| f.set(true));
    let failed = Office::create(&folder, &new_office(ID), &keys);
    FAIL_AT.with(|f| f.set(None));
    FAIL_CLEANUP.with(|f| f.set(false));

    assert!(failed.is_err());
    assert!(
        folder.join("org.db").exists(),
        "the cleanup was meant to fail"
    );
    assert!(
        keys.load(ID).unwrap().is_some(),
        "the key went while its databases stayed"
    );
    // What is left is an unfinished create: discard removes it and the key.
    Office::discard(&folder, ID, &keys).unwrap();
    assert!(!folder.exists());
    assert!(keys.load(ID).unwrap().is_none());
}

#[test]
fn a_crashed_create_before_the_organization_commit_is_discardable() {
    // org.db holds the office id but no organization yet (a crash at "databases"/"workspace").
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(&folder).unwrap();
    let key = Key::generate().unwrap();
    keys.store(ID, &key).unwrap();
    let org = Store::open(&folder.join("org.db"), &key, Opening::New, &ORG_SCHEMA).unwrap();
    put_meta(&org, &[("office_id", ID)]).unwrap();
    drop(org);

    Office::discard(&folder, ID, &keys).unwrap();

    assert!(!folder.exists());
    assert!(keys.load(ID).unwrap().is_none());
}

#[test]
fn files_are_removed_logs_first_and_main_databases_last() {
    let mut names = [
        "org.db",
        "workspace",
        "office.db-wal",
        "office.db",
        "org.db-shm",
        "office.json",
    ];
    names.sort_by_key(|name| removal_rank(name));
    let position = |name: &str| names.iter().position(|n| *n == name).unwrap();
    assert!(position("org.db-shm") < position("org.db"));
    assert!(position("office.db-wal") < position("office.db"));
    assert!(position("workspace") < position("org.db-shm"));
    assert!(position("office.json") < position("office.db-wal"));
    // The same order holds for what a failed create removes: logs before their main file.
    let at = |n: &str| OFFICE_ENTRIES.iter().position(|e| *e == n).unwrap();
    assert!(at("org.db-wal") < at("org.db") && at("org.db-shm") < at("org.db"));
    assert!(at("office.db-wal") < at("office.db") && at("office.db-shm") < at("office.db"));
}

// ---- review round 3 -------------------------------------------------------------------------

#[test]
fn discard_refuses_a_folder_holding_something_an_office_does_not_make() {
    // A wrong path whose last name happens to be an id: its own files are not ours to delete.
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    keys.store(ID, &Key::from_bytes([1; 32])).unwrap();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(folder.join("sub/deep")).unwrap();
    fs::write(folder.join("notes.txt"), b"mine").unwrap();
    fs::write(folder.join("sub/deep/x"), b"mine too").unwrap();
    fs::create_dir(folder.join("workspace")).unwrap();

    assert!(matches!(
        Office::discard(&folder, ID, &keys),
        Err(OfficeError::Invalid(_))
    ));

    assert_eq!(fs::read(folder.join("notes.txt")).unwrap(), b"mine");
    assert_eq!(fs::read(folder.join("sub/deep/x")).unwrap(), b"mine too");
    assert!(
        folder.join("workspace").exists(),
        "something went before the refusal"
    );
    assert!(keys.load(ID).unwrap().is_some(), "the key went");
}

#[test]
fn a_folder_that_is_a_link_is_never_followed() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    keys.store(ID, &Key::from_bytes([1; 32])).unwrap();
    let target = dir.path().join("elsewhere");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("keep"), b"mine").unwrap();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &folder).unwrap();

    assert!(matches!(
        Office::discard(&folder, ID, &keys),
        Err(OfficeError::Invalid(_))
    ));
    assert!(matches!(
        Office::create(&folder, &new_office(ID), &keys),
        Err(OfficeError::Invalid(_))
    ));
    assert!(matches!(
        Office::open(&folder, &keys),
        Err(OfficeError::Invalid(_))
    ));

    assert_eq!(
        names_in(&target),
        ["keep"],
        "something was made or removed through the link"
    );
    assert!(keys.load(ID).unwrap().is_some());
}

#[test]
fn discard_refuses_a_folder_with_office_json_even_if_nothing_is_committed() {
    let (dir, keys, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    // Only the marker and the lock are left: still a finished office as far as discard knows.
    for entry in [
        "org.db",
        "org.db-wal",
        "org.db-shm",
        "office.db",
        "office.db-wal",
        "office.db-shm",
    ] {
        let _ = fs::remove_file(folder.join(entry));
    }
    assert!(matches!(
        Office::discard(&folder, ID, &keys),
        Err(OfficeError::OfficeExists)
    ));
    assert!(folder.join("office.json").exists());
    assert!(keys.load(ID).unwrap().is_some());
}

#[test]
fn the_organization_is_committed_before_office_json_is_written() {
    let keys = std::sync::Arc::new(InMemoryKeys::new());
    let seen_keys = std::sync::Arc::clone(&keys);
    ON_STEP.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |folder, step| {
            if step == "office_json" {
                let key = seen_keys.load(ID).unwrap().unwrap();
                let committed = Store::inspect(&folder.join("org.db"), &key, |db| {
                    db.query_row(
                        "SELECT EXISTS (SELECT 1 FROM meta WHERE key = 'organization_id')",
                        (),
                        |row| row.get::<_, bool>(0),
                    )
                    .map_err(OfficeError::storage)
                })
                .unwrap();
                assert!(
                    committed,
                    "office.json would be written before the organization"
                );
            }
        }));
    });
    let dir = TempDir::new().unwrap();
    let result = Office::create(folder_of(&dir, ID), &new_office(ID), &*keys);
    ON_STEP.with(|hook| *hook.borrow_mut() = None);
    result.unwrap();
}

#[test]
fn discard_removes_logs_before_main_databases_in_the_order_it_says() {
    let dir = TempDir::new().unwrap();
    let folder = dir.path().join("f");
    fs::create_dir_all(folder.join("workspace")).unwrap();
    for name in [
        "org.db",
        "org.db-wal",
        "org.db-shm",
        "office.db",
        "office.db-wal",
        "office.json.tmp",
        LOCK_FILE,
    ] {
        fs::write(folder.join(name), b"x").unwrap();
    }
    let mut order = Vec::new();

    empty_folder_but_the_lock(&folder, &mut |name| order.push(name.to_owned())).unwrap();

    assert_eq!(names_in(&folder), [LOCK_FILE]);
    let ranks: Vec<u8> = order.iter().map(|n| removal_rank(n)).collect();
    assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "{order:?}");
    assert_eq!(order.len(), 7, "{order:?}");
}

#[test]
fn the_last_step_never_removes_a_folder_a_create_has_started_filling() {
    let dir = TempDir::new().unwrap();
    let folder = dir.path().join("f");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join(LOCK_FILE), b"").unwrap();
    fs::write(folder.join("org.db"), b"a new create's").unwrap();

    remove_the_empty_folder(&folder).unwrap();

    assert_eq!(fs::read(folder.join("org.db")).unwrap(), b"a new create's");
}

#[test]
fn a_trailing_slash_does_not_hide_a_link() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let target = dir.path().join("elsewhere");
    fs::create_dir_all(&target).unwrap();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &folder).unwrap();
    let slashed = PathBuf::from(format!("{}/", folder.display()));

    assert!(matches!(
        Office::create(&slashed, &new_office(ID), &keys),
        Err(OfficeError::Invalid(_))
    ));
    assert!(matches!(
        Office::open(&slashed, &keys),
        Err(OfficeError::Invalid(_))
    ));
    assert!(matches!(
        Office::discard(&slashed, ID, &keys),
        Err(OfficeError::Invalid(_))
    ));
    assert!(
        names_in(&target).is_empty(),
        "something was made through the link"
    );

    // A real folder with a trailing slash works as without.
    let real = folder_of(&dir, "OFFICE2");
    let slashed = PathBuf::from(format!("{}/", real.display()));
    Office::create(&slashed, &new_office("OFFICE2"), &keys).unwrap();
}

#[test]
fn a_failed_key_store_deletes_nothing() {
    // The keychain says "exists" for a key some other office made under this id: it must stay.
    struct Refuses {
        deleted: std::sync::atomic::AtomicBool,
    }
    impl KeyProvider for Refuses {
        fn load(&self, _id: &str) -> Result<Option<Key>, KeyError> {
            Ok(None)
        }
        fn store(&self, _id: &str, _key: &Key) -> Result<(), KeyError> {
            Err("exists".into())
        }
        fn delete(&self, _id: &str) -> Result<(), KeyError> {
            self.deleted
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }
    let dir = TempDir::new().unwrap();
    let keys = Refuses {
        deleted: false.into(),
    };

    assert!(Office::create(folder_of(&dir, ID), &new_office(ID), &keys).is_err());

    assert!(!keys.deleted.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn a_cleanup_that_fails_part_way_stops_there_and_keeps_the_key() {
    let dir = TempDir::new().unwrap();
    let keys = InMemoryKeys::new();
    let folder = folder_of(&dir, ID);

    FAIL_AT.with(|f| f.set(Some("workspace")));
    FAIL_REMOVING.with(|f| f.set(Some("org.db-wal")));
    let failed = Office::create(&folder, &new_office(ID), &keys);
    FAIL_AT.with(|f| f.set(None));
    FAIL_REMOVING.with(|f| f.set(None));

    assert!(failed.is_err());
    // Everything before the failing entry went; nothing after it, the main file included.
    assert!(!folder.join("workspace").exists());
    assert!(
        folder.join("org.db").exists(),
        "the main file went before its log"
    );
    assert!(keys.load(ID).unwrap().is_some());
    Office::discard(&folder, ID, &keys).unwrap();
}
