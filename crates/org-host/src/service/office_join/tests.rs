use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::{write_pending_join, PendingJoinRecord, Service};

use org_host_control::{
    ops::{
        AccountVerify, AccountVerifyParams, OfficeJoin, OfficeJoinParams, OfficeReEnroll,
        OfficeReEnrollParams, OfficeRemovePending, OfficeRemovePendingParams,
    },
    HostOpResult, InvitationTokenSecret, Password, SessionHandle,
};
use org_office_launch::OfficeFile;
use org_pki::{
    cert::build_org_ca_cert,
    invitation::{InvitationToken, NewInvitationParams},
    SigningKey,
};
use org_proto::{CallError, ErrorCode, Hello, OfficeRole};

use crate::{authentication::hash_password, control_listener, service::tests as service_tests};

fn make_invitation(org_id: &str, office_id: &str, hq_id: &str, expires_at_ms: u64) -> String {
    let ca_key = SigningKey::generate().unwrap();
    let ca = build_org_ca_cert(&ca_key, org_id, 1_760_000_000).unwrap();
    let inv_key = SigningKey::generate().unwrap();
    InvitationToken::new(
        NewInvitationParams {
            invitation_id: crate::ids::uuid_v7().unwrap(),
            org_id: org_id.into(),
            org_name: "Acme".into(),
            office_id: office_id.into(),
            office_name: "Lisbon".into(),
            hq_office_id: hq_id.into(),
            hq_addresses: vec!["127.0.0.1:9443".into()],
            ca_cert_der: ca.cert_der,
            expires_at_ms,
        },
        &inv_key,
    )
    .unwrap()
    .encode()
    .unwrap()
    .to_string()
}

const JOIN_WORKER_ENV: &str = "ORG_HOST_TEST_JOIN_WORKER";

fn write_join_worker_script(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let body = format!(
        "{JOIN_WORKER_ENV}=1 exec '{}' --exact service::office_join::tests::satellite_join_server \
             --ignored --test-threads=1 >/dev/null",
        std::env::current_exe().unwrap().display()
    );
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
#[ignore = "run only as the satellite --join worker of office_join tests"]
fn satellite_join_server() {
    use org_office_launch::{HostFirstLine, OfficeFile, RendezvousEvent, ServerEvent};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    if std::env::var_os(JOIN_WORKER_ENV).is_none() {
        return;
    }
    let mut stdin = std::io::BufReader::new(std::io::stdin());
    let Ok(Some(HostFirstLine::Join(join))) =
        org_office_launch::read_first_line(&mut stdin, org_office_launch::FirstLine::Join)
    else {
        std::process::exit(2);
    };
    let mut stream = UnixStream::connect(&join.office.rendezvous).unwrap();
    org_proto::frame::write_message(&mut stream, &org_office_launch::ReadyForSecrets).unwrap();
    let fence = std::sync::Mutex::new(());
    let mut receiver = org_socket::rights::StreamReceiver::new(stream.try_clone().unwrap());
    let mut recv_step = |count: usize, marker: bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let got = if marker {
                receiver
                    .receive_marker(count, org_office_launch::FD_MARKER, &fence)
                    .map(|fds| (org_office_launch::FD_MARKER, fds))
            } else {
                receiver.receive_byte(count, &fence)
            };
            match got {
                Ok(v) => return v,
                Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::yield_now()
                }
                Err(_) => std::process::exit(4),
            }
        }
    };
    let mut header = Vec::new();
    loop {
        let (b, _) = recv_step(0, false);
        if b == b'\n' {
            break;
        }
        header.push(b);
    }
    let Ok(org_office_launch::Handoff::Join { fds, .. }) =
        org_office_launch::decode_handoff(&header)
    else {
        std::process::exit(5);
    };
    let (_, mut transfer) = recv_step(fds.len(), true);
    drop(receiver);
    stream.set_nonblocking(false).unwrap();
    org_office_launch::write_rendezvous_event(
        &mut stream,
        &RendezvousEvent::FdsReceived {
            of: "handoff".into(),
        },
    )
    .unwrap();
    let transfer = transfer.pop().unwrap();
    let mut control =
        org_socket::rights::DatagramReceiver::new(std::os::unix::net::UnixDatagram::from(transfer));
    let mut payload = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
    let deadline = Instant::now() + Duration::from_secs(5);
    let received = loop {
        match control.receive_up_to(&mut payload, 1, &fence) {
            Ok(r) => break r,
            Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(_) => std::process::exit(7),
        }
    };
    let Ok(org_office_launch::TransferRecord::WriterControl { channel_id, .. }) =
        org_office_launch::decode_transfer_record(&payload[..received.bytes])
    else {
        std::process::exit(8);
    };
    let writer = UnixStream::from(received.descriptors.into_iter().next().unwrap());
    writer.set_nonblocking(false).unwrap();
    let mut writer = std::io::BufReader::new(writer);
    let _ = org_office_launch::writer::read::<org_office_launch::writer::Admission>(&mut writer);
    org_office_launch::write_rendezvous_event(
        &mut stream,
        &RendezvousEvent::FdsReceived { of: channel_id },
    )
    .unwrap();
    org_office_launch::write_rendezvous_event(
        &mut stream,
        &RendezvousEvent::Server(ServerEvent::JoinPending {
            request_id: format!("req-{}", join.office.office_id),
            check_code: "ABCD-EFGH".into(),
        }),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(40));
    let office_file = OfficeFile::selected(
        &join.office.office_id,
        &join.office_name,
        OfficeRole::Satellite,
        &join.org_id,
        &join.org_name,
        "2026-11-23T00:00:00.000Z",
    )
    .unwrap();
    org_office_launch::write_rendezvous_event(
        &mut stream,
        &RendezvousEvent::Server(ServerEvent::Joined {
            office: office_file,
        }),
    )
    .unwrap();
    let mut sink = [0u8; 64];
    let _ = std::io::Read::read(&mut stdin, &mut sink);
}

