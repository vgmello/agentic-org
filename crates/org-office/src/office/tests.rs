use std::os::unix::fs::PermissionsExt;

use tempfile::TempDir;

use super::*;
mod work;
use crate::model::OfficeRole;
use org_proto::org::CommandStatus;

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

/// The key the host would send (the office never makes one).
fn test_key() -> Key {
    Key::from_bytes([5; 32])
}

fn made() -> (TempDir, Key, Office) {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let office = Office::create(folder_of(&dir, ID), &new_office(ID), key.clone()).unwrap();
    (dir, key, office)
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

// ---- create ---------------------------------------------------------------------------------

#[test]
fn create_makes_the_whole_folder_and_office_json() {
    let (dir, _key, office) = made();
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

    // Owner-only everywhere.
    assert_eq!(mode(&folder), 0o700);
    assert_eq!(mode(&folder.join("workspace")), 0o700);
    for file in ["office.json", "office.lock", "org.db", "office.db"] {
        assert_eq!(mode(&folder.join(file)), 0o600, "{file}");
    }
}

#[test]
fn both_databases_are_encrypted_with_the_one_key() {
    let (dir, key, office) = made();
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
    for (file, schema) in [("org.db", &org_db::SCHEMA), ("office.db", &OFFICE_SCHEMA)] {
        Store::open(&folder.join(file), &key, Opening::Existing, schema).unwrap();
    }
}

#[test]
fn a_failed_create_leaves_nothing_but_the_lock_file_and_can_be_repeated() {
    for step in [
        "databases",
        "workspace",
        "work",
        "organization",
        "office_json",
    ] {
        let dir = TempDir::new().unwrap();
        let key = test_key();
        let folder = folder_of(&dir, ID);

        FAIL_AT.with(|f| f.set(Some(step)));
        let failed = Office::create(&folder, &new_office(ID), key.clone());
        FAIL_AT.with(|f| f.set(None));

        assert!(matches!(failed, Err(OfficeError::Storage(_))), "{step}");
        assert_eq!(names_in(&folder), ["office.lock"], "{step}");
        // The id can be created again.
        Office::create(&folder, &new_office(ID), key.clone()).unwrap();
    }
}

#[test]
fn office_json_is_written_after_every_other_step() {
    REACHED.with(|reached| reached.borrow_mut().clear());
    let (dir, _key, _office) = made();
    let steps = REACHED.with(|reached| reached.borrow().clone());
    assert_eq!(
        steps,
        [
            ("databases", false),
            ("workspace", false),
            ("work", false),
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
    let key = test_key();
    let folder = folder_of(&dir, ID);
    let office = Office::create(&folder, &new_office(ID), key.clone()).unwrap();
    drop(office);
    fs::remove_file(folder.join("office.json")).unwrap();
    // ...and `open` does not treat it as an office.
    assert!(matches!(
        Office::open(&folder, key.clone()),
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
        let key = test_key();
        let folder = folder_of(&dir, ID);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(planted), b"precious").unwrap();
        let before = names_in(&folder);

        let result = Office::create(&folder, &new_office(ID), key.clone());

        assert!(
            matches!(result, Err(OfficeError::OfficeExists)),
            "{planted}"
        );
        let after = names_in(&folder);
        assert!(
            before.iter().all(|n| after.contains(n)),
            "{planted} vanished"
        );
        assert_eq!(fs::read(folder.join(planted)).unwrap(), b"precious");
    }
}

#[test]
fn create_checks_its_arguments_before_touching_anything() {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let make = |change: &dyn Fn(&mut NewOffice)| {
        let mut new = new_office(ID);
        change(&mut new);
        Office::create(folder_of(&dir, ID), &new, key.clone())
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
        key.clone(),
    );
    assert!(matches!(wrong, Err(OfficeError::Invalid(_))));
    assert!(!dir.path().join("offices").exists(), "something was made");
}

#[test]
fn names_are_trimmed_into_office_json() {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let mut new = new_office(ID);
    new.org_name = "  Acme Corp ".into();
    let office = Office::create(folder_of(&dir, ID), &new, key.clone()).unwrap();
    assert_eq!(office.info().org_name, "Acme Corp");
}

// ---- open -----------------------------------------------------------------------------------

#[test]
fn open_reopens_what_create_made() {
    let (dir, key, office) = made();
    let info = office.info().clone();
    drop(office);

    let again = Office::open(folder_of(&dir, ID), key.clone()).unwrap();

    assert_eq!(again.info(), &info);
}

#[test]
fn one_process_at_a_time_owns_an_office_and_a_new_one_waits_for_the_old() {
    let (dir, key, office) = made();
    let folder = folder_of(&dir, ID);

    assert!(matches!(
        Office::open_waiting(&folder, key.clone(), SHORT),
        Err(OfficeError::InUse)
    ));
    assert!(matches!(
        Office::create_waiting(&folder, &new_office(ID), key.clone(), SHORT),
        Err(OfficeError::InUse)
    ));
    assert!(matches!(
        Office::discard_waiting(&folder, ID, Some(&key), SHORT),
        Err(OfficeError::InUse)
    ));

    // A replacement that starts while the old one is still shutting down waits for it.
    let waiter = {
        let (folder, key) = (folder.clone(), key);
        std::thread::spawn(move || {
            Office::open_waiting(&folder, key.clone(), Duration::from_secs(10)).map(|_| ())
        })
    };
    std::thread::sleep(Duration::from_millis(200));
    drop(office);
    waiter.join().unwrap().unwrap();
}

#[test]
fn open_makes_nothing() {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let folder = folder_of(&dir, ID);

    // No folder.
    assert!(matches!(
        Office::open(&folder, key.clone()),
        Err(OfficeError::NotFound)
    ));
    assert!(!dir.path().join("offices").exists());

    // A folder with no office.json: an unfinished create, not an office.
    fs::create_dir_all(&folder).unwrap();
    assert!(matches!(
        Office::open(&folder, key.clone()),
        Err(OfficeError::NotFound)
    ));
    assert_eq!(names_in(&folder), ["office.lock"]);
}

#[test]
fn open_with_a_key_that_does_not_open_the_databases_is_key_missing_and_changes_nothing() {
    let (dir, _key, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    let before = names_in(&folder);
    let org_before = fs::read(folder.join("org.db")).unwrap();

    // The host sent another key: the databases are not opened, nor a key made in its place.
    assert!(matches!(
        Office::open(&folder, Key::from_bytes([1; 32])),
        Err(OfficeError::KeyMissing)
    ));

    assert_eq!(fs::read(folder.join("org.db")).unwrap(), org_before);
    assert_eq!(names_in(&folder), before);
}

#[test]
fn open_refuses_an_office_json_that_names_another_office_or_organization() {
    let (dir, key, office) = made();
    let folder = folder_of(&dir, ID);
    let info = office.info().clone();
    drop(office);

    // Another office id than the folder's name.
    let mut other = info.clone();
    other.office_id = "ELSEWHERE".into();
    fs::remove_file(folder.join("office.json")).unwrap();
    office_file::write(&folder, &other).unwrap();
    assert!(matches!(
        Office::open(&folder, key.clone()),
        Err(OfficeError::Invalid(_))
    ));

    // Another organization than the databases hold.
    let mut other = info;
    other.org_id = "SOMEONEELSE".into();
    fs::remove_file(folder.join("office.json")).unwrap();
    office_file::write(&folder, &other).unwrap();
    assert!(matches!(
        Office::open(&folder, key.clone()),
        Err(OfficeError::Invalid(_))
    ));
}

#[test]
fn open_refuses_a_missing_database_and_does_not_make_one() {
    let (dir, key, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    for name in ["office.db", "office.db-wal", "office.db-shm"] {
        let _ = fs::remove_file(folder.join(name));
    }

    assert!(matches!(
        Office::open(&folder, key.clone()),
        Err(OfficeError::Storage(_))
    ));
    assert!(!folder.join("office.db").exists());
}

// ---- discard --------------------------------------------------------------------------------

#[test]
fn discard_removes_an_unfinished_create() {
    // (a) a folder with only the lock; (b) databases made but the organization not committed.
    for with_schema_only_database in [false, true] {
        let dir = TempDir::new().unwrap();
        let key = test_key();
        let folder = folder_of(&dir, ID);
        fs::create_dir_all(&folder).unwrap();
        if with_schema_only_database {
            drop(Store::open(&folder.join("org.db"), &key, Opening::New, &org_db::SCHEMA).unwrap());
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

        Office::discard(&folder, ID, Some(&key)).unwrap();

        assert!(!folder.exists(), "the folder is still there");
    }
}

#[test]
fn discard_never_removes_a_finished_office() {
    let (dir, key, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    let before = names_in(&folder);

    assert!(matches!(
        Office::discard(&folder, ID, Some(&key)),
        Err(OfficeError::OfficeExists)
    ));

    assert_eq!(names_in(&folder), before);
    Office::open(&folder, key.clone()).unwrap();
}

#[test]
fn discard_never_removes_an_office_that_lost_its_office_json() {
    let (dir, key, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    fs::remove_file(folder.join("office.json")).unwrap();

    assert!(matches!(
        Office::discard(&folder, ID, Some(&key)),
        Err(OfficeError::OfficeExists)
    ));

    assert!(folder.join("org.db").exists());
}

#[test]
fn discard_refuses_databases_it_cannot_check() {
    let (dir, _key, office) = made();
    let folder = folder_of(&dir, ID);
    drop(office);
    fs::remove_file(folder.join("office.json")).unwrap();

    // The host has no key for it: databases are not looked into, and are not deleted.
    assert!(matches!(
        Office::discard(&folder, ID, None),
        Err(OfficeError::Storage(_))
    ));
    assert!(folder.join("org.db").exists());

    // The key is wrong: it does not open them (KeyMissing), and nothing is deleted.
    assert!(matches!(
        Office::discard(&folder, ID, Some(&Key::from_bytes([3; 32]))),
        Err(OfficeError::KeyMissing)
    ));
    assert!(folder.join("org.db").exists());
    assert!(folder.join("office.db").exists());
}

#[test]
fn discard_with_no_key_removes_only_a_folder_with_no_database_file() {
    let dir = TempDir::new().unwrap();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(folder.join("workspace")).unwrap();

    Office::discard(&folder, ID, None).unwrap();

    assert!(!folder.exists());
}

#[test]
fn discard_checks_the_id_and_the_folder_name() {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(&folder).unwrap();

    for (path, id) in [(&folder, "has-dash"), (&folder, "OTHER"), (&folder, "")] {
        assert!(
            matches!(
                Office::discard(path, id, Some(&key)),
                Err(OfficeError::Invalid(_))
            ),
            "{id:?}"
        );
    }
    assert!(folder.exists());
}

#[test]
fn discard_without_a_folder_is_fine_and_with_a_held_lock_gives_up() {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let folder = folder_of(&dir, ID);

    // With the folder absent the discard takes no lock and makes nothing.
    Office::discard(&folder, ID, Some(&key)).unwrap();
    assert!(!dir.path().join("offices").exists(), "it made a folder");

    // While someone holds the folder's lock (a create of the same id) it waits, then gives up.
    fs::create_dir_all(&folder).unwrap();
    let _held = lock_folder(&folder, SHORT).unwrap();
    assert!(matches!(
        Office::discard_waiting(&folder, ID, Some(&key), SHORT),
        Err(OfficeError::InUse)
    ));
    assert!(folder.exists());
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
    let held = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = std::sync::Arc::clone(&held);
    ON_STEP.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |folder, step| {
            if step == "cleanup" {
                // Another process trying the lock now must find it taken.
                let probe = private_file(&folder.join(LOCK_FILE)).unwrap();
                let taken = matches!(probe.try_lock(), Err(TryLockError::WouldBlock));
                seen.store(taken, std::sync::atomic::Ordering::SeqCst);
            }
        }));
    });

    FAIL_AT.with(|f| f.set(Some("workspace")));
    let failed = Office::create(&folder, &new_office(ID), test_key());
    FAIL_AT.with(|f| f.set(None));
    ON_STEP.with(|hook| *hook.borrow_mut() = None);

    assert!(failed.is_err());
    assert!(held.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn a_loose_folder_is_tightened_by_create() {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(&folder).unwrap();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();

    Office::create(&folder, &new_office(ID), key.clone()).unwrap();

    assert_eq!(mode(&folder), 0o700);
}

#[test]
fn open_checks_that_each_database_belongs_to_the_office() {
    for (file, schema) in [("org.db", &org_db::SCHEMA), ("office.db", &OFFICE_SCHEMA)] {
        let (dir, key, office) = made();
        let folder = folder_of(&dir, ID);
        drop(office);
        let store = Store::open(&folder.join(file), &key, Opening::Existing, schema).unwrap();
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
            matches!(
                Office::open(&folder, key.clone()),
                Err(OfficeError::Invalid(_))
            ),
            "{file}"
        );
    }
}

#[test]
fn discard_looks_at_office_db_alone_too_and_refuses_what_it_cannot_check() {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(&folder).unwrap();
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
        Office::discard(&folder, ID, None),
        Err(OfficeError::Storage(_))
    ));
    assert!(folder.join("office.db").exists());

    // An empty org.db beside a log that is not empty: not looked at (SQLite would delete
    // the log), refused.
    fs::write(folder.join("org.db-wal"), b"frames").unwrap();
    assert!(matches!(
        Office::discard(&folder, ID, Some(&key)),
        Err(OfficeError::Storage(_))
    ));
    assert_eq!(fs::read(folder.join("org.db-wal")).unwrap(), b"frames");
}

#[test]
fn discard_of_a_missing_folder_takes_no_lock_and_makes_nothing() {
    let dir = TempDir::new().unwrap();

    Office::discard(folder_of(&dir, ID), ID, None).unwrap();

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
    let key = test_key();
    let folder = folder_of(&dir, ID);
    assert!(matches!(
        lock_folder(&folder, SHORT),
        Err(OfficeError::NotFound)
    ));
    assert!(matches!(
        Office::open(&folder, key.clone()),
        Err(OfficeError::NotFound)
    ));
}

#[test]
fn a_failed_create_whose_cleanup_fails_leaves_an_unfinished_create_and_discard_finishes() {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let folder = folder_of(&dir, ID);

    FAIL_AT.with(|f| f.set(Some("workspace")));
    FAIL_CLEANUP.with(|f| f.set(true));
    let failed = Office::create(&folder, &new_office(ID), key.clone());
    FAIL_AT.with(|f| f.set(None));
    FAIL_CLEANUP.with(|f| f.set(false));

    assert!(failed.is_err());
    assert!(
        folder.join("org.db").exists(),
        "the cleanup was meant to fail"
    );
    // What is left is an unfinished create: discard, given the key, removes it.
    Office::discard(&folder, ID, Some(&key)).unwrap();
    assert!(!folder.exists());
}

#[test]
fn a_crashed_create_before_the_organization_commit_is_discardable() {
    // org.db holds the office id but no organization yet (a crash at "databases"/"workspace").
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(&folder).unwrap();
    let org = Store::open(&folder.join("org.db"), &key, Opening::New, &org_db::SCHEMA).unwrap();
    put_meta(&org, &[("office_id", ID)]).unwrap();
    drop(org);

    Office::discard(&folder, ID, Some(&key)).unwrap();

    assert!(!folder.exists());
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
    let key = test_key();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(folder.join("sub/deep")).unwrap();
    fs::write(folder.join("notes.txt"), b"mine").unwrap();
    fs::write(folder.join("sub/deep/x"), b"mine too").unwrap();
    fs::create_dir(folder.join("workspace")).unwrap();

    assert!(matches!(
        Office::discard(&folder, ID, Some(&key)),
        Err(OfficeError::Invalid(_))
    ));

    assert_eq!(fs::read(folder.join("notes.txt")).unwrap(), b"mine");
    assert_eq!(fs::read(folder.join("sub/deep/x")).unwrap(), b"mine too");
    assert!(
        folder.join("workspace").exists(),
        "something went before the refusal"
    );
}

#[test]
fn a_folder_that_is_a_link_is_never_followed() {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let target = dir.path().join("elsewhere");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("keep"), b"mine").unwrap();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &folder).unwrap();

    assert!(matches!(
        Office::discard(&folder, ID, Some(&key)),
        Err(OfficeError::Invalid(_))
    ));
    assert!(matches!(
        Office::create(&folder, &new_office(ID), key.clone()),
        Err(OfficeError::Invalid(_))
    ));
    assert!(matches!(
        Office::open(&folder, key.clone()),
        Err(OfficeError::Invalid(_))
    ));

    assert_eq!(
        names_in(&target),
        ["keep"],
        "something was made or removed through the link"
    );
}

#[test]
fn discard_refuses_a_folder_with_office_json_even_if_nothing_is_committed() {
    let (dir, key, office) = made();
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
        Office::discard(&folder, ID, Some(&key)),
        Err(OfficeError::OfficeExists)
    ));
    assert!(folder.join("office.json").exists());
}

