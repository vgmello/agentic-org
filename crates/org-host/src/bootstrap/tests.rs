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
    exercise_restarted_worker(
        &host,
        &native_path,
        &provisioned.office_id,
        p.password.expose(),
    );
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

/// Actual disposable worker loss/restart proves the host lifecycle used by the
/// gateway's separate control/cache regressions. No OS presence is requested.
#[cfg(target_os = "macos")]
fn exercise_restarted_worker(host: &BootstrapHost, socket: &Path, office: &str, password: &str) {
    use org_host_control::{ops, SessionHandle};
    use org_proto::{frame, Call, HelloParams};
    use org_socket::rights::{ReceiveError, StreamReceiver};
    use std::{
        io::{self, Write},
        os::fd::{AsRawFd, OwnedFd},
        time::{Duration, Instant},
    };
    struct Native {
        write: UnixStream,
        read: StreamReceiver,
        id: u64,
        workers: Vec<std::io::BufReader<UnixStream>>,
    }
    impl Native {
        fn byte(&mut self, expected: usize, until: Instant) -> io::Result<(u8, Vec<OwnedFd>)> {
            let timed_out = || io::Error::new(io::ErrorKind::TimedOut, "native response deadline");
            loop {
                if Instant::now() >= until {
                    return Err(timed_out());
                }
                match self
                    .read
                    .receive_byte(expected, &crate::cloexec::SPAWN_LOCK)
                {
                    Ok(byte) => return Ok(byte),
                    Err(ReceiveError::WouldBlock) => {
                        let left = until
                            .checked_duration_since(Instant::now())
                            .ok_or_else(timed_out)?;
                        let fd = self.read.fd().ok_or_else(|| {
                            io::Error::new(io::ErrorKind::UnexpectedEof, "native reader closed")
                        })?;
                        match org_socket::poll_readable(fd.as_raw_fd(), left) {
                            Ok(true) => {}
                            Ok(false) => return Err(timed_out()),
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                            Err(error) => return Err(error),
                        }
                    }
                    Err(ReceiveError::Fatal(error))
                        if error.kind() == io::ErrorKind::UnexpectedEof =>
                    {
                        return Err(error);
                    }
                    // As in the production gateway, malformed ancillary data
                    // may have installed unreported descriptors on macOS.
                    Err(ReceiveError::Fatal(_)) => std::process::exit(1),
                }
            }
        }
        fn call<C: Call>(&mut self, params: &C::Params) -> Result<C::Result, ApiError> {
            self.id += 1;
            let request = Request::new::<C>(self.id, params).unwrap();
            let mut bytes = serde_json::to_vec(&request).unwrap();
            bytes.push(b'\n');
            self.write.write_all(&bytes).unwrap();
            // One response deadline, never extended by each arriving byte.
            let until = Instant::now() + Duration::from_secs(30);
            let mut bytes = Vec::new();
            loop {
                let (b, fds) = self.byte(0, until).unwrap();
                assert!(fds.is_empty());
                if b == b'\n' {
                    break;
                }
                assert!(bytes.len() < org_proto::MAX_FRAME_BYTES);
                bytes.push(b);
            }
            let response: Response = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(response.id, self.id);
            response.into_result::<C>().map_err(|e| match e {
                org_proto::CallError::Api(e) => e,
                _ => panic!("typed response"),
            })
        }
        fn open(&mut self, handle: &SessionHandle, office: &str) -> String {
            let opened = self
                .call::<ops::OfficeOpenChannel>(&ops::OfficeOpenChannelParams {
                    session: SessionHandle::new(handle.expose().into()).unwrap(),
                    office_id: office.into(),
                })
                .unwrap();
            let (marker, mut fds) = self
                .byte(1, Instant::now() + Duration::from_secs(30))
                .unwrap();
            assert_eq!(marker, ops::HOST_SOCK_MARKER);
            let stream = org_socket::rights::connected_unix_stream(fds.pop().unwrap()).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(30)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(30)))
                .unwrap();
            self.call::<ops::FdsReceived>(&ops::FdsReceivedParams {
                of: opened.channel_id.clone(),
            })
            .unwrap();
            let mut worker = std::io::BufReader::new(stream);
            frame::write_message(
                worker.get_mut(),
                &Request::new::<Hello>(
                    1,
                    &HelloParams::offering(
                        org_proto::ApiRange { min: 2, max: 2 },
                        "restarted-worker-test",
                    ),
                )
                .unwrap(),
            )
            .unwrap();
            let response: Response = frame::read_message(&mut worker).unwrap().unwrap();
            assert_eq!(
                response
                    .into_result::<Hello>()
                    .unwrap()
                    .office
                    .unwrap()
                    .office
                    .id,
                office
            );
            // Keep the real worker connection alive through grant/revocation
            // checks; closing it here would test a different failure boundary.
            self.workers.push(worker);
            opened.channel_id
        }
    }
    let stream = UnixStream::connect(socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut native = Native {
        read: StreamReceiver::new(stream.try_clone().unwrap()),
        write: stream,
        id: 0,
        workers: Vec::new(),
    };
    native.call::<Hello>(&HelloParams::default()).unwrap();
    let signed = native
        .call::<ops::AccountVerify>(&ops::AccountVerifyParams {
            login: "Owner".into(),
            password: Password::new(password.into()).unwrap(),
        })
        .unwrap();
    let channel = native.open(&signed.session, office);
    let grant_params = |channel_id: String| ops::SessionGrantParams {
        session: SessionHandle::new(signed.session.expose().into()).unwrap(),
        office_id: office.into(),
        channel_id,
    };
    let first = native
        .call::<ops::SessionGrant>(&grant_params(channel.clone()))
        .unwrap();
    let offices = {
        let state = host.state.state.lock().unwrap();
        Arc::clone(&state.host.as_ref().unwrap().offices)
    };
    let old = offices.endpoint(office).unwrap();
    old.fail();
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        if offices
            .endpoint(office)
            .is_some_and(|new| new.is_live() && !Arc::ptr_eq(&old, &new))
        {
            break;
        }
        assert!(Instant::now() < until, "actual worker did not restart");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(matches!(
        native
            .call::<ops::SessionGrant>(&grant_params(channel))
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized | ErrorCode::Unavailable
    ));
    native
        .call::<ops::SessionInspect>(&ops::SessionParams {
            session: SessionHandle::new(signed.session.expose().into()).unwrap(),
        })
        .unwrap();
    let replacement = native.open(&signed.session, office);
    let second = native
        .call::<ops::SessionGrant>(&grant_params(replacement.clone()))
        .unwrap();
    assert_ne!(
        first.grant.claims.channel_id,
        second.grant.claims.channel_id
    );
    assert_eq!(
        first.grant.claims.session_id,
        second.grant.claims.session_id
    );
    native
        .call::<ops::SessionEnd>(&ops::SessionParams {
            session: SessionHandle::new(signed.session.expose().into()).unwrap(),
        })
        .unwrap();
    assert_eq!(
        native
            .call::<ops::SessionGrant>(&grant_params(replacement))
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        native
            .call::<ops::OfficeOpenChannel>(&ops::OfficeOpenChannelParams {
                session: signed.session,
                office_id: office.into()
            })
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
}

#[cfg(target_os = "macos")]
#[path = "recovery_tests.rs"]
mod recovery_tests;

#[cfg(target_os = "macos")]
#[path = "web_process_tests.rs"]
mod web_process_tests;
