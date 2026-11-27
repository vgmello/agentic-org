//! Dual-engine (`Sqlite` + `Memory`) and SQLite-specific office creation, rebuild, crash recovery,
//! and B2 command-window tests (`TASK-255`).

use std::{cell::Cell, collections::BTreeMap, sync::Arc};

use org_proto::{
    org::{CommandStatus, TeamCreateParams},
    ErrorCode, OperationOutcome,
};
use org_store::{
    testkit::{identity, CrashPoint, DiskOp, FlushSite, Target},
    Sha256,
};

use org_office::{
    office_local::{records as local, testkit::ca_material},
    organization_family::{
        testkit::{creation_files, CREATION_AT},
        wire,
    },
    rules::OrgCommand,
    testkit_support::office_harness,
    EngineChoice, Key, Office, OfficeError,
};

use super::{
    creation_id, folders, names, team_create, tree, Site, CREATOR, ENGINES, HR, TEAM_A, TEAM_B,
    TOP_TEAM,
};

#[test]
fn creation_lays_out_the_same_durable_folders_on_every_engine() {
    let [sqlite, memory] = ENGINES.map(|engine| {
        let site = Site::new();
        site.provision(engine).unwrap().close().unwrap();
        folders(&site.roots.durable)
    });
    assert!(sqlite.contains("authority/agent-execution"), "{sqlite:?}");
    assert_eq!(sqlite, memory);
}

#[test]
fn creation_writes_exactly_the_golden_files_and_the_ca_records() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        let material = ca_material(&identity().organization_id, office_harness::CA_SEED);

        let mut expected = creation_files();
        expected.insert(
            "state/security/ca-escrow.v1.bin".into(),
            material.package.clone(),
        );
        let state: BTreeMap<String, Vec<u8>> = tree(&site.durable("state"))
            .into_iter()
            .map(|(path, bytes)| (format!("state/{path}"), bytes))
            .collect();
        assert_eq!(state, expected, "{engine:?}");
        let key = material.record(&identity(), &creation_id());
        let key_path = local::ca_key_path(&identity().office_id);
        let key_path = key_path.as_str().strip_prefix("local/").unwrap().to_owned();
        let expected_local = BTreeMap::from([(key_path, local::encode_ca_key(&key))]);
        let mut local_tree = tree(&site.durable("local"));
        let replication_path =
            format!("offices/{}/replication/state.v1.json", identity().office_id);
        let bytes = local_tree
            .remove(&replication_path)
            .expect("the creation writes the replication state");
        let replication: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(replication["version"], 1, "{engine:?}");
        assert_eq!(
            replication["local_office"],
            identity().office_id.to_string(),
            "{engine:?}"
        );
        assert_eq!(replication["seq"], 0, "{engine:?}");
        assert_eq!(local_tree, expected_local, "{engine:?}");
        assert_eq!(
            names(&site.roots.durable),
            [
                "accounting",
                "authority",
                "budgets",
                "local",
                "private",
                "recovery",
                "state",
            ],
            "{engine:?}"
        );
        let receipts = tree(&site.durable("recovery/commands"));
        let receipts: Vec<&String> = receipts.keys().collect();
        assert_eq!(receipts, [&format!("{}.json", creation_id())], "{engine:?}");

        assert_eq!(
            names(&site.folder),
            ["hq-creation.v1.json", "office.json", "office.lock"]
        );
        let frozen: serde_json::Value = serde_json::from_slice(
            &std::fs::read(site.folder.join("hq-creation.v1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(frozen["version"], 3);
        assert_eq!(frozen["operation"], office_harness::OPERATION);

        let public = office.hq_creation_public_record().unwrap().unwrap();
        assert_eq!(public.package, material.package);
        assert_eq!(key.package_sha256, Sha256::of(&public.package));
        assert_eq!(public.operation_id, creation_id().as_str());
        let record = office.hq_creation_record().unwrap().unwrap();
        assert_eq!(record.wrapped_ca_key(), material.wrapped_ca_key.as_slice());
        assert!(office.is_hq().unwrap());
        assert_eq!(office.org_snapshot().unwrap().position.seq, 1);
        office.close().unwrap();
    }
}

#[test]
fn a_foreign_file_where_creation_writes_stops_it_and_is_named() {
    for planted in ["state/organization.yaml", "state/repositories.yaml"] {
        let site = Site::new();
        std::fs::create_dir_all(site.durable("state")).unwrap();
        std::fs::write(site.durable(planted), b"planted: true\n").unwrap();
        let error = site.provision(EngineChoice::Sqlite).unwrap_err();
        if planted == "state/organization.yaml" {
            assert!(
                matches!(
                    error,
                    OfficeError::Invalid("the data roots hold an organization before its creation")
                ),
                "{planted}: {error:?}"
            );
            let anchor = org_store::journal_dir(&site.roots.durable, &identity());
            assert!(!anchor.join("anchor.json").exists(), "{planted}");
        } else {
            assert!(
                matches!(error, OfficeError::Storage(_)),
                "{planted}: {error:?}"
            );
            let cause = std::error::Error::source(&error).unwrap().to_string();
            assert!(cause.contains(planted), "{cause}");
        }
        assert!(!error.to_string().contains(".yaml"), "{error}");
        let kept = std::fs::read(site.durable(planted)).unwrap();
        assert_eq!(kept, b"planted: true\n");
        assert!(!site.folder.join("office.json").exists());
    }
}

#[test]
fn deleting_the_runtime_folder_rebuilds_an_equal_projection() {
    let site = Site::new();
    let office = site.provision(EngineChoice::Sqlite).unwrap();
    let answer = office.submit_org(CREATOR, team_create(1, TEAM_A, TOP_TEAM));
    assert_eq!(answer.unwrap().unwrap().status, CommandStatus::Committed);
    let (dump, snapshot) = (office.dump(), office.org_snapshot().unwrap());
    office.close().unwrap();
    std::fs::remove_dir_all(site.runtime_folder()).unwrap();

    let office = site.open(EngineChoice::Sqlite).unwrap();
    assert_eq!(office.dump(), dump);
    assert_eq!(office.org_snapshot().unwrap(), snapshot);
    assert!(site.runtime_folder().join("office.db").is_file());
    office.close().unwrap();
}

#[test]
fn an_answered_team_survives_a_kill_and_only_a_close_is_a_clean_stop() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        let answered = office.submit_org(CREATOR, team_create(1, TEAM_A, TOP_TEAM));
        let answered = answered.unwrap().unwrap();
        site.killed(office);

        let office = site.open(engine).unwrap();
        assert!(!office.stopped_cleanly(), "{engine:?}");
        let teams = office.org_snapshot().unwrap().teams;
        assert!(teams.iter().any(|team| team.id == TEAM_A), "{engine:?}");
        let again = office.submit_org(CREATOR, team_create(1, TEAM_A, TOP_TEAM));
        assert_eq!(again.unwrap(), Ok(answered), "{engine:?}");
        office.close().unwrap();

        let office = site.open(engine).unwrap();
        assert!(office.stopped_cleanly(), "{engine:?}");
        office.close().unwrap();
    }
}