#[test]
fn the_organization_is_committed_before_office_json_is_written() {
    let key = test_key();
    let seen_key = key.clone();
    ON_STEP.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |folder, step| {
            if step == "office_json" {
                let key = seen_key.clone();
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
    let result = Office::create(folder_of(&dir, ID), &new_office(ID), key);
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
    let key = test_key();
    let target = dir.path().join("elsewhere");
    fs::create_dir_all(&target).unwrap();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &folder).unwrap();
    let slashed = PathBuf::from(format!("{}/", folder.display()));

    assert!(matches!(
        Office::create(&slashed, &new_office(ID), key.clone()),
        Err(OfficeError::Invalid(_))
    ));
    assert!(matches!(
        Office::open(&slashed, key.clone()),
        Err(OfficeError::Invalid(_))
    ));
    assert!(matches!(
        Office::discard(&slashed, ID, Some(&key)),
        Err(OfficeError::Invalid(_))
    ));
    assert!(
        names_in(&target).is_empty(),
        "something was made through the link"
    );

    // A real folder with a trailing slash works as without.
    let real = folder_of(&dir, "OFFICE2");
    let slashed = PathBuf::from(format!("{}/", real.display()));
    Office::create(&slashed, &new_office("OFFICE2"), key.clone()).unwrap();
}

#[test]
fn a_cleanup_that_fails_part_way_stops_there() {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let folder = folder_of(&dir, ID);

    FAIL_AT.with(|f| f.set(Some("workspace")));
    FAIL_REMOVING.with(|f| f.set(Some("org.db-wal")));
    let failed = Office::create(&folder, &new_office(ID), key.clone());
    FAIL_AT.with(|f| f.set(None));
    FAIL_REMOVING.with(|f| f.set(None));

    assert!(failed.is_err());
    // Everything before the failing entry went; nothing after it, the main file included.
    assert!(!folder.join("workspace").exists());
    assert!(
        folder.join("org.db").exists(),
        "the main file went before its log"
    );
    Office::discard(&folder, ID, Some(&key)).unwrap();
}

#[test]
fn a_created_office_has_its_owner_in_the_first_log_entry_and_keeps_it_across_a_restart() {
    let (dir, key, office) = made();
    let snapshot = office.org_snapshot().unwrap();
    assert_eq!(
        snapshot.position,
        org_proto::org::Position { epoch: 1, seq: 1 }
    );
    let [owner] = &snapshot.members[..] else {
        panic!("{:?}", snapshot.members)
    };
    assert!(owner.owner && owner.version == 1);
    assert!(
        org_proto::is_valid_member_id(&owner.id),
        "the owner's id is a member id: {}",
        owner.id
    );
    drop(office);

    let again = Office::open(folder_of(&dir, ID), key.clone()).unwrap();
    assert_eq!(again.org_snapshot().unwrap(), snapshot);
}

/// A command id made `ago_ms` ago (negative: ahead), distinct for each `n`.
fn cmd(n: u8, ago_ms: i64) -> String {
    crate::ulid::from_ms((time::now_ms() as i64 - ago_ms) as u64, n)
}

fn team_create(command_id: &str, team_id: &str, name: &str) -> OrgCommand {
    OrgCommand::TeamCreate(org_proto::org::TeamCreateParams {
        command_id: command_id.into(),
        team_id: team_id.into(),
        name: name.into(),
        owner_team_id: None,
    })
}

fn at(seq: u64) -> org_proto::org::Position {
    org_proto::org::Position { epoch: 1, seq }
}

fn code(office: &Office, command: OrgCommand) -> org_proto::ErrorCode {
    office.submit_org(command).unwrap().unwrap_err().code
}

const DAY_MS: i64 = 86_400_000;

#[test]
fn a_command_is_logged_at_the_next_position_and_changes_the_snapshot() {
    let (_dir, _key, office) = made();

    let accepted = office
        .submit_org(team_create(&cmd(1, 0), "T1", "Platform"))
        .unwrap()
        .unwrap();

    assert_eq!(accepted.status, CommandStatus::Committed);
    assert_eq!(accepted.position, Some(at(2)));
    let snapshot = office.org_snapshot().unwrap();
    assert_eq!(snapshot.position, at(2));
    assert_eq!(snapshot.teams.len(), 1);
    assert_eq!(snapshot.teams[0].name, "Platform");
}

#[test]
fn a_retried_command_gets_its_own_commit_back_and_changes_nothing_again() {
    let (dir, key, office) = made();
    let (c1, c2) = (cmd(1, 0), cmd(2, 0));
    let first = office
        .submit_org(team_create(&c1, "T1", "Platform"))
        .unwrap()
        .unwrap();
    office
        .submit_org(team_create(&c2, "T2", "Design"))
        .unwrap()
        .unwrap();
    let snapshot = office.org_snapshot().unwrap();
    assert_eq!(first.position, Some(at(2)));
    assert_eq!(snapshot.position, at(3));

    // Same id again, in this process and after a restart: the first commit's position, not the
    // head.
    assert_eq!(
        office
            .submit_org(team_create(&c1, "T1", "Platform"))
            .unwrap()
            .unwrap(),
        first
    );
    drop(office);
    let office = Office::open(folder_of(&dir, ID), key.clone()).unwrap();
    assert_eq!(
        office
            .submit_org(team_create(&c1, "T1", "Platform"))
            .unwrap()
            .unwrap(),
        first
    );

    assert_eq!(office.org_snapshot().unwrap(), snapshot);
}

#[test]
fn the_same_command_sent_at_once_commits_once() {
    let (_dir, _key, office) = made();
    let c1 = cmd(1, 0);

    let answers: Vec<_> = std::thread::scope(|scope| {
        let sends: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| office.submit_org(team_create(&c1, "T1", "Platform"))))
            .collect();
        sends
            .into_iter()
            .map(|send| send.join().unwrap().unwrap().unwrap())
            .collect()
    });

    assert!(answers.iter().all(|answer| *answer == answers[0]));
    assert_eq!(answers[0].position, Some(at(2)));
    assert_eq!(office.org_snapshot().unwrap().position, at(2));
}

