use super::*;
use crate::keys::memory::MemoryKeys;
use crate::supervisor::SupervisorConfig;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn a_stored_folder_is_never_handed_to_discard() {
    // Under /tmp, with `run/`: the launch rendezvous path must fit a socket address.
    let dir = tempfile::Builder::new()
        .prefix("of")
        .tempdir_in("/tmp")
        .unwrap();
    let home = dir.path();
    fs::create_dir(home.join("run")).unwrap();
    // A server that never refuses: it records each run and exits. Only the host's own check
    // can keep a stored folder from it. It is a `#!` script, started by its path (the test
    // admission): a system that executes verified bytes starts no script.
    let script = home.join("server");
    fs::write(
        &script,
        "#!/bin/sh\necho \"$@\" >> \"$ORG_OFFICE_HOME/ran\"\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let supervisor = Supervisor::new(
        SupervisorConfig {
            program: script,
            home: home.to_path_buf(),
            runtime_dir: None,
            logs_dir: home.join("logs"),
            log_max_bytes: 1024,
            timings: Timings::default(),
            keys: Arc::new(MemoryKeys::default()),
            server_check: Arc::new(crate::server_check::ServerCheck::ByPath),
            sweep_orphans: false,
            federation: None,
        },
        INSTANCE,
    );
    const STORED: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071d1";
    const UNFINISHED: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071d2";
    for (id, stored) in [(STORED, true), (UNFINISHED, false)] {
        let folder = home.join("offices").join(id);
        fs::create_dir_all(&folder).unwrap();
        if stored {
            fs::write(folder.join(org_office_launch::OFFICE_FILE_NAME), b"{}").unwrap();
        }
        discard_unfinished(home, &supervisor, id);
    }
    let ran = fs::read_to_string(home.join("ran")).unwrap_or_default();
    assert_eq!(ran, format!("--discard {UNFINISHED}\n"));
}

const OFFICE: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0";

fn hq() -> OfficeFile {
    OfficeFile::new(
        OFFICE,
        "HQ",
        org_proto::OfficeRole::Hq,
        "01ORGA",
        "Acme",
        "2026-11-08T00:00:00.000Z",
    )
}

/// Offices over `catalog` in `home`, whose server program doesn't exist (nothing here needs
/// it to run). With `stopped`, the supervisor is shut down first, so it can't start anything.
fn offices(home: &Path, catalog: Catalog, stopped: bool) -> Arc<Offices> {
    let supervisor = Supervisor::new(
        SupervisorConfig {
            program: home.join("no-such-server"),
            home: home.to_path_buf(),
            runtime_dir: None,
            logs_dir: home.join("logs"),
            log_max_bytes: 1024,
            timings: Timings::default(),
            keys: Arc::new(MemoryKeys::default()),
            server_check: Arc::new(crate::server_check::ServerCheck::Skip),
            sweep_orphans: false,
            federation: None,
        },
        INSTANCE,
    );
    if stopped {
        supervisor.shutdown();
    }
    let broadcast = Arc::new(Broadcast::default());
    Offices::start(
        home,
        catalog,
        Vec::new(),
        supervisor,
        broadcast,
        &Timings::default(),
    )
}