#[test]
fn a_creation_retried_after_a_crash_point_keeps_its_ca_and_its_one_entry() {
    let points = [
        CrashPoint::DuringDecide,
        CrashPoint::AfterAppend,
        CrashPoint::AfterCommit,
        CrashPoint::MaterializeTempFlushed,
        CrashPoint::MaterializeRenamed,
        CrashPoint::BeforeMark,
    ];
    for engine in ENGINES {
        for point in points {
            let context = format!("{engine:?} {point:?}");
            let site = Site::new();
            let failed = site.provision_with(engine, |prior| {
                site.faults.arm(point, 1);
                prior.map_or_else(|| office_harness::record(office_harness::CA_SEED), Ok)
            });
            assert!(failed.is_err(), "{context}");
            assert!(
                site.faults.fired(),
                "{context}: armed crash point never fired"
            );
            drop(failed);
            site.faults.revive();
            assert!(!site.folder.join("office.json").exists(), "{context}");

            let prior = Cell::new(None);
            let office = site
                .provision_with(engine, |found| {
                    prior.set(Some(found.is_some()));
                    found.map_or_else(|| office_harness::record(9), Ok)
                })
                .unwrap_or_else(|e| panic!("{context}: {e:?}"));
            let appended = point != CrashPoint::DuringDecide;
            assert_eq!(prior.get(), Some(appended), "{context}");
            let seed = if appended { office_harness::CA_SEED } else { 9 };
            let expected = ca_material(&identity().organization_id, seed);
            let public = office.hq_creation_public_record().unwrap().unwrap();
            assert_eq!(public.public_key, expected.public_key, "{context}");
            assert_eq!(office.org_snapshot().unwrap().position.seq, 1, "{context}");
            let receipts = tree(&site.durable("recovery/commands"));
            assert_eq!(receipts.len(), 1, "{context}");
            assert!(site.folder.join("office.json").is_file(), "{context}");
            office.close().unwrap();
        }
    }
}

