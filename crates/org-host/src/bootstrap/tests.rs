use super::*;
use org_host_control::{listen::ListenOrigin, Password};
use std::os::unix::fs::PermissionsExt;

struct Fixture {
    _temp: tempfile::TempDir,
    state: Arc<BootstrapService>,
}
impl Fixture {
    fn new(https: bool) -> Self {
        let temp = tempfile::tempdir_in("/private/tmp").unwrap();
        let base = temp.path().canonicalize().unwrap();
        for name in ["temp", "keys"] {
            fs::create_dir(base.join(name)).unwrap();
            fs::set_permissions(base.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut config = Config::new(base.join("unused-home"), base.join("unused-worker"));
        config.dev_key_dir = Some(base.join("keys"));
        let origin = if https {
            "https://127.0.0.1:8443"
        } else {
            "http://127.0.0.1:8123"
        };
        let listen = serde_json::from_value(serde_json::json!({
            "address":"127.0.0.1", "port":if https {8443} else {8123},
            "scheme":if https {"https"} else {"http_loopback"}, "origin":origin,"hsts":"off"
        }))
        .unwrap();
        let state = BootstrapHost::prepare(
            config,
            &base.join("locator"),
            &base.join("temp"),
            None,
            &base.join("instance"),
            listen,
        )
        .unwrap();
        Self { _temp: temp, state }
    }
    fn db(&self) -> Arc<HostDb> {
        let state = self.state.state.lock().unwrap();
        self.state.database(&state).unwrap().unwrap()
    }
}
fn params() -> BootstrapParams {
    BootstrapParams {
        operation_id: crate::ids::generate().unwrap(),
        hq_operation_id: crate::ids::generate().unwrap(),
        login: "Owner".into(),
        password: Password::new("synthetic first password".into()).unwrap(),
        tls: None,
    }
}

#[test]
#[cfg(target_os = "macos")]
fn private_browser_revoke_control_rejects_stale_malformed_and_unsafe_requests_before_registry_mutation(
) {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let fixture = Fixture::new(false);
    let p = params();
    fixture.state.initialize(&p).unwrap();
    let db = fixture.db();
    let account =
        crate::accounts::identity::AccountId::parse(&db.bootstrap_record().unwrap().account)
            .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let request = dir.path().join("request.json");
    let receipt = dir.path().join("receipt.json");
    assert!(!super::browser_fixture::revoke_requested(
        &request, &receipt, &db, &account
    ));
    let valid = format!(
        r#"{{"version":1,"requestId":"{}"}}"#,
        crate::ids::uuid_v7().unwrap()
    );
    for (bytes, mode, stale) in [
        (
            r#"{"version":1,"requestId":"invalid"}"#.to_owned(),
            0o600,
            false,
        ),
        (
            valid.replace("\"version\":1", "\"version\":1,\"version\":1"),
            0o600,
            false,
        ),
        (
            valid.replace("\"version\":1", "\"version\":2"),
            0o600,
            false,
        ),
        (
            valid.replace("\"version\":1", "\"version\":1,\"unexpected\":true"),
            0o600,
            false,
        ),
        (" ".repeat(513), 0o600, false),
        (valid.clone(), 0o644, false),
        (valid.clone(), 0o600, true),
    ] {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&request)
            .unwrap();
        file.write_all(bytes.as_bytes()).unwrap();
        drop(file);
        fs::set_permissions(&request, fs::Permissions::from_mode(mode)).unwrap();
        if stale {
            fs::write(&receipt, b"stale").unwrap();
        }
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::browser_fixture::revoke_requested(&request, &receipt, &db, &account)
        }))
        .is_err());
        assert!(db
            .account_authority(&account)
            .unwrap()
            .unwrap()
            .revoked_at_ms()
            .is_none());
        fs::remove_file(&request).unwrap();
        if stale {
            fs::remove_file(&receipt).unwrap();
        }
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&request)
        .unwrap();
    file.write_all(valid.as_bytes()).unwrap();
    drop(file);
    assert!(super::browser_fixture::revoke_requested(
        &request, &receipt, &db, &account
    ));
    let result: serde_json::Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    assert_eq!(result["status"], "completed");
    assert_eq!(result["accountId"], account.as_str());
    let request: serde_json::Value = serde_json::from_str(&valid).unwrap();
    assert_eq!(result["requestId"], request["requestId"]);
    assert!(db
        .account_authority(&account)
        .unwrap()
        .unwrap()
        .revoked_at_ms()
        .is_some());
}

