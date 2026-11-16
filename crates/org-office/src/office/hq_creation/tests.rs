use super::*;
use zeroize::Zeroizing;
const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
fn binding(root: &Path) -> Binding {
    let public_root = root.join("state");
    std::fs::create_dir_all(&public_root).unwrap();
    Binding {
        operation_id: "01ARZ3NDEKTSV4RRFFQ69G5FAW".into(),
        owner_member_id: "01ARZ3NDEKTSV4RRFFQ69G5FAX".into(),
        public_root: std::fs::canonicalize(public_root).unwrap(),
        bootstrap: None,
    }
}
fn new() -> NewOffice {
    NewOffice {
        office_id: "HQ".into(),
        office_name: "HQ".into(),
        role: crate::OfficeRole::Hq,
        org_id: "ORG".into(),
        org_name: "Example".into(),
    }
}
fn material(prior: Option<LocalRecord>) -> Result<LocalRecord, OfficeError> {
    if let Some(prior) = prior {
        return Ok(prior);
    }
    // Synthetic persistence-only fixture. Worker tests validate actual crypto.
    let mut public = [9; 65];
    public[0] = 4;
    LocalRecord::new(
        "ORG".into(),
        "01ARZ3NDEKTSV4RRFFQ69G5FAW".into(),
        public,
        vec![7; 140],
        Zeroizing::new(vec![5; 100]),
    )
}
#[test]
fn hq_creation_retries_before_after_atomic_material_and_created_marker() {
    for stage in [
        "hq_after_intent",
        "hq_before_material",
        "hq_before_material_commit",
        "hq_after_material",
        "hq_before_public_escrow",
        "hq_after_public_escrow",
        "hq_before_created",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("HQ");
        let key = Key::from_bytes([8; 32]);
        FAIL_AT.set(Some(stage));
        assert!(Office::provision_hq_for_instance(
            &folder,
            &new(),
            key.clone(),
            INSTANCE,
            &binding(dir.path()),
            material
        )
        .is_err());
        FAIL_AT.set(None);
        assert!(!folder.join("office.json").exists());
        assert!(matches!(
            Office::discard(&folder, "HQ", Some(&key)),
            Err(OfficeError::OfficeExists)
        ));
        let had_committed = matches!(
            stage,
            "hq_after_material"
                | "hq_before_public_escrow"
                | "hq_after_public_escrow"
                | "hq_before_created"
        );
        let office = Office::provision_hq_for_instance(
            &folder,
            &new(),
            key.clone(),
            INSTANCE,
            &binding(dir.path()),
            |prior| {
                assert_eq!(prior.is_some(), had_committed);
                material(prior)
            },
        )
        .unwrap();
        let record = office.hq_creation_record().unwrap().unwrap();
        assert_eq!(record.package, vec![7; 140]);
        let snapshot = office.org_snapshot().unwrap();
        assert_eq!(snapshot.position.seq, 1);
        assert_eq!(snapshot.members.len(), 1);
        assert_eq!(snapshot.members[0].id, binding(dir.path()).owner_member_id);
        drop(office);
        // Lost create reply: exact record; no owner/genesis rerun.
        let office = Office::provision_hq_for_instance(
            &folder,
            &new(),
            key.clone(),
            INSTANCE,
            &binding(dir.path()),
            |prior| {
                assert!(prior.is_some());
                material(prior)
            },
        )
        .unwrap();
        assert_eq!(office.org_snapshot().unwrap().position.seq, 1);
        drop(office);
        assert!(Office::open(&folder, key.clone()).is_err());
        let reopened = Office::open_for_instance(&folder, key, INSTANCE).unwrap();
        assert_eq!(
            reopened.hq_creation_record().unwrap().unwrap().package,
            record.package
        );
    }
}
#[test]
fn hq_creation_changed_inputs_and_missing_committed_material_never_generate() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("HQ");
    let key = Key::from_bytes([8; 32]);
    let office = Office::provision_hq_for_instance(
        &folder,
        &new(),
        key.clone(),
        INSTANCE,
        &binding(dir.path()),
        material,
    )
    .unwrap();
    drop(office);
    let before = fs::read(folder.join("office.db")).unwrap();
    let mut changed = binding(dir.path());
    changed.operation_id = "01ARZ3NDEKTSV4RRFFQ69G5FAY".into();
    assert!(Office::provision_hq_for_instance(
        &folder,
        &new(),
        key.clone(),
        INSTANCE,
        &changed,
        |_| panic!("changed request must not reach material factory")
    )
    .is_err());
    assert_eq!(fs::read(folder.join("office.db")).unwrap(), before);
    let office = Office::open_for_instance(&folder, key.clone(), INSTANCE).unwrap();
    assert!(office
        .local_store()
        .write(|db| {
            db.execute("DELETE FROM local_hq_creation", [])
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .is_err());
    // Explicit corruption fixture, not a production repair path.
    office
        .local_store()
        .write(|db| {
            db.execute_batch(
                "DROP TRIGGER local_hq_creation_delete; DELETE FROM local_hq_creation;",
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    drop(office);
    assert!(Office::provision_hq_for_instance(
        &folder,
        &new(),
        key.clone(),
        INSTANCE,
        &binding(dir.path()),
        |_| panic!("missing committed material must not regenerate")
    )
    .is_err());
    assert!(Office::open_for_instance(&folder, key, INSTANCE).is_err());
}
#[test]
fn hq_creation_finishes_empty_preschema_database_but_retains_foreign_schema() {
    for foreign in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("HQ");
        let key = Key::from_bytes([8; 32]);
        FAIL_AT.set(Some("hq_after_intent"));
        assert!(Office::provision_hq_for_instance(
            &folder,
            &new(),
            key.clone(),
            INSTANCE,
            &binding(dir.path()),
            material
        )
        .is_err());
        FAIL_AT.set(None);
        // Simulate the initial SQLCipher file opened, with no schema transaction.
        let schema = Schema {
            admit: None,
            version: 0,
            create: if foreign {
                |tx| {
                    tx.execute_batch("CREATE TABLE foreign_data(x TEXT); INSERT INTO foreign_data VALUES('retain');")
                }
            } else {
                |_| Ok(())
            },
            upgrade: |_, _| unreachable!(),
        };
        let db = Store::open(&folder.join("office.db"), &key, Opening::New, &schema).unwrap();
        drop(db);
        let result = Office::provision_hq_for_instance(
            &folder,
            &new(),
            key.clone(),
            INSTANCE,
            &binding(dir.path()),
            material,
        );
        assert_eq!(result.is_err(), foreign);
        if foreign {
            assert!(Store::inspect(&folder.join("office.db"), &key, |db| {
                assert_eq!(
                    db.query_row("SELECT x FROM foreign_data", [], |r| r.get::<_, String>(0))
                        .unwrap(),
                    "retain"
                );
                Ok(())
            })
            .is_ok());
        }
    }
}

#[test]
fn public_escrow_repairs_absence_but_retains_conflicts_and_bound_location() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("HQ");
    let key = Key::from_bytes([8; 32]);
    let original = binding(dir.path());
    let office = Office::provision_hq_for_instance(
        &folder,
        &new(),
        key.clone(),
        INSTANCE,
        &original,
        material,
    )
    .unwrap();
    let public = office.hq_creation_public_record().unwrap().unwrap();
    assert_eq!(public.package, vec![7; 140]);
    let copy = original.public_root.join(PUBLIC_ESCROW);
    assert_eq!(fs::read(&copy).unwrap(), public.package);
    drop(office);
    fs::remove_file(&copy).unwrap();
    let office = Office::open_for_instance(&folder, key.clone(), INSTANCE).unwrap();
    assert_eq!(
        office.hq_creation_public_record().unwrap(),
        Some(public.clone())
    );
    drop(office);
    let mut changed = original.clone();
    changed.public_root = dir.path().join("different-state");
    fs::create_dir(&changed.public_root).unwrap();
    changed.public_root = fs::canonicalize(&changed.public_root).unwrap();
    assert!(Office::provision_hq_for_instance(
        &folder,
        &new(),
        key.clone(),
        INSTANCE,
        &changed,
        |_| panic!("location change must not reach material")
    )
    .is_err());
    assert!(!changed.public_root.join("security").exists());
    fs::write(&copy, b"retained conflicting public bytes").unwrap();
    assert!(Office::open_for_instance(&folder, key.clone(), INSTANCE).is_err());
    assert_eq!(
        fs::read(&copy).unwrap(),
        b"retained conflicting public bytes"
    );
    // A hostile link is never followed, even to an otherwise valid package.
    fs::remove_file(&copy).unwrap();
    let other = dir.path().join("opaque-source");
    fs::write(&other, &public.package).unwrap();
    std::os::unix::fs::symlink(&other, &copy).unwrap();
    assert!(Office::open_for_instance(&folder, key, INSTANCE).is_err());
    assert!(fs::symlink_metadata(&copy).unwrap().is_symlink());
    assert_eq!(fs::read(other).unwrap(), public.package);
}

#[test]
fn selected_creation_refuses_missing_or_mismatched_bootstrap_before_material_or_files() {
    let id = |n| format!("01900000-0000-7000-8000-{n:012x}");
    for mismatch in ["missing", "office", "organization", "name"] {
        let dir = tempfile::tempdir().unwrap();
        let mut new = new();
        new.office_id = id(2);
        new.org_id = id(1);
        let folder = dir.path().join(&new.office_id);
        let mut binding = binding(dir.path());
        if mismatch != "missing" {
            let mut prepared = org_proto::selected_bootstrap::PreparedV1 {
                plan: org_proto::selected_bootstrap::PlanV1 {
                    version: 1,
                    name: new.org_name.clone(),
                    slug: "example".into(),
                    repository_remote_url: "https://example.test/org.git".into(),
                    state_branch: "org/state".into(),
                    home_team_name: "Core".into(),
                },
                organization_id: id(1),
                office_id: id(2),
                repository_id: id(3),
                home_team_id: id(4),
                hr_employee_id: id(5),
            };
            match mismatch {
                "office" => prepared.office_id = id(6),
                "organization" => prepared.organization_id = id(6),
                "name" => prepared.plan.name = "Different".into(),
                _ => unreachable!(),
            }
            binding.bootstrap = Some(prepared);
        }
        assert!(
            Office::provision_hq_for_instance(
                &folder,
                &new,
                Key::from_bytes([9; 32]),
                INSTANCE,
                &binding,
                |_| panic!("invalid selected bootstrap must not generate key material"),
            )
            .is_err(),
            "{mismatch}"
        );
        assert!(!folder.exists(), "{mismatch}");
    }
}

// Independently frozen accepted schema7 from 0d4035d7; never calls current create.
const FROZEN_BASE_V6: &str = r#"CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
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
fn frozen_selected_v7(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE selected_directory (
            organization_id TEXT NOT NULL, generation INTEGER NOT NULL CHECK(generation>0),
            data TEXT NOT NULL, payload_sha256 TEXT NOT NULL,
            PRIMARY KEY(organization_id,generation)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_directory_head (
            organization_id TEXT PRIMARY KEY, generation INTEGER NOT NULL CHECK(generation>0),
            FOREIGN KEY(organization_id,generation) REFERENCES selected_directory(organization_id,generation)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_owner_authority (
            organization_id TEXT PRIMARY KEY, account_id TEXT NOT NULL,
            member_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
            active INTEGER NOT NULL CHECK(active IN (0,1))
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_tasks (
            organization_id TEXT NOT NULL, task_id TEXT NOT NULL,
            revision INTEGER NOT NULL CHECK(revision>0),
            team_id TEXT NOT NULL, project_id TEXT,
            assigned_team_id TEXT, assignee_id TEXT,
            canonical_path TEXT NOT NULL, markdown BLOB NOT NULL, payload_sha256 TEXT NOT NULL,
            PRIMARY KEY(organization_id,task_id), UNIQUE(canonical_path)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_events (
            organization_id TEXT NOT NULL, task_id TEXT NOT NULL,
            revision INTEGER NOT NULL CHECK(revision>0),
            schema_version INTEGER NOT NULL, resource_profile TEXT NOT NULL,
            payload_sha256 TEXT NOT NULL, event_bytes BLOB NOT NULL,
            PRIMARY KEY(organization_id,task_id,revision),
            FOREIGN KEY(organization_id,task_id) REFERENCES selected_tasks(organization_id,task_id)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_receipts (
            organization_id TEXT NOT NULL, operation_id TEXT NOT NULL,
            instance_id TEXT NOT NULL, account_id TEXT NOT NULL,
            actor_kind TEXT NOT NULL, actor_id TEXT,
            method TEXT NOT NULL, command_version INTEGER NOT NULL,
            fingerprint TEXT NOT NULL, result BLOB NOT NULL,
            expires_at_ms INTEGER NOT NULL CHECK(expires_at_ms>=0),
            PRIMARY KEY(organization_id,operation_id),
            CHECK ((actor_kind='employee')=(actor_id IS NOT NULL))
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_adoptions (
            organization_id TEXT NOT NULL, task_id TEXT NOT NULL,
            writer_epoch INTEGER NOT NULL CHECK(writer_epoch>0),
            origin_generation INTEGER NOT NULL CHECK(origin_generation>0),
            manifest_sha256 TEXT NOT NULL, source_metadata TEXT NOT NULL,
            PRIMARY KEY(organization_id,task_id),
            FOREIGN KEY(organization_id,task_id) REFERENCES selected_tasks(organization_id,task_id)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_artifacts (
            namespace TEXT NOT NULL CHECK(namespace IN ('public','protected')),
            path TEXT NOT NULL, content BLOB NOT NULL,
            immutable INTEGER NOT NULL CHECK(immutable IN (0,1)),
            PRIMARY KEY(namespace,path)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_journal_cursor (
            singleton INTEGER PRIMARY KEY CHECK(singleton=1),
            identity TEXT NOT NULL, ordinal INTEGER NOT NULL CHECK(ordinal>=0),
            digest TEXT NOT NULL, origin_incarnation TEXT NOT NULL,
            origin_high_water INTEGER NOT NULL CHECK(origin_high_water>=0)
         ) STRICT;",
    )?;
    for table in [
        "selected_events",
        "selected_receipts",
        "selected_directory",
        "selected_adoptions",
    ] {
        let key=match table {
            "selected_events"=>"organization_id=NEW.organization_id AND task_id=NEW.task_id AND revision=NEW.revision",
            "selected_receipts"=>"organization_id=NEW.organization_id AND operation_id=NEW.operation_id",
            "selected_directory"=>"organization_id=NEW.organization_id AND generation=NEW.generation",
            _=>"organization_id=NEW.organization_id AND task_id=NEW.task_id",
        };
        tx.execute_batch(&format!(
            "CREATE TRIGGER {table}_immutable_insert BEFORE INSERT ON {table}
             WHEN EXISTS(SELECT 1 FROM {table} WHERE {key})
             BEGIN SELECT RAISE(ABORT,'selected retained identity already exists'); END;
             CREATE TRIGGER {table}_immutable_update BEFORE UPDATE ON {table}
             BEGIN SELECT RAISE(ABORT,'selected retained source is immutable'); END;
             CREATE TRIGGER {table}_immutable_delete BEFORE DELETE ON {table}
             BEGIN SELECT RAISE(ABORT,'selected retained source cannot be deleted'); END;"
        ))?;
    }
    tx.execute_batch("CREATE TRIGGER selected_gate_child_replace BEFORE INSERT ON work_ids
        WHEN EXISTS(SELECT 1 FROM work_ids i JOIN selected_adoptions a ON a.task_id=i.item_id WHERE i.id=NEW.id)
        BEGIN SELECT RAISE(ABORT,'selected task requires selected contract'); END;")?;
    tx.execute_batch(
        "CREATE TRIGGER selected_receipt_legacy_collision BEFORE INSERT ON selected_receipts
        WHEN EXISTS(SELECT 1 FROM work_change_results WHERE change_id=NEW.operation_id)
        BEGIN SELECT RAISE(ABORT,'work operation identity already exists'); END;
        CREATE TRIGGER legacy_receipt_selected_collision BEFORE INSERT ON work_change_results
        WHEN EXISTS(SELECT 1 FROM selected_receipts WHERE operation_id=NEW.change_id)
        BEGIN SELECT RAISE(ABORT,'work operation identity already exists'); END;",
    )?;
    // These are permanent format gates, including raw replication/Undo writes.
    // Selected state is separate; no selected update mutates legacy StoredItem.
    for (table, id) in [
        ("work_items", "id"),
        ("work_events", "target_id"),
        ("work_ids", "item_id"),
    ] {
        for (operation, row) in [("INSERT", "NEW"), ("UPDATE", "OLD"), ("DELETE", "OLD")] {
            let condition = if operation == "UPDATE" {
                format!("task_id=OLD.{id} OR task_id=NEW.{id}")
            } else {
                format!("task_id={row}.{id}")
            };
            tx.execute_batch(&format!(
                "CREATE TRIGGER selected_gate_{table}_{operation} BEFORE {operation} ON {table}
                 WHEN EXISTS(SELECT 1 FROM selected_adoptions WHERE {condition})
                 BEGIN SELECT RAISE(ABORT,'selected task requires selected contract'); END;"
            ))?;
        }
    }
    Ok(())
}

#[test]
fn frozen_schema7_upgrades_to8_preserving_instance_origin_status_and_directory() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("office.db");
    let key = Key::from_bytes([29; 32]);
    let schema = Schema {
        version: 7,
        admit: None,
        create: |tx| {
            tx.execute_batch(FROZEN_BASE_V6)?;
            frozen_selected_v7(tx)
        },
        upgrade: |_, _| unreachable!(),
    };
    let old = Store::open(&path, &key, Opening::New, &schema).unwrap();
    old.write(|db| {
        assert_eq!(db.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0)).unwrap(),7);
        assert_eq!(db.query_row("SELECT count(*) FROM sqlite_master WHERE name='local_hq_creation'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        db.execute_batch("INSERT INTO meta VALUES('office_id','HQ'),('instance_id','01ARZ3NDEKTSV4RRFFQ69G5FAV'),('incarnation','original');
          INSERT INTO work_origin_counter VALUES('original',11);
          INSERT INTO member_status VALUES('task-only',NULL,'TASK','now',NULL),('status-only','working',NULL,'now','later'),('both','busy','TASK','now','later');
          INSERT INTO selected_directory VALUES('ORG',1,'{}','retained');
          INSERT INTO selected_directory_head VALUES('ORG',1);") .map_err(OfficeError::storage)?;
        assert!(db.execute("INSERT INTO member_status VALUES('bad',NULL,NULL,'now',NULL)",[]).is_err());
        Ok(((),None))
    }).unwrap();
    drop(old);
    for _ in 0..2 {
        let current = Store::open_bound(
            &path,
            &key,
            Opening::Existing,
            &OFFICE_SCHEMA,
            Some(INSTANCE),
        )
        .unwrap();
        current.read(|db| {
            assert_eq!(db.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0)).unwrap(),8);
            assert_eq!(db.query_row("SELECT count(*) FROM local_hq_creation",[],|r|r.get::<_,i64>(0)).unwrap(),0);
            assert_eq!(db.query_row("SELECT count(*) FROM member_status",[],|r|r.get::<_,i64>(0)).unwrap(),3);
            assert_eq!(db.query_row("SELECT value FROM meta WHERE key='instance_id'",[],|r|r.get::<_,String>(0)).unwrap(),INSTANCE);
            assert_eq!(db.query_row("SELECT value FROM meta WHERE key='incarnation'",[],|r|r.get::<_,String>(0)).unwrap(),"original");
            assert_eq!(db.query_row("SELECT last_seq FROM work_origin_counter WHERE origin_incarnation='original'",[],|r|r.get::<_,i64>(0)).unwrap(),11);
            assert_eq!(db.query_row("SELECT payload_sha256 FROM selected_directory WHERE organization_id='ORG'",[],|r|r.get::<_,String>(0)).unwrap(),"retained");
            assert_eq!(db.query_row("SELECT current_task_id FROM member_status WHERE member_id='task-only'",[],|r|r.get::<_,String>(0)).unwrap(),"TASK");
            Ok(())
        }).unwrap();
        drop(current);
    }
}
