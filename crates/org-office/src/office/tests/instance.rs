use super::*;

// X-02 rev15 §5.4: instance binding is local metadata, not a schema version.
const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const OTHER_INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

#[test]
fn v3_create_persists_instance_and_v2_open_leaves_it_untouched() {
    let dir = TempDir::new().unwrap();
    let folder = folder_of(&dir, ID);
    let key = test_key();
    let office =
        Office::create_for_instance(&folder, &new_office(ID), key.clone(), INSTANCE).unwrap();
    assert_eq!(
        meta(&office.local, "instance_id").unwrap().as_deref(),
        Some(INSTANCE)
    );
    drop(office);
    let office = Office::open(&folder, key.clone()).unwrap();
    assert_eq!(
        meta(&office.local, "instance_id").unwrap().as_deref(),
        Some(INSTANCE)
    );
    drop(office);
    let before = fs::read(folder.join(OFFICE_DB)).unwrap();
    assert!(matches!(
        Office::open_for_instance(&folder, key.clone(), OTHER_INSTANCE),
        Err(OfficeError::Invalid(_))
    ));
    assert_eq!(fs::read(folder.join(OFFICE_DB)).unwrap(), before);
    drop(Office::open_for_instance(&folder, key, INSTANCE).unwrap());
}

fn legacy_instance_office(version: i64, bound: Option<&str>) -> (TempDir, Key, PathBuf) {
    let (dir, key, office) = made();
    let folder = office.folder().to_owned();
    drop(office);
    fs::remove_file(folder.join(OFFICE_DB)).unwrap();
    let schema = Schema {
        version,
        create: match version {
            1 => |tx| {
                tx.execute_batch(include_str!(
                    "../../store/work/tests/fixtures/office-v1.sql"
                ))
            },
            2 => |tx| {
                tx.execute_batch(include_str!(
                    "../../store/work/tests/fixtures/office-v2.sql"
                ))
            },
            3 => |tx| {
                tx.execute_batch(include_str!(
                    "../../store/work/tests/fixtures/office-v3.sql"
                ))
            },
            4 => |tx| {
                tx.execute_batch(include_str!(
                    "../../store/work/tests/fixtures/office-v4.sql"
                ))
            },
            5 => OFFICE_SCHEMA.create,
            _ => unreachable!(),
        },
        upgrade: |_, _| unreachable!("fixture creates only its declared released schema"),
    };
    let store = Store::open(&folder.join(OFFICE_DB), &key, Opening::New, &schema).unwrap();
    put_meta(&store, &[("office_id", ID), ("incarnation", "17")]).unwrap();
    if let Some(instance) = bound {
        put_meta(&store, &[("instance_id", instance)]).unwrap();
    }
    drop(store);
    (dir, key, folder)
}