#[test]
fn a_refusal_is_remembered_and_a_retry_gets_the_same_refusal_even_if_it_would_now_succeed() {
    let (dir, key, office) = made();
    let (c1, c2) = (cmd(1, 0), cmd(2, 0));
    let before = office.org_snapshot().unwrap();

    // Refused: the owner team does not exist yet.
    let command = |owner: Option<&str>| {
        OrgCommand::TeamCreate(org_proto::org::TeamCreateParams {
            command_id: c1.clone(),
            team_id: "T2".into(),
            name: "Sub".into(),
            owner_team_id: owner.map(str::to_owned),
        })
    };
    assert_eq!(
        code(&office, command(Some("T1"))),
        org_proto::ErrorCode::NotFound
    );
    assert_eq!(office.org_snapshot().unwrap(), before);

    // The state changes so the same command would now commit...
    office
        .submit_org(team_create(&c2, "T1", "Platform"))
        .unwrap()
        .unwrap();
    // ...but the old id still gets the old refusal, in this process and after a restart.
    assert_eq!(
        code(&office, command(Some("T1"))),
        org_proto::ErrorCode::NotFound
    );
    drop(office);
    let office = Office::open(folder_of(&dir, ID), key.clone()).unwrap();
    assert_eq!(
        code(&office, command(Some("T1"))),
        org_proto::ErrorCode::NotFound
    );
    assert_eq!(office.org_snapshot().unwrap().position, at(2));

    // A new id commits, at the next position: a refusal takes none.
    let mut again = command(Some("T1"));
    if let OrgCommand::TeamCreate(p) = &mut again {
        p.command_id = cmd(3, 0);
    }
    assert_eq!(
        office.submit_org(again).unwrap().unwrap().position,
        Some(at(3))
    );
}

