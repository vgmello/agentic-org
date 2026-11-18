use super::*;
use crate::{
    accounts::identity::LoginKey,
    broadcast::Broadcast,
    catalog::Catalog,
    server_check::ServerCheck,
    supervisor::{Supervisor, SupervisorConfig},
    Timings,
};
use org_host_control::SessionHandle;
use org_proto::selected_bootstrap::PlanV1;
use std::os::unix::fs::PermissionsExt;

fn plan() -> PlanV1 {
    PlanV1 {
        version: 1,
        name: "Example".into(),
        slug: "example".into(),
        repository_remote_url: "https://example.test/organization.git".into(),
        state_branch: "org/state".into(),
        home_team_name: "Core".into(),
    }
}
fn params() -> Params {
    Params {
        session: SessionHandle::new("unused-by-storage-fixture".into()).unwrap(),
        operation_id: crate::ids::generate().unwrap(),
        plan: plan(),
    }
}
fn account(db: &HostDb) -> Account {
    let c = db
        .account_credentials(&LoginKey::new("Owner").unwrap())
        .unwrap()
        .unwrap();
    let id = crate::accounts::identity::AccountId::parse(c.account().account_id()).unwrap();
    db.account_authority(&id).unwrap().unwrap()
}
fn offices(
    root: &Path,
    keys: Arc<dyn KeyStore>,
    writers: Arc<Writers>,
    instance: &str,
    check: ServerCheck,
    program: PathBuf,
) -> Arc<Offices> {
    for name in ["offices", "logs", "run"] {
        private(&root.join(name)).unwrap();
    }
    let supervisor = Supervisor::new(SupervisorConfig {
        program,
        home: root.into(),
        runtime_dir: Some(root.join("run")),
        logs_dir: root.join("logs"),
        log_max_bytes: 1024 * 1024,
        timings: Timings::default(),
        keys,
        server_check: Arc::new(check),
        sweep_orphans: false,
    })
    .with_v3(instance)
    .with_writers(writers);
    Offices::start(
        root,
        Catalog::default(),
        vec![],
        supervisor,
        Arc::new(Broadcast::default()),
        &Timings::default(),
    )
}
#[test]
fn failed_worker_retains_bound_code_key_owner_and_writer_and_never_replaces_missing_key() {
    let instance = crate::ids::generate().unwrap();
    let (dir, auth, _) = crate::service::authentication::tests::fixture_for(&instance);
    let root = dir.path().canonicalize().unwrap();
    let (db, custody) = auth.provisioning_storage();
    let keys = Arc::new(crate::keys::memory::MemoryKeys::default());
    let writers = Writers::open(custody.clone(), db.clone(), &instance, &root).unwrap();
    let office_home = root.join("office-state");
    private(&office_home).unwrap();
    let offices = offices(
        &office_home,
        keys.clone(),
        writers.clone(),
        &instance,
        ServerCheck::Refuse,
        root.join("absent"),
    );
    let controller = Provisioning::new(
        db.clone(),
        custody.clone(),
        keys.clone(),
        writers.clone(),
        &instance,
        &root,
        &office_home,
    );
    let p = params();
    let actor = account(&db);
    assert!(controller.run(&actor, &p, &offices).is_err());
    let fp = journal::fingerprint(&p).unwrap();
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let r = db
        .provisioning(&actor, &p.operation_id, &fp, wall)
        .unwrap()
        .unwrap();
    let key = keys.load(&r.prepared.office_id).unwrap().unwrap();
    let before = fingerprint(&key);
    drop(key);
    assert_eq!(r.key_fingerprint.as_ref(), Some(&before));
    assert!(!r.linked);
    db.verify_selected_owner(&r.prepared.organization_id, &r.account, &r.owner_member)
        .unwrap();
    assert!(writers.required(&r.prepared.office_id).unwrap());
    assert!(crate::catalog::unfinished_folders(&office_home)
        .unwrap()
        .is_empty());
    let handle = Name::parse(&r.recovery_handle).unwrap();
    let staged = custody.load(&handle).unwrap().unwrap();
    assert!(controller.run(&actor, &p, &offices).is_err());
    assert_eq!(
        custody.load(&handle).unwrap().unwrap().expose(),
        staged.expose()
    );
    assert_eq!(
        fingerprint(&keys.load(&r.prepared.office_id).unwrap().unwrap()),
        before
    );
    keys.delete(&r.prepared.office_id).unwrap();
    assert!(controller.run(&actor, &p, &offices).is_err());
    assert!(keys.load(&r.prepared.office_id).unwrap().is_none());
    // Another plan never attaches to the original code/IDs.
    let mut other = params();
    other.operation_id = p.operation_id.clone();
    other.plan.slug = "different".into();
    assert!(controller.run(&actor, &other, &offices).is_err());
    assert_eq!(
        db.provisioning_offices().unwrap(),
        [r.prepared.office_id].into()
    );
    offices.shutdown();
}