#[test]
fn v3_start_adopts_released_v1_through_v5_once_and_reopens() {
    for version in 1..=5 {
        let (_dir, key, folder) = legacy_instance_office(version, None);
        // A v2 open can migrate the schema but must never create the instance key.
        let office = Office::open(&folder, key.clone()).unwrap();
        assert!(meta(&office.local, "instance_id").unwrap().is_none());
        drop(office);
        // Use another genuine legacy fixture: v3 must adopt in the migration transaction,
        // rather than testing only a file already upgraded by the preceding v2 control.
        let (_legacy_dir, key, folder) = legacy_instance_office(version, None);
        let office = Office::open_for_instance(&folder, key.clone(), INSTANCE).unwrap();
        assert_eq!(
            meta(&office.local, "instance_id").unwrap().as_deref(),
            Some(INSTANCE)
        );
        assert_eq!(
            meta(&office.local, "incarnation").unwrap().as_deref(),
            Some("17")
        );
        drop(office);
        let office = Office::open_for_instance(&folder, key, INSTANCE).unwrap();
        assert_eq!(
            meta(&office.local, "instance_id").unwrap().as_deref(),
            Some(INSTANCE)
        );
        office
            .local
            .read(|db| {
                assert_eq!(
                    db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                        .unwrap(),
                    5
                );
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn v3_mismatch_on_released_v1_through_v5_changes_neither_database() {
    for version in 1..=5 {
        let (_dir, key, folder) = legacy_instance_office(version, Some(INSTANCE));
        let before: Vec<_> = [OFFICE_DB, ORG_DB]
            .map(|name| fs::read(folder.join(name)).unwrap())
            .into();
        assert!(
            matches!(
                Office::open_for_instance(&folder, key.clone(), OTHER_INSTANCE),
                Err(OfficeError::Invalid("office belongs to another instance"))
            ),
            "v{version}"
        );
        for (name, expected) in [OFFICE_DB, ORG_DB].into_iter().zip(before) {
            assert_eq!(
                fs::read(folder.join(name)).unwrap(),
                expected,
                "v{version} {name}"
            );
        }
        // Version1 is still version1, including no new work/status/message tables.
        let schema = Schema {
            version,
            create: |_| unreachable!(),
            upgrade: |_, _| unreachable!(),
        };
        let store = Store::open(&folder.join(OFFICE_DB), &key, Opening::Existing, &schema).unwrap();
        store
            .read(|db| {
                assert_eq!(
                    db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                        .unwrap(),
                    version
                );
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn v3_adoption_rolls_back_with_failed_upgrade_and_retry_can_adopt_another_instance() {
    let (_dir, key, folder) = legacy_instance_office(1, None);
    let corrupt = Schema {
        version: 1,
        create: |tx| {
            tx.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL) STRICT;")
        },
        upgrade: |_, _| unreachable!(),
    };
    let store = Store::open(&folder.join(OFFICE_DB), &key, Opening::Existing, &corrupt).unwrap();
    store
        .write(|db| {
            db.execute_batch("CREATE TABLE msg_threads(block_upgrade INTEGER);")
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    drop(store);
    assert!(matches!(
        Office::open_for_instance(&folder, key.clone(), INSTANCE),
        Err(OfficeError::Storage(_))
    ));
    let store = Store::open(&folder.join(OFFICE_DB), &key, Opening::Existing, &corrupt).unwrap();
    assert!(meta(&store, "instance_id").unwrap().is_none());
    store
        .write(|db| {
            db.execute_batch("DROP TABLE msg_threads")
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    drop(store);
    let office = Office::open_for_instance(&folder, key, OTHER_INSTANCE).unwrap();
    assert_eq!(
        meta(&office.local, "instance_id").unwrap().as_deref(),
        Some(OTHER_INSTANCE)
    );
}

#[test]
fn v3_invalid_instance_is_refused_before_creating_or_opening_any_path() {
    for instance in [
        "",
        "not-an-id",
        "81ARZ3NDEKTSV4RRFFQ69G5FAV",
        "01arz3ndektsv4rrffq69g5fav",
    ] {
        let dir = TempDir::new().unwrap();
        let folder = folder_of(&dir, ID);
        assert!(matches!(
            Office::create_for_instance(&folder, &new_office(ID), test_key(), instance),
            Err(OfficeError::Invalid(_))
        ));
        assert!(!folder.exists());
        assert!(matches!(
            Office::open_for_instance(&folder, test_key(), instance),
            Err(OfficeError::Invalid(_))
        ));
        assert!(!folder.exists());
    }
}

#[test]
fn v3_refuses_wrong_local_or_org_identity_before_adopting_instance() {
    for (target, missing) in ["local-office", "org-office", "org-organization"]
        .into_iter()
        .flat_map(|target| [(target, false), (target, true)])
    {
        let (_dir, key, folder) = legacy_instance_office(1, None);
        let (file, schema, column) = match target {
            "local-office" => (
                OFFICE_DB,
                Schema {
                    version: 1,
                    create: |_| unreachable!(),
                    upgrade: |_, _| unreachable!(),
                },
                "office_id",
            ),
            "org-office" => (ORG_DB, org_db::SCHEMA, "office_id"),
            _ => (ORG_DB, org_db::SCHEMA, "organization_id"),
        };
        let store = Store::open(&folder.join(file), &key, Opening::Existing, &schema).unwrap();
        store
            .write(move |db| {
                if missing {
                    db.execute("DELETE FROM meta WHERE key=?1", [column])
                        .map_err(OfficeError::storage)?;
                } else {
                    db.execute("UPDATE meta SET value='OTHER' WHERE key=?1", [column])
                        .map_err(OfficeError::storage)?;
                }
                Ok(((), None))
            })
            .unwrap();
        drop(store);
        let before = fs::read(folder.join(OFFICE_DB)).unwrap();
        assert!(
            matches!(
                Office::open_for_instance(&folder, key, INSTANCE),
                Err(OfficeError::Invalid(
                    "the databases belong to another office"
                ))
            ),
            "{target}"
        );
        assert_eq!(
            fs::read(folder.join(OFFICE_DB)).unwrap(),
            before,
            "{target}"
        );
    }
}

#[test]
fn v2_open_preserves_an_unrecognized_instance_value_and_wrong_key_cannot_adopt() {
    let (_dir, key, folder) = legacy_instance_office(5, Some("legacy-unrecognized-value"));
    let office = Office::open(&folder, key.clone()).unwrap();
    assert_eq!(
        meta(&office.local, "instance_id").unwrap().as_deref(),
        Some("legacy-unrecognized-value")
    );
    drop(office);
    let (_dir, key, folder) = legacy_instance_office(1, None);
    let before = fs::read(folder.join(OFFICE_DB)).unwrap();
    assert!(matches!(
        Office::open_for_instance(&folder, Key::from_bytes([9; 32]), INSTANCE),
        Err(OfficeError::KeyMissing)
    ));
    assert_eq!(fs::read(folder.join(OFFICE_DB)).unwrap(), before);
    let office = Office::open_for_instance(&folder, key, INSTANCE).unwrap();
    assert_eq!(
        meta(&office.local, "instance_id").unwrap().as_deref(),
        Some(INSTANCE)
    );
}

#[test]
fn v3_newer_schema_is_refused_without_adopting_instance() {
    let (_dir, key, folder) = legacy_instance_office(5, None);
    let store = Store::open(
        &folder.join(OFFICE_DB),
        &key,
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store
        .write(|db| {
            db.pragma_update(None, "user_version", 6)
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    drop(store);
    let before = fs::read(folder.join(OFFICE_DB)).unwrap();
    assert!(matches!(
        Office::open_for_instance(&folder, key.clone(), INSTANCE),
        Err(OfficeError::Storage(_))
    ));
    assert_eq!(fs::read(folder.join(OFFICE_DB)).unwrap(), before);
    let future = Schema {
        version: 6,
        create: |_| unreachable!(),
        upgrade: |_, _| unreachable!(),
    };
    let store = Store::open(&folder.join(OFFICE_DB), &key, Opening::Existing, &future).unwrap();
    assert!(meta(&store, "instance_id").unwrap().is_none());
}

#[test]
fn v3_instance_mismatch_precedes_any_legacy_org_migration() {
    let (_dir, key, folder) = legacy_instance_office(1, Some(INSTANCE));
    fs::remove_file(folder.join(ORG_DB)).unwrap();
    let schema = Schema {
        version: 1,
        create: |tx| tx.execute_batch(include_str!("../../store/org/fixtures/schema-v1.sql")),
        upgrade: |_, _| unreachable!(),
    };
    let org = Store::open(&folder.join(ORG_DB), &key, Opening::New, &schema).unwrap();
    put_meta(
        &org,
        &[("office_id", ID), ("organization_id", &format!("ORG{ID}"))],
    )
    .unwrap();
    drop(org);
    let before = fs::read(folder.join(ORG_DB)).unwrap();
    assert!(matches!(
        Office::open_for_instance(&folder, key.clone(), OTHER_INSTANCE),
        Err(OfficeError::Invalid("office belongs to another instance"))
    ));
    assert_eq!(fs::read(folder.join(ORG_DB)).unwrap(), before);
    // Positive control: the same genuine org v1 fixture can be migrated on a matching start.
    drop(Office::open_for_instance(&folder, key, INSTANCE).unwrap());
    assert_ne!(fs::read(folder.join(ORG_DB)).unwrap(), before);
}

#[test]
fn v3_failed_create_removes_instance_binding_and_allows_a_different_retry() {
    for step in ["databases", "workspace", "organization", "office_json"] {
        let dir = TempDir::new().unwrap();
        let folder = folder_of(&dir, ID);
        FAIL_AT.with(|f| f.set(Some(step)));
        let result = Office::create_for_instance(&folder, &new_office(ID), test_key(), INSTANCE);
        FAIL_AT.with(|f| f.set(None));
        assert!(matches!(result, Err(OfficeError::Storage(_))), "{step}");
        assert_eq!(names_in(&folder), ["office.lock"], "{step}");
        let office =
            Office::create_for_instance(&folder, &new_office(ID), test_key(), OTHER_INSTANCE)
                .unwrap();
        assert_eq!(
            meta(&office.local, "instance_id").unwrap().as_deref(),
            Some(OTHER_INSTANCE)
        );
    }
}

#[test]
fn v3_preflight_preserves_a_wal_beside_an_empty_missing_or_non_file_database() {
    for kind in ["empty", "missing", "directory"] {
        let (_dir, key, folder) = legacy_instance_office(5, None);
        let database = folder.join(OFFICE_DB);
        fs::remove_file(&database).unwrap();
        match kind {
            "empty" => fs::write(&database, b"").unwrap(),
            "directory" => fs::create_dir(&database).unwrap(),
            _ => {}
        }
        let wal = folder.join("office.db-wal");
        fs::write(&wal, b"recoverable frames").unwrap();
        let org_before = fs::read(folder.join(ORG_DB)).unwrap();
        let Err(OfficeError::Storage(cause)) = Office::open_for_instance(&folder, key, INSTANCE)
        else {
            panic!("{kind}: invalid database was not refused");
        };
        if kind != "missing" {
            assert_eq!(cause.to_string(), "the database file is missing or empty");
        }
        assert_eq!(
            fs::read(&wal).ok().as_deref(),
            Some(b"recoverable frames".as_slice()),
            "{kind}: inspection must not delete the log"
        );
        assert_eq!(fs::read(folder.join(ORG_DB)).unwrap(), org_before);
        match kind {
            "empty" => assert_eq!(fs::metadata(database).unwrap().len(), 0),
            "directory" => assert!(database.is_dir()),
            _ => assert!(!database.exists()),
        }
    }
}
