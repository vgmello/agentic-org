use super::*;
use crate::host_secrets::{HostSecrets, Name, Value};
use crate::hostdb::accounts::Account;
use crate::hostdb::provisioning::Reservation;
use crate::hostdb::HostDb;
use crate::keys::OfficeSecretsKeys;
use crate::offices::Offices;
use crate::writer::Writers;
use crate::{
    accounts::identity::LoginKey,
    broadcast::Broadcast,
    catalog::Catalog,
    server_check::ServerCheck,
    supervisor::{Supervisor, SupervisorConfig},
    Timings,
};
use org_host_control::HostOpResult;
use org_proto::selected_bootstrap::{PlanV1, PreparedV1};
use sha2::{Digest, Sha256};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn plan() -> PlanV1 {
    PlanV1 {
        version: 1,
        name: "Example".into(),
        slug: "example".into(),
        repository_remote_url: "https://example.test/organization.git".into(),
        state_branch: "org/state".into(),
    }
}
/// One `organization.create` as the service hands it over.
struct Params {
    operation_id: String,
    plan: PlanV1,
}
fn params() -> Params {
    Params {
        operation_id: crate::ids::uuid_v7().unwrap(),
        plan: plan(),
    }
}
fn fp(p: &Params) -> [u8; 32] {
    Sha256::digest(serde_json::to_vec(&p.plan).unwrap()).into()
}
/// Asks for the create, waits for the attempt it started to end and says whether the
/// organization is linked.
fn creates(controller: &Arc<Provisioning>, actor: &Account, p: &Params) -> bool {
    if controller
        .create(actor, &p.operation_id, &p.plan, fp(p))
        .is_err()
    {
        return false;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while controller.driving_now() != 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "a creation never ended"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    controller
        .db
        .provisioning_lookup(actor.account_id(), &p.operation_id)
        .unwrap()
        .is_some_and(|r| r.linked)
}
fn account(db: &dyn HostStore) -> Account {
    let c = db
        .account_credentials(&LoginKey::new("Owner").unwrap())
        .unwrap()
        .unwrap();
    let id = crate::accounts::identity::AccountId::parse(c.account().account_id()).unwrap();
    db.account_authority(&id).unwrap().unwrap()
}
fn offices(
    root: &Path,
    keys: Arc<dyn OfficeSecretsKeys>,
    writers: Arc<Writers>,
    instance: &str,
    check: ServerCheck,
    program: PathBuf,
) -> Arc<Offices> {
    offices_in(
        root,
        &root.join("run"),
        keys,
        writers,
        instance,
        check,
        program,
    )
}
/// As [`offices`], with the launch rendezvous in `runtime` (short enough for a socket path).
fn offices_in(
    root: &Path,
    runtime: &Path,
    keys: Arc<dyn OfficeSecretsKeys>,
    writers: Arc<Writers>,
    instance: &str,
    check: ServerCheck,
    program: PathBuf,
) -> Arc<Offices> {
    for name in ["offices", "logs"] {
        private(&root.join(name)).unwrap();
    }
    private(runtime).unwrap();
    let supervisor = Supervisor::new(
        SupervisorConfig {
            program,
            home: root.into(),
            runtime_dir: Some(runtime.into()),
            logs_dir: root.join("logs"),
            log_max_bytes: 1024 * 1024,
            timings: Timings::default(),
            keys,
            server_check: Arc::new(check),
            sweep_orphans: false,
            federation: None,
        },
        instance,
    )
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
    let instance = crate::ids::uuid_v7().unwrap();
    let (dir, auth, _) = crate::authentication::tests::fixture_for(&instance);
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
    let controller = Arc::new(Provisioning::new(
        db.clone(),
        custody.clone(),
        keys.clone(),
        writers.clone(),
        &instance,
        &office_home,
    ));
    controller.start(offices.clone());
    let p = params();
    let actor = account(&*db);
    assert!(!creates(&controller, &actor, &p));
    let fp = fp(&p);
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
    // The creator is the account's person: a UUIDv7 that is also the linked member id,
    // named after the username, with the username as a valid handle.
    assert!(org_proto::is_uuid_v7(&r.owner_member));
    assert_eq!(r.prepared.creator.employee_id, r.owner_member);
    assert_eq!(r.prepared.creator.name, actor.login());
    assert_eq!(r.prepared.creator.handle, "owner-1");
    writers.admit(&r.prepared.office_id).unwrap();
    assert!(crate::catalog::unfinished_folders(&office_home)
        .unwrap()
        .is_empty());
    let handle = Name::parse(&r.recovery_handle).unwrap();
    let staged = custody.load(&handle).unwrap().unwrap();
    assert!(!creates(&controller, &actor, &p));
    assert_eq!(
        custody.load(&handle).unwrap().unwrap().expose(),
        staged.expose()
    );
    assert_eq!(
        fingerprint(&keys.load(&r.prepared.office_id).unwrap().unwrap()),
        before
    );
    keys.delete(&r.prepared.office_id).unwrap();
    assert!(!creates(&controller, &actor, &p));
    assert!(keys.load(&r.prepared.office_id).unwrap().is_none());
    // Another plan never attaches to the original code/IDs.
    let mut other = params();
    other.operation_id = p.operation_id.clone();
    other.plan.slug = "different".into();
    assert!(!creates(&controller, &actor, &other));
    assert_eq!(
        db.provisioning_offices().unwrap(),
        [r.prepared.office_id].into()
    );
    offices.shutdown();
}

/// R2-B2: the host's reservation marker keeps a reserved create safe across a host restart.
/// Custody is reserved, the worker is killed before it wrote its intent, the host restarts:
/// the folder is not an unfinished create (so it is never discarded), the key and the
/// custody record are kept, and the retried create completes over the same custody.
#[test]
fn a_reserved_create_killed_before_its_intent_survives_a_host_restart_and_completes() {
    use crate::supervisor::tests::write_create_server;
    let instance = crate::ids::uuid_v7().unwrap();
    let (dir, auth, _) = crate::authentication::tests::fixture_for(&instance);
    let root = dir.path().canonicalize().unwrap();
    let (db, custody) = auth.provisioning_storage();
    let keys = Arc::new(crate::keys::memory::MemoryKeys::default());
    let home = root.join("office-state");
    private(&home).unwrap();
    // Under /tmp: the rendezvous path must fit a socket address.
    let short = tempfile::Builder::new()
        .prefix("pv")
        .tempdir_in("/tmp")
        .unwrap();
    let short_path = short.path().canonicalize().unwrap();
    let pid_file = short_path.join("pid");
    let p = params();
    let actor = account(&*db);
    let fp = fp(&p);
    let wall = org_proto::uuid_v7_time_ms(&p.operation_id).unwrap() as i64;

    // The first host: its worker takes the create, custody included, and is killed.
    let hanging = short_path.join("hanging");
    write_create_server(&hanging, &pid_file, true);
    let writers = Writers::open(custody.clone(), db.clone(), &instance, &root).unwrap();
    let first = offices_in(
        &home,
        &short_path.join("run1"),
        keys.clone(),
        writers.clone(),
        &instance,
        ServerCheck::ByPath,
        hanging,
    );
    let controller = Arc::new(Provisioning::new(
        db.clone(),
        custody.clone(),
        keys.clone(),
        writers.clone(),
        &instance,
        &home,
    ));
    controller.start(first.clone());
    controller
        .create(&actor, &p.operation_id, &p.plan, fp)
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let pid: libc::pid_t = loop {
        if let Ok(text) = std::fs::read_to_string(&pid_file) {
            if let Ok(pid) = text.trim().parse() {
                break pid;
            }
        }
        assert!(std::time::Instant::now() < deadline, "the worker never ran");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    // SIGKILL to the disposable worker this test started.
    org_platform::posix::kill(pid, libc::SIGKILL).unwrap();
    while controller.driving_now() != 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "a creation never ended"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let r = db
        .provisioning(&actor, &p.operation_id, &fp, wall)
        .unwrap()
        .unwrap();
    assert!(!r.linked);
    let office = r.prepared.office_id.clone();
    let folder = home.join("offices").join(&office);
    // Killed before its intent: no `office.json`, no intent; only the host's marker.
    assert!(!folder.join(org_office_launch::OFFICE_FILE_NAME).exists());
    assert!(!folder.join("hq-creation.v1.json").exists());
    assert!(folder.join("selected-writer.required").is_file());
    writers.admit(&office).unwrap();
    let credential = {
        use sha2::{Digest, Sha256};
        let hash = org_office_launch::recovery::hex(&Sha256::digest(office.as_bytes()));
        Name::parse(&format!("writer.{hash}")).unwrap()
    };
    let reserved = custody
        .load(&credential)
        .unwrap()
        .unwrap()
        .expose()
        .to_vec();
    let key = fingerprint(&keys.load(&office).unwrap().unwrap());
    controller.stop_by(std::time::Instant::now());
    first.shutdown();
    drop((controller, first, writers));

    // The host restarts. Its startup discards only unfinished folders, and this is not one:
    // the marker is what keeps it (without it, the folder would be discarded).
    assert!(crate::catalog::unfinished_folders(&home)
        .unwrap()
        .is_empty());
    let marker = folder.join("selected-writer.required");
    let aside = folder.with_file_name("marker-aside");
    std::fs::rename(&marker, &aside).unwrap();
    assert_eq!(
        crate::catalog::unfinished_folders(&home).unwrap(),
        std::slice::from_ref(&office)
    );
    std::fs::rename(&aside, &marker).unwrap();
    assert!(db.provisioning_offices().unwrap().contains(&office));
    assert!(folder.is_dir());
    assert_eq!(fingerprint(&keys.load(&office).unwrap().unwrap()), key);

    // The retried create completes over the same custody.
    let seen = short_path.join("pid.admission");
    let _ = std::fs::remove_file(&seen);
    let creating = short_path.join("creating");
    write_create_server(&creating, &pid_file, false);
    let writers = Writers::open(custody.clone(), db.clone(), &instance, &root).unwrap();
    let second = offices_in(
        &home,
        &short_path.join("run2"),
        keys.clone(),
        writers.clone(),
        &instance,
        ServerCheck::ByPath,
        creating,
    );
    let controller = Arc::new(Provisioning::new(
        db.clone(),
        custody.clone(),
        keys.clone(),
        writers,
        &instance,
        &home,
    ));
    controller.start(second.clone());
    assert!(creates(&controller, &actor, &p));
    assert_eq!(
        custody.load(&credential).unwrap().unwrap().expose(),
        &reserved[..]
    );
    // What the worker received on its custody channel: a valid admission naming this
    // organization and its data roots, which exist, owner-only, for the host's user.
    let admission: org_office_launch::writer::Admission =
        serde_json::from_slice(&std::fs::read(&seen).unwrap()).unwrap();
    assert!(admission.check());
    assert_eq!(
        admission.identity.organization_id,
        r.prepared.organization_id
    );
    assert_eq!(admission.identity.office_id, office);
    assert_eq!(admission.owner_member, r.owner_member);
    let data = root.join(&r.prepared.organization_id);
    assert_eq!(Path::new(&admission.durable_root), data.join("durable"));
    assert_eq!(Path::new(&admission.runtime_root), data.join("runtime"));
    for path in [&admission.durable_root, &admission.runtime_root] {
        let meta = std::fs::symlink_metadata(path).unwrap();
        assert!(meta.is_dir(), "{path}");
        assert_eq!(meta.uid(), crate::home::euid(), "{path}");
        assert_eq!(meta.mode() & 0o7777, 0o700, "{path}");
    }
    assert_eq!(fingerprint(&keys.load(&office).unwrap().unwrap()), key);
    controller.stop_by(std::time::Instant::now());
    second.shutdown();
}

fn confirm_pending_checkpoint(writers: &Arc<Writers>, office: &str) {
    use org_office_launch::writer::{self as wire, Head, HighWaters, Request, Response};
    use std::{io::BufReader, os::unix::net::UnixStream, sync::atomic::AtomicBool};
    let (host, worker) = UnixStream::pair().unwrap();
    writers
        .serve(office, host, Arc::new(AtomicBool::new(true)))
        .unwrap();
    let mut worker = BufReader::new(worker);
    let admission: wire::Admission = wire::read(&mut worker).unwrap().unwrap();
    let checkpoint = Head {
        ordinal: 0,
        sha256: "a".repeat(64),
        high_waters: HighWaters {
            work: 73,
            ..HighWaters::default()
        },
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
    crate::hostdb::HostDbInit {
        instance_id: instance.into(),
        clock_high_ms: org_proto::uuid_v7_time_ms(&crate::ids::uuid_v7().unwrap()).unwrap() as i64,
    }
}
fn seed_owner(db: &HostDb) {
    let account = crate::ids::generate().unwrap();
    let verifier = crate::authentication::hash_password(
        &org_host_control::Password::new("synthetic pending recovery password".into()).unwrap(),
    )
    .unwrap();
    db.seed_account(&account, "Owner", &verifier, true, 1)
        .unwrap();
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
    let instance = crate::ids::uuid_v7().unwrap();
    let (reservation, frozen, credentials) = {
        let custody = Arc::new(DevSecrets::open(base.path(), &root, &instance).unwrap());
        let db =
            Arc::new(HostDb::create(&root, &*custody, &pending_registry_init(&instance)).unwrap());
        seed_owner(&db);
        let store: Arc<dyn HostStore> = db.clone();
        let writers = Writers::open(custody.clone(), store.clone(), &instance, &root).unwrap();
        let controller = Provisioning::new(
            store,
            custody.clone(),
            Arc::new(crate::keys::memory::MemoryKeys::default()),
            writers,
            &instance,
            &root,
        );
        let p = params();
        let actor = account(&*db);
        let handle = Name::parse(&format!("recovery.{}", p.operation_id)).unwrap();
        // Stop at the actual production phase after custody/SQL reservation,
        // before reserve_writer can allocate any writer or device identity.
        let pending = controller
            .new_pending(&actor, &p.operation_id, &p.plan, fp(&p), &handle)
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
    assert!(
        crate::hostdb::pending_recovery::pending_recovery(&custody, &reservation)
            .unwrap()
            .expose()
            == original.code.expose()
    );
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
        let instance = crate::ids::uuid_v7().unwrap();
        let (dir, auth, _) = crate::authentication::tests::fixture_for(&instance);
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
        let controller = Arc::new(Provisioning::new(
            db.clone(),
            custody.clone(),
            keys.clone(),
            writers.clone(),
            &instance,
            &home,
        ));
        controller.start(offices.clone());
        let p = params();
        let actor = account(&*db);
        assert!(!creates(&controller, &actor, &p));
        let fp = fp(&p);
        let wall = org_proto::uuid_v7_time_ms(&p.operation_id).unwrap() as i64;
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
        let data = root.join(&reservation.prepared.organization_id);
        let material = [
            data.join("durable/checkpoint-evidence"),
            data.join("runtime/projection"),
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
        assert!(!creates(&controller, &actor, &p));
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
        assert_eq!(retained["head"]["high_waters"]["work"], 73);
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
    let instance = crate::ids::uuid_v7().unwrap();
    let (_root, auth, _) = crate::authentication::tests::fixture_for(&instance);
    let (db, _) = auth.provisioning_storage();
    let actor = account(&*db);
    let p = params();
    let fp = fp(&p);
    let owner_member = crate::ids::uuid_v7().unwrap();
    let prepared = PreparedV1 {
        plan: p.plan.clone(),
        organization_id: crate::ids::uuid_v7().unwrap(),
        office_id: crate::ids::uuid_v7().unwrap(),
        repository_id: crate::ids::uuid_v7().unwrap(),
        home_team_id: crate::ids::uuid_v7().unwrap(),
        hr_employee_id: crate::ids::uuid_v7().unwrap(),
        creator: org_proto::selected_bootstrap::CreatorV1::from_login(owner_member.clone(), "Ana"),
    };
    let r = Reservation {
        operation_id: p.operation_id.clone(),
        account: actor.account_id().into(),
        fingerprint: fp,
        prepared,
        owner_member,
        recovery_handle: format!("recovery.{}", p.operation_id),
        key_fingerprint: None,
        linked: false,
    };
    let mut other = r.clone();
    other.owner_member = crate::ids::uuid_v7().unwrap();
    assert!(
        db.reserve_provisioning(&actor, &other).is_err(),
        "the owner member must be the creator's person"
    );
    db.reserve_provisioning(&actor, &r).unwrap();
    assert_eq!(
        db.reserve_provisioning(&actor, &r).unwrap().prepared,
        r.prepared
    );
    let wall = org_proto::uuid_v7_time_ms(&r.operation_id).unwrap() as i64;
    assert!(matches!(
        db.provisioning(&actor, &r.operation_id, &[0; 32], wall),
        Err(crate::hostdb::Error::OperationReused)
    ));
    // A known operation answers its stored state whatever its age.
    let old = wall + 31 * 86_400_000;
    assert_eq!(
        db.provisioning(&actor, &r.operation_id, &fp, old)
            .unwrap()
            .map(|known| (known.prepared, known.linked)),
        Some((r.prepared.clone(), false))
    );
    // Only an id the host has never seen is too old (this one was made 31 days before
    // the host's clock) or too far ahead of it.
    let unseen = crate::ids::uuid_v7().unwrap();
    assert!(matches!(
        db.provisioning(&actor, &unseen, &fp, old),
        Err(crate::hostdb::Error::Expired)
    ));
    assert!(matches!(
        db.provisioning(&actor, &r.operation_id, &[0; 32], old),
        Err(crate::hostdb::Error::OperationReused)
    ));
    // Retention is explicit even when the ordinary retry window has expired.
    assert!(db
        .provisioning_offices()
        .unwrap()
        .contains(&r.prepared.office_id));
    auth.sqlite_for_test()
        .change_authority_fixture(
            &crate::accounts::identity::AccountId::parse(actor.account_id()).unwrap(),
            true,
        )
        .unwrap();
    assert!(matches!(
        db.reserve_provisioning(&actor, &r),
        Err(crate::hostdb::Error::NotAdministrator)
    ));
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN; actual worker rendezvous requires permitted local bind"]
fn actual_selected_hq_creation_and_exact_retry() {
    use std::time::{Duration, Instant};
    let binary = std::env::var_os("ORG_TEST_OFFICE_BIN").expect("explicit built office binary");
    let instance = crate::ids::uuid_v7().unwrap();
    let (_account_root, auth, _) = crate::authentication::tests::fixture_for(&instance);
    let (db, custody) = auth.provisioning_storage();
    let dir =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let keys = Arc::new(crate::keys::memory::MemoryKeys::default());
    let writers = Writers::open(custody.clone(), db.clone(), &instance, &root).unwrap();
    let (keys_for_restart, writers_for_restart): (Arc<dyn OfficeSecretsKeys>, _) =
        (keys.clone(), writers.clone());
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
        &office_home,
    ));
    controller.start(offices.clone());
    struct Cleanup(Arc<Offices>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }
    let _cleanup = Cleanup(offices.clone());
    let actor = account(&*db);
    assert!(actor.links().is_empty());
    let p = params();
    let deadline = Instant::now() + Duration::from_secs(25);
    let result = loop {
        match controller
            .create(&actor, &p.operation_id, &p.plan, fp(&p))
            .unwrap()
        {
            HostOpResult::Completed { result } => break result,
            HostOpResult::Accepted => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => panic!("unexpected create result"),
        }
    };
    let state = root.join(&result.id).join("durable/state");
    let initial = std::fs::read(state.join("organization.yaml")).unwrap();
    assert!(!initial.is_empty());
    let replay = controller
        .create(&actor, &p.operation_id, &p.plan, fp(&p))
        .unwrap();
    assert!(matches!(replay, HostOpResult::Completed { result: r } if r == result));
    let linked = account(&*db);
    assert_eq!(linked.links().len(), 1);
    assert_eq!(linked.links()[0].member_id(), result.employee_id);
    // Exact public bytes and identities, no optional owner employee or credentials.
    assert_eq!(
        std::fs::read(state.join("organization.yaml")).unwrap(),
        initial
    );
    assert!(state.join("security/ca-escrow.v1.bin").is_file());

    // The office the create made is served, under the host's custody.
    let office = |offices: &Offices| {
        offices
            .offices()
            .into_iter()
            .find(|office| office.organization_id == result.id)
            .expect("the organization's HQ")
    };
    let active = |offices: &Offices, what: &str| {
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            let found = office(offices);
            if found.status == org_proto::OfficeStatus::Active {
                return found;
            }
            assert!(Instant::now() < deadline, "{what}: {found:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let hq = active(&offices, "after the create");
    // The host restarts: the office starts again from its folder and custody.
    offices.shutdown();
    let again = |what: &str| {
        let started = Offices::start(
            &office_home,
            Catalog::scan(&office_home).unwrap(),
            vec![],
            Supervisor::new(
                SupervisorConfig {
                    program: std::env::var_os("ORG_TEST_OFFICE_BIN").unwrap().into(),
                    home: office_home.clone(),
                    runtime_dir: Some(office_home.join("run")),
                    logs_dir: office_home.join("logs"),
                    log_max_bytes: 1024 * 1024,
                    timings: Timings::default(),
                    keys: keys_for_restart.clone(),
                    server_check: Arc::new(ServerCheck::Skip),
                    sweep_orphans: false,
                    federation: None,
                },
                &instance,
            )
            .with_writers(writers_for_restart.clone()),
            Arc::new(Broadcast::default()),
            &Timings::default(),
        );
        let found = active(&started, what);
        (started, found)
    };
    let (restarted, found) = again("after a host restart");
    assert_eq!(found.id, hq.id);
    restarted.shutdown();
    // The projection is disposable: deleted, the next start rebuilds it from the journal.
    let projection = root.join(&result.id).join("runtime").join(&hq.id);
    assert!(projection.is_dir(), "{}", projection.display());
    std::fs::remove_dir_all(&projection).unwrap();
    let (rebuilt, found) = again("after the projection was deleted");
    assert_eq!(found.id, hq.id);
    assert!(projection.is_dir());
    rebuilt.shutdown();
    assert_eq!(
        std::fs::read(state.join("organization.yaml")).unwrap(),
        initial
    );
}
