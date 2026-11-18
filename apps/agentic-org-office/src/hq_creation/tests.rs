use super::*;
use std::path::Path;
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
        role: org_office::OfficeRole::Hq,
        org_id: "ORG".into(),
        org_name: "Example".into(),
    }
}
fn key() -> Key {
    Key::from_db_key(org_proto::DbKey::from_bytes([8; 32]))
}
fn code() -> RecoveryCode {
    RecoveryCode::new("0001-0203-0405-0607-0809-0A0B-0C0D-0E0F-1011-1213".into()).unwrap()
}
#[test]
fn actual_ca_creation_reopens_retries_exactly_and_starts_without_pending_code() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("HQ");
    let office = prepare(
        &folder,
        &new(),
        Some(key()),
        INSTANCE,
        &binding(dir.path()),
        code(),
    )
    .unwrap();
    let record = office.hq_creation_record().unwrap().unwrap();
    let package = record.package.clone();
    let public = record.public_key;
    let copied = office.hq_creation_public_record().unwrap().unwrap();
    assert_eq!(copied.package, package);
    assert_eq!(copied.public_key, public);
    assert_eq!(
        std::fs::read(
            binding(dir.path())
                .public_root
                .join("security/ca-escrow.v1.bin")
        )
        .unwrap(),
        package
    );
    let restored = org_pki::recover(
        &package,
        "ORG",
        &org_pki::Code::parse(code().expose()).unwrap(),
    )
    .unwrap();
    assert_eq!(public, restored.public_key());
    assert_eq!(
        office.org_snapshot().unwrap().members[0].id,
        binding(dir.path()).owner_member_id
    );
    verify_existing(&office).unwrap();
    drop(office);
    let office = prepare(
        &folder,
        &new(),
        Some(key()),
        INSTANCE,
        &binding(dir.path()),
        code(),
    )
    .unwrap();
    assert_eq!(
        office.hq_creation_record().unwrap().unwrap().package,
        package
    );
    drop(office);
    let wrong =
        RecoveryCode::new("FFFF-FFFF-FFFF-FFFF-FFFF-FFFF-FFFF-FFFF-FFFF-FFFF".into()).unwrap();
    assert!(prepare(
        &folder,
        &new(),
        Some(key()),
        INSTANCE,
        &binding(dir.path()),
        wrong
    )
    .is_err());
    let office = Office::open_for_instance(&folder, key(), INSTANCE).unwrap();
    verify_existing(&office).unwrap();
    assert_eq!(
        office.hq_creation_record().unwrap().unwrap().public_key,
        public
    );
    assert_eq!(
        office.hq_creation_record().unwrap().unwrap().package,
        package
    );
    // Neither local key nor code appears in the copied org snapshot/export DTO.
    let snapshot = serde_json::to_vec(&office.org_snapshot().unwrap()).unwrap();
    let der = unwrap_ca(&key(), &record, "HQ")
        .unwrap()
        .local_der()
        .to_vec();
    for secret in [der.as_slice(), record.wrapped_ca_key()] {
        assert!(!snapshot.windows(secret.len()).any(|w| w == secret));
    }
    assert!(!snapshot
        .windows(code().expose().len())
        .any(|w| w == code().expose().as_bytes()));
}

#[test]
fn selected_uuid_hq_marker_keeps_ca_identity_through_retry_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let mut new = new();
    new.org_id = "019a8928-1337-7123-8123-0123456789ab".into();
    new.office_id = "019a8928-1337-7456-8456-0123456789ac".into();
    let folder = dir.path().join(&new.office_id);
    let mut binding = binding(dir.path());
    binding.bootstrap = Some(org_proto::selected_bootstrap::PreparedV1 {
        plan: org_proto::selected_bootstrap::PlanV1 {
            version: 1,
            name: new.org_name.clone(),
            slug: "example".into(),
            repository_remote_url: "https://example.test/org.git".into(),
            state_branch: "org/state".into(),
            home_team_name: "Core".into(),
        },
        organization_id: new.org_id.clone(),
        office_id: new.office_id.clone(),
        repository_id: "019a8928-1337-7456-8456-0123456789ad".into(),
        home_team_id: "019a8928-1337-7456-8456-0123456789ae".into(),
        hr_employee_id: "019a8928-1337-7456-8456-0123456789af".into(),
    });
    let office = prepare(&folder, &new, Some(key()), INSTANCE, &binding, code()).unwrap();
    let record = office.hq_creation_record().unwrap().unwrap();
    let marker = std::fs::read(folder.join("office.json")).unwrap();
    let marker = org_office_launch::OfficeFile::from_slice(&marker).unwrap();
    assert_eq!(
        marker.version(),
        org_office_launch::SELECTED_OFFICE_FILE_VERSION
    );
    assert_eq!(marker.office_id, new.office_id);
    assert_eq!(marker.org_id, new.org_id);
    drop(office);
    let office = prepare(&folder, &new, Some(key()), INSTANCE, &binding, code()).unwrap();
    assert_eq!(
        office.hq_creation_record().unwrap().unwrap().package,
        record.package
    );
    drop(office);
    let office = Office::open_for_instance(&folder, key(), INSTANCE).unwrap();
    verify_existing(&office).unwrap();
    let current = office.hq_creation_public_record().unwrap().unwrap();
    assert_eq!(current.public_key, record.public_key);
    assert_eq!(current.organization_id, new.org_id);
    let recovered = org_pki::recover(
        &current.package,
        &new.org_id,
        &org_pki::Code::parse(code().expose()).unwrap(),
    )
    .unwrap();
    assert_eq!(recovered.public_key(), record.public_key);
    assert_eq!(office.org_snapshot().unwrap().position.seq, 1);
}