#[test]
fn an_id_reused_with_other_params_is_refused_whether_it_committed_or_was_refused() {
    let (_dir, _key, office) = made();
    let (c1, c2) = (cmd(1, 0), cmd(2, 0));
    office
        .submit_org(team_create(&c1, "T1", "Platform"))
        .unwrap()
        .unwrap();
    assert_eq!(
        code(&office, team_create(&c1, "T9", "Other")),
        org_proto::ErrorCode::OperationReused
    );
    // A refused one, too: the blank name is `invalid_name`; other params reuse the operation.
    assert_eq!(
        code(&office, team_create(&c2, "T3", "  ")),
        org_proto::ErrorCode::InvalidName
    );
    assert_eq!(
        code(&office, team_create(&c2, "T3", "Fine")),
        org_proto::ErrorCode::OperationReused
    );
    assert_eq!(office.org_snapshot().unwrap().position, at(2));
}

#[test]
fn org_operation_get_uses_the_read_clock_without_advancing_the_mark() {
    let (_dir, _key, office) = made();
    let id = cmd(1, 0);
    office
        .submit_org(team_create(&id, "T1", "Engineering"))
        .unwrap()
        .unwrap();
    let before = office.org_snapshot().unwrap();
    office.stores.org.run_sql_for_test(
        "UPDATE commands SET expires_at='1970-01-01T00:00:00.001Z';
         UPDATE meta SET value='0' WHERE key='clock_high_ms'",
    );
    assert_eq!(
        office.org_operation_get(&id).unwrap().unwrap_err().code,
        org_proto::ErrorCode::Expired
    );
    let high: String = office
        .stores
        .org
        .read(|db| {
            db.query_row(
                "SELECT value FROM meta WHERE key='clock_high_ms'",
                [],
                |row| row.get(0),
            )
            .map_err(OfficeError::storage)
        })
        .unwrap();
    assert_eq!(high, "0");
    assert_eq!(office.org_snapshot().unwrap(), before);
}

