//! Focused regressions for the B1-14a LOW findings.
//! Contract: X-05 federation draft, "Apply, acknowledgement and snapshot rules": "Stream into bounded
//! private staging, verify manifest/length/digest and schema"; Rowan's X-05 decisions
//! §3: "restore_pending" fences writes, and Round 3: transport key "is never either
//! office database key". Tomas's 2026-11-11 storage review LOW1/3/6 additionally
//! require retryable bootstrap availability, area parents, and post-commit wakeups.
use std::io::{self, Read};
use std::sync::Arc;

use org_proto::org::TeamCreateParams;
use org_sync::organization::{AuthorityRecord, AuthorityVerifier};
use tempfile::TempDir;

use super::tests::{key, new_office, Hq};
use super::*;
use crate::rules::OrgCommand;
use crate::{Key, NewOffice, Office, OfficeRole, SnapshotStageMode};

const LIMIT: u64 = 1024 * 1024;

#[test]
fn post_apply_area_relationship_requires_its_project_and_rolls_back() {
    let (_dir, hq, _satellite) = pair();
    let checkpoint = hq.replication_checkpoint().unwrap();
    let apply = |with_parent| {
        hq.org_store().sync_write(move |db| {
            if with_parent {
                let project = org_proto::org::Project::new("PROJECT", "Project");
                db.execute(
                    "INSERT INTO projects VALUES (?1, ?2)",
                    (&project.id, serde_json::to_string(&project).unwrap()),
                )
                .map_err(storage_error)?;
            }
            let area = org_proto::org::Area {
                id: "AREA".into(),
                project_id: "PROJECT".into(),
                name: "Area".into(),
                team_id: None,
                lead_member_id: None,
                general: false,
                deleted: false,
                replaced_by: None,
                version: 1,
            };
            db.execute(
                "INSERT INTO areas VALUES (?1, ?2, ?3)",
                (
                    &area.id,
                    &area.project_id,
                    serde_json::to_string(&area).unwrap(),
                ),
            )
            .map_err(storage_error)?;
            // Native v1 emits no areas yet. Exercise the real post-apply validator
            // directly inside the same SQL writer/savepoint used by the adapter.
            relationships::after_apply(
                db,
                &[OrgChange::AreaUpserted { area }],
                std::collections::HashSet::new(),
            )?;
            Ok(())
        })
    };
    assert!(matches!(
        apply(false),
        Err(SyncError::Refused(Refusal::IntegrityFailure))
    ));
    assert!(hq.org_snapshot().unwrap().areas.is_empty());
    assert_eq!(hq.replication_checkpoint().unwrap(), checkpoint);
    apply(true).unwrap();
    assert_eq!(hq.org_snapshot().unwrap().areas.len(), 1);
}

fn pair() -> (TempDir, Office, Office) {
    let dir = TempDir::new().unwrap();
    let hq = new_office(&dir, "HQ");
    hq.bind_replication_hq(Digest::ZERO, LIMIT).unwrap();
    let satellite = new_office(&dir, "SAT");
    satellite
        .bind_replication_satellite(
            OfficeId::new("HQ").unwrap(),
            Digest::ZERO,
            Epoch::new(1).unwrap(),
            Digest::ZERO,
        )
        .unwrap();
    (dir, hq, satellite)
}

fn stage(
    receiver: &Office,
    exported: &crate::OrganizationSnapshotExport,
    transport: &Key,
) -> Result<ValidatedSnapshot, SyncError> {
    receiver.stage_replication_snapshot(
        &Hq,
        &exported.manifest,
        transport,
        &*exported.ciphertext,
        LIMIT,
        SnapshotStageMode::Normal,
    )
}

#[test]
fn snapshot_checksum_guard_rejects_a_wrong_claim_over_valid_ciphertext() {
    let (_dir, hq, satellite) = pair();
    let transport = Key::from_bytes([8; 32]);
    let mut exported = hq.export_replication_snapshot(&transport, LIMIT).unwrap();
    let valid = exported.manifest.ciphertext_digest;
    exported.manifest.ciphertext_digest = Digest::from_bytes([7; 32]);
    assert!(matches!(
        stage(&satellite, &exported, &transport),
        Err(SyncError::Refused(Refusal::IntegrityFailure))
    ));
    exported.manifest.ciphertext_digest = valid;
    stage(&satellite, &exported, &transport).unwrap();
}

#[test]
fn snapshot_header_binding_is_checked_before_a_stage_token_is_returned() {
    let (_dir, hq, satellite) = pair();
    let transport = Key::from_bytes([8; 32]);
    let mut exported = hq.export_replication_snapshot(&transport, LIMIT).unwrap();
    let valid = exported.manifest.digest;
    exported.manifest.digest = Digest::from_bytes([7; 32]);
    assert!(matches!(
        stage(&satellite, &exported, &transport),
        Err(SyncError::Refused(Refusal::IntegrityFailure))
    ));
    exported.manifest.digest = valid;
    stage(&satellite, &exported, &transport).unwrap();
}