#[test]
fn a_creation_that_dies_around_the_journal_anchor_completes_on_retry() {
    for engine in ENGINES {
        for after in [false, true] {
            let site = Site::new();
            site.faults.crash(DiskOp::Link, Target::File, 1, after);
            assert!(site.provision(engine).is_err(), "{engine:?} {after}");
            site.faults.revive();
            let anchor = org_store::journal_dir(&site.roots.durable, &identity());
            assert_eq!(anchor.join("anchor.json").exists(), after, "{engine:?}");
            let office = site.provision(engine).unwrap();
            assert_eq!(office.org_snapshot().unwrap().position.seq, 1);
            office.close().unwrap();
        }
    }
}

type Spoil = Box<dyn Fn(&Site)>;

fn rewrite_key(site: &Site, edit: impl Fn(&mut local::CaKey)) {
    let path = site.durable(local::ca_key_path(&identity().office_id).as_str());
    let mut key = local::decode_ca_key(&std::fs::read(&path).unwrap()).unwrap();
    edit(&mut key);
    std::fs::write(&path, local::encode_ca_key(&key)).unwrap();
}

#[test]
fn opening_refuses_missing_or_mismatched_ca_records() {
    let escrow = "state/security/ca-escrow.v1.bin";
    let cases: Vec<(&str, Spoil)> = vec![
        (
            "no escrow package",
            Box::new(|site| std::fs::remove_file(site.durable(escrow)).unwrap()),
        ),
        (
            "no key record",
            Box::new(|site| {
                let key = local::ca_key_path(&identity().office_id);
                std::fs::remove_file(site.durable(key.as_str())).unwrap();
            }),
        ),
        (
            "another package",
            Box::new(|site| {
                let mut bytes = std::fs::read(site.durable(escrow)).unwrap();
                *bytes.last_mut().unwrap() ^= 1;
                std::fs::write(site.durable(escrow), bytes).unwrap();
            }),
        ),
        (
            "another public key",
            Box::new(|site| {
                let other = ca_material(&identity().organization_id, 5).public_key;
                rewrite_key(site, |key| key.public_key = other);
            }),
        ),
        (
            "another organization",
            Box::new(|site| {
                rewrite_key(site, |key| {
                    key.organization_id = org_store::testkit::id(0xB2);
                });
            }),
        ),
        (
            "another operation",
            Box::new(|site| {
                rewrite_key(site, |key| key.operation_id = org_store::testkit::id(0x99));
            }),
        ),
    ];
    for (case, spoil) in cases {
        let site = Site::new();
        site.provision(EngineChoice::Sqlite)
            .unwrap()
            .close()
            .unwrap();
        spoil(&site);
        let refused = site.open(EngineChoice::Sqlite).unwrap_err();
        assert!(
            matches!(
                refused,
                OfficeError::Invalid("HQ provisioning requires recovery")
            ),
            "{case}: {refused:?}"
        );
        let called = Cell::new(false);
        let retried = site.provision_with(EngineChoice::Sqlite, |prior| {
            called.set(true);
            prior.map_or_else(|| office_harness::record(9), Ok)
        });
        assert!(retried.is_err(), "{case}");
        assert!(!called.get(), "{case}");
    }
}

#[test]
fn fatal_reports_a_flush_error() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        assert_eq!(office.fatal(), None);
        site.faults
            .arm(CrashPoint::FlushError(FlushSite::Segment), 1);
        let refused = office.submit_org(CREATOR, team_create(1, TEAM_A, TOP_TEAM));
        assert!(refused.is_err(), "{engine:?}: {refused:?}");
        let fatal = office.fatal().expect("stopped");
        assert!(fatal.starts_with("the office store stopped: "), "{fatal}");
        assert!(fatal.contains(".log"), "{fatal}");
        assert!(office.close().is_err());
    }
}