#[test]
fn commands_are_logged_one_after_another_without_gaps() {
    let (_dir, _key, office) = made();
    for (n, name) in ["A", "B", "C"].into_iter().enumerate() {
        let accepted = office
            .submit_org(team_create(&cmd(n as u8, 0), &format!("T{n}"), name))
            .unwrap()
            .unwrap();
        assert_eq!(accepted.position, Some(at(n as u64 + 2)));
    }
}

#[test]
fn a_command_id_that_is_not_a_ulid_is_refused_and_not_remembered() {
    let (_dir, _key, office) = made();
    let before = office.org_snapshot().unwrap();
    let lower = cmd(1, 0).to_lowercase();

    for bad in ["", "has space", "plain-id", lower.as_str(), &"X".repeat(70)] {
        assert_eq!(
            code(&office, team_create(bad, "T1", "Platform")),
            org_proto::ErrorCode::InvalidRequest,
            "{bad:?}"
        );
    }

    assert_eq!(office.org_snapshot().unwrap(), before);
}

#[test]
fn an_id_dated_outside_the_window_is_expired_or_clock_skew_and_changes_nothing() {
    let (_dir, _key, office) = made();
    let before = office.org_snapshot().unwrap();

    assert_eq!(
        code(&office, team_create(&cmd(1, 30 * DAY_MS), "T1", "Platform")),
        org_proto::ErrorCode::Expired
    );
    assert_eq!(
        code(
            &office,
            team_create(&cmd(2, -10 * 60_000), "T1", "Platform")
        ),
        org_proto::ErrorCode::ClockSkew
    );
    assert_eq!(office.org_snapshot().unwrap(), before);
    // Inside the window on both sides is fine, and the refused ones were not remembered.
    for (n, ago) in [(3, 28 * DAY_MS), (4, -4 * 60_000)] {
        office
            .submit_org(team_create(
                &cmd(n, ago),
                &format!("T{n}"),
                &format!("N{n}"),
            ))
            .unwrap()
            .unwrap();
    }
}