#[test]
#[cfg(target_os = "macos")]
fn real_bootstrap_registry_has_a_protected_admin_without_an_employee_and_refusals_are_unstored() {
    let fixture = Fixture::new(false);
    let p = params();
    let mut short = params();
    short.password = Password::new("short".into()).unwrap();
    assert_eq!(
        fixture.state.initialize(&short).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert!(!fixture.state.home.join("host-registry.db").exists());
    assert!(fixture.state.custody.list().unwrap().is_empty());
    assert!(fixture.state.initialize(&p).unwrap().is_none());
    let db = fixture.db();
    let record = db.bootstrap_record().unwrap();
    assert_eq!(record.operation_id, p.operation_id);
    assert!(!record.finished);
    let credentials = db
        .account_credentials(&LoginKey::new("OWNER").unwrap())
        .unwrap()
        .unwrap();
    assert!(credentials.account().admin());
    assert!(credentials.account().links().is_empty());
    assert_eq!(record.account, credentials.account().account_id());
    assert!(fixture
        .state
        .home
        .join("offices")
        .read_dir()
        .unwrap()
        .next()
        .is_none());
    db.finish_bootstrap().unwrap();
    assert!(!db.bootstrap_record().unwrap().finished);
    let different = params();
    assert!(matches!(
        fixture
            .state
            .bootstrap(&different, &Connection::for_test())
            .unwrap(),
        BootstrapOutcome::Rejected {
            refusal: BootstrapRefusal::AlreadyBootstrapped
        }
    ));
    assert!(fixture.state.initialize(&p).is_err()); // cannot replace installed identity
    assert_eq!(
        fixture.db().bootstrap_record().unwrap().account,
        record.account
    );
    let actor = db
        .account_authority(&crate::accounts::identity::AccountId::parse(&record.account).unwrap())
        .unwrap()
        .unwrap();
    let reservation = crate::hostdb::provisioning::Reservation {
        operation_id: p.hq_operation_id.clone(),
        account: record.account.clone(),
        fingerprint: [9; 32],
        prepared: org_proto::selected_bootstrap::PreparedV1 {
            plan: org_proto::selected_bootstrap::PlanV1 {
                version: 1,
                name: "Example".into(),
                slug: "example".into(),
                repository_remote_url: "https://example.test/org.git".into(),
                state_branch: "org/state".into(),
                home_team_name: "Core".into(),
            },
            organization_id: crate::ids::uuid_v7().unwrap(),
            office_id: crate::ids::uuid_v7().unwrap(),
            repository_id: crate::ids::uuid_v7().unwrap(),
            home_team_id: crate::ids::uuid_v7().unwrap(),
            hr_employee_id: crate::ids::uuid_v7().unwrap(),
        },
        owner_member: crate::ids::generate().unwrap(),
        recovery_handle: format!("recovery.{}", p.hq_operation_id),
        key_fingerprint: None,
        linked: false,
    };
    db.reserve_provisioning(&actor, &reservation).unwrap();
    db.finish_bootstrap().unwrap();
    assert!(!db.bootstrap_record().unwrap().finished);
    db.provision_key_stored(&reservation, &"aa".repeat(32))
        .unwrap();
    db.provision_linked(&reservation).unwrap();
    db.finish_bootstrap().unwrap();
    assert!(db.bootstrap_record().unwrap().finished);
    assert!(matches!(
        fixture
            .state
            .bootstrap(&p, &Connection::for_test())
            .unwrap(),
        BootstrapOutcome::Rejected {
            refusal: BootstrapRefusal::BootstrapFinished
        }
    ));
    assert!(matches!(
        fixture
            .state
            .bootstrap(&different, &Connection::for_test())
            .unwrap(),
        BootstrapOutcome::Rejected {
            refusal: BootstrapRefusal::AlreadyBootstrapped
        }
    ));
}

#[test]
#[cfg(target_os = "macos")]
fn first_bootstrap_tls_refusals_publish_neither_registry_nor_secret() {
    let fixture = Fixture::new(true);
    let mut p = params();
    assert!(matches!(
        fixture.state.initialize(&p).unwrap(),
        Some(BootstrapOutcome::Rejected {
            refusal: BootstrapRefusal::TlsRequired
        })
    ));
    p.tls = Some(org_host_control::ops::TlsPair {
        cert: "not a certificate".into(),
        key: org_host_control::TlsPrivateKey::new("not a private key".into()).unwrap(),
    });
    assert!(matches!(
        fixture.state.initialize(&p).unwrap(),
        Some(BootstrapOutcome::Rejected {
            refusal: BootstrapRefusal::Tls(TlsRefusal::TlsInvalid)
        })
    ));
    assert!(!fixture.state.home.join("host-registry.db").exists());
    assert!(fixture.state.custody.list().unwrap().is_empty());
    assert_eq!(
        fixture.state.listen.origin(),
        &ListenOrigin::parse("https://127.0.0.1:8443").unwrap()
    );
}

#[test]
#[cfg(target_os = "macos")]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN; actual protected reverse/native listeners require permitted local bind"]
fn actual_protected_reverse_bootstrap_provisions_and_replays_without_an_owner_employee() {
    use org_host_control::{
        ops,
        provision::{OrganizationProvision, Params},
        HostOpResult,
    };
    use org_proto::{frame, selected_bootstrap::PlanV1, Call, HelloParams};
    use std::{
        io::BufReader,
        os::unix::net::UnixListener,
        time::{Duration, Instant},
    };
    let binary = std::env::var_os("ORG_TEST_OFFICE_BIN").expect("explicit built worker");
    let temp = tempfile::tempdir_in("/private/tmp").unwrap();
    let base = temp.path().canonicalize().unwrap();
    for name in ["temp", "keys"] {
        fs::create_dir(base.join(name)).unwrap();
        fs::set_permissions(base.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let socket = base.join("setup.sock");
    let listening = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    // Only generated disposable roots/custody; this exact test executable is
    // allowed by the existing explicit DEBUG peer policy, not a production mock.
    let mut config = Config::new(base.join("unused-home"), binary);
    config.dev_key_dir = Some(base.join("keys"));
    config.dev_app_path = Some(std::env::current_exe().unwrap());
    let listen = serde_json::from_value(serde_json::json!({"address":"127.0.0.1","port":8123,"scheme":"http_loopback","origin":"http://127.0.0.1:8123","hsts":"off"})).unwrap();
    let host = BootstrapHost::start(
        config,
        &base.join("locator"),
        &base.join("temp"),
        None,
        &base.join("instance"),
        listen,
        &socket,
    )
    .unwrap();
    let (stream, _) = listening.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut wire = BufReader::new(stream);
    fn answer<C: Call>(
        wire: &mut BufReader<UnixStream>,
        id: u64,
        p: &C::Params,
    ) -> Result<C::Result, ApiError> {
        frame::write_message(wire.get_mut(), &Request::new::<C>(id, p).unwrap()).unwrap();
        let response: Response = frame::read_message(wire).unwrap().unwrap();
        assert_eq!(response.id, id);
        response.into_result::<C>().map_err(|error| match error {
            org_proto::CallError::Api(error) => error,
            _ => panic!("malformed typed response"),
        })
    }
    answer::<Hello>(&mut wire, 1, &HelloParams::default()).unwrap();
    let p = params();
    let BootstrapOutcome::Completed { result: first } =
        answer::<Bootstrap>(&mut wire, 2, &p).unwrap()
    else {
        panic!("first bootstrap refused")
    };
    let inspected = answer::<ops::SessionInspect>(
        &mut wire,
        3,
        &ops::SessionParams {
            session: first.session,
        },
    )
    .unwrap();
    assert!(inspected.admin);
    assert!(inspected.orgs.is_empty());
    let mut changed = params();
    changed.operation_id = p.operation_id.clone();
    changed.password = Password::new("incorrect synthetic password".into()).unwrap();
    assert_eq!(
        answer::<Bootstrap>(&mut wire, 4, &changed)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    changed.password = Password::new(p.password.expose().to_owned()).unwrap();
    assert_eq!(
        answer::<Bootstrap>(&mut wire, 5, &changed)
            .unwrap_err()
            .code,
        ErrorCode::OperationReused
    );
    let BootstrapOutcome::Completed { result: retry } =
        answer::<Bootstrap>(&mut wire, 6, &p).unwrap()
    else {
        panic!("exact bootstrap replay refused")
    };
    let prepared = Params {
        session: retry.session,
        operation_id: p.hq_operation_id.clone(),
        plan: PlanV1 {
            version: 1,
            name: "Example".into(),
            slug: "example".into(),
            repository_remote_url: "https://example.test/organization.git".into(),
            state_branch: "org/state".into(),
            home_team_name: "Core".into(),
        },
    };
    let until = Instant::now() + Duration::from_secs(30);
    let mut id = 7;
    let provisioned = loop {
        let result = answer::<OrganizationProvision>(&mut wire, id, &prepared).unwrap();
        id += 1;
        match result {
            HostOpResult::Completed { result } => break result,
            HostOpResult::Accepted => {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => panic!("unexpected provisioning result"),
        }
    };
    let public = base
        .join("instance/durable/state")
        .join(&provisioned.organization_id);
    assert!(public.join("organization.yaml").is_file());
    let agents = public
        .join("teams")
        .join(&provisioned.home_team_id)
        .join("agents");
    assert_eq!(agents.read_dir().unwrap().count(), 1); // HR only, no optional owner employee
                                                       // Actual authenticated supervisor one-shot public-record exchange. This
                                                       // reads the committed Office projection through launch-v3, not a SQL seed,
                                                       // path hint or private DER export. No OS authentication is invoked.
    let recovery_status = answer::<org_host_control::recovery::Status>(
        &mut wire,
        id,
        &org_host_control::recovery::StatusParams {
            session: org_host_control::SessionHandle::new(prepared.session.expose().into())
                .unwrap(),
            org_id: provisioned.organization_id.clone(),
            after_ordinal: None,
        },
    )
    .unwrap();
    id += 1;
    assert_eq!(
        recovery_status.state,
        org_host_control::recovery::DeliveryState::Pending
    );
    assert_eq!(recovery_status.delivery_ordinal, "0");
    assert!(recovery_status.audit.is_empty());
    assert!(!recovery_status.hq_loss_recovery_supported);
    recovery_tests::exercise(
        &host,
        &mut wire,
        &prepared,
        &provisioned,
        &mut id,
        p.password.expose(),
    );
    let inspection = answer::<ops::SessionInspect>(
        &mut wire,
        id,
        &ops::SessionParams {
            session: prepared.session,
        },
    )
    .unwrap();
    id += 1;
    assert_eq!(inspection.orgs.len(), 1);
    // Ordinary native host.sock cannot bootstrap even with a valid hello.
    let native_path = {
        let state = host.state.state.lock().unwrap();
        state.host.as_ref().unwrap().socket_path().to_owned()
    };
    let mut native = BufReader::new(UnixStream::connect(&native_path).unwrap());
    answer::<Hello>(&mut native, 1, &HelloParams::default()).unwrap();
    assert_eq!(
        answer::<Bootstrap>(&mut native, 2, &p).unwrap_err().code,
        ErrorCode::NotAuthorized
    );
    {
        let db = {
            let state = host.state.state.lock().unwrap();
            host.state.database(&state).unwrap().unwrap()
        };
        crate::human::native_auth_tests::exercise(
            &base,
            &native_path,
            &host.state.id,
            &provisioned,
            &public,
            p.password.expose(),
            &db,
        );
    }
    answer::<Exit>(&mut wire, id, &Empty {}).unwrap();
    host.shutdown();
    drop(native);
    drop(wire);
    // SQLCipher/custody reopen proves exact completion, not OS power-loss safety.
    let registry = Registry::open(&base.join("locator"), &base.join("temp")).unwrap();
    let instance = registry.reserve(None, &base.join("instance")).unwrap();
    let custody = crate::host_secrets::store_for(
        Some(&base.join("keys")),
        instance.root(),
        instance.id(),
        crate::code_signing::own_signing,
    )
    .unwrap();
    let paths = instance.prepare_layout().unwrap();
    let db = HostDb::open(&paths.office_home, &*custody).unwrap();
    let record = db.bootstrap_record().unwrap();
    assert!(record.finished);
    assert_eq!(record.operation_id, p.operation_id);
    assert_eq!(db.provisioning_offices().unwrap().len(), 1);
}

#[cfg(target_os = "macos")]
#[path = "recovery_tests.rs"]
mod recovery_tests;