#[test]
fn receiving_office_key_cannot_be_used_as_the_snapshot_transport_key() {
    let dir = TempDir::new().unwrap();
    let hq = new_office(&dir, "HQ");
    hq.bind_replication_hq(Digest::ZERO, LIMIT).unwrap();
    let receiver_key = Key::from_bytes([4; 32]);
    let satellite = Office::create(
        dir.path().join("SAT"),
        &NewOffice {
            office_id: "SAT".into(),
            office_name: "Satellite".into(),
            role: OfficeRole::Hq,
            org_id: "ORG".into(),
            org_name: "Organization".into(),
        },
        receiver_key.clone(),
    )
    .unwrap();
    satellite
        .bind_replication_satellite(
            OfficeId::new("HQ").unwrap(),
            Digest::ZERO,
            Epoch::new(1).unwrap(),
            Digest::ZERO,
        )
        .unwrap();
    // The source has a different DB key, so its export is valid in isolation.
    let exported = hq
        .export_replication_snapshot(&receiver_key, LIMIT)
        .unwrap();
    assert!(matches!(
        stage(&satellite, &exported, &receiver_key),
        Err(SyncError::Refused(Refusal::InvalidInput))
    ));
    assert!(matches!(
        hq.export_replication_snapshot(&key(), LIMIT),
        Err(OfficeError::Invalid(_))
    ));
}

struct NoArtifactRead;
impl Read for NoArtifactRead {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("a stale snapshot was read before its cursor was refused")
    }
}

#[test]
fn stale_normal_snapshot_is_refused_before_any_artifact_input() {
    let (_dir, hq, satellite) = pair();
    let transport = Key::from_bytes([8; 32]);
    let old = hq.export_replication_snapshot(&transport, LIMIT).unwrap();
    hq.submit_org(OrgCommand::TeamCreate(TeamCreateParams {
        command_id: ulid::generate().unwrap(),
        team_id: "NEW".into(),
        name: "New".into(),
        owner_team_id: None,
    }))
    .unwrap()
    .unwrap();
    let fresh = hq.export_replication_snapshot(&transport, LIMIT).unwrap();
    organization::install_snapshot(&satellite, stage(&satellite, &fresh, &transport).unwrap())
        .unwrap();
    let before = satellite.replication_checkpoint().unwrap();
    assert!(matches!(
        satellite.stage_replication_snapshot(
            &Hq,
            &old.manifest,
            &transport,
            NoArtifactRead,
            LIMIT,
            SnapshotStageMode::Normal
        ),
        Err(SyncError::Refused(Refusal::Divergence))
    ));
    assert_eq!(satellite.replication_checkpoint().unwrap(), before);
}

#[test]
fn restore_pending_hq_cannot_export_even_a_well_formed_projection() {
    let (_dir, hq, _satellite) = pair();
    let transport = Key::from_bytes([8; 32]);
    hq.export_replication_snapshot(&transport, LIMIT).unwrap();
    hq.org_store()
        .run_sql_for_test("UPDATE org_replica SET restore_pending = 1");
    let before = hq.replication_checkpoint().unwrap();
    assert!(matches!(
        hq.export_replication_snapshot(&transport, LIMIT),
        Err(OfficeError::Invalid(
            "organization snapshot requires unfenced HQ authority"
        ))
    ));
    assert_eq!(hq.replication_checkpoint().unwrap(), before);
}

#[test]
fn initial_snapshot_is_not_yet_available_rather_than_a_missing_office() {
    let (_dir, _hq, satellite) = pair();
    let error = satellite.org_snapshot().unwrap_err();
    assert!(matches!(error, OfficeError::SnapshotPending));
    assert_eq!(
        error.to_string(),
        "organization snapshot is not available yet"
    );
}

struct VerifiedSignature;
impl AuthorityVerifier for VerifiedSignature {
    fn verify(&self, _: &AuthorityRecord) -> Result<(), Refusal> {
        Ok(())
    }
}

#[test]
fn same_base_authority_acceptance_wakes_native_followers_after_commit() {
    let (_dir, hq, _satellite) = pair();
    let before = hq.replication_checkpoint().unwrap();
    let token = hq.org_change_token();
    organization::accept_authority(
        &hq,
        AuthorityRecord {
            org_id: before.org_id,
            hq_office: before.hq_office,
            ca: before.ca,
            epoch: Epoch::new(2).unwrap(),
            restored_position: before.position,
            restored_digest: before.digest,
            signature: Arc::from([1]),
        },
        Arc::new(VerifiedSignature),
    )
    .unwrap();
    assert_eq!(hq.org_head().unwrap().epoch, 2);
    assert_ne!(hq.org_change_token(), token);
}

