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
        meta(&office.stores.local, "instance_id")
            .unwrap()
            .as_deref(),
        Some(INSTANCE)
    );
    drop(office);
    let office = Office::open(&folder, key.clone()).unwrap();
    assert_eq!(
        meta(&office.stores.local, "instance_id")
            .unwrap()
            .as_deref(),
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

// Frozen from accepted 0fdb972 office v6, independently of OFFICE_SCHEMA.create.
// Includes nullable task-only member status, owned processes and v6 work-key admission.
const RELEASED_V6_SQL: &str = r#"CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
CREATE TABLE msg_threads (
             thread_id TEXT PRIMARY KEY, subject TEXT, created_at TEXT NOT NULL,
             last_msg_id TEXT NOT NULL, last_msg_at TEXT NOT NULL,
             message_count INTEGER NOT NULL, participants_json TEXT NOT NULL
         ) STRICT;
         CREATE INDEX idx_msg_threads_last ON msg_threads (last_msg_at DESC, thread_id DESC);
         CREATE TABLE msg_thread_participants (
             member_id TEXT NOT NULL,
             thread_id TEXT NOT NULL REFERENCES msg_threads (thread_id) ON DELETE CASCADE,
             PRIMARY KEY (member_id, thread_id)
         ) STRICT;
         CREATE TABLE msg_headers (
             id TEXT PRIMARY KEY, thread_id TEXT NOT NULL, reply_to TEXT,
             origin_office_id TEXT NOT NULL, origin_incarnation TEXT NOT NULL,
             origin_seq INTEGER NOT NULL, local_seq INTEGER NOT NULL UNIQUE,
             org_epoch INTEGER NOT NULL, org_seq INTEGER NOT NULL,
             from_member_id TEXT NOT NULL, from_display_name TEXT NOT NULL,
             to_member_id TEXT NOT NULL, to_display_name TEXT NOT NULL,
             kind TEXT NOT NULL, priority TEXT NOT NULL DEFAULT 'normal', subject TEXT,
             created_at TEXT NOT NULL, expires_at TEXT, ttl_ms INTEGER,
             body_bytes INTEGER NOT NULL, meta_bytes INTEGER NOT NULL DEFAULT 0,
             content_hash BLOB NOT NULL, flags INTEGER NOT NULL DEFAULT 0, dead_reason TEXT
         ) STRICT;
         CREATE UNIQUE INDEX idx_msg_origin
             ON msg_headers (origin_office_id, origin_incarnation, origin_seq);
         CREATE UNIQUE INDEX idx_msg_delivery_failed_once
             ON msg_headers (reply_to) WHERE kind = 'delivery_failed';
         CREATE INDEX idx_msg_to_seq ON msg_headers (to_member_id, local_seq DESC);
         CREATE INDEX idx_msg_thread_seq ON msg_headers (thread_id, local_seq DESC);
         CREATE INDEX idx_msg_reply_to ON msg_headers (reply_to) WHERE reply_to IS NOT NULL;
         CREATE TABLE msg_bodies (
             id TEXT PRIMARY KEY REFERENCES msg_headers (id) ON DELETE CASCADE,
             compression INTEGER NOT NULL, body_blob BLOB NOT NULL, meta_blob BLOB
         ) STRICT;
         CREATE TABLE msg_state (
             id TEXT PRIMARY KEY REFERENCES msg_headers (id) ON DELETE CASCADE,
             state TEXT NOT NULL, triage_gen INTEGER NOT NULL DEFAULT 0,
             snoozed_until TEXT, snooze_after_task INTEGER NOT NULL DEFAULT 0,
             reason TEXT, read_at TEXT, updated_at TEXT NOT NULL,
             owner_office_id TEXT NOT NULL, origin_incarnation TEXT NOT NULL,
             origin_seq INTEGER NOT NULL, org_epoch INTEGER NOT NULL, org_seq INTEGER NOT NULL
         ) STRICT;
         CREATE TABLE msg_unread (
             to_member_id TEXT NOT NULL, local_seq INTEGER NOT NULL,
             id TEXT NOT NULL UNIQUE REFERENCES msg_headers (id) ON DELETE CASCADE,
             from_member_id TEXT NOT NULL, kind TEXT NOT NULL,
             priority TEXT NOT NULL DEFAULT 'normal', urgent INTEGER NOT NULL DEFAULT 0,
             triage_gen INTEGER NOT NULL DEFAULT 0, reply_to TEXT, created_at TEXT NOT NULL,
             expires_at TEXT, snoozed_until TEXT,
             snooze_after_task INTEGER NOT NULL DEFAULT 0, snooze_note TEXT, triaged_at TEXT,
             PRIMARY KEY (to_member_id, local_seq)
         ) STRICT;
         CREATE INDEX idx_msg_unread_due ON msg_unread (to_member_id, snoozed_until)
             WHERE snoozed_until IS NOT NULL;
         CREATE INDEX idx_msg_unread_after_task ON msg_unread (to_member_id)
             WHERE snooze_after_task = 1;
CREATE TABLE member_status (
             member_id TEXT PRIMARY KEY, status_text TEXT, current_task_id TEXT,
             updated_at TEXT NOT NULL, expires_at TEXT,
             CHECK((status_text IS NULL) = (expires_at IS NULL)),
             CHECK(status_text IS NOT NULL OR current_task_id IS NOT NULL)
         ) STRICT;
CREATE TABLE work_feed (
             seq INTEGER PRIMARY KEY AUTOINCREMENT,
             change_id TEXT, targets TEXT NOT NULL
         ) STRICT;
         CREATE TABLE work_origin_counter (
             origin_incarnation TEXT PRIMARY KEY,
             last_seq INTEGER NOT NULL CHECK (last_seq >= 0)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE work_origin_log (
             origin_office_id TEXT NOT NULL, origin_incarnation TEXT NOT NULL,
             origin_seq INTEGER NOT NULL CHECK (origin_seq >= 1),
             feed_seq INTEGER NOT NULL REFERENCES work_feed(seq),
             target_id TEXT NOT NULL, generation INTEGER NOT NULL CHECK (generation >= 1),
             version INTEGER NOT NULL CHECK (version >= 0),
             org_epoch INTEGER NOT NULL CHECK (org_epoch >= 0),
             org_seq INTEGER NOT NULL CHECK (org_seq >= 0),
             committed_at_ms INTEGER NOT NULL CHECK (committed_at_ms >= 0),
             UNIQUE (target_id, generation, version),
             PRIMARY KEY (origin_office_id, origin_incarnation, origin_seq)
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX idx_work_origin_stream ON work_origin_log(origin_incarnation, origin_seq);
         CREATE INDEX idx_work_origin_target_version ON work_origin_log(target_id, version);
CREATE TABLE work_items (
             id TEXT PRIMARY KEY, generation INTEGER NOT NULL CHECK (generation >= 1),
             version INTEGER NOT NULL CHECK (version >= 1), data TEXT NOT NULL,
             CHECK (json_extract(data, '$.id') IS id
                 AND json_extract(data, '$.generation') IS generation
                 AND json_extract(data, '$.version') IS version)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE work_ids (
             id TEXT PRIMARY KEY, item_id TEXT NOT NULL REFERENCES work_items(id),
             kind TEXT NOT NULL CHECK (kind IN ('item', 'step', 'link', 'comment'))
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX idx_work_ids_item ON work_ids(item_id);
         CREATE TABLE work_events (
             target_id TEXT NOT NULL REFERENCES work_items(id),
             version INTEGER NOT NULL CHECK (version >= 1),
             feed_seq INTEGER NOT NULL CHECK (feed_seq >= 1) REFERENCES work_feed(seq),
             data TEXT NOT NULL,
             CHECK (json_extract(data, '$.target_id') IS target_id
                 AND json_extract(data, '$.version') IS version),
             PRIMARY KEY (target_id, version)
         ) STRICT, WITHOUT ROWID;
         CREATE TRIGGER work_events_no_replace BEFORE INSERT ON work_events
             WHEN EXISTS (SELECT 1 FROM work_events
                 WHERE target_id = NEW.target_id AND version = NEW.version)
             BEGIN SELECT RAISE(ABORT, 'work events are immutable'); END;
         CREATE TRIGGER work_events_no_update BEFORE UPDATE ON work_events
             BEGIN SELECT RAISE(ABORT, 'work events are immutable'); END;
         CREATE TRIGGER work_events_no_delete BEFORE DELETE ON work_events
             BEGIN SELECT RAISE(ABORT, 'work events are immutable'); END;
CREATE TABLE work_change_results (
             change_id TEXT PRIMARY KEY CHECK (length(change_id) BETWEEN 1 AND 128),
             params_hash BLOB NOT NULL CHECK (length(params_hash) = 32),
             answer TEXT NOT NULL,
             expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms >= 0),
             principal_account TEXT, principal_member TEXT,
             actor_kind TEXT CHECK (actor_kind IS NULL OR length(actor_kind) > 0)
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX idx_work_change_results_expiry ON work_change_results(expires_at_ms);
         CREATE TABLE work_number_counters (
             scope TEXT PRIMARY KEY, last INTEGER NOT NULL CHECK (last >= 0)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE work_clock (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             high_water_ms INTEGER NOT NULL CHECK (high_water_ms >= 0)
         ) STRICT;
CREATE TABLE owned_processes (
             id INTEGER PRIMARY KEY AUTOINCREMENT, generation INTEGER NOT NULL,
             mark TEXT NOT NULL UNIQUE CHECK(mark <> ''),
             leader_pid INTEGER, leader_pgid INTEGER, leader_started TEXT, leader_boot TEXT,
             tool_sessions TEXT NOT NULL, inventory TEXT NOT NULL, created_at TEXT NOT NULL,
             CHECK((leader_pid IS NULL) = (leader_pgid IS NULL)
                 AND (leader_pid IS NULL) = (leader_started IS NULL)
                 AND (leader_pid IS NULL) = (leader_boot IS NULL))
         ) STRICT;"#;

fn released_v6_admit(tx: &rusqlite::Transaction<'_>) -> Result<(), OfficeError> {
    let invalid: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM work_items
         WHERE json_extract(data,'$.kind') IN ('task','bug')
         AND json_extract(data,'$.number') IS NOT NULL
         AND json_extract(data,'$.project_id') IS NULL)",
            [],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    if invalid {
        return Err(OfficeError::Invalid(
            "work_items contains a numbered task or bug without a project",
        ));
    }
    tx.execute_batch(r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_project_number
    ON work_items(json_extract(data,'$.project_id'), json_extract(data,'$.kind'), json_extract(data,'$.number'))
    WHERE json_extract(data,'$.kind') IN ('task','bug') AND json_extract(data,'$.number') IS NOT NULL;
    CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_request_number
    ON work_items(json_extract(data,'$.number'))
    WHERE json_extract(data,'$.kind') = 'request' AND json_extract(data,'$.number') IS NOT NULL;"#).map_err(OfficeError::storage)
}

const RELEASED_V6: Schema = Schema {
    admit: Some(released_v6_admit),
    version: 6,
    create: |tx| tx.execute_batch(RELEASED_V6_SQL),
    upgrade: |_, _| unreachable!("released v6 fixture never upgrades"),
};

fn legacy_instance_office(version: i64, bound: Option<&str>) -> (TempDir, Key, PathBuf) {
    let (dir, key, office) = made();
    let folder = office.folder().to_owned();
    drop(office);
    fs::remove_file(folder.join(OFFICE_DB)).unwrap();
    let schema = Schema {
        admit: if version == 6 {
            RELEASED_V6.admit
        } else {
            None
        },
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
            5 => |tx| {
                tx.execute_batch(include_str!(
                    "../../store/work/tests/fixtures/office-v5.sql"
                ))
            },
            6 => RELEASED_V6.create,
            _ => unreachable!(),
        },
        upgrade: |_, _| unreachable!("fixture creates only its declared released schema"),
    };
    let store = Store::open(&folder.join(OFFICE_DB), Opening::New, &schema).unwrap();
    put_meta(&store, &[("office_id", ID), ("incarnation", "17")]).unwrap();
    if let Some(instance) = bound {
        put_meta(&store, &[("instance_id", instance)]).unwrap();
    }
    drop(store);
    (dir, key, folder)
}

#[test]
fn v3_start_adopts_v1_through_v6_once_and_reopens() {
    for version in 1..=6 {
        let (_dir, key, folder) = legacy_instance_office(version, None);
        // A v2 open can migrate the schema but must never create the instance key.
        let office = Office::open(&folder, key.clone()).unwrap();
        assert!(meta(&office.stores.local, "instance_id").unwrap().is_none());
        drop(office);
        // Use another genuine legacy fixture: v3 must adopt in the migration transaction,
        // rather than testing only a file already upgraded by the preceding v2 control.
        let (_legacy_dir, key, folder) = legacy_instance_office(version, None);
        let office = Office::open_for_instance(&folder, key.clone(), INSTANCE).unwrap();
        assert_eq!(
            meta(&office.stores.local, "instance_id")
                .unwrap()
                .as_deref(),
            Some(INSTANCE)
        );
        assert_eq!(
            meta(&office.stores.local, "incarnation")
                .unwrap()
                .as_deref(),
            Some("17")
        );
        drop(office);
        let office = Office::open_for_instance(&folder, key, INSTANCE).unwrap();
        assert_eq!(
            meta(&office.stores.local, "instance_id")
                .unwrap()
                .as_deref(),
            Some(INSTANCE)
        );
        office
            .stores
            .local
            .read(|db| {
                assert_eq!(
                    db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                        .unwrap(),
                    OFFICE_SCHEMA.version
                );
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn v3_mismatch_on_v1_through_v6_changes_neither_database() {
    for version in 1..=6 {
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
            admit: None,
            version,
            create: |_| unreachable!(),
            upgrade: |_, _| unreachable!(),
        };
        let store = Store::open(&folder.join(OFFICE_DB), Opening::Existing, &schema).unwrap();
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
        admit: None,
        version: 1,
        create: |tx| {
            tx.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL) STRICT;")
        },
        upgrade: |_, _| unreachable!(),
    };
    let store = Store::open(&folder.join(OFFICE_DB), Opening::Existing, &corrupt).unwrap();
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
    let store = Store::open(&folder.join(OFFICE_DB), Opening::Existing, &corrupt).unwrap();
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
        meta(&office.stores.local, "instance_id")
            .unwrap()
            .as_deref(),
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
                    admit: None,
                    version: 1,
                    create: |_| unreachable!(),
                    upgrade: |_, _| unreachable!(),
                },
                "office_id",
            ),
            "org-office" => (ORG_DB, org_db::SCHEMA, "office_id"),
            _ => (ORG_DB, org_db::SCHEMA, "organization_id"),
        };
        let store = Store::open(&folder.join(file), Opening::Existing, &schema).unwrap();
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
fn v2_open_preserves_an_unrecognized_instance_value_and_adoption_needs_no_key() {
    let (_dir, key, folder) = legacy_instance_office(5, Some("legacy-unrecognized-value"));
    let office = Office::open(&folder, key.clone()).unwrap();
    assert_eq!(
        meta(&office.stores.local, "instance_id")
            .unwrap()
            .as_deref(),
        Some("legacy-unrecognized-value")
    );
    drop(office);
    let (_dir, key, folder) = legacy_instance_office(1, None);
    let office = Office::open_for_instance(&folder, key, INSTANCE).unwrap();
    assert_eq!(
        meta(&office.stores.local, "instance_id")
            .unwrap()
            .as_deref(),
        Some(INSTANCE)
    );
}

#[test]
fn v3_newer_schema_is_refused_without_adopting_instance() {
    let (_dir, key, folder) = legacy_instance_office(5, None);
    let store = Store::open(&folder.join(OFFICE_DB), Opening::Existing, &OFFICE_SCHEMA).unwrap();
    store
        .write(|db| {
            db.pragma_update(None, "user_version", OFFICE_SCHEMA.version + 1)
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
        admit: None,
        version: OFFICE_SCHEMA.version + 1,
        create: |_| unreachable!(),
        upgrade: |_, _| unreachable!(),
    };
    let store = Store::open(&folder.join(OFFICE_DB), Opening::Existing, &future).unwrap();
    assert!(meta(&store, "instance_id").unwrap().is_none());
}

#[test]
fn v3_instance_mismatch_precedes_any_legacy_org_migration() {
    let (_dir, key, folder) = legacy_instance_office(1, Some(INSTANCE));
    fs::remove_file(folder.join(ORG_DB)).unwrap();
    let schema = Schema {
        admit: None,
        version: 1,
        create: |tx| tx.execute_batch(include_str!("../../store/org/fixtures/schema-v1.sql")),
        upgrade: |_, _| unreachable!(),
    };
    let org = Store::open(&folder.join(ORG_DB), Opening::New, &schema).unwrap();
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
            meta(&office.stores.local, "instance_id")
                .unwrap()
                .as_deref(),
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

// Compare every pre-existing row using SQLite values, including blob and NULL cells.
fn released_rows(db: &rusqlite::Connection) -> Vec<(String, Vec<Vec<rusqlite::types::Value>>)> {
    let names = db.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'selected_%' AND name <> 'local_hq_creation' ORDER BY name")
        .unwrap().query_map([], |r| r.get::<_, String>(0)).unwrap()
        .collect::<rusqlite::Result<Vec<_>>>().unwrap();
    names
        .into_iter()
        .map(|name| {
            let mut statement = db.prepare(&format!("SELECT * FROM \"{name}\"")).unwrap();
            let n = statement.column_count();
            let rows = statement
                .query_map([], |r| {
                    (0..n)
                        .map(|i| r.get(i))
                        .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            (name, rows)
        })
        .collect()
}

#[test]
fn genuine_v6_constraints_metadata_and_all_rows_survive_v7_and_reopen() {
    let (_dir, key, folder) = legacy_instance_office(6, Some(INSTANCE));
    let old = Store::open(&folder.join(OFFICE_DB), Opening::Existing, &RELEASED_V6).unwrap();
    old.write(|db| {
        assert_eq!(db.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0)).unwrap(),6);
        assert_eq!(db.query_row("SELECT count(*) FROM sqlite_schema WHERE name LIKE 'selected_%'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(db.query_row("SELECT count(*) FROM sqlite_schema WHERE name='local_hq_creation'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(db.query_row("SELECT [notnull] FROM pragma_table_info('member_status') WHERE name='status_text'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(db.query_row("SELECT count(*) FROM pragma_index_list('work_items') WHERE [unique]=1 AND name IN ('idx_work_items_project_number','idx_work_items_request_number')",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        db.execute_batch("INSERT INTO member_status VALUES
            ('task-only',NULL,'opaque-task','now',NULL),
            ('status-only','working',NULL,'now','later'),
            ('combined','working','opaque-task','now','later');
            INSERT INTO meta VALUES('message_sequence','41'),('fixture','retained');
            INSERT INTO work_clock VALUES(1,987654321);
            INSERT INTO work_number_counters VALUES('requests',19),('project:opaque:task',7);
            INSERT INTO work_change_results VALUES('old-operation',zeroblob(32),'{\"err\":{\"opaque\":1.000e+02}}',3000,'account','member','owner');
            INSERT INTO owned_processes(generation,mark,tool_sessions,inventory,created_at) VALUES(7,'frozen-process','[]','{}','now');
            INSERT INTO work_items VALUES('opaque-legacy',1,1,'{\"id\":\"opaque-legacy\",\"generation\":1,\"version\":1,\"kind\":\"request\",\"number\":19,\"unknown\":1.000e+02}');
            INSERT INTO work_ids VALUES('opaque-legacy','opaque-legacy','item');
            INSERT INTO work_feed(seq,change_id,targets) VALUES(7,'old-operation','[\"opaque-legacy\"]');
            INSERT INTO work_events VALUES('opaque-legacy',1,7,'{\"target_id\":\"opaque-legacy\",\"version\":1,\"unknown\":-0e+00}');
            INSERT INTO work_origin_counter VALUES('17',1);")
            .map_err(OfficeError::storage)?;
        db.execute("INSERT INTO work_origin_log VALUES(?1,'17',1,7,'opaque-legacy',1,1,2,9,111)",[ID]).map_err(OfficeError::storage)?;
        for values in ["('empty',NULL,NULL,'now',NULL)","('no-expiry','status',NULL,'now',NULL)","('no-status',NULL,'task','now','expiry')"] {
            assert!(db.execute_batch(&format!("INSERT INTO member_status VALUES{values}")).is_err());
        }
        assert!(db.execute_batch("INSERT INTO work_items VALUES('duplicate',1,1,'{\"id\":\"duplicate\",\"generation\":1,\"version\":1,\"kind\":\"request\",\"number\":19}')").is_err());
        Ok(((),None))
    }).unwrap();
    let invalid = Schema {
        create: |tx| {
            tx.execute_batch(RELEASED_V6_SQL)?;
            tx.execute_batch("INSERT INTO work_items VALUES('invalid',1,1,'{\"id\":\"invalid\",\"generation\":1,\"version\":1,\"kind\":\"task\",\"number\":7}')")
        },
        ..RELEASED_V6
    };
    assert!(matches!(
        Store::open(&folder.join("invalid-admission.db"), Opening::New, &invalid),
        Err(OfficeError::Invalid(
            "work_items contains a numbered task or bug without a project"
        ))
    ));
    let before = old.read(|db| Ok(released_rows(db))).unwrap();
    drop(old);
    for _ in 0..2 {
        let office = Office::open_for_instance(&folder, key.clone(), INSTANCE).unwrap();
        office
            .stores
            .local
            .read({
                let before = before.clone();
                move |db| {
                    assert_eq!(
                        db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                            .unwrap(),
                        OFFICE_SCHEMA.version
                    );
                    assert_eq!(released_rows(db), before);
                    assert_eq!(
                        db.query_row("SELECT count(*) FROM local_hq_creation", [], |r| {
                            r.get::<_, i64>(0)
                        })
                        .unwrap(),
                        0
                    );
                    assert_eq!(
                        db.query_row("SELECT value FROM meta WHERE key='instance_id'", [], |r| {
                            r.get::<_, String>(0)
                        })
                        .unwrap(),
                        INSTANCE
                    );
                    assert_eq!(
                        db.query_row("SELECT value FROM meta WHERE key='incarnation'", [], |r| {
                            r.get::<_, String>(0)
                        })
                        .unwrap(),
                        "17"
                    );
                    Ok(())
                }
            })
            .unwrap();
    }
}