#[test]
fn the_clock_never_goes_back_so_a_stepped_back_clock_cannot_reopen_the_window() {
    let (_dir, _key, office) = made();
    // HQ's clock once read the year 2100 (the high-water mark is kept); it now reads the
    // present again. An id dated now is then decades behind the mark: `expired`, as it would
    // have been with the clock that never stepped back.
    office.stores.org.run_sql_for_test(
        "INSERT INTO meta (key, value) VALUES ('clock_high_ms', '4102444800000')",
    );

    assert_eq!(
        code(&office, team_create(&cmd(1, 0), "T1", "Platform")),
        org_proto::ErrorCode::Expired
    );
}

#[test]
fn the_high_water_mark_follows_the_clock_forward() {
    let (_dir, _key, office) = made();
    office
        .submit_org(team_create(&cmd(1, 0), "T1", "Platform"))
        .unwrap()
        .unwrap();
    let mark = |office: &Office| {
        office
            .stores
            .org
            .meta("clock_high_ms")
            .unwrap()
            .unwrap()
            .parse::<u64>()
            .unwrap()
    };
    let first = mark(&office);
    assert!(first > 0);
    office
        .submit_org(team_create(&cmd(2, 0), "T2", "Design"))
        .unwrap()
        .unwrap();
    assert!(mark(&office) >= first);
}

