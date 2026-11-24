//! End-to-end office-level tests for `TASK-52` Slice 2 (`ARCH-02`, `ARCH-03`, `doc-67`, D25):
//! - HQ invitation creation, TLS-exporter proof check, 3-wrong-codes cancellation
//! - Approval -> satellite `ClientAuth`-only cert issuance -> HQ office registration
//! - Satellite provisioning, fail-closed private notes (`ARCH-02`), local offline execution
//!   storage (`ARCH-02`), replicated `state/` snapshot application, and D25 re-enrollment
//! - Certificate revocation via `remove_satellite_office`
//! - `org-store` durable journal consumer watermark retention across reopen (`ARCH-03`)

use std::sync::Arc;

use org_pki::{
    cert::{
        build_org_ca_cert, issue_satellite_cert_from_pubkey, sign_satellite_key_pop,
        IssueSatelliteCertParams,
    },
    invitation::{sign_exporter_proof, InvitationToken},
    SigningKey, Zeroizing,
};
use org_proto::ErrorCode;
use org_store::{testkit::FakeCustody, Id, OfficeIdentity, WriterAuthority};

use crate::{
    model::{NewOffice, OfficeRole},
    office::{
        CreateInvitationParams, Custody, DataRoots, InvitationPollStatus, InvitationStatus,
        JoinRequestParams, Office, Options, SatelliteProvisionSpec, SatelliteReEnrollSpec,
        TestOffice,
    },
    store::open::EngineChoice,
    Key,
};

fn build_satellite_custody(org_id: &str, sat_office_id: &str) -> Custody {
    Custody {
        identity: OfficeIdentity {
            instance_id: Id::parse("0195a000-0000-7000-8000-000000000050").unwrap(),
            organization_id: Id::parse(org_id).unwrap(),
            office_id: Id::parse(sat_office_id).unwrap(),
            journal_incarnation: Id::parse("0195a000-0000-7000-8000-000000000051").unwrap(),
            writer_epoch: 1,
            machine_id: Id::parse("0195a000-0000-7000-8000-000000000052").unwrap(),
        },
        authority: Arc::new(FakeCustody::new()) as Arc<dyn WriterAuthority>,
    }
}

