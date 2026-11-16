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