#[test]
fn authority_acceptance_rollback_cannot_wake_followers_or_advance_the_epoch() {
    let (_dir, hq, _satellite) = pair();
    let before = hq.replication_checkpoint().unwrap();
    hq.org_store().run_sql_for_test(
        "CREATE TABLE authority_parent (id INTEGER PRIMARY KEY);
         CREATE TABLE authority_child (id INTEGER REFERENCES authority_parent(id)
             DEFERRABLE INITIALLY DEFERRED);
         CREATE TRIGGER fail_authority_commit AFTER UPDATE ON org_replica
         BEGIN INSERT INTO authority_child VALUES (999); END;",
    );
    let token = hq.org_change_token();
    let record = AuthorityRecord {
        org_id: before.org_id.clone(),
        hq_office: before.hq_office.clone(),
        ca: before.ca,
        epoch: Epoch::new(2).unwrap(),
        restored_position: before.position,
        restored_digest: before.digest,
        signature: Arc::from([1]),
    };
    assert!(
        organization::accept_authority(&hq, record.clone(), Arc::new(VerifiedSignature)).is_err()
    );
    assert_eq!(hq.org_change_token(), token);
    assert_eq!(hq.replication_checkpoint().unwrap(), before);
    hq.org_store()
        .run_sql_for_test("DROP TRIGGER fail_authority_commit");
    organization::accept_authority(&hq, record, Arc::new(VerifiedSignature)).unwrap();
    assert_ne!(hq.org_change_token(), token);
}

#[test]
fn opening_the_office_sweeps_its_crash_left_snapshot_scratch() {
    let dir = TempDir::new().unwrap();
    let hq = new_office(&dir, "HQ");
    let stale = dir
        .path()
        .join("HQ/.snapshot-staging")
        .join(format!("snapshot-{}", ulid::generate().unwrap()));
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("snapshot.db"), b"encrypted crash fixture").unwrap();
    drop(hq);
    let _hq = Office::open(dir.path().join("HQ"), key()).unwrap();
    assert!(
        !stale.exists(),
        "crash-left encrypted snapshot was retained"
    );
}

#[test]
fn opening_without_snapshot_scratch_does_not_create_it() {
    let dir = TempDir::new().unwrap();
    let hq = new_office(&dir, "HQ");
    drop(hq);
    let _hq = Office::open(dir.path().join("HQ"), key()).unwrap();
    assert!(!dir.path().join("HQ/.snapshot-staging").exists());
}

#[test]
fn exporting_uses_and_cleans_the_office_private_snapshot_folder() {
    let (dir, hq, _satellite) = pair();
    hq.export_replication_snapshot(&Key::from_bytes([8; 32]), LIMIT)
        .unwrap();
    let root = dir.path().join("HQ/.snapshot-staging");
    assert!(root.is_dir());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(root.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn snapshot_root_symlink_is_refused_without_changing_its_target() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (dir, hq, _satellite) = pair();
    let outside = dir.path().join("OUTSIDE");
    std::fs::create_dir(&outside).unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o755)).unwrap();
    let canary = outside.join("preserve");
    std::fs::write(&canary, b"not snapshot state").unwrap();
    symlink(&outside, dir.path().join("HQ/.snapshot-staging")).unwrap();
    assert!(hq
        .export_replication_snapshot(&Key::from_bytes([8; 32]), LIMIT)
        .is_err());
    assert_eq!(std::fs::read(&canary).unwrap(), b"not snapshot state");
    assert_eq!(
        outside.metadata().unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 1);
}

#[test]
fn snapshot_sweep_removes_its_stale_link_without_following_it() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new().unwrap();
    let hq = new_office(&dir, "HQ");
    let root = dir.path().join("HQ/.snapshot-staging");
    std::fs::create_dir(&root).unwrap();
    let outside = dir.path().join("OUTSIDE");
    std::fs::create_dir(&outside).unwrap();
    let canary = outside.join("preserve");
    std::fs::write(&canary, b"keep").unwrap();
    let link = root.join(format!("snapshot-{}", ulid::generate().unwrap()));
    symlink(&outside, &link).unwrap();
    drop(hq);
    let _hq = Office::open(dir.path().join("HQ"), key()).unwrap();
    assert!(std::fs::symlink_metadata(link).is_err());
    assert_eq!(std::fs::read(canary).unwrap(), b"keep");
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn snapshot_sweep_refuses_unknown_entries_without_deleting_them() {
    let dir = TempDir::new().unwrap();
    let hq = new_office(&dir, "HQ");
    let root = dir.path().join("HQ/.snapshot-staging");
    std::fs::create_dir(&root).unwrap();
    drop(hq);
    for name in [
        "not-owned-by-the-snapshot-sweeper",
        "snapshot-invalid",
        "snapshot-8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
    ] {
        let canary = root.join(name);
        std::fs::write(&canary, b"keep").unwrap();
        assert!(matches!(
            Office::open(dir.path().join("HQ"), key()),
            Err(OfficeError::Invalid("snapshot scratch cleanup failed"))
        ));
        assert_eq!(std::fs::read(&canary).unwrap(), b"keep");
        std::fs::remove_file(canary).unwrap();
    }
}