#[test]
fn hq_invitation_three_wrong_codes_cancels_and_approve_provisions_satellite_and_d25_reenrolls() {
    let hq = Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    });
    let owner = hq.owner_member_id().to_owned();
    let org_id = hq.info().org_id.clone();

    // 1. Register durable replication consumer watermark on HQ (`ARCH-03`).
    let wm0 = hq.register_replication_consumer("sat-replication").unwrap();

    // 2. Invitation #1: verify 3 wrong check codes cancel the invitation.
    verify_three_wrong_check_codes_cancel(&hq, &owner);

    // 3. Invitation #2: happy path approval -> register satellite -> provision satellite -> sync state.
    let created2 = hq
        .create_satellite_invitation(
            &owner,
            CreateInvitationParams {
                office_name: "Lisbon Studio".into(),
                hq_addresses: vec!["127.0.0.1:9443".into()],
                ttl_secs: Some(600),
                existing_office_id: None,
            },
        )
        .unwrap()
        .unwrap();
    let tok2 = InvitationToken::decode(&created2.token).unwrap();
    let inv2_key = tok2.signing_key().unwrap();
    let sat2_tls = SigningKey::generate().unwrap();
    let exporter2 = [0xBBu8; 32];
    let pop2 = sign_satellite_key_pop(
        &sat2_tls,
        &tok2.org_id,
        &tok2.office_id,
        &tok2.invitation_id,
    )
    .unwrap();
    let proof2 = sign_exporter_proof(
        &inv2_key,
        &exporter2,
        &tok2.invitation_id,
        &tok2.office_id,
        &sat2_tls.public_key(),
    )
    .unwrap();
    let accepted2 = hq
        .register_satellite_join_request(JoinRequestParams {
            invitation_id: tok2.invitation_id.clone(),
            request_id: "req-2".into(),
            office_id: tok2.office_id.clone(),
            joining_pub_key: sat2_tls.public_key(),
            pop_signature_der: pop2.clone(),
            exporter_binding: exporter2,
            exporter_proof_der: proof2,
        })
        .unwrap()
        .unwrap();
    let check_code2 = match accepted2 {
        crate::office::JoinRequestAccepted::Pending { check_code, .. } => check_code,
        other => panic!("expected Pending, got {other:?}"),
    };
    // Conflicting joining key while pending is refused (F9).
    let rogue_tls = SigningKey::generate().unwrap();
    let rogue_proof = sign_exporter_proof(
        &inv2_key,
        &exporter2,
        &tok2.invitation_id,
        &tok2.office_id,
        &rogue_tls.public_key(),
    )
    .unwrap();
    let conflict = hq
        .register_satellite_join_request(JoinRequestParams {
            invitation_id: tok2.invitation_id.clone(),
            request_id: "req-rogue".into(),
            office_id: tok2.office_id.clone(),
            joining_pub_key: rogue_tls.public_key(),
            pop_signature_der: pop2.clone(),
            exporter_binding: exporter2,
            exporter_proof_der: rogue_proof,
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(conflict.code, ErrorCode::Conflict);

    // Approve using lowercase check code without hyphen to verify Crockford normalization.
    let approved = hq
        .approve_satellite_invitation(
            &owner,
            &tok2.invitation_id,
            &check_code2.replace('-', "").to_ascii_lowercase(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(approved.invitation.status, InvitationStatus::Approved);
    // Reconnecting after TLS drop with the same (request_id, joining_pub_key) + fresh exporter
    // replays the issued certificate idempotently (F9).
    let fresh_exporter = [0xCCu8; 32];
    let fresh_proof = sign_exporter_proof(
        &inv2_key,
        &fresh_exporter,
        &tok2.invitation_id,
        &tok2.office_id,
        &sat2_tls.public_key(),
    )
    .unwrap();
    let replay_approved = hq
        .register_satellite_join_request(JoinRequestParams {
            invitation_id: tok2.invitation_id.clone(),
            request_id: "req-2".into(),
            office_id: tok2.office_id.clone(),
            joining_pub_key: sat2_tls.public_key(),
            pop_signature_der: pop2,
            exporter_binding: fresh_exporter,
            exporter_proof_der: fresh_proof,
        })
        .unwrap()
        .unwrap();
    assert!(matches!(
        replay_approved,
        crate::office::JoinRequestAccepted::AlreadyApproved { ref cert_der, .. }
            if *cert_der == approved.cert_der
    ));
    assert_eq!(approved.registered.office_id.as_str(), tok2.office_id);
    assert_ne!(approved.registered.office_code, "A");
    // Verify ARCH-03: registered consumer retained the new journal entry even after sync().
    let retained = hq.journal_entries_after(wm0);
    assert!(
        !retained.is_empty(),
        "registered consumer must hold back journal segment release after sync"
    );
    let last_ord = retained.last().unwrap().ordinal();
    hq.advance_replication_consumer("sat-replication", last_ord)
        .unwrap();
    verify_satellite_provision_isolation_reopen_d25_and_revocation(
        &hq, &owner, &org_id, &tok2, &approved, &sat2_tls,
    );
}

fn verify_three_wrong_check_codes_cancel(hq: &Office, owner: &str) {
    let created1 = hq
        .create_satellite_invitation(
            owner,
            CreateInvitationParams {
                office_name: "Bad Code Office".into(),
                hq_addresses: vec!["127.0.0.1:9443".into()],
                ttl_secs: Some(600),
                existing_office_id: None,
            },
        )
        .unwrap()
        .unwrap();
    let tok1 = InvitationToken::decode(&created1.token).unwrap();
    let inv1_key = tok1.signing_key().unwrap();
    let sat1_tls = SigningKey::generate().unwrap();
    let exporter1 = [0xAAu8; 32];
    let pop1 = sign_satellite_key_pop(
        &sat1_tls,
        &tok1.org_id,
        &tok1.office_id,
        &tok1.invitation_id,
    )
    .unwrap();
    let proof1 = sign_exporter_proof(
        &inv1_key,
        &exporter1,
        &tok1.invitation_id,
        &tok1.office_id,
        &sat1_tls.public_key(),
    )
    .unwrap();
    hq.register_satellite_join_request(JoinRequestParams {
        invitation_id: tok1.invitation_id.clone(),
        request_id: "req-1".into(),
        office_id: tok1.office_id.clone(),
        joining_pub_key: sat1_tls.public_key(),
        pop_signature_der: pop1,
        exporter_binding: exporter1,
        exporter_proof_der: proof1,
    })
    .unwrap()
    .unwrap();
    for attempt in 1..=3 {
        let err = hq
            .approve_satellite_invitation(owner, &tok1.invitation_id, "0000-0000")
            .unwrap()
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest, "attempt {attempt}");
    }
    assert!(matches!(
        hq.poll_satellite_invitation(&tok1.invitation_id).unwrap(),
        InvitationPollStatus::Terminal { code, .. } if code == "cancelled"
    ));
}

fn verify_satellite_provision_isolation_reopen_d25_and_revocation(
    hq: &Office,
    owner: &str,
    org_id: &str,
    tok2: &InvitationToken,
    approved: &crate::office::ApprovedSatelliteCert,
    sat2_tls: &SigningKey,
) {
    // 4. Provision the satellite on its own temporary roots (`ARCH-02`).
    let sat_dir = tempfile::tempdir().unwrap();
    let sat_root = sat_dir.path().canonicalize().unwrap();
    let sat_folder = sat_root.join("home/offices").join(&tok2.office_id);
    let sat_roots = DataRoots {
        durable: sat_root.join("data").join(org_id).join("durable"),
        runtime: sat_root.join("data").join(org_id).join("runtime"),
    };
    std::fs::create_dir_all(&sat_roots.durable).unwrap();
    std::fs::create_dir_all(&sat_roots.runtime).unwrap();
    let sat_custody = build_satellite_custody(org_id, &tok2.office_id);
    let sat_key = Key::from_bytes([0x22; 32]);
    let sat_opts = Options {
        engine: EngineChoice::Sqlite,
        clock: Arc::new(org_store::SystemClock),
        testing: Default::default(),
        before_final_checks: None,
    };
    let sat = Office::provision_satellite_with(
        sat_folder.clone(),
        SatelliteProvisionSpec {
            new: NewOffice {
                office_id: tok2.office_id.clone(),
                office_name: "Lisbon Studio".into(),
                role: OfficeRole::Satellite,
                org_id: org_id.to_owned(),
                org_name: hq.info().org_name.clone(),
            },
            operation_id: "0195a000-0000-7000-8000-000000000060".into(),
            hq_office_id: hq.info().office_id.clone(),
            hq_addresses: tok2.hq_addresses.clone(),
            epoch: approved.epoch,
            ca_cert_der: approved.ca_cert_der.clone(),
            office_cert_der: approved.cert_der.clone(),
            office_key_der: Zeroizing::new(sat2_tls.local_der().to_vec()),
        },
        sat_key.clone(),
        sat_roots.clone(),
        sat_custody.clone(),
        sat_opts.clone(),
        true,
    )
    .unwrap();

    // Verify ARCH-02: satellite MUST NOT create `<durable>/private/task-notes.db` and private notes fail closed.
    assert!(!sat_roots.durable.join("private/task-notes.db").exists());
    assert!(!sat.is_hq().unwrap());
    let loaded_tls = sat.load_satellite_tls_identity().unwrap();
    assert_eq!(loaded_tls.federation.hq_office_id, hq.info().office_id);

    // 5. Replicate HQ's authoritative `state/` snapshot onto the satellite and verify projection.
    let hq_files = hq.collect_authoritative_state_snapshot().unwrap();
    assert!(!hq_files.is_empty());
    let batch: Vec<(String, Option<Vec<u8>>)> =
        hq_files.into_iter().map(|(p, b)| (p, Some(b))).collect();
    sat.apply_replicated_state_files("0195a000-0000-7000-8000-000000000061", batch)
        .unwrap();
    let sat_snap = sat.org_snapshot().unwrap();
    assert!(!sat_snap.teams.is_empty());
    assert!(!sat_snap.members.is_empty());
    assert_eq!(
        sat.store.read(crate::store::open::OfficesCount).unwrap().0,
        2
    );

    // 6. Close and reopen the satellite through `Office::open_with` (`ARCH-02`).
    sat.close().unwrap();
    let sat_instance = sat_custody.identity.instance_id.to_string();
    let sat_reopened = Office::open_with(
        sat_folder,
        Some(sat_key),
        &sat_instance,
        sat_roots,
        sat_custody,
        sat_opts,
    )
    .unwrap();
    assert_eq!(sat_reopened.info().role, OfficeRole::Satellite);
    assert_eq!(
        sat_reopened
            .store
            .read(crate::store::open::OfficesCount)
            .unwrap()
            .0,
        2
    );
    verify_d25_reenroll_and_revocation(hq, owner, org_id, tok2, approved, &sat_reopened);
}

fn verify_d25_reenroll_and_revocation(
    hq: &Office,
    owner: &str,
    org_id: &str,
    tok2: &InvitationToken,
    approved: &crate::office::ApprovedSatelliteCert,
    sat_reopened: &Office,
) {
    // 7. D25: Re-enroll satellite onto a replacement HQ (fresh Org CA + new HQ office ID).
    let replacement_ca_key = SigningKey::generate().unwrap();
    let now_unix = (org_store::Clock::now(&org_store::SystemClock).0 / 1000) as i64;
    let replacement_ca = build_org_ca_cert(&replacement_ca_key, org_id, now_unix).unwrap();
    let new_sat_key = SigningKey::generate().unwrap();
    let new_inv_id = "0195a000-0000-7000-8000-000000000077";
    let new_pop =
        sign_satellite_key_pop(&new_sat_key, org_id, &tok2.office_id, new_inv_id).unwrap();
    let new_cert = issue_satellite_cert_from_pubkey(
        &replacement_ca_key,
        &replacement_ca.cert_der,
        &IssueSatelliteCertParams {
            org_id,
            office_id: &tok2.office_id,
            invitation_id: new_inv_id,
            subject_pub_key_uncompressed: &new_sat_key.public_key(),
            pop_signature_der: &new_pop,
            now_unix,
            validity_days: 365,
        },
    )
    .unwrap();
    let new_hq_id = "0195a000-0000-7000-8000-000000000088";
    sat_reopened
        .re_enroll_satellite(SatelliteReEnrollSpec {
            org_id: org_id.to_owned(),
            new_hq_office_id: new_hq_id.into(),
            new_hq_addresses: vec!["127.0.0.1:9444".into()],
            new_epoch: 1,
            new_ca_cert_der: replacement_ca.cert_der,
            new_office_cert_der: new_cert.cert_der,
            new_office_key_der: Zeroizing::new(new_sat_key.local_der().to_vec()),
        })
        .unwrap();
    let status = sat_reopened.sync_status().unwrap();
    assert_eq!(status.hq_office, new_hq_id);
    assert!(status.initial_snapshot_required);
    assert_eq!(status.seq, 0);

    // 8. HQ removal revokes the satellite's original certificate serial immediately.
    let removed = hq
        .remove_satellite_office(owner, &tok2.office_id)
        .unwrap()
        .unwrap();
    assert_eq!(removed.revoked_serials, vec![approved.serial_hex.clone()]);
    assert!(hq.is_cert_serial_revoked(&approved.serial_hex).unwrap());
}

#[test]
fn removal_drains_in_flight_peer_activity_before_publishing_revocation_and_stops_sessions_on_inv_save_error(
) {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };
    let hq = Arc::new(Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = hq.owner_member_id().to_owned();
    let sat_a = "0195a000-0000-7000-8000-0000000000a1";
    let sat_b = "0195a000-0000-7000-8000-0000000000b2";

    // Part 1 (P20-#2): An admitted `run_if_peer_active` closure finishes BEFORE
    // `remove_satellite_office` writes `revocations.v1.json`.
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let hq_op = Arc::clone(&hq);
    let op_thread = std::thread::spawn(move || {
        crate::office::run_if_peer_active(&hq_op, sat_a, None, || {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            assert!(
                !crate::office::is_peer_revoked(&hq_op, sat_a, None).unwrap(),
                "revocation published before admitted peer operation finished"
            );
        })
        .unwrap()
    });
    entered_rx.recv().unwrap();
    let hq_rm = Arc::clone(&hq);
    let owner_rm = owner.clone();
    let rm_thread = std::thread::spawn(move || {
        hq_rm
            .remove_satellite_office(&owner_rm, sat_a)
            .unwrap()
            .unwrap()
    });
    std::thread::sleep(std::time::Duration::from_millis(60));
    assert!(
        !crate::office::is_peer_revoked(&hq, sat_a, None).unwrap(),
        "removal published revocation while fenced operation was still in flight"
    );
    // Removing `sat_a` must NOT block an unrelated peer `sat_b` on its own activity fence (`TASK-52` P25-#2).
    assert_eq!(
        crate::office::run_if_peer_active(&hq, sat_b, None, || 42u32).unwrap(),
        Some(42)
    );
    release_tx.send(()).unwrap();
    assert_eq!(op_thread.join().unwrap(), Some(()));
    rm_thread.join().unwrap();
    assert!(crate::office::is_peer_revoked(&hq, sat_a, None).unwrap());

    // Part 2 (P20-#3): Even if `save_invitations` fails after `save_revocations`,
    // registered session stop flags are still signaled before removal returns.
    let stop_flag = Arc::new(AtomicBool::new(false));
    let _guard = crate::office::register_peer_session_stop(&hq, sat_b, Arc::clone(&stop_flag));
    let inv_dir = hq
        .durable
        .join("local")
        .join("offices")
        .join(&hq.info().office_id)
        .join("invitations");
    let _ = std::fs::remove_dir_all(&inv_dir);
    std::fs::create_dir_all(inv_dir.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("/nonexistent/agentic-org-invitations-fail", &inv_dir).unwrap();
    let err = hq.remove_satellite_office(&owner, sat_b);
    let _ = std::fs::remove_file(&inv_dir);
    assert!(err.is_err(), "expected invitation save failure");
    assert!(
        crate::office::is_peer_revoked(&hq, sat_b, None).unwrap(),
        "revocation must stay durable"
    );
    assert!(
        stop_flag.load(Ordering::Acquire),
        "live session stop flag must still be signaled when invitation save fails"
    );
}

#[test]
fn streaming_snapshot_export_pages_to_disk_and_honors_byte_budget() {
    let hq = Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    });
    let dir_ok = tempfile::tempdir().unwrap();
    let mut observed_bytes = 0u64;
    let exported = crate::office::export_authoritative_state_snapshot_pages(
        &hq,
        dir_ok.path(),
        2048,
        1024,
        |ev| {
            // Proves `federation_io_lock` is NOT held while streaming pages to disk so concurrent
            // revocation checks and preparation keepalives never block (`TASK-52` P23-#1).
            assert!(!crate::office::is_peer_revoked(
                &hq,
                "0195a000-0000-7000-8000-0000000000cc",
                None
            )
            .unwrap());
            if let crate::office::SnapshotExportProgress::ReservePageBytes(b) = ev {
                observed_bytes += b;
            }
            true
        },
    )
    .expect("paged export succeeds");
    assert!(
        exported.total_pages >= 2,
        "expected multi-page export under 2 KiB cap, got {}",
        exported.total_pages
    );
    assert_eq!(exported.total_bytes, observed_bytes);
    for page_idx in 0..exported.total_pages {
        let p = dir_ok.path().join(format!("page-{page_idx:06}.json"));
        let raw = std::fs::read(&p).expect("page file exists on disk");
        assert!(raw.len() <= 2048);
        let entries: Vec<(String, String)> = serde_json::from_slice(&raw).expect("valid page JSON");
        assert!(!entries.is_empty());
    }

    // Refusing budget on page 2 aborts cleanly.
    let dir_budget = tempfile::tempdir().unwrap();
    let mut pages_seen = 0usize;
    let err = crate::office::export_authoritative_state_snapshot_pages(
        &hq,
        dir_budget.path(),
        2048,
        1024,
        |ev| {
            if let crate::office::SnapshotExportProgress::ReservePageBytes(_) = ev {
                pages_seen += 1;
            }
            pages_seen < 2
        },
    );
    assert!(err.is_err(), "budget rejection must abort snapshot export");

    // Concurrent export B waiting on Export A must NOT block `is_peer_revoked`, and must cancel
    // promptly via `Heartbeat` (`TASK-52` P24-#1 / P24-#2).
    let hq = Arc::new(hq);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let hq_a = Arc::clone(&hq);
    let dir_a = tempfile::tempdir().unwrap();
    let path_a = dir_a.path().to_path_buf();
    let t_a = std::thread::spawn(move || {
        let mut paused = false;
        crate::office::export_authoritative_state_snapshot_pages(&hq_a, &path_a, 2048, 1024, |ev| {
            if !paused
                && matches!(
                    ev,
                    crate::office::SnapshotExportProgress::ReservePageBytes(_)
                )
            {
                paused = true;
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }
            true
        })
    });
    entered_rx.recv().unwrap();
    let hq_b = Arc::clone(&hq);
    let dir_b = tempfile::tempdir().unwrap();
    let path_b = dir_b.path().to_path_buf();
    let cancel_b = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_b_ref = Arc::clone(&cancel_b);
    let t_b = std::thread::spawn(move || {
        crate::office::export_authoritative_state_snapshot_pages(&hq_b, &path_b, 2048, 1024, |_| {
            !cancel_b_ref.load(std::sync::atomic::Ordering::Acquire)
        })
    });
    std::thread::sleep(std::time::Duration::from_millis(40));
    assert!(
        !crate::office::is_peer_revoked(&hq, "0195a000-0000-7000-8000-0000000000dd", None).unwrap(),
        "concurrent exports must never block revocation checks / keepalives"
    );
    cancel_b.store(true, std::sync::atomic::Ordering::Release);
    assert!(
        t_b.join().unwrap().is_err(),
        "queued Export B must cancel promptly while Export A is streaming"
    );
    release_tx.send(()).unwrap();
    assert!(t_a.join().unwrap().is_ok());
}

#[test]
fn snapshot_export_fails_closed_on_corrupt_channel_and_invalid_filename() {
    let hq = Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    });
    // Corrupt/unreadable `conversation.json` must fail snapshot export closed instead of skipping
    // the channel (`TASK-52` P25-#1).
    let corrupt_dir = hq
        .durable
        .join("messaging")
        .join("conversations")
        .join("0195a000-0000-7000-8000-0000000000ee");
    std::fs::create_dir_all(&corrupt_dir).unwrap();
    std::fs::write(corrupt_dir.join("conversation.json"), b"{not-valid-json").unwrap();
    let dir_corrupt = tempfile::tempdir().unwrap();
    let corrupt_err = crate::office::export_authoritative_state_snapshot_pages(
        &hq,
        dir_corrupt.path(),
        2048,
        1024,
        |_| true,
    );
    let _ = std::fs::remove_dir_all(&corrupt_dir);
    assert!(
        corrupt_err.is_err(),
        "corrupt channel conversation.json must fail export closed"
    );

    // Invalid filename containing `\` directly under `state/` must fail closed instead of being
    // rewritten into a subdirectory path (`TASK-52` P27-#2).
    let bad_file = hq.durable.join("state").join(r"projects\orphan.yaml");
    std::fs::write(&bad_file, b"orphan: true\n").unwrap();
    let dir_bad = tempfile::tempdir().unwrap();
    let bad_err = crate::office::export_authoritative_state_snapshot_pages(
        &hq,
        dir_bad.path(),
        2048,
        1024,
        |_| true,
    );
    let _ = std::fs::remove_file(&bad_file);
    assert!(
        bad_err.is_err(),
        "invalid backslash filename under state/ must fail snapshot export closed"
    );

    // A symlink inside `messaging/conversations/` must fail snapshot export closed (`TASK-52` P28-#1).
    let link_target = tempfile::tempdir().unwrap();
    let link_path = hq
        .durable
        .join("messaging")
        .join("conversations")
        .join("0195a000-0000-7000-8000-0000000000ef");
    std::fs::create_dir_all(link_path.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(link_target.path(), &link_path).unwrap();
    let dir_link = tempfile::tempdir().unwrap();
    let link_err = crate::office::export_authoritative_state_snapshot_pages(
        &hq,
        dir_link.path(),
        2048,
        1024,
        |_| true,
    );
    let _ = std::fs::remove_file(&link_path);
    assert!(
        link_err.is_err(),
        "symlink channel directory must fail snapshot export closed"
    );

    // Replacing `durable/messaging` itself with a symlink to an empty dir must also fail closed
    // (`TASK-52` P29-#1).
    let msg_root = hq.durable.join("messaging");
    let msg_backup = hq.durable.join("messaging-backup-test");
    if msg_root.exists() {
        std::fs::rename(&msg_root, &msg_backup).unwrap();
    }
    let empty_target = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(empty_target.path(), &msg_root).unwrap();
    let dir_msg_link = tempfile::tempdir().unwrap();
    let msg_link_err = crate::office::export_authoritative_state_snapshot_pages(
        &hq,
        dir_msg_link.path(),
        2048,
        1024,
        |_| true,
    );
    let _ = std::fs::remove_file(&msg_root);
    if msg_backup.exists() {
        std::fs::rename(&msg_backup, &msg_root).unwrap();
    }
    assert!(
        msg_link_err.is_err(),
        "symlink durable/messaging parent must fail snapshot export closed"
    );
}