fn confirm_pending_checkpoint(writers: &Arc<Writers>, office: &str) {
    use org_office_launch::writer::{self as wire, Request, Response};
    use org_proto::work::JournalPosition;
    use std::{io::BufReader, os::unix::net::UnixStream, sync::atomic::AtomicBool};
    let (host, worker) = UnixStream::pair().unwrap();
    writers
        .serve(office, host, Arc::new(AtomicBool::new(true)))
        .unwrap();
    let mut worker = BufReader::new(worker);
    let admission: wire::Admission = wire::read(&mut worker).unwrap().unwrap();
    let checkpoint = JournalPosition {
        ordinal: 0,
        sha256: "a".repeat(64),
        previous_sha256: None,
        origin_incarnation: "retained-origin".into(),
        origin_high_water: 73,
    };
    wire::write(
        worker.get_mut(),
        &Request::Confirm {
            identity: admission.identity.clone(),
            before: None,
            after: checkpoint,
        },
    )
    .unwrap();
    assert!(matches!(
        wire::read::<Response>(&mut worker).unwrap(),
        Some(Response::Confirmed)
    ));
}

fn custody_snapshot(custody: &dyn HostSecrets) -> Vec<(Name, Vec<u8>)> {
    custody
        .list()
        .unwrap()
        .into_iter()
        .map(|name| {
            let bytes = custody.load(&name).unwrap().unwrap().expose().to_vec();
            (name, bytes)
        })
        .collect()
}