#[test]
fn the_projection_holds_no_form_of_the_ca_key() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        let wrapped =
            ca_material(&identity().organization_id, office_harness::CA_SEED).wrapped_ca_key;
        let hex: String = wrapped.iter().map(|b| format!("{b:02x}")).collect();
        let holds = |bytes: &[u8]| {
            bytes
                .windows(wrapped.len())
                .any(|w| w == wrapped.as_slice())
                || bytes.windows(hex.len()).any(|w| w == hex.as_bytes())
        };
        assert!(!holds(office.dump().join("\n").as_bytes()), "{engine:?}");
        office.close().unwrap();
        let runtime = tree(&site.roots.runtime);
        for (path, bytes) in runtime
            .into_iter()
            .chain(tree(&site.durable("state")))
            .chain(tree(&site.durable("recovery")))
        {
            assert!(!holds(&bytes), "{engine:?}: {path}");
        }
    }
}

#[test]
fn the_final_checks_stop_office_json() {
    let site = Site::new();
    let (new, binding) = office_harness::request();
    let mut options = site.options(EngineChoice::Sqlite);
    let key = site.durable(local::ca_key_path(&identity().office_id).as_str());
    options.before_final_checks = Some(Arc::new(move || {
        std::fs::remove_file(&key).unwrap();
    }));
    let refused = Office::provision_with(
        site.folder.clone(),
        &new,
        Some(Key::from_bytes([7; 32])),
        &binding,
        site.roots.clone(),
        office_harness::custody(Arc::clone(&site.custody)),
        |prior| prior.map_or_else(|| office_harness::record(office_harness::CA_SEED), Ok),
        options,
    )
    .unwrap_err();
    assert!(
        matches!(
            refused,
            OfficeError::Invalid("HQ provisioning requires recovery")
        ),
        "{refused:?}"
    );
    assert!(site.durable("state/organization.yaml").is_file());
    assert!(!site.folder.join("office.json").exists());
}

#[test]
fn dry_run_command_id_window_and_operation_lookup_enforce_b2_rules() {
    let uuid_at = |ms: u64, seq: u64| -> String {
        let hi = (ms >> 16) as u32;
        let mid = (ms & 0xffff) as u16;
        format!("{hi:08x}-{mid:04x}-7000-8000-{seq:012x}")
    };

    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        let now = CREATION_AT.0;
        let before = office.org_snapshot().unwrap();

        let dry_ok = office
            .check_org(
                CREATOR,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: String::new(),
                    team_id: TEAM_A.into(),
                    name: "Dry Team".into(),
                    owner_team_id: Some(TOP_TEAM.into()),
                }),
            )
            .unwrap()
            .unwrap();
        assert_eq!(dry_ok.status, CommandStatus::Checked);
        assert_eq!(dry_ok.position, None);
        assert_eq!(office.org_snapshot().unwrap(), before);

        let expired_id = uuid_at(now - wire::MAX_COMMAND_AGE_MS - 1, 2);
        let expired = office
            .submit_org(
                CREATOR,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: expired_id.clone(),
                    team_id: TEAM_A.into(),
                    name: "Too Old".into(),
                    owner_team_id: Some(TOP_TEAM.into()),
                }),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(expired.code, ErrorCode::Expired);
        assert_eq!(
            office
                .operation(CREATOR, &expired_id)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        let committed_id = uuid_at(now - wire::MAX_COMMAND_AGE_MS, 4);
        let committed = office
            .submit_org(
                CREATOR,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: committed_id.clone(),
                    team_id: TEAM_A.into(),
                    name: "Platform".into(),
                    owner_team_id: Some(TOP_TEAM.into()),
                }),
            )
            .unwrap()
            .unwrap();
        assert_eq!(committed.status, CommandStatus::Committed);
        let looked = office.operation(CREATOR, &committed_id).unwrap().unwrap();
        assert_eq!(
            looked,
            OperationOutcome::Committed {
                result: serde_json::to_value(committed).unwrap(),
            }
        );
        assert_eq!(
            office
                .operation(HR, &committed_id)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        let rejected_id = uuid_at(now + wire::MAX_COMMAND_SKEW_MS, 5);
        let refusal = office
            .submit_org(
                CREATOR,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: rejected_id.clone(),
                    team_id: TEAM_B.into(),
                    name: "   ".into(),
                    owner_team_id: Some(TOP_TEAM.into()),
                }),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(refusal.code, ErrorCode::InvalidName);
        assert_eq!(
            office.operation(CREATOR, &rejected_id).unwrap().unwrap(),
            OperationOutcome::Rejected { refusal }
        );
        office.close().unwrap();
    }
}
