//! HQ creation through the worker's composition: the office on its store, the host's
//! writer custody through the worker's own adapter (`test_support::FakeWriter`), and the CA
//! made, sealed and wrapped here.
use super::*;
use crate::test_support::{test_admission, FakeWriter};
use org_office::organization_family::operation_id::from_wire_ulid;
use std::path::PathBuf;

pub(crate) const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607182";
pub(crate) const ORG: &str = "019a8928-1337-7123-8123-0123456789ab";
pub(crate) const OFFICE: &str = "019a8928-1337-7456-8456-0123456789ac";
pub(crate) const OWNER: &str = "01900000-0000-7000-8000-0000000000ee";
const OPERATION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

/// One HQ's places: its folder, its data roots and the host's custody of its writer.
pub(crate) struct World {
    _dir: tempfile::TempDir,
    pub folder: PathBuf,
    pub roots: org_office::DataRoots,
    pub writer: FakeWriter,
}

impl World {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let folder = root.join("home/offices").join(OFFICE);
        let data = root.join("data").join(ORG);
        let roots = org_office::DataRoots {
            durable: data.join("durable"),
            runtime: data.join("runtime"),
        };
        for made in [&roots.durable, &roots.runtime] {
            std::fs::create_dir_all(made).unwrap();
        }
        let admission =
            test_admission(INSTANCE, ORG, OFFICE, OWNER, &roots.durable, &roots.runtime);
        Self {
            _dir: dir,
            folder,
            roots,
            writer: FakeWriter::new(admission),
        }
    }

    pub(crate) fn prepare(
        &self,
        key: Option<Key>,
        code: RecoveryCode,
    ) -> Result<Office, OfficeError> {
        let (custody, roots) = self.writer.admitted();
        prepare(&self.folder, &new(), key, &binding(), roots, custody, code)
    }

    pub(crate) fn open(&self, key: Option<Key>) -> Result<Office, OfficeError> {
        let (custody, roots) = self.writer.admitted();
        Office::open(&self.folder, key, INSTANCE, roots, custody)
    }

    /// Every file under the office's folder and its data roots.
    fn files(&self) -> Vec<PathBuf> {
        fn walk(path: &std::path::Path, found: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, found);
                } else {
                    found.push(path);
                }
            }
        }
        let mut found = Vec::new();
        for root in [&self.folder, &self.roots.durable, &self.roots.runtime] {
            walk(root, &mut found);
        }
        found
    }
}

pub(crate) fn binding() -> Binding {
    Binding {
        operation_id: OPERATION.into(),
        owner_member_id: OWNER.into(),
        bootstrap: org_proto::selected_bootstrap::PreparedV1 {
            plan: org_proto::selected_bootstrap::PlanV1 {
                version: 1,
                name: "Example".into(),
                slug: "example".into(),
                repository_remote_url: "https://example.test/org.git".into(),
                state_branch: "org/state".into(),
            },
            organization_id: ORG.into(),
            office_id: OFFICE.into(),
            repository_id: "019a8928-1337-7456-8456-0123456789ad".into(),
            home_team_id: "019a8928-1337-7456-8456-0123456789ae".into(),
            hr_employee_id: "019a8928-1337-7456-8456-0123456789af".into(),
            creator: org_proto::selected_bootstrap::CreatorV1::from_login(OWNER.into(), "Ana"),
        },
    }
}
pub(crate) fn new() -> NewOffice {
    NewOffice {
        office_id: OFFICE.into(),
        office_name: "HQ".into(),
        role: org_office::OfficeRole::Hq,
        org_id: ORG.into(),
        org_name: "Example".into(),
    }
}
pub(crate) fn key() -> Key {
    Key::from_office_secrets_key(org_proto::OfficeSecretsKey::from_bytes([8; 32]))
}
pub(crate) fn code() -> RecoveryCode {
    RecoveryCode::new("0001-0203-0405-0607-0809-0A0B-0C0D-0E0F-1011-1213".into()).unwrap()
}