#[test]
fn the_log_after_a_position_is_what_the_commands_made_oldest_first() {
    let (_dir, _key, office) = made();
    let (c1, c2, c3) = (cmd(1, 0), cmd(2, 0), cmd(3, 0));
    for (id, team, name) in [(&c1, "T1", "A"), (&c2, "T2", "B"), (&c3, "T3", "C")] {
        office
            .submit_org(team_create(id, team, name))
            .unwrap()
            .unwrap();
    }
    // A refusal is not in the log.
    assert!(office
        .submit_org(team_create(&cmd(4, 0), "T4", " "))
        .unwrap()
        .is_err());

    let after_genesis = office.org_changes_after(at(1), 100).unwrap();
    let seen: Vec<_> = after_genesis
        .iter()
        .map(|data| (data.position, data.command_id.as_str()))
        .collect();
    assert_eq!(
        seen,
        vec![
            (at(2), c1.as_str()),
            (at(3), c2.as_str()),
            (at(4), c3.as_str())
        ]
    );
    assert!(matches!(
        &after_genesis[0].changes[..],
        [org_proto::org::OrgChange::TeamUpserted { team }] if team.name == "A"
    ));
    // A page, the rest, and nothing past the head.
    assert_eq!(office.org_changes_after(at(1), 2).unwrap().len(), 2);
    assert_eq!(office.org_changes_after(at(3), 100).unwrap().len(), 1);
    assert!(office.org_changes_after(at(4), 100).unwrap().is_empty());
    assert_eq!(office.org_head().unwrap(), at(4));
}