fn pending_registry_init(instance: &str) -> crate::hostdb::HostDbInit {
    use crate::hostdb::{BootstrapRecord, FirstAccount, HostDbInit};
    let account = crate::ids::generate().unwrap();
    HostDbInit {
        instance_id: instance.into(),
        clock_high_ms: org_proto::ulid_time_ms(&account).unwrap() as i64,
        bootstrap: BootstrapRecord {
            operation_id: crate::ids::generate().unwrap(),
            hq_operation_id: crate::ids::generate().unwrap(),
            fingerprint: "pending recovery fixture".into(),
            account: account.clone(),
            finished: false,
        },
        first_account: FirstAccount::new(
            &account,
            "Owner",
            crate::service::authentication::hash_password(
                &org_host_control::Password::new("synthetic pending recovery password".into())
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap(),
        tls_pair: None,
    }
}

#[test]
fn pending_recovery_survives_registry_loss_before_writer_reservation() {
    use crate::host_secrets::dev::DevSecrets;
    let directory = tempfile::tempdir().unwrap();
    let base = tempfile::tempdir().unwrap();
    for path in [directory.path(), base.path()] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let root = directory.path().canonicalize().unwrap();
    let instance = crate::ids::generate().unwrap();
    let (reservation, frozen, credentials) = {
        let custody = Arc::new(DevSecrets::open(base.path(), &root, &instance).unwrap());
        let db =
            Arc::new(HostDb::create(&root, &*custody, &pending_registry_init(&instance)).unwrap());
        let writers = Writers::open(custody.clone(), db.clone(), &instance, &root).unwrap();
        let controller = Provisioning::new(
            db.clone(),
            custody.clone(),
            Arc::new(crate::keys::memory::MemoryKeys::default()),
            writers,
            &instance,
            &root,
            &root,
        );
        let p = params();
        let actor = account(&db);
        let handle = Name::parse(&format!("recovery.{}", p.operation_id)).unwrap();
        // Stop at the actual production phase after custody/SQL reservation,
        // before reserve_writer can allocate any writer or device identity.
        let pending = controller
            .new_pending(&actor, &p, journal::fingerprint(&p).unwrap(), &handle)
            .unwrap();
        let reservation = db
            .reserve_provisioning(&actor, &pending.reservation)
            .unwrap();
        let frozen = custody.load(&handle).unwrap().unwrap().expose().to_vec();
        assert!(frozen == serde_json::to_vec(&pending).unwrap());
        custody
            .put(
                &Name::parse(&format!("tls.{}", "cd".repeat(32))).unwrap(),
                &Value::new(b"unrelated retained TLS key".to_vec()).unwrap(),
            )
            .unwrap();
        let credentials = custody_snapshot(&*custody);
        assert_eq!(credentials.len(), 2);
        assert!(credentials.iter().all(|(name, _)| {
            name.as_str() != "writer-device" && !name.as_str().starts_with("writer.")
        }));
        (reservation, frozen, credentials)
    }; // Close the registry and all custody/controller/writer handles before loss.
    let original_registry = std::fs::read(root.join("host-registry.db")).unwrap();
    assert!(original_registry.starts_with(b"SQLite format 3\0"));
    std::fs::rename(
        root.join("host-registry.db"),
        root.join("retained-registry.db"),
    )
    .unwrap();
    let retained: Vec<_> = [
        "host-registry.db-wal",
        "host-registry.db-shm",
        "host-registry.db-journal",
        "host-registry.db.tmp",
        "host-registry.db.tmp-wal",
        "host-registry.db.tmp-shm",
        "host-registry.db.tmp-journal",
    ]
    .into_iter()
    .map(|name| {
        let bytes = format!("retained {name}").into_bytes();
        std::fs::write(root.join(name), &bytes).unwrap();
        (name, bytes)
    })
    .collect();
    let custody = DevSecrets::open(base.path(), &root, &instance).unwrap();
    assert!(custody_snapshot(&custody) == credentials);
    let replacement = pending_registry_init(&instance);
    assert_ne!(replacement.first_account.account_id(), reservation.account);
    assert!(matches!(
        HostDb::create(&root, &custody, &replacement),
        Err(crate::hostdb::Error::Unsafe)
    ));
    assert!(!root.join("host-registry.db").exists());
    assert!(custody_snapshot(&custody) == credentials);
    let actual = custody
        .load(&Name::parse(&reservation.recovery_handle).unwrap())
        .unwrap()
        .unwrap();
    assert!(actual.expose() == frozen);
    let original: Pending = serde_json::from_slice(&frozen).unwrap();
    assert_eq!(original.reservation.prepared, reservation.prepared);
    assert!(pending_recovery(&custody, &reservation).unwrap().expose() == original.code.expose());
    assert_eq!(
        std::fs::read(root.join("retained-registry.db")).unwrap(),
        original_registry
    );
    for (name, bytes) in retained {
        assert_eq!(std::fs::read(root.join(name)).unwrap(), bytes);
    }
}

#[test]
fn pending_provisioning_retry_refuses_lost_writer_with_or_without_marker_and_device() {
    use sha2::{Digest, Sha256};
    for keep_marker in [false, true] {
        let instance = crate::ids::generate().unwrap();
        let (dir, auth, _) = crate::service::authentication::tests::fixture_for(&instance);
        let root = dir.path().canonicalize().unwrap();
        let (db, custody) = auth.provisioning_storage();
        let keys = Arc::new(crate::keys::memory::MemoryKeys::default());
        let writers = Writers::open(custody.clone(), db.clone(), &instance, &root).unwrap();
        let home = root.join("office-state");
        private(&home).unwrap();
        let offices = offices(
            &home,
            keys.clone(),
            writers.clone(),
            &instance,
            ServerCheck::Refuse,
            root.join("absent"),
        );
        let controller = Provisioning::new(
            db.clone(),
            custody.clone(),
            keys.clone(),
            writers.clone(),
            &instance,
            &root,
            &home,
        );
        let p = params();
        let actor = account(&db);
        assert!(controller.run(&actor, &p, &offices).is_err());
        let fp = journal::fingerprint(&p).unwrap();
        let wall = org_proto::ulid_time_ms(&p.operation_id).unwrap() as i64;
        let reservation = db
            .provisioning(&actor, &p.operation_id, &fp, wall)
            .unwrap()
            .unwrap();
        assert!(!reservation.linked);
        let office = &reservation.prepared.office_id;
        let hash = org_office_launch::recovery::hex(&Sha256::digest(office.as_bytes()));
        let credential = Name::parse(&format!("writer.{hash}")).unwrap();
        confirm_pending_checkpoint(&writers, office);
        let original = custody.load(&credential).unwrap().unwrap();
        let marker = home
            .join("offices")
            .join(office)
            .join("selected-writer.required");
        if !keep_marker {
            std::fs::remove_file(&marker).unwrap();
        }
        let material = [
            root.join("durable/authority")
                .join(office)
                .join("checkpoint-evidence"),
            root.join("durable/state")
                .join(&reservation.prepared.organization_id)
                .join("public-package"),
            home.join("offices")
                .join(office)
                .join("private-office-state"),
        ];
        for path in &material {
            std::fs::write(path, b"retained exact material").unwrap();
        }
        custody.delete(&credential).unwrap();
        if !keep_marker {
            custody
                .delete(&Name::parse("writer-device").unwrap())
                .unwrap();
        }
        let before = custody_snapshot(&*custody);
        let key_before = fingerprint(&keys.load(office).unwrap().unwrap());
        assert!(controller.run(&actor, &p, &offices).is_err());
        assert!(
            custody.load(&credential).unwrap().is_none(),
            "lost writer was recreated"
        );
        let after = custody_snapshot(&*custody);
        assert_eq!(before, after);
        assert_eq!(
            fingerprint(&keys.load(office).unwrap().unwrap()),
            key_before
        );
        assert_eq!(marker.exists(), keep_marker);
        for path in &material {
            assert_eq!(std::fs::read(path).unwrap(), b"retained exact material");
        }
        let retained: serde_json::Value = serde_json::from_slice(original.expose()).unwrap();
        assert_eq!(retained["position"]["origin_high_water"], 73);
        assert!(
            !db.provisioning(&actor, &p.operation_id, &fp, wall)
                .unwrap()
                .unwrap()
                .linked
        );
        offices.shutdown();
    }
}

#[test]
fn selected_reservation_checks_account_fingerprint_age_and_live_admin() {
    let instance = crate::ids::generate().unwrap();
    let (_root, auth, _) = crate::service::authentication::tests::fixture_for(&instance);
    let (db, _) = auth.provisioning_storage();
    let actor = account(&db);
    let p = params();
    let fp = journal::fingerprint(&p).unwrap();
    let prepared = PreparedV1 {
        plan: p.plan.clone(),
        organization_id: crate::ids::uuid_v7().unwrap(),
        office_id: crate::ids::uuid_v7().unwrap(),
        repository_id: crate::ids::uuid_v7().unwrap(),
        home_team_id: crate::ids::uuid_v7().unwrap(),
        hr_employee_id: crate::ids::uuid_v7().unwrap(),
    };
    let r = Reservation {
        operation_id: p.operation_id.clone(),
        account: actor.account_id().into(),
        fingerprint: fp,
        prepared,
        owner_member: crate::ids::generate().unwrap(),
        recovery_handle: format!("recovery.{}", p.operation_id),
        key_fingerprint: None,
        linked: false,
    };
    db.reserve_provisioning(&actor, &r).unwrap();
    assert_eq!(
        db.reserve_provisioning(&actor, &r).unwrap().prepared,
        r.prepared
    );
    let wall = org_proto::ulid_time_ms(&r.operation_id).unwrap() as i64;
    assert!(matches!(
        db.provisioning(&actor, &r.operation_id, &[0; 32], wall),
        Err(crate::hostdb::Error::OperationReused)
    ));
    assert!(matches!(
        db.provisioning(&actor, &r.operation_id, &fp, wall + 30 * 86_400_000),
        Err(crate::hostdb::Error::Expired)
    ));
    // Retention is explicit even when the ordinary retry window has expired.
    assert!(db
        .provisioning_offices()
        .unwrap()
        .contains(&r.prepared.office_id));
    db.change_authority_fixture(
        &crate::accounts::identity::AccountId::parse(actor.account_id()).unwrap(),
        true,
    )
    .unwrap();
    assert!(matches!(
        db.reserve_provisioning(&actor, &r),
        Err(crate::hostdb::Error::Unsafe)
    ));
}

#[test]
#[cfg(target_os = "macos")]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN; actual worker rendezvous requires permitted local bind"]
fn actual_selected_hq_provisioning_native_route_and_exact_retry() {
    use crate::service::{
        listener::{Listener, ListenerConfig},
        Service,
    };
    use org_proto::{frame, Call, Hello, HelloParams};
    use std::{
        io::BufReader,
        os::unix::net::UnixStream,
        time::{Duration, Instant},
    };
    let binary = std::env::var_os("ORG_TEST_OFFICE_BIN").expect("explicit built office binary");
    let instance = crate::ids::generate().unwrap();
    let (_account_root, auth, _) = crate::service::authentication::tests::fixture_for(&instance);
    let (db, custody) = auth.provisioning_storage();
    let auth = Arc::new(auth);
    let dir = tempfile::tempdir_in("/private/tmp").unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let keys = Arc::new(crate::keys::memory::MemoryKeys::default());
    let writers = Writers::open(custody.clone(), db.clone(), &instance, &root).unwrap();
    let office_home = root.join("office-state");
    private(&office_home).unwrap();
    let offices = offices(
        &office_home,
        keys.clone(),
        writers.clone(),
        &instance,
        ServerCheck::Skip,
        binary.into(),
    );
    let controller = Arc::new(Provisioning::new(
        db.clone(),
        custody,
        keys,
        writers,
        &instance,
        &root,
        &office_home,
    ));
    let service = Arc::new(
        Service::new(
            offices.clone(),
            Arc::new(Broadcast::default()),
            Some(instance.clone()),
        )
        .with_authentication(Some(auth))
        .with_provisioning(Some(controller)),
    );
    let socket = root.join("host.sock");
    let bound = crate::service::listener::bind(&socket).unwrap();
    let listener = Listener::start(
        bound,
        ListenerConfig {
            max_connections: 4,
            max_in_flight: 4,
            expected_uid: crate::home::euid(),
        },
        service,
    )
    .unwrap();
    struct Cleanup(Option<Listener>, Arc<Offices>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Some(mut l) = self.0.take() {
                l.close();
                l.join();
            }
            self.1.shutdown();
        }
    }
    let _cleanup = Cleanup(Some(listener), offices.clone());
    let stream = UnixStream::connect(&socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut wire = BufReader::new(stream);
    fn call<C: Call>(wire: &mut BufReader<UnixStream>, id: u64, p: &C::Params) -> C::Result {
        frame::write_message(
            wire.get_mut(),
            &org_proto::Request::new::<C>(id, p).unwrap(),
        )
        .unwrap();
        let response: org_proto::Response = frame::read_message(wire).unwrap().unwrap();
        assert_eq!(response.id, id);
        response.into_result::<C>().unwrap()
    }
    let _: org_proto::HelloResult = call::<Hello>(&mut wire, 1, &HelloParams::default());
    let login = call::<org_host_control::ops::AccountVerify>(
        &mut wire,
        2,
        &org_host_control::ops::AccountVerifyParams {
            login: "Owner".into(),
            password: org_host_control::Password::new("synthetic test password".into()).unwrap(),
        },
    );
    assert!(login.admin);
    assert!(login.orgs.is_empty());
    let mut p = params();
    p.session = login.session;
    let deadline = Instant::now() + Duration::from_secs(25);
    let mut id = 3;
    let result = loop {
        let answer = call::<org_host_control::provision::OrganizationProvision>(&mut wire, id, &p);
        id += 1;
        match answer {
            HostOpResult::Completed { result } => break result,
            HostOpResult::Accepted => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => panic!("unexpected provisioning result"),
        }
    };
    assert_eq!(result.remote_readiness, RemoteReadiness::Unverified);
    let state = root.join("durable/state").join(&result.organization_id);
    let initial = std::fs::read(state.join("organization.yaml")).unwrap();
    assert!(!initial.is_empty());
    let replay = call::<org_host_control::provision::OrganizationProvision>(&mut wire, id, &p);
    assert!(matches!(replay,HostOpResult::Completed{result:r} if r==result));
    let inspection = call::<org_host_control::ops::SessionInspect>(
        &mut wire,
        id + 1,
        &org_host_control::ops::SessionParams { session: p.session },
    );
    assert_eq!(inspection.orgs.len(), 1);
    assert!(!inspection.orgs[0].member_id.is_empty());
    // Exact public bytes and identities, no optional owner employee or credentials.
    assert_eq!(
        std::fs::read(state.join("organization.yaml")).unwrap(),
        initial
    );
    assert!(state.join("security/ca-escrow.v1.bin").is_file());
}