#[test]
fn actual_ca_creation_reopens_retries_exactly_and_starts_without_pending_code() {
    let world = World::new();
    let office = world.prepare(Some(key()), code()).unwrap();
    let record = office.hq_creation_record().unwrap().unwrap();
    let package = record.package.clone();
    let public = record.public_key;
    // The CA records name the store operation the wire's ULID maps to (M7).
    let mapped = from_wire_ulid(OPERATION).unwrap().to_string();
    assert_eq!(record.operation_id, mapped);
    assert!(org_proto::is_uuid_v7(&record.operation_id));
    let copied = office.hq_creation_public_record().unwrap().unwrap();
    assert_eq!(copied.package, package);
    assert_eq!(copied.public_key, public);
    assert_eq!(copied.operation_id, mapped);
    assert_eq!(
        std::fs::read(world.roots.durable.join("state/security/ca-escrow.v1.bin")).unwrap(),
        package
    );
    let restored = org_pki::recover(
        &package,
        ORG,
        &org_pki::Code::parse(code().expose()).unwrap(),
    )
    .unwrap();
    assert_eq!(public, restored.public_key());
    assert!(office
        .org_snapshot()
        .unwrap()
        .members
        .iter()
        .any(|member| member.id == OWNER && member.owner));
    assert_eq!(verify_existing(&office).unwrap(), CaStatus::Available);
    drop(office);
    // The same request again: the same CA, never a second.
    let office = world.prepare(Some(key()), code()).unwrap();
    assert_eq!(
        office.hq_creation_record().unwrap().unwrap().package,
        package
    );
    drop(office);
    let wrong =
        RecoveryCode::new("FFFF-FFFF-FFFF-FFFF-FFFF-FFFF-FFFF-FFFF-FFFF-FFFF".into()).unwrap();
    assert!(world.prepare(Some(key()), wrong).is_err());
    let office = world.open(Some(key())).unwrap();
    assert_eq!(verify_existing(&office).unwrap(), CaStatus::Available);
    let reopened = office.hq_creation_record().unwrap().unwrap();
    assert_eq!((reopened.public_key, reopened.package), (public, package));
    // Neither the local key nor the code appears in the organization snapshot.
    let snapshot = serde_json::to_vec(&office.org_snapshot().unwrap()).unwrap();
    let der = unwrap_ca(&key(), &record, OFFICE)
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
fn the_selected_office_marker_and_first_generation_are_written() {
    let world = World::new();
    let office = world.prepare(Some(key()), code()).unwrap();
    let marker = std::fs::read(world.folder.join("office.json")).unwrap();
    let marker = org_office_launch::OfficeFile::from_slice(&marker).unwrap();
    assert_eq!(
        marker.version(),
        org_office_launch::SELECTED_OFFICE_FILE_VERSION
    );
    assert_eq!(
        (marker.office_id.as_str(), marker.org_id.as_str()),
        (OFFICE, ORG)
    );
    assert_eq!(office.org_snapshot().unwrap().position.seq, 1);
    assert!(office.is_hq().unwrap());
    // The host holds the head the creation confirmed.
    assert!(world.writer.head().is_some_and(|head| head.ordinal >= 1));
}

/// ENC-5 (library level): the stored CA key is only a wrapped record. It holds no
/// PKCS#8 bytes, unwraps with the office secrets key to the recorded public key, and
/// opens for no other key, office or organization. No file the office keeps, in its
/// folder or its data roots, holds the plaintext DER, open or closed.
#[test]
fn enc5_ca_key_is_stored_only_wrapped_with_the_office_secrets_key() {
    let world = World::new();
    let office = world.prepare(Some(key()), code()).unwrap();
    let record = office.hq_creation_record().unwrap().unwrap();
    let wrapped = record.wrapped_ca_key();
    assert!(org_pki::SigningKey::from_local_der(wrapped).is_err());
    let ca = unwrap_ca(&key(), &record, OFFICE).unwrap();
    assert_eq!(ca.public_key(), record.public_key);
    let der = ca.local_der().to_vec();
    assert!(!wrapped.windows(der.len()).any(|w| w == der.as_slice()));
    let scan = |phase: &str| {
        let files = world.files();
        assert!(
            files.iter().any(|path| path.ends_with("office.db")),
            "{phase}"
        );
        for path in &files {
            let bytes = std::fs::read(path).unwrap();
            assert!(
                !bytes.windows(der.len()).any(|w| w == der.as_slice()),
                "{phase} {path:?}"
            );
        }
    };
    scan("open");
    office.close().unwrap();
    scan("closed");
    let other = Key::from_office_secrets_key(org_proto::OfficeSecretsKey::from_bytes([9; 32]));
    assert!(unwrap_ca(&other, &record, OFFICE).is_err());
    assert!(unwrap_ca(&key(), &record, "019a8928-1337-7456-8456-0123456789bb").is_err());
    let mut moved = LocalRecord::new(
        "019a8928-1337-7123-8123-0123456789cc".into(),
        record.operation_id.clone(),
        record.public_key,
        record.package.clone(),
        wrapped.to_vec(),
    )
    .unwrap();
    assert!(unwrap_ca(&key(), &moved, OFFICE).is_err());
    moved.organization_id = record.organization_id.clone();
    assert!(unwrap_ca(&key(), &moved, OFFICE).is_ok());
}

/// ENC-1 (ADR-0016 decision 3): without the office secrets key an HQ create retry is
/// recovery-required, while an existing HQ office still opens and only its CA is unavailable.
#[test]
fn enc1_missing_secrets_key_makes_create_recovery_required_and_ca_unavailable() {
    let world = World::new();
    let refused = world
        .prepare(None, code())
        .expect_err("create without a secrets key must not provision");
    assert!(matches!(
        refused,
        OfficeError::Invalid("HQ provisioning requires recovery")
    ));
    let office = world.prepare(Some(key()), code()).unwrap();
    let record = office.hq_creation_record().unwrap().unwrap();
    drop(office);
    let retry = world
        .prepare(None, code())
        .expect_err("a create retry without a secrets key is recovery-required");
    assert!(matches!(
        retry,
        OfficeError::Invalid("HQ provisioning requires recovery")
    ));
    let office = world.open(None).unwrap();
    assert_eq!(office.info().office_id, OFFICE);
    assert_eq!(
        office.hq_creation_record().unwrap().unwrap().public_key,
        record.public_key
    );
    assert_eq!(verify_existing(&office).unwrap(), CaStatus::Unavailable);
    drop(office);
    let wrong = Key::from_office_secrets_key(org_proto::OfficeSecretsKey::from_bytes([9; 32]));
    let office = world.open(Some(wrong)).unwrap();
    assert_eq!(verify_existing(&office).unwrap(), CaStatus::Unavailable);
    drop(office);
    let office = world.open(Some(key())).unwrap();
    assert_eq!(verify_existing(&office).unwrap(), CaStatus::Available);
}
