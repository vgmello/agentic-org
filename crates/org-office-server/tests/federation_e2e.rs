//! Full live TCP + Org-CA mTLS multi-office federation E2E test (`TASK-52` / `S1`):
//! 1. Fresh HQ starts `HqFederationServer` on loopback TLS.
//! 2. HQ creates invitation -> satellite enrolls via Org-CA-pinned TLS + RFC 5705 exporter proof
//!    -> check code verified & approved at HQ -> satellite receives `ClientAuth`-only cert.
//! 3. Satellite provisions on its own roots (`ARCH-02` isolation), connects mTLS, pulls initial
//!    snapshot, and projects HQ's organization & both offices.
//! 4. HQ appends a live `OrgEvent` -> satellite pulls and converges.
//! 5. Satellite disconnects (offline), enqueues durable work change + cross-office message while
//!    offline -> reconnects mTLS -> flushes work outbox & cross-office message + wake relay to HQ.
//! 6. HQ revokes satellite certificate via `remove_satellite_office` -> next mTLS connection fails.
//! 7. D25: Replacement HQ with a brand-new Org CA & HQ ID re-enrolls the existing satellite
//!    preserving its `office_id` and syncs cleanly.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use org_office::{
    CreateInvitationParams, Custody, DataRoots, Key, NewOffice, Office, OfficeRole,
    SatelliteProvisionSpec, SatelliteReEnrollSpec, TestOffice,
};
use org_office_server::federation::{
    enroll_satellite_with_hq, HqFederationServer, RelayedWake, SatelliteSyncSession,
};
use org_pki::{invitation::InvitationToken, SigningKey, Zeroizing};
use org_store::{testkit::FakeCustody, Id, OfficeIdentity, WriterAuthority};
use org_sync::{
    messages::{enqueue as enqueue_msg, Message},
    work::Origin,
};
use org_sync_proto::{Digest, OfficeId, OrgId, OriginIncarnation};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn sat_custody(org_id: &str, sat_office_id: &str) -> Custody {
    Custody {
        identity: OfficeIdentity {
            instance_id: Id::parse("0195b000-0000-7000-8000-000000000001").unwrap(),
            organization_id: Id::parse(org_id).unwrap(),
            office_id: Id::parse(sat_office_id).unwrap(),
            journal_incarnation: Id::parse("0195b000-0000-7000-8000-000000000002").unwrap(),
            writer_epoch: 1,
            machine_id: Id::parse("0195b000-0000-7000-8000-000000000003").unwrap(),
        },
        authority: Arc::new(FakeCustody::new()) as Arc<dyn WriterAuthority>,
    }
}