struct JoinTestHarness {
    _home: tempfile::TempDir,
    _root: tempfile::TempDir,
    _local: control_listener::LocalConnection,
    service: Arc<Service>,
    peer: std::io::BufReader<std::os::unix::net::UnixStream>,
    session_token: Option<String>,
    now_ms: u64,
    org_id: String,
    hq_id: String,
    sat_id: String,
    enrolled_sat_id: String,
}

impl JoinTestHarness {
    fn start_with_mode(with_admin: bool, with_worker: bool) -> Self {
        use crate::broadcast::Broadcast;
        use crate::catalog::Catalog;
        use crate::keys::memory::MemoryKeys;
        use crate::offices::Offices;
        use crate::supervisor::{Supervisor, SupervisorConfig};
        use crate::timings::Timings;

        let home = tempfile::Builder::new()
            .prefix("aoj")
            .tempdir_in("/tmp")
            .unwrap();
        let runtime_dir = home.path().join("run");
        fs::create_dir_all(&runtime_dir).unwrap();
        let instance_id = crate::ids::uuid_v7().unwrap();
        let (root, authentication) = if with_admin {
            let (r, auth, _) = crate::authentication::tests::fixture();
            (r, auth)
        } else {
            crate::authentication::tests::fixture_without_accounts()
        };
        let (db, _) = authentication.provisioning_storage();
        let now = super::browser_now_ms().unwrap();
        let session_login = if with_admin {
            let verifier =
                hash_password(&Password::new("synthetic test password".into()).unwrap()).unwrap();
            let admin_acct = crate::ids::generate().unwrap();
            db.seed_account(&admin_acct, "Admin", &verifier, true, now)
                .unwrap();
            Some("Admin")
        } else {
            None
        };

        let hq_id = crate::ids::uuid_v7().unwrap();
        let sat_id = crate::ids::uuid_v7().unwrap();
        let enrolled_sat_id = crate::ids::uuid_v7().unwrap();

        let program = if with_worker {
            let p = home.path().join("join-worker.sh");
            write_join_worker_script(&p);
            p
        } else {
            home.path().join("no-such-server")
        };
        let custody =
            crate::writer::TestCustody::reserve(home.path(), &instance_id, &[&enrolled_sat_id]);
        let org_id = custody.organization.clone();
        let supervisor = Supervisor::new(
            SupervisorConfig {
                program,
                home: home.path().to_path_buf(),
                runtime_dir: Some(runtime_dir),
                logs_dir: home.path().join("logs"),
                log_max_bytes: 1024,
                timings: Timings::default(),
                keys: Arc::new(MemoryKeys::default()),
                server_check: Arc::new(crate::server_check::ServerCheck::Skip),
                sweep_orphans: false,
                federation: None,
            },
            &instance_id,
        )
        .with_writers(custody.writers);
        let broadcast = Arc::new(Broadcast::default());
        let offices = Offices::start(
            home.path(),
            Catalog::default(),
            Vec::new(),
            supervisor,
            Arc::clone(&broadcast),
            &Timings::default(),
        );
        let service = Service::new(offices, broadcast, Some(instance_id))
            .with_authentication(Some(Arc::new(authentication)));
        service.offices.insert_catalog_for_test(OfficeFile::new(
            &enrolled_sat_id,
            "Existing Sat",
            OfficeRole::Satellite,
            &org_id,
            "Acme",
            "2026-11-21T00:00:00.000Z",
        ));
        let service = Arc::new(service);
        let (host_stream, peer_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let handler: Arc<dyn control_listener::Handler> = service.clone();
        let local = control_listener::LocalConnection::start(host_stream, handler).unwrap();
        peer_stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut peer = std::io::BufReader::new(peer_stream);
        service_tests::exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        let session_token = session_login.map(|login| {
            let verified = service_tests::exchange::<AccountVerify>(
                &mut peer,
                2,
                &AccountVerifyParams {
                    login: login.into(),
                    password: Password::new("synthetic test password".into()).unwrap(),
                },
            )
            .unwrap()
            .into_result::<AccountVerify>()
            .unwrap();
            verified.session.expose().into()
        });
        Self {
            _home: home,
            _root: root,
            _local: local,
            service,
            peer,
            session_token,
            now_ms: now as u64,
            org_id,
            hq_id,
            sat_id,
            enrolled_sat_id,
        }
    }

    fn session(&self) -> Option<SessionHandle> {
        self.session_token
            .as_ref()
            .map(|t| SessionHandle::new(t.clone()).unwrap())
    }
}

#[test]
fn office_join_runs_supervisor_rendezvous_and_enforces_consent_and_replay() {
    let mut h = JoinTestHarness::start_with_mode(true, true);
    let token = make_invitation(&h.org_id, &h.sat_id, &h.hq_id, h.now_ms + 600_000);
    let s1 = h.session();
    let no_consent = service_tests::exchange::<OfficeJoin>(
        &mut h.peer,
        3,
        &OfficeJoinParams {
            session: s1,
            operation_id: crate::ids::uuid_v7().unwrap(),
            invitation: InvitationTokenSecret::new(token.clone()).unwrap(),
            consent: false,
        },
    )
    .unwrap()
    .into_result::<OfficeJoin>();
    assert!(matches!(
        no_consent,
        Err(CallError::Api(ref e)) if e.code == ErrorCode::InvalidRequest
    ));

    let join_op = crate::ids::uuid_v7().unwrap();
    let s2 = h.session();
    let joined = service_tests::exchange::<OfficeJoin>(
        &mut h.peer,
        4,
        &OfficeJoinParams {
            session: s2,
            operation_id: join_op.clone(),
            invitation: InvitationTokenSecret::new(token.clone()).unwrap(),
            consent: true,
        },
    )
    .unwrap()
    .into_result::<OfficeJoin>()
    .unwrap();
    match joined {
        HostOpResult::Completed { result } => {
            assert_eq!(result.office_id, h.sat_id);
            assert_eq!(
                result.state,
                org_host_control::ops::OfficeJoinState::JoinPending
            );
            assert_eq!(result.check_code.as_deref(), Some("ABCD-EFGH"));
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    let s3 = h.session();
    let replay = service_tests::exchange::<OfficeJoin>(
        &mut h.peer,
        5,
        &OfficeJoinParams {
            session: s3,
            operation_id: join_op,
            invitation: InvitationTokenSecret::new(token).unwrap(),
            consent: true,
        },
    )
    .unwrap()
    .into_result::<OfficeJoin>()
    .unwrap();
    assert!(matches!(replay, HostOpResult::Completed { .. }));

    // Background join watcher adopts the worker into catalog once `Joined` arrives!
    let mut adopted = false;
    for _ in 0..40 {
        if h.service.offices.offices().iter().any(|o| o.id == h.sat_id) {
            adopted = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(
        adopted,
        "joined satellite must be adopted into host catalog"
    );
    h.service.offices.shutdown();
}

#[test]
fn office_join_fresh_instance_admits_local_peer_and_accounts_require_admin() {
    // Fresh satellite-only instance (zero accounts): verified local peer with consent succeeds!
    let mut fresh = JoinTestHarness::start_with_mode(false, true);
    let token = make_invitation(
        &fresh.org_id,
        &fresh.sat_id,
        &fresh.hq_id,
        fresh.now_ms + 600_000,
    );
    let joined = service_tests::exchange::<OfficeJoin>(
        &mut fresh.peer,
        3,
        &OfficeJoinParams {
            session: None,
            operation_id: crate::ids::uuid_v7().unwrap(),
            invitation: InvitationTokenSecret::new(token).unwrap(),
            consent: true,
        },
    )
    .unwrap()
    .into_result::<OfficeJoin>()
    .unwrap();
    match joined {
        HostOpResult::Completed { result } => {
            assert_eq!(result.check_code.as_deref(), Some("ABCD-EFGH"));
        }
        other => panic!("expected Completed on fresh instance, got {other:?}"),
    }
    fresh.service.offices.shutdown();

    // Instance with an admin account refuses session-less join AND refuses non-admin owner!
    let mut with_acct = JoinTestHarness::start_with_mode(true, true);
    let (db, _) = with_acct
        .service
        .authentication
        .as_ref()
        .unwrap()
        .provisioning_storage();
    let verifier =
        hash_password(&Password::new("synthetic nonadmin password".into()).unwrap()).unwrap();
    let non_admin_acct = crate::ids::generate().unwrap();
    db.seed_account(
        &non_admin_acct,
        "NonAdminMember",
        &verifier,
        false,
        with_acct.now_ms as i64,
    )
    .unwrap();
    let verified_non_admin = service_tests::exchange::<AccountVerify>(
        &mut with_acct.peer,
        3,
        &AccountVerifyParams {
            login: "NonAdminMember".into(),
            password: Password::new("synthetic nonadmin password".into()).unwrap(),
        },
    )
    .unwrap()
    .into_result::<AccountVerify>()
    .unwrap();
    let non_admin_session = SessionHandle::new(verified_non_admin.session.expose().into()).unwrap();
    let tok2 = make_invitation(
        &with_acct.org_id,
        &with_acct.sat_id,
        &with_acct.hq_id,
        with_acct.now_ms + 600_000,
    );
    let refused = service_tests::exchange::<OfficeJoin>(
        &mut with_acct.peer,
        4,
        &OfficeJoinParams {
            session: Some(non_admin_session),
            operation_id: crate::ids::uuid_v7().unwrap(),
            invitation: InvitationTokenSecret::new(tok2).unwrap(),
            consent: true,
        },
    )
    .unwrap()
    .into_result::<OfficeJoin>();
    assert!(
        matches!(refused, Err(CallError::Api(ref e)) if e.code == ErrorCode::NotAuthorized),
        "non-admin account must be refused office.join: {refused:?}"
    );
    with_acct.service.offices.shutdown();
}

#[test]
fn office_d25_re_enroll_and_remove_pending() {
    let mut h = JoinTestHarness::start_with_mode(true, true);
    let new_hq_id = crate::ids::uuid_v7().unwrap();
    let re_token = make_invitation(
        &h.org_id,
        &h.enrolled_sat_id,
        &new_hq_id,
        h.now_ms + 600_000,
    );
    let sat_id = h.enrolled_sat_id.clone();
    let s1 = h.session();
    let re_res = service_tests::exchange::<OfficeReEnroll>(
        &mut h.peer,
        7,
        &OfficeReEnrollParams {
            session: s1,
            operation_id: crate::ids::uuid_v7().unwrap(),
            office_id: sat_id,
            invitation: InvitationTokenSecret::new(re_token).unwrap(),
            consent: true,
        },
    )
    .unwrap()
    .into_result::<OfficeReEnroll>()
    .unwrap();
    match re_res {
        HostOpResult::Completed { result } => {
            assert_eq!(result.office_id, h.enrolled_sat_id);
            assert_eq!(result.hq_office_id, new_hq_id);
            assert_eq!(
                result.state,
                org_host_control::ops::OfficeJoinState::JoinPending
            );
            assert_eq!(result.check_code.as_deref(), Some("ABCD-EFGH"));
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    // Write a pending-join file for an unjoined office and remove it via `office.remove_pending`.
    let pending_id = crate::ids::uuid_v7().unwrap();
    write_pending_join(
        h.service.offices.home(),
        &PendingJoinRecord {
            v: 1,
            office_id: pending_id.clone(),
            office_name: "Pending".into(),
            org_id: h.org_id.clone(),
            org_name: "Acme".into(),
            hq_office_id: h.hq_id.clone(),
            hq_addresses: vec!["127.0.0.1:9443".into()],
            invitation_id: crate::ids::uuid_v7().unwrap(),
            ca_fingerprint_hex: "00".repeat(32),
            re_enroll: false,
            created_at_ms: h.now_ms as i64,
            expires_at_ms: h.now_ms + 600_000,
        },
    )
    .unwrap();
    let s2 = h.session();
    let removed = service_tests::exchange::<OfficeRemovePending>(
        &mut h.peer,
        8,
        &OfficeRemovePendingParams {
            session: s2,
            operation_id: crate::ids::uuid_v7().unwrap(),
            office_id: pending_id,
        },
    )
    .unwrap()
    .into_result::<OfficeRemovePending>()
    .unwrap();
    assert!(matches!(removed, HostOpResult::Completed { .. }));
    h.service.offices.shutdown();
}