#[test]
fn legacy_satellite_without_hq_receipt_starts_but_retained_hq_material_requires_authority() {
    for provisioned in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("HQ");
        let office = if provisioned {
            prepare(
                &folder,
                &new(),
                Some(key()),
                INSTANCE,
                &binding(dir.path()),
                code(),
            )
            .unwrap()
        } else {
            Office::create(&folder, &new(), key()).unwrap()
        };
        verify_existing(&office).unwrap();
        let mut source = new();
        source.office_id = "SOURCE".into();
        let source = Office::create(dir.path().join("SOURCE"), &source, key()).unwrap();
        source
            .bind_replication_hq(Default::default(), 1024 * 1024)
            .unwrap();
        let pin = source.replication_checkpoint().unwrap();
        office
            .bind_replication_satellite(
                pin.hq_office,
                pin.ca,
                pin.highest_epoch,
                Default::default(),
            )
            .unwrap();
        assert!(!office.is_replication_hq().unwrap());
        if provisioned {
            assert!(verify_existing(&office).is_err());
        } else {
            assert!(office.hq_creation_record().unwrap().is_none());
            verify_existing(&office).unwrap();
            drop(office);
            let office = Office::open(&folder, key()).unwrap();
            verify_existing(&office).unwrap();
        }
    }
}

/// ENC-5 (library level): the stored CA key is only a wrapped record. It holds no
/// PKCS#8 bytes, unwraps with the office secrets key to the recorded public key,
/// and opens for no other key, office or organization.
#[test]
fn enc5_ca_key_is_stored_only_wrapped_with_the_office_secrets_key() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("HQ");
    let office = prepare(
        &folder,
        &new(),
        Some(key()),
        INSTANCE,
        &binding(dir.path()),
        code(),
    )
    .unwrap();
    let record = office.hq_creation_record().unwrap().unwrap();
    let wrapped = record.wrapped_ca_key();
    assert!(org_pki::SigningKey::from_local_der(wrapped).is_err());
    let ca = unwrap_ca(&key(), &record, "HQ").unwrap();
    assert_eq!(ca.public_key(), record.public_key);
    let der = ca.local_der();
    assert!(!wrapped.windows(der.len()).any(|w| w == der));
    // Every byte the office stores locally, including `office.db-wal` while the office is
    // still open and after it closes: no plaintext DER anywhere.
    let scan = |phase: &str| {
        let mut scanned = 0;
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                let bytes = std::fs::read(&path).unwrap();
                assert!(
                    !bytes.windows(der.len()).any(|w| w == der),
                    "{phase} {path:?}"
                );
                scanned += 1;
            }
        }
        assert!(folder.join("office.db").is_file(), "{phase}");
        scanned
    };
    assert!(folder.join("office.db-wal").is_file());
    assert!(scan("open") > 0);
    drop(office);
    assert!(scan("closed") > 0);
    let other = Key::from_db_key(org_proto::DbKey::from_bytes([9; 32]));
    assert!(unwrap_ca(&other, &record, "HQ").is_err());
    assert!(unwrap_ca(&key(), &record, "SAT").is_err());
    let mut moved = LocalRecord::new(
        "OTHER".into(),
        record.operation_id.clone(),
        record.public_key,
        record.package.clone(),
        wrapped.to_vec(),
    )
    .unwrap();
    assert!(unwrap_ca(&key(), &moved, "HQ").is_err());
    moved.organization_id = record.organization_id.clone();
    assert!(unwrap_ca(&key(), &moved, "HQ").is_ok());
}

/// ENC-1 (ADR-0016 decision 3): without the office secrets key an HQ create retry is
/// recovery-required, while an existing HQ office still opens and only its CA is unavailable.
#[test]
fn enc1_missing_secrets_key_makes_create_recovery_required_and_ca_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("HQ");
    let refused = prepare(
        &folder,
        &new(),
        None,
        INSTANCE,
        &binding(dir.path()),
        code(),
    )
    .expect_err("create without a secrets key must not provision");
    assert!(matches!(
        refused,
        OfficeError::Invalid("HQ provisioning requires recovery")
    ));
    let office = prepare(
        &folder,
        &new(),
        Some(key()),
        INSTANCE,
        &binding(dir.path()),
        code(),
    )
    .unwrap();
    let record = office.hq_creation_record().unwrap().unwrap();
    drop(office);
    let retry = prepare(
        &folder,
        &new(),
        None,
        INSTANCE,
        &binding(dir.path()),
        code(),
    )
    .expect_err("a create retry without a secrets key is recovery-required");
    assert!(matches!(
        retry,
        OfficeError::Invalid("HQ provisioning requires recovery")
    ));
    let office = Office::open_for_instance(&folder, None, INSTANCE).unwrap();
    assert_eq!(office.info().office_id, "HQ");
    assert_eq!(
        office.hq_creation_record().unwrap().unwrap().public_key,
        record.public_key
    );
    assert_eq!(verify_existing(&office).unwrap(), CaStatus::Unavailable);
    drop(office);
    let wrong = Key::from_db_key(org_proto::DbKey::from_bytes([9; 32]));
    let office = Office::open_for_instance(&folder, wrong, INSTANCE).unwrap();
    assert_eq!(verify_existing(&office).unwrap(), CaStatus::Unavailable);
    drop(office);
    let office = Office::open_for_instance(&folder, key(), INSTANCE).unwrap();
    assert_eq!(verify_existing(&office).unwrap(), CaStatus::Available);
}