#[test]
fn multi_office_mtls_enrollment_snapshot_offline_convergence_relay_revocation_and_d25_reenroll() {
    let hq = Arc::new(Office::for_test_with_real_ca(TestOffice {
        engine: org_office::EngineChoice::Sqlite,
    }));
    let owner = hq.owner_member_id().to_owned();
    let org_id = hq.info().org_id.clone();

    let wakes_received: Arc<Mutex<Vec<RelayedWake>>> = Arc::new(Mutex::new(Vec::new()));
    let wakes_cb = Arc::clone(&wakes_received);
    let hq_server = HqFederationServer::start(
        Arc::clone(&hq),
        "127.0.0.1:0".parse().unwrap(),
        &["127.0.0.1".into()],
        Some(Arc::new(move |w| {
            wakes_cb.lock().unwrap().push(w);
            Ok(())
        })),
    )
    .unwrap();
    let hq_addr = hq_server.local_addr().to_string();

    // 1. HQ creates satellite invitation & satellite enrolls over live Org-CA-pinned TLS.
    let created = hq
        .create_satellite_invitation(
            &owner,
            CreateInvitationParams {
                office_name: "Porto Satellite".into(),
                hq_addresses: vec![hq_addr.clone()],
                ttl_secs: Some(600),
                existing_office_id: None,
            },
        )
        .unwrap()
        .unwrap();
    let token = InvitationToken::decode(&created.token).unwrap();
    let sat_office_id = token.office_id.clone();
    let sat_tls_key = SigningKey::generate().unwrap();
    let sat_tls_der = sat_tls_key.local_der().to_vec();

    let (tx_code, rx_code) = std::sync::mpsc::channel::<String>();
    let token_clone = token.clone();
    let enroll_thread = thread::spawn(move || {
        enroll_satellite_with_hq(&token_clone, &sat_tls_key, move |code| {
            let _ = tx_code.send(code.to_owned());
        })
    });
    let shown_code = rx_code.recv().unwrap();
    let approved = hq
        .approve_satellite_invitation(&owner, &token.invitation_id, &shown_code)
        .unwrap()
        .unwrap();
    let enrolled = enroll_thread.join().unwrap().unwrap();
    assert_eq!(enrolled.hq_office_id, hq.info().office_id);
    assert_eq!(enrolled.check_code, shown_code);

    // 2. Provision satellite on its own temporary roots & connect mTLS.
    let sat_dir = tempfile::tempdir().unwrap();
    let sat_root = sat_dir.path().canonicalize().unwrap();
    let sat_folder = sat_root.join("home/offices").join(&sat_office_id);
    let sat_roots = DataRoots {
        durable: sat_root.join("data").join(&org_id).join("durable"),
        runtime: sat_root.join("data").join(&org_id).join("runtime"),
    };
    std::fs::create_dir_all(&sat_roots.durable).unwrap();
    std::fs::create_dir_all(&sat_roots.runtime).unwrap();
    let sat = Arc::new(
        Office::provision_satellite(
            sat_folder,
            SatelliteProvisionSpec {
                new: NewOffice {
                    office_id: sat_office_id.clone(),
                    office_name: "Porto Satellite".into(),
                    role: OfficeRole::Satellite,
                    org_id: org_id.clone(),
                    org_name: hq.info().org_name.clone(),
                },
                operation_id: "0195b000-0000-7000-8000-000000000010".into(),
                hq_office_id: enrolled.hq_office_id.clone(),
                hq_addresses: vec![hq_addr.clone()],
                epoch: enrolled.epoch,
                ca_cert_der: enrolled.ca_cert_der.clone(),
                office_cert_der: enrolled.cert_der.clone(),
                office_key_der: Zeroizing::new(sat_tls_der),
            },
            Key::from_bytes([0x33; 32]),
            sat_roots.clone(),
            sat_custody(&org_id, &sat_office_id),
        )
        .unwrap(),
    );
    assert!(!sat_roots.durable.join("private/task-notes.db").exists());

    // 3. Initial snapshot + live OrgEvent replication over mTLS.
    {
        let mut session = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();
        let files_applied = session
            .sync_initial_snapshot("0195b000-0000-7000-8000-000000000011")
            .unwrap();
        assert!(files_applied > 0);
        assert!(!sat.sync_status().unwrap().initial_snapshot_required);

        // Perform a REAL HQ domain mutation (creating a team in `org-store`), then pull via
        // mTLS: HQ publishes from its `ARCH-03` journal consumer and satellite applies the typed
        // `OrgDeltaPayload` into its own `org-store` so `org_snapshot().teams` reflects it (F2)!
        let team_cmd_id = hq.next_uuid_v7();
        let team_id = hq.next_uuid_v7();
        hq.submit_org(
            &owner,
            org_office::rules::OrgCommand::TeamCreate(org_proto::org::TeamCreateParams {
                command_id: team_cmd_id,
                team_id: team_id.clone(),
                name: "Replicated Engineering".into(),
                owner_team_id: None,
            }),
        )
        .unwrap()
        .unwrap();
        let pulled = session.pull_org_events().unwrap();
        assert!(pulled >= 1);
        assert!(sat.sync_status().unwrap().seq >= 1);
        let sat_teams = sat.org_snapshot().unwrap().teams;
        assert!(
            sat_teams.iter().any(|t| t.name == "Replicated Engineering"),
            "satellite org_snapshot must contain HQ-replicated team; got {sat_teams:?}"
        );

        // TASK-140: a catch-up burst of more than one batch of HQ events (one per task)
        // applies in verified batches and converges exactly.
        let burst: Vec<String> = (0..70)
            .map(|n| create_hq_task(&hq, &owner, &team_id, &format!("Burst task {n}")))
            .collect();
        let journal_head = sat
            .journal_entries_after(0)
            .last()
            .map_or(0, |entry| entry.ordinal());
        let pulled = session.pull_org_events().unwrap();
        assert!(pulled >= burst.len(), "pulled {pulled}");
        // One replicated-files operation per batch of up to 64 events, not one per event.
        let applies = sat.journal_entries_after(journal_head).len();
        assert!(
            applies <= 4,
            "{applies} journal entries for {pulled} events"
        );
        assert_eq!(
            sat.sync_status().unwrap().seq,
            hq.sync_status().unwrap().seq,
            "the satellite reaches HQ's organization position"
        );
        for task_id in &burst {
            assert!(
                sat.task_inspect(task_id).unwrap().is_ok(),
                "replicated task {task_id} is on the satellite"
            );
        }
        assert_eq!(
            session.pull_org_events().unwrap(),
            0,
            "nothing left to apply"
        );
    }

    // 4. Offline phase: satellite creates a REAL canonical task while disconnected; the production
    //    `task_create` writer automatically feeds the durable `org-sync-work-outbox` consumer (H-6)
    //    with zero manual test enqueuing! Also verify satellite budget writes fail closed (M-20).
    let sat_team_id = sat.org_snapshot().unwrap().teams[0].id.clone();
    let offline_task_id = sat.next_uuid_v7();
    let offline_task_cmd = sat.next_uuid_v7();
    sat.task_create(
        &owner,
        org_proto::tasks::TasksCreateParams {
            command_id: offline_task_cmd,
            task_id: offline_task_id.clone(),
            team_id: Some(sat_team_id.clone()),
            project_id: None,
            assigned_team_id: None,
            assignee_id: None,
            task_type: None,
            work_type: None,
            title: "Offline Satellite Task 99".into(),
            body_markdown: "Created while HQ was unreachable".into(),
            priority: None,
            effort: None,
            severity: None,
            rank_order: None,
            labels: Vec::new(),
            reference_files: Vec::new(),
            modified_files: Vec::new(),
            depends_on: Vec::new(),
            details: None,
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        sat.sync_status().unwrap().pending_work_count,
        1,
        "satellite task_create must automatically enqueue work outbox entry"
    );
    let sat_budget_refused = sat
        .budget_set(
            &owner,
            org_proto::budgets::BudgetSetParams {
                operation_id: sat.next_uuid_v7(),
                scope_kind: org_proto::budgets::BudgetScopeKind::Task,
                task_id: Some(offline_task_id.clone()),
                agent_id: None,
                release_id: None,
                period_kind: org_proto::budgets::BudgetPeriodKind::Lifetime,
                unit: org_proto::budgets::BudgetUnit::MicroUsd,
                limit_amount: 1_000_000,
                time_zone: Some("UTC".into()),
                time_zone_source: None,
                reason: Some("should fail on satellite".into()),
            },
        )
        .unwrap();
    assert!(
        matches!(sat_budget_refused, Err(ref e) if e.code == org_proto::ErrorCode::NotAuthorized),
        "satellite must refuse budget.set writes: {sat_budget_refused:?}"
    );

    let org_sync_id = OrgId::new(&org_id).unwrap();
    let sat_sync_id = OfficeId::new(&sat_office_id).unwrap();
    let hq_sync_id = OfficeId::new(&hq.info().office_id).unwrap();
    let inc = OriginIncarnation::new("0195b000-0000-7000-8000-000000000002").unwrap();
    let created_ms = now_ms();
    enqueue_msg(
        sat.replication().as_ref(),
        Message {
            org_id: org_sync_id,
            origin: sat_sync_id.clone(),
            destination: hq_sync_id,
            id: "cross-msg-1".into(),
            fingerprint: Digest::from_bytes([0x55; 32]),
            payload: Arc::from(b"hello-from-porto".as_slice()),
            deadline_ms: created_ms + 300_000,
        },
        created_ms,
    )
    .unwrap();
    assert_eq!(sat.sync_status().unwrap().pending_work_count, 1);
    assert_eq!(sat.sync_status().unwrap().pending_message_count, 1);

    // 5. Reconnect mTLS -> work outbox + cross-office message + wake relay converge at HQ!
    {
        let mut session = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();
        assert_eq!(session.flush_work_outbox().unwrap(), 1);
        assert_eq!(session.flush_message_outbox().unwrap(), 1);
        assert_eq!(sat.sync_status().unwrap().pending_work_count, 0);
        assert_eq!(sat.sync_status().unwrap().pending_message_count, 0);
        assert_eq!(
            hq.replication()
                .remote_work_high_water(&Origin {
                    office: sat_sync_id,
                    incarnation: inc,
                })
                .unwrap(),
            1
        );
        assert_eq!(
            hq.replication().delivered_message_payloads().unwrap().len(),
            1
        );
        // Verify real offline-created satellite task is now queryable on HQ via `hq.task_list`
        // (P2-2) and HQ refuses satellite work payloads targeting HQ-owned organization paths (P2-1).
        let hq_tasks = hq
            .task_list(org_proto::tasks::TasksListParams::default())
            .unwrap()
            .unwrap()
            .tasks;
        assert!(
            hq_tasks
                .iter()
                .any(|t| t.id == offline_task_id && t.title == "Offline Satellite Task 99"),
            "HQ task_list must contain satellite offline-created task; got {hq_tasks:?}"
        );
        assert!(
            hq.apply_replicated_work_files(
                &hq.next_uuid_v7(),
                vec![(
                    "state/organization/org.yaml".into(),
                    Some(b"tamper".to_vec())
                )],
            )
            .is_err(),
            "HQ must refuse satellite work writes outside projector-owned work families"
        );

        // Forged wake with unaccepted `source_id` MUST be ignored by HQ (F5).
        session
            .send_wake(RelayedWake {
                origin_office: sat_office_id.clone(),
                destination_office: hq.info().office_id.clone(),
                employee_id: owner.clone(),
                kind: "message".into(),
                source_id: "forged-unknown-source".into(),
                sender_id: "rogue".into(),
                sender_name: "Rogue".into(),
                sender_is_person: false,
                conversation_id: None,
                summary: "Forged".into(),
                ask_answer: None,
                ask_withdrawal: None,
            })
            .unwrap();
        // Authentic wake referencing the accepted `cross-msg-1` IS delivered!
        session
            .send_wake(RelayedWake {
                origin_office: sat_office_id.clone(),
                destination_office: hq.info().office_id.clone(),
                employee_id: owner.clone(),
                kind: "message".into(),
                source_id: "cross-msg-1".into(),
                sender_id: "agent-porto".into(),
                sender_name: "Porto Dev".into(),
                sender_is_person: false,
                conversation_id: None,
                summary: "Task 99 ready".into(),
                ask_answer: None,
                ask_withdrawal: None,
            })
            .unwrap();
    }
    // Brief wait for HQ connection thread to deliver wake callback.
    for _ in 0..40 {
        if !wakes_received.lock().unwrap().is_empty() {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(25));
    }
    assert_eq!(wakes_received.lock().unwrap().len(), 1);
    assert_eq!(wakes_received.lock().unwrap()[0].source_id, "cross-msg-1");

    // 6. Revoke satellite at HQ -> subsequent mTLS connect MUST fail!
    let removed = hq
        .remove_satellite_office(&owner, &sat_office_id)
        .unwrap()
        .unwrap();
    assert_eq!(removed.revoked_serials, vec![approved.serial_hex]);
    assert!(SatelliteSyncSession::connect(Arc::clone(&sat)).is_err());

    // 7. D25: Real R1 replacement HQ imported from HQ's durable state/ with a brand-new HQ ID
    //    and fresh Org CA re-enrolls the SAME satellite office_id and converges cleanly!
    hq_server.stop();
    let imported = org_office::import::validate_state_dir(
        &hq.durable_root().join("state"),
        "https://example.com/acme-org.git",
        "main",
    )
    .unwrap();
    let rep_hq_id = "0195b000-0000-7000-8000-000000000099";
    let rep_op_id = "0195b000-0000-7000-8000-000000000098";
    let rep_dir = tempfile::tempdir().unwrap();
    let rep_root = rep_dir.path().canonicalize().unwrap();
    let rep_folder = rep_root.join("home/offices").join(rep_hq_id);
    let rep_roots = DataRoots {
        durable: rep_root.join("data").join(&org_id).join("durable"),
        runtime: rep_root.join("data").join(&org_id).join("runtime"),
    };
    std::fs::create_dir_all(&rep_roots.durable).unwrap();
    std::fs::create_dir_all(&rep_roots.runtime).unwrap();
    let rep_key = Key::from_bytes([0x77; 32]);
    let rep_key_wrap = rep_key.clone();
    let rep_org_id = org_id.clone();
    let rep_custody = Custody {
        identity: OfficeIdentity {
            instance_id: Id::parse("0195b000-0000-7000-8000-000000000090").unwrap(),
            organization_id: Id::parse(&org_id).unwrap(),
            office_id: Id::parse(rep_hq_id).unwrap(),
            journal_incarnation: Id::parse("0195b000-0000-7000-8000-000000000091").unwrap(),
            writer_epoch: 1,
            machine_id: Id::parse("0195b000-0000-7000-8000-000000000092").unwrap(),
        },
        authority: Arc::new(FakeCustody::new()) as Arc<dyn WriterAuthority>,
    };
    let creator_person = imported
        .summary
        .people
        .iter()
        .find(|p| p.id == owner)
        .expect("imported owner person");
    let rep_prepared = org_proto::selected_bootstrap::PreparedV1 {
        plan: org_proto::selected_bootstrap::PlanV1 {
            version: 1,
            name: hq.info().org_name.clone(),
            slug: imported.summary.slug.clone(),
            repository_remote_url: "https://example.com/acme-org.git".into(),
            state_branch: "main".into(),
        },
        organization_id: org_id.clone(),
        office_id: rep_hq_id.into(),
        repository_id: "0195b000-0000-7000-8000-000000000093".into(),
        home_team_id: "0195b000-0000-7000-8000-000000000094".into(),
        hr_employee_id: "0195b000-0000-7000-8000-000000000095".into(),
        creator: org_proto::selected_bootstrap::CreatorV1 {
            employee_id: owner.clone(),
            name: creator_person.name.clone(),
            handle: creator_person.handle.clone(),
        },
    };
    let rep_binding = org_office::hq_creation::Binding {
        operation_id: rep_op_id.into(),
        owner_member_id: owner.clone(),
        bootstrap: rep_prepared,
    };
    let replacement_hq = Arc::new(
        Office::import_hq(
            rep_folder,
            &NewOffice {
                office_id: rep_hq_id.into(),
                office_name: "Replacement HQ".into(),
                role: OfficeRole::Hq,
                org_id: org_id.clone(),
                org_name: hq.info().org_name.clone(),
            },
            rep_key,
            &rep_binding,
            rep_roots,
            rep_custody,
            move || Ok((imported, None)),
            move |prior| {
                if let Some(p) = prior {
                    return Ok(p);
                }
                let ca = SigningKey::generate().expect("replacement CA key");
                let pub_key = ca.public_key();
                let code = org_pki::Code::generate().expect("recovery code");
                let pkg = ca.seal(&rep_org_id, &code).expect("seal replacement CA");
                let wrapped = org_pki::wrap::wrap(
                    rep_key_wrap.expose_bytes(),
                    &org_pki::wrap::WrapBinding {
                        organization_id: &rep_org_id,
                        office_id: rep_hq_id,
                        purpose: org_pki::wrap::WrapPurpose::OrganizationCa,
                    },
                    ca.local_der(),
                )
                .expect("wrap replacement CA");
                org_office::hq_creation::LocalRecord::new(
                    rep_org_id,
                    rep_op_id.into(),
                    pub_key,
                    pkg,
                    wrapped,
                )
            },
        )
        .unwrap(),
    );
    let rep_owner = replacement_hq.owner_member_id().to_owned();
    let rep_server = HqFederationServer::start(
        Arc::clone(&replacement_hq),
        "127.0.0.1:0".parse().unwrap(),
        &["127.0.0.1".into()],
        None,
    )
    .unwrap();
    let rep_addr = rep_server.local_addr().to_string();
    let rep_inv = replacement_hq
        .create_satellite_invitation(
            &rep_owner,
            CreateInvitationParams {
                office_name: "Porto Satellite".into(),
                hq_addresses: vec![rep_addr.clone()],
                ttl_secs: Some(600),
                existing_office_id: Some(sat_office_id.clone()),
            },
        )
        .unwrap()
        .unwrap();
    let rep_tok = InvitationToken::decode(&rep_inv.token).unwrap();
    let new_sat_key = SigningKey::generate().unwrap();
    let new_sat_der = new_sat_key.local_der().to_vec();
    let (tx2, rx2) = std::sync::mpsc::channel::<String>();
    let rep_tok_clone = rep_tok.clone();
    let t2 = thread::spawn(move || {
        enroll_satellite_with_hq(&rep_tok_clone, &new_sat_key, move |c| {
            let _ = tx2.send(c.to_owned());
        })
    });
    let code2 = rx2.recv().unwrap();
    replacement_hq
        .approve_satellite_invitation(&rep_owner, &rep_tok.invitation_id, &code2)
        .unwrap()
        .unwrap();
    let bundle2 = t2.join().unwrap().unwrap();
    assert_eq!(bundle2.hq_office_id, rep_hq_id);
    // Create an offline satellite task AFTER old HQ was revoked/lost: D25 snapshot reconciliation
    // must preserve this retained local work & immutable event file (H-9) and rebase the outbox
    // incarnation so replacement HQ accepts it cleanly (H-8)!
    let pre_loss_task_id = sat.next_uuid_v7();
    sat.task_create(
        &owner,
        org_proto::tasks::TasksCreateParams {
            command_id: sat.next_uuid_v7(),
            task_id: pre_loss_task_id.clone(),
            team_id: Some(sat_team_id),
            project_id: None,
            assigned_team_id: None,
            assignee_id: None,
            task_type: None,
            work_type: None,
            title: "Pre-Loss Offline Satellite Task".into(),
            body_markdown: "Survived HQ replacement via D25".into(),
            priority: None,
            effort: None,
            severity: None,
            rank_order: None,
            labels: Vec::new(),
            reference_files: Vec::new(),
            modified_files: Vec::new(),
            depends_on: Vec::new(),
            details: None,
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(sat.sync_status().unwrap().pending_work_count, 1);

    sat.re_enroll_satellite(SatelliteReEnrollSpec {
        org_id: org_id.clone(),
        new_hq_office_id: bundle2.hq_office_id.clone(),
        new_hq_addresses: vec![rep_addr],
        new_epoch: bundle2.epoch,
        new_ca_cert_der: bundle2.ca_cert_der,
        new_office_cert_der: bundle2.cert_der,
        new_office_key_der: Zeroizing::new(new_sat_der),
    })
    .unwrap();
    assert!(sat.sync_status().unwrap().initial_snapshot_required);
    let mut rep_session = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();
    let re_files = rep_session
        .sync_initial_snapshot("0195b000-0000-7000-8000-000000000019")
        .unwrap();
    assert!(re_files > 0);
    assert!(!sat.sync_status().unwrap().initial_snapshot_required);
    assert_eq!(sat.sync_status().unwrap().hq_office, rep_hq_id);
    assert_eq!(rep_session.flush_work_outbox().unwrap(), 1);
    let rep_tasks = replacement_hq
        .task_list(org_proto::tasks::TasksListParams::default())
        .unwrap()
        .unwrap()
        .tasks;
    assert!(
        rep_tasks
            .iter()
            .any(|t| t.id == pre_loss_task_id && t.title == "Pre-Loss Offline Satellite Task"),
        "replacement HQ must receive preserved offline satellite task after D25 re-enrollment"
    );
    rep_server.stop();
}

/// Creates a task at HQ in `team_id` and returns its ID.
fn create_hq_task(hq: &Office, owner: &str, team_id: &str, title: &str) -> String {
    let task_id = hq.next_uuid_v7();
    hq.task_create(
        owner,
        org_proto::tasks::TasksCreateParams {
            command_id: hq.next_uuid_v7(),
            task_id: task_id.clone(),
            team_id: Some(team_id.to_owned()),
            project_id: None,
            assigned_team_id: None,
            assignee_id: None,
            task_type: None,
            work_type: None,
            title: title.into(),
            body_markdown: "Created at HQ".into(),
            priority: None,
            effort: None,
            severity: None,
            rank_order: None,
            labels: Vec::new(),
            reference_files: Vec::new(),
            modified_files: Vec::new(),
            depends_on: Vec::new(),
            details: None,
        },
    )
    .unwrap()
    .unwrap();
    task_id
}

/// `TASK-80.1` pass 2: an organization exported before the built-in Chief of Staff existed gets
/// her during `import_hq` itself (not only at the next open), with the same derived ID, once.
#[test]
fn an_organization_imported_without_its_chief_of_staff_gets_her_at_import() {
    let hq = Office::for_test_with_real_ca(TestOffice {
        engine: org_office::EngineChoice::Sqlite,
    });
    let owner = hq.owner_member_id().to_owned();
    let org_id = hq.info().org_id.clone();
    let tess = hq.chief_of_staff().unwrap().expect("Tess");
    hq.forget_chief_of_staff_for_test().unwrap();
    assert!(
        hq.chief_of_staff().unwrap().is_none(),
        "an older organization"
    );
    let imported = org_office::import::validate_state_dir(
        &hq.durable_root().join("state"),
        "https://example.com/acme-org.git",
        "main",
    )
    .unwrap();
    assert!(!imported
        .summary
        .people
        .iter()
        .any(|p| p.id == tess.id.as_str()));
    let rep_hq_id = "0195b000-0000-7000-8000-0000000000a9";
    let rep_op_id = "0195b000-0000-7000-8000-0000000000a8";
    let rep_dir = tempfile::tempdir().unwrap();
    let rep_root = rep_dir.path().canonicalize().unwrap();
    let rep_roots = DataRoots {
        durable: rep_root.join("data").join(&org_id).join("durable"),
        runtime: rep_root.join("data").join(&org_id).join("runtime"),
    };
    std::fs::create_dir_all(&rep_roots.durable).unwrap();
    std::fs::create_dir_all(&rep_roots.runtime).unwrap();
    let rep_key = Key::from_bytes([0x78; 32]);
    let rep_key_wrap = rep_key.clone();
    let rep_org_id = org_id.clone();
    let rep_custody = Custody {
        identity: OfficeIdentity {
            instance_id: Id::parse("0195b000-0000-7000-8000-0000000000a0").unwrap(),
            organization_id: Id::parse(&org_id).unwrap(),
            office_id: Id::parse(rep_hq_id).unwrap(),
            journal_incarnation: Id::parse("0195b000-0000-7000-8000-0000000000a1").unwrap(),
            writer_epoch: 1,
            machine_id: Id::parse("0195b000-0000-7000-8000-0000000000a2").unwrap(),
        },
        authority: Arc::new(FakeCustody::new()) as Arc<dyn WriterAuthority>,
    };
    let creator = imported
        .summary
        .people
        .iter()
        .find(|p| p.id == owner)
        .expect("imported owner person")
        .clone();
    let binding = org_office::hq_creation::Binding {
        operation_id: rep_op_id.into(),
        owner_member_id: owner.clone(),
        bootstrap: org_proto::selected_bootstrap::PreparedV1 {
            plan: org_proto::selected_bootstrap::PlanV1 {
                version: 1,
                name: hq.info().org_name.clone(),
                slug: imported.summary.slug.clone(),
                repository_remote_url: "https://example.com/acme-org.git".into(),
                state_branch: "main".into(),
            },
            organization_id: org_id.clone(),
            office_id: rep_hq_id.into(),
            repository_id: "0195b000-0000-7000-8000-0000000000a3".into(),
            home_team_id: "0195b000-0000-7000-8000-0000000000a4".into(),
            hr_employee_id: "0195b000-0000-7000-8000-0000000000a5".into(),
            creator: org_proto::selected_bootstrap::CreatorV1 {
                employee_id: owner.clone(),
                name: creator.name.clone(),
                handle: creator.handle.clone(),
            },
        },
    };
    let replacement = Office::import_hq(
        rep_root.join("home/offices").join(rep_hq_id),
        &NewOffice {
            office_id: rep_hq_id.into(),
            office_name: "Imported HQ".into(),
            role: OfficeRole::Hq,
            org_id: org_id.clone(),
            org_name: hq.info().org_name.clone(),
        },
        rep_key,
        &binding,
        rep_roots,
        rep_custody,
        move || Ok((imported, None)),
        move |prior| {
            if let Some(p) = prior {
                return Ok(p);
            }
            let ca = SigningKey::generate().expect("replacement CA key");
            let pub_key = ca.public_key();
            let code = org_pki::Code::generate().expect("recovery code");
            let pkg = ca.seal(&rep_org_id, &code).expect("seal replacement CA");
            let wrapped = org_pki::wrap::wrap(
                rep_key_wrap.expose_bytes(),
                &org_pki::wrap::WrapBinding {
                    organization_id: &rep_org_id,
                    office_id: rep_hq_id,
                    purpose: org_pki::wrap::WrapPurpose::OrganizationCa,
                },
                ca.local_der(),
            )
            .expect("wrap replacement CA");
            org_office::hq_creation::LocalRecord::new(
                rep_org_id,
                rep_op_id.into(),
                pub_key,
                pkg,
                wrapped,
            )
        },
    )
    .unwrap();
    // She is there the moment the import returns: same derived ID, her default handle, once.
    let again = replacement
        .chief_of_staff()
        .unwrap()
        .expect("Tess at import");
    assert_eq!(again.id, tess.id);
    assert_eq!(again.handle, "tess");
    let count = replacement
        .list_employees()
        .unwrap()
        .iter()
        .filter(|e| e.builtin_key == tess.builtin_key)
        .count();
    assert_eq!(count, 1);
    assert_eq!(replacement.ensure_chief_of_staff().unwrap(), None);
    replacement.close().unwrap();
}