/// At startup, after the discards, the keys of offices with no folder are swept, but only
/// when the host may (`sweep_orphans`): one marked 7 days or more ago is deleted, a new one
/// is only marked, and a key whose folder exists is always kept.
#[test]
fn the_startup_sweeps_orphan_keys_only_when_allowed() {
    use crate::keys::OfficeSecretsKeys;
    use org_proto::OfficeSecretsKey;
    const ORPHAN: &str = "01ORPHANORPHANORPHANORPHAN";
    const NEW_ORPHAN: &str = "01NEWORPHANNEWORPHANNEWORP";
    for sweep in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        // Two orphans beside three live offices (more than half orphaned isn't swept).
        const LIVE: [&str; 3] = [
            OFFICE,
            "01LIVEBLIVEBLIVEBLIVEBLIVE",
            "01LIVECLIVECLIVECLIVECLIVE",
        ];
        let keys = Arc::new(MemoryKeys::default());
        for id in LIVE {
            fs::create_dir_all(home.join("offices").join(id)).unwrap();
        }
        for id in LIVE.into_iter().chain([ORPHAN, NEW_ORPHAN]) {
            keys.create(id, &OfficeSecretsKey::from_bytes([1; 32]))
                .unwrap();
        }
        // ORPHAN was marked long ago; NEW_ORPHAN never was.
        let record = home.join("orphan-keys.json");
        let marked = format!("{{\"v\":1,\"first_seen_ms\":{{\"{ORPHAN}\":1000}}}}");
        fs::write(&record, &marked).unwrap();
        let supervisor = Supervisor::new(
            SupervisorConfig {
                program: home.join("no-such-server"),
                home: home.to_path_buf(),
                runtime_dir: None,
                logs_dir: home.join("logs"),
                log_max_bytes: 1024,
                timings: Timings::default(),
                keys: Arc::clone(&keys) as Arc<dyn OfficeSecretsKeys>,
                server_check: Arc::new(crate::server_check::ServerCheck::Skip),
                sweep_orphans: sweep,
                federation: None,
            },
            INSTANCE,
        );
        let offices = Offices::start(
            home,
            Catalog::default(),
            Vec::new(),
            supervisor,
            Arc::new(Broadcast::default()),
            &Timings::default(),
        );
        offices
            .wait_for_startup(Instant::now() + Duration::from_secs(10))
            .unwrap();
        assert_eq!(keys.has(ORPHAN), !sweep, "sweep {sweep}");
        // Never deleted on first sight, only marked.
        assert!(keys.has(NEW_ORPHAN) && keys.has(OFFICE), "sweep {sweep}");
        let now_marked = fs::read_to_string(&record).unwrap();
        if sweep {
            assert!(now_marked.contains(NEW_ORPHAN) && !now_marked.contains(ORPHAN));
        } else {
            assert_eq!(now_marked, marked);
        }
        offices.shutdown();
    }
}

/// `office.restart` on an office the host couldn't supervise, when it still can't: listed
/// `unavailable`, and the restart answers `unavailable` too.
#[test]
fn restarting_an_office_the_host_still_cant_supervise_is_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = Catalog::default();
    catalog.insert(hq());
    let offices = offices(dir.path(), catalog, true);
    assert_eq!(offices.state_of(OFFICE).status, OfficeStatus::Unavailable);
    let err = offices.restart_office(OFFICE).unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert!(offices.supervisor.state(OFFICE).is_none());
    offices.shutdown();
}

/// `office.restart` on an office the host doesn't supervise, when it now can: supervision
/// starts. (The office is put in the catalog after the start, so supervision never began for
/// it: the state an office is left in when the host couldn't supervise it.)
#[test]
fn restarting_an_office_the_host_couldnt_supervise_starts_it() {
    let dir = tempfile::tempdir().unwrap();
    let offices = offices(dir.path(), Catalog::default(), false);
    lock(&offices.catalog).insert(hq());
    assert!(offices.supervisor.state(OFFICE).is_none());
    assert_eq!(offices.state_of(OFFICE).status, OfficeStatus::Unavailable);
    offices.restart_office(OFFICE).unwrap();
    assert!(offices.supervisor.state(OFFICE).is_some());
    offices.shutdown();
}