#[test]
fn a_subscriber_can_resume_only_from_a_position_this_log_has() {
    let (_dir, _key, office) = made();
    office
        .submit_org(team_create(&cmd(1, 0), "T1", "A"))
        .unwrap()
        .unwrap();
    let position = |epoch, seq| org_proto::org::Position { epoch, seq };

    for (after, resumable) in [
        (position(1, 1), true),  // the genesis: a snapshot taken right after create
        (position(1, 2), true),  // the head
        (position(1, 3), false), // beyond what this office has
        (position(1, 0), false),
        (position(0, 0), false),
        (position(2, 1), false), // another epoch (a promotion started one)
    ] {
        assert_eq!(
            office.org_can_resume_from(after).unwrap(),
            resumable,
            "{after:?}"
        );
    }
}

#[test]
fn followers_are_woken_by_a_commit_and_a_wait_that_sees_a_newer_token_returns_at_once() {
    let (_dir, _key, office) = made();
    let before = office.org_change_token();

    // Nothing committed: the wait runs out its time and the token is the same.
    let started = Instant::now();
    assert_eq!(
        office.wait_for_org_change(before, Duration::from_millis(100)),
        before
    );
    assert!(started.elapsed() >= Duration::from_millis(90));

    // A commit from another thread wakes a waiter long before its timeout.
    let woken = std::thread::scope(|scope| {
        let waiter = scope.spawn(|| office.wait_for_org_change(before, Duration::from_secs(30)));
        std::thread::sleep(Duration::from_millis(50));
        office
            .submit_org(team_create(&cmd(1, 0), "T1", "A"))
            .unwrap()
            .unwrap();
        waiter.join().unwrap()
    });
    assert_ne!(woken, before);
    // A token read before a commit returns at once: the commit is never missed.
    let started = Instant::now();
    assert_ne!(
        office.wait_for_org_change(before, Duration::from_secs(30)),
        before
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_key_that_opens_nothing_never_deletes_more_than_no_key_would() {
    // office.db holds data and org.db is missing: a folder only a key can vouch for.
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let folder = folder_of(&dir, ID);
    fs::create_dir_all(&folder).unwrap();
    drop(
        Store::open(
            &folder.join("office.db"),
            &key,
            Opening::New,
            &OFFICE_SCHEMA,
        )
        .unwrap(),
    );
    let before = fs::read(folder.join("office.db")).unwrap();

    // No key and a WRONG key both refuse, and leave the file byte for byte as it was.
    assert!(matches!(
        Office::discard(&folder, ID, None),
        Err(OfficeError::Storage(_))
    ));
    assert!(matches!(
        Office::discard(&folder, ID, Some(&Key::from_bytes([9; 32]))),
        Err(OfficeError::KeyMissing)
    ));
    assert_eq!(fs::read(folder.join("office.db")).unwrap(), before);

    // The key that opens it may remove the unfinished create.
    Office::discard(&folder, ID, Some(&key)).unwrap();
    assert!(!folder.exists());
}

#[test]
fn a_log_left_beside_an_empty_database_is_never_looked_into_or_deleted() {
    for name in ["org.db-wal", "office.db-wal"] {
        let dir = TempDir::new().unwrap();
        let folder = folder_of(&dir, ID);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(name), b"frames").unwrap();

        for key in [None, Some(test_key())] {
            assert!(
                matches!(
                    Office::discard(&folder, ID, key.as_ref()),
                    Err(OfficeError::Storage(_))
                ),
                "{name} {key:?}"
            );
            assert_eq!(fs::read(folder.join(name)).unwrap(), b"frames");
        }
    }
}

#[path = "tests/instance.rs"]
mod instance;