/// Regression test (TASK-38 review #1): when a multi-office org owner handover fails while
/// updating the second office's writer custody, `HostDb::org_owners` MUST be restored BEFORE
/// reverting office 1's writer custody so `Writers::verify` succeeds and office 1 stays
/// admittable under the previous owner.
#[test]
fn org_owner_handover_rollback_restores_db_before_reverting_earlier_offices() {
    use crate::accounts::identity::AccountId;
    use crate::accounts::rules::Username;
    use crate::accounts::store::NewAccount;
    use crate::hostdb::journal::Verifier;
    use crate::hostdb::multi_person::{InviteRegistration, OrgOwnerHandoverOutcome};
    use crate::hostdb::HostAccountsStore;

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let office1 = crate::ids::uuid_v7().unwrap();
    let office2 = crate::ids::uuid_v7().unwrap();
    // Reserve writer custody ONLY for office1; office2 intentionally lacks custody so
    // `update_owner` fails on office2 after succeeding on office1.
    let custody = crate::writer::TestCustody::reserve(home, INSTANCE, &[&office1]);
    let org_id = custody.organization.clone();
    let owner_id = AccountId::parse(&custody.account).unwrap();
    let owner_member = custody.member.clone();

    // Create and link Sam in the same organization as the handover target.
    let sam_member = crate::ids::uuid_v7().unwrap();
    let invite = custody
        .db
        .issue_org_invite(&org_id, &sam_member, 10_000)
        .unwrap();
    let verifier = Verifier::new(
        "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
    )
    .unwrap();
    let new_sam = NewAccount::new(Username::new("Sam").unwrap(), verifier).unwrap();
    let sam_id = match custody
        .db
        .redeem_invite_register(&invite.token, &new_sam, 10_100)
        .unwrap()
    {
        InviteRegistration::Registered { account_id, .. } => account_id,
        other => panic!("unexpected invite registration: {other:?}"),
    };

    // Catalog lists office1 (HQ, ordered first) and office2 (Satellite, ordered second).
    let mut catalog = Catalog::default();
    catalog.insert(OfficeFile::new(
        &office1,
        "HQ",
        org_proto::OfficeRole::Hq,
        &org_id,
        "Acme",
        "2026-11-08T00:00:00.000Z",
    ));
    catalog.insert(OfficeFile::new(
        &office2,
        "Satellite",
        org_proto::OfficeRole::Satellite,
        &org_id,
        "Acme",
        "2026-11-08T00:00:01.000Z",
    ));
    let supervisor = Supervisor::new(
        SupervisorConfig {
            program: home.join("no-such-server"),
            home: home.to_path_buf(),
            runtime_dir: None,
            logs_dir: home.join("logs"),
            log_max_bytes: 1024,
            timings: Timings::default(),
            keys: Arc::new(MemoryKeys::default()),
            server_check: Arc::new(crate::server_check::ServerCheck::Skip),
            sweep_orphans: false,
            federation: None,
        },
        INSTANCE,
    )
    .with_writers(Arc::clone(&custody.writers));
    supervisor.shutdown();
    let offices = Offices::start(
        home,
        catalog,
        Vec::new(),
        supervisor,
        Arc::new(Broadcast::default()),
        &Timings::default(),
    );

    let transferred = custody
        .db
        .handover_org_owner(&org_id, &owner_id, &sam_member)
        .unwrap();
    assert_eq!(
        transferred,
        OrgOwnerHandoverOutcome::Transferred {
            previous_member_id: owner_member.clone(),
            to_account_id: sam_id.clone(),
            to_member_id: sam_member.clone(),
        }
    );

    let err = offices
        .reload_org_offices(
            &org_id,
            owner_id.as_str(),
            &owner_member,
            sam_id.as_str(),
            &sam_member,
            || {
                custody
                    .db
                    .restore_org_owner(&org_id, &owner_id, &owner_member)
            },
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::StorageError);

    // Both HostDb::org_owners AND office1's writer custody must be back on the previous owner
    // and office1 must still pass `admit()` / `verify()`.
    assert_eq!(
        custody.db.org_owner(&org_id).unwrap(),
        Some((owner_id, owner_member.clone()))
    );
    let bound = custody.writers.admit(&office1).unwrap();
    assert_eq!(bound.owner_member, owner_member);
    offices.shutdown();
}
