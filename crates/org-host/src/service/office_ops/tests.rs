use std::sync::Arc;

use org_host_control::{
    ops::{
        AccountVerify, AccountVerifyParams, OfficeList, OfficeListParams, OfficeRestart,
        OfficeRestartParams,
    },
    HostOpResult, Password, SessionHandle,
};
use org_office_launch::OfficeFile;
use org_proto::{CallError, ErrorCode, Hello, OfficeRole};

use crate::{authentication::hash_password, control_listener, service::tests as service_tests};

fn office_file(org_id: &str, office_id: &str, org_name: &str) -> OfficeFile {
    OfficeFile::new(
        office_id,
        "HQ",
        OfficeRole::Hq,
        org_id,
        org_name,
        "2026-11-21T00:00:00.000Z",
    )
}

fn assert_api_code(error: CallError, expected: ErrorCode) {
    match error {
        CallError::Api(api) => assert_eq!(api.code, expected, "{api:?}"),
        other => panic!("expected Api({expected:?}), got {other:?}"),
    }
}

fn sign_in_native(
    peer: &mut std::io::BufReader<std::os::unix::net::UnixStream>,
    id: u64,
    login: &str,
) -> SessionHandle {
    let verified = service_tests::exchange::<AccountVerify>(
        peer,
        id,
        &AccountVerifyParams {
            login: login.to_owned(),
            password: Password::new("synthetic test password".into()).unwrap(),
        },
    )
    .unwrap()
    .into_result::<AccountVerify>()
    .unwrap();
    SessionHandle::new(verified.session.expose().into()).unwrap()
}

#[test]
fn office_list_and_restart_enforce_authority_scoping_and_idempotency() {
    let home = tempfile::tempdir().unwrap();
    let (_root, authentication, _) = crate::authentication::tests::fixture();
    let (db, _) = authentication.provisioning_storage();
    let now = super::browser_now_ms().unwrap();
    let verifier =
        hash_password(&Password::new("synthetic test password".into()).unwrap()).unwrap();

    let org_a = crate::ids::uuid_v7().unwrap();
    let org_b = crate::ids::uuid_v7().unwrap();
    let office_a = crate::ids::uuid_v7().unwrap();
    let office_a2 = crate::ids::uuid_v7().unwrap();
    let office_b = crate::ids::uuid_v7().unwrap();

    let owner_acct = crate::ids::generate().unwrap();
    let member_acct = crate::ids::generate().unwrap();
    db.seed_account(&owner_acct, "OrgOwner", &verifier, false, now)
        .unwrap();
    db.seed_account(&member_acct, "Member", &verifier, false, now)
        .unwrap();
    db.seed_owner_for_test(&owner_acct, &org_a, "mem_owner")
        .unwrap();
    db.seed_link_for_test(&member_acct, &org_a, "mem_regular")
        .unwrap();

    let mut service =
        service_tests::service(home.path()).with_authentication(Some(Arc::new(authentication)));
    service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    service
        .offices
        .insert_catalog_for_test(office_file(&org_a, &office_a, "Alpha"));
    service
        .offices
        .insert_catalog_for_test(office_file(&org_a, &office_a2, "Alpha"));
    service
        .offices
        .insert_catalog_for_test(office_file(&org_b, &office_b, "Beta"));
    let offices = service.offices.clone();

    let (host_stream, peer_stream) = std::os::unix::net::UnixStream::pair().unwrap();
    let _local = control_listener::LocalConnection::start(host_stream, Arc::new(service)).unwrap();
    peer_stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let mut peer = std::io::BufReader::new(peer_stream);
    service_tests::exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();

    // 1. Regular linked member sees only org_a's offices with endpoint stripped.
    let member_session = sign_in_native(&mut peer, 2, "Member");
    let listed = service_tests::exchange::<OfficeList>(
        &mut peer,
        3,
        &OfficeListParams {
            session: SessionHandle::new(member_session.expose().into()).unwrap(),
            org_id: None,
        },
    )
    .unwrap()
    .into_result::<OfficeList>()
    .unwrap();
    assert_eq!(listed.offices.len(), 2);
    assert!(listed.offices.iter().all(|o| o.organization_id == org_a));
    assert!(listed.offices.iter().all(|o| o.endpoint.is_none()));

    // Regular member requesting foreign org_b gets NotFound.
    let foreign_err = service_tests::exchange::<OfficeList>(
        &mut peer,
        4,
        &OfficeListParams {
            session: SessionHandle::new(member_session.expose().into()).unwrap(),
            org_id: Some(org_b.clone()),
        },
    )
    .unwrap()
    .into_result::<OfficeList>()
    .unwrap_err();
    assert_api_code(foreign_err, ErrorCode::NotFound);

    // Regular member (not admin, not owner) is refused restart with NotAuthorized.
    let denied = service_tests::exchange::<OfficeRestart>(
        &mut peer,
        5,
        &OfficeRestartParams {
            session: SessionHandle::new(member_session.expose().into()).unwrap(),
            operation_id: crate::ids::uuid_v7().unwrap(),
            office_id: office_a.clone(),
        },
    )
    .unwrap()
    .into_result::<OfficeRestart>()
    .unwrap_err();
    assert_api_code(denied, ErrorCode::NotAuthorized);

    // Regular member restarting foreign office_b gets NotFound (no existence oracle).
    let foreign_restart = service_tests::exchange::<OfficeRestart>(
        &mut peer,
        6,
        &OfficeRestartParams {
            session: SessionHandle::new(member_session.expose().into()).unwrap(),
            operation_id: crate::ids::uuid_v7().unwrap(),
            office_id: office_b.clone(),
        },
    )
    .unwrap()
    .into_result::<OfficeRestart>()
    .unwrap_err();
    assert_api_code(foreign_restart, ErrorCode::NotFound);

    // 2. Non-admin organization owner CAN restart unsupervised office_a -> Completed.
    let owner_session = sign_in_native(&mut peer, 7, "OrgOwner");
    let op_id = crate::ids::uuid_v7().unwrap();
    let restarted = service_tests::exchange::<OfficeRestart>(
        &mut peer,
        8,
        &OfficeRestartParams {
            session: SessionHandle::new(owner_session.expose().into()).unwrap(),
            operation_id: op_id.clone(),
            office_id: office_a.clone(),
        },
    )
    .unwrap()
    .into_result::<OfficeRestart>()
    .unwrap();
    assert!(matches!(restarted, HostOpResult::Completed { .. }));

    // Replaying the same operation_id for the same office returns Completed idempotently.
    let replayed = service_tests::exchange::<OfficeRestart>(
        &mut peer,
        9,
        &OfficeRestartParams {
            session: SessionHandle::new(owner_session.expose().into()).unwrap(),
            operation_id: op_id.clone(),
            office_id: office_a.clone(),
        },
    )
    .unwrap()
    .into_result::<OfficeRestart>()
    .unwrap();
    assert!(matches!(replayed, HostOpResult::Completed { .. }));

    // Reusing the same operation_id for a different office returns OperationReused.
    let reused = service_tests::exchange::<OfficeRestart>(
        &mut peer,
        10,
        &OfficeRestartParams {
            session: SessionHandle::new(owner_session.expose().into()).unwrap(),
            operation_id: op_id,
            office_id: office_a2,
        },
    )
    .unwrap()
    .into_result::<OfficeRestart>()
    .unwrap_err();
    assert_api_code(reused, ErrorCode::OperationReused);

    drop(peer);
    offices.shutdown();
}

#[test]
fn browser_office_restart_and_sessionless_calls_enforce_boundaries() {
    use org_host_control::{
        browser::{
            BrowserOfficeRestart, BrowserOfficeRestartParams, BrowserOrigin, Digest32,
            SessionSignIn, SessionSignInParams,
        },
        organization::BrowserAuth,
    };
    use std::io::{BufRead, Write};

    let home = tempfile::tempdir().unwrap();
    let (_root, authentication, _) = crate::authentication::tests::fixture();
    let (db, _) = authentication.provisioning_storage();
    let now = super::browser_now_ms().unwrap();
    let verifier =
        hash_password(&Password::new("synthetic test password".into()).unwrap()).unwrap();

    let org_a = crate::ids::uuid_v7().unwrap();
    let org_b = crate::ids::uuid_v7().unwrap();
    let office_a = crate::ids::uuid_v7().unwrap();
    let office_a2 = crate::ids::uuid_v7().unwrap();
    let office_b = crate::ids::uuid_v7().unwrap();

    let owner_acct = crate::ids::generate().unwrap();
    let member_acct = crate::ids::generate().unwrap();
    db.seed_account(&owner_acct, "OrgOwner", &verifier, false, now)
        .unwrap();
    db.seed_account(&member_acct, "Member", &verifier, false, now)
        .unwrap();
    // Owner owns org_a and is also linked to org_b (to test cross-org mismatch).
    db.seed_owner_for_test(&owner_acct, &org_a, "mem_owner_a")
        .unwrap();
    db.seed_owner_for_test(&owner_acct, &org_b, "mem_owner_b")
        .unwrap();
    db.seed_link_for_test(&member_acct, &org_a, "mem_regular_a")
        .unwrap();

    let auth = Arc::new(authentication);
    let service = service_tests::service(home.path()).with_authentication(Some(auth.clone()));
    service
        .offices
        .insert_catalog_for_test(office_file(&org_a, &office_a, "Alpha"));
    service
        .offices
        .insert_catalog_for_test(office_file(&org_a, &office_a2, "Alpha"));
    service
        .offices
        .insert_catalog_for_test(office_file(&org_b, &office_b, "Beta"));

    let (web, mut peer, offices) = service_tests::web_peer_for(
        service,
        r#"{"address":"127.0.0.1","port":48124,"scheme":"http_loopback","origin":"http://127.0.0.1:48124","hsts":"off"}"#,
    );
    service_tests::exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();

    let origin = BrowserOrigin {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port: 48124,
    };
    let browser_sign_in = |peer: &mut std::io::BufReader<std::os::unix::net::UnixStream>,
                           id: u64,
                           login: &str,
                           seed: u8|
     -> BrowserAuth {
        let cookie_digest = Digest32([seed; 32]);
        let proof_digest = Digest32([seed.wrapping_add(1); 32]);
        service_tests::exchange::<SessionSignIn>(
            peer,
            id,
            &SessionSignInParams {
                login: login.to_owned(),
                password: Password::new("synthetic test password".into()).unwrap(),
                cookie_digest,
                proof_digest,
                origin: origin.clone(),
                client_address: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            },
        )
        .unwrap()
        .into_result::<SessionSignIn>()
        .unwrap();
        BrowserAuth {
            cookie_digest,
            proof_digest,
            origin: origin.clone(),
            local_authless: false,
        }
    };

    let owner_auth = browser_sign_in(&mut peer, 2, "OrgOwner", 11);
    let member_auth = browser_sign_in(&mut peer, 3, "Member", 22);

    // (a) Linked to both org_a and org_b, but mismatched `(org_a, office_b)` -> NotFound.
    let mismatch = service_tests::exchange::<BrowserOfficeRestart>(
        &mut peer,
        4,
        &BrowserOfficeRestartParams {
            auth: owner_auth.clone(),
            operation_id: crate::ids::uuid_v7().unwrap(),
            org_id: org_a.clone(),
            office_id: office_b,
        },
    )
    .unwrap()
    .into_result::<BrowserOfficeRestart>()
    .unwrap_err();
    assert_api_code(mismatch, ErrorCode::NotFound);

    // (b) Regular linked member on `browser.office_restart` -> NotAuthorized.
    let member_denied = service_tests::exchange::<BrowserOfficeRestart>(
        &mut peer,
        5,
        &BrowserOfficeRestartParams {
            auth: member_auth,
            operation_id: crate::ids::uuid_v7().unwrap(),
            org_id: org_a.clone(),
            office_id: office_a.clone(),
        },
    )
    .unwrap()
    .into_result::<BrowserOfficeRestart>()
    .unwrap_err();
    assert_api_code(member_denied, ErrorCode::NotAuthorized);

    // (c) Non-admin org owner succeeds, replays idempotently, and refuses reuse with another office.
    let op_id = crate::ids::uuid_v7().unwrap();
    let completed = service_tests::exchange::<BrowserOfficeRestart>(
        &mut peer,
        6,
        &BrowserOfficeRestartParams {
            auth: owner_auth.clone(),
            operation_id: op_id.clone(),
            org_id: org_a.clone(),
            office_id: office_a.clone(),
        },
    )
    .unwrap()
    .into_result::<BrowserOfficeRestart>()
    .unwrap();
    assert!(matches!(completed, HostOpResult::Completed { .. }));

    let replayed = service_tests::exchange::<BrowserOfficeRestart>(
        &mut peer,
        7,
        &BrowserOfficeRestartParams {
            auth: owner_auth.clone(),
            operation_id: op_id.clone(),
            org_id: org_a.clone(),
            office_id: office_a.clone(),
        },
    )
    .unwrap()
    .into_result::<BrowserOfficeRestart>()
    .unwrap();
    assert!(matches!(replayed, HostOpResult::Completed { .. }));

    let reused = service_tests::exchange::<BrowserOfficeRestart>(
        &mut peer,
        8,
        &BrowserOfficeRestartParams {
            auth: owner_auth,
            operation_id: op_id,
            org_id: org_a,
            office_id: office_a2,
        },
    )
    .unwrap()
    .into_result::<BrowserOfficeRestart>()
    .unwrap_err();
    assert_api_code(reused, ErrorCode::OperationReused);

    drop(web);
    drop(peer);

    // (d) Retired session-less `office.list` and `office.restart` on `host.sock` with active
    // authentication must fail closed with `NotAuthorized` before parameter validation.
    let mut local_service = service_tests::service(home.path()).with_authentication(Some(auth));
    local_service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    let local_offices = local_service.offices.clone();
    let (host_stream, peer_stream) = std::os::unix::net::UnixStream::pair().unwrap();
    let _local =
        control_listener::LocalConnection::start(host_stream, Arc::new(local_service)).unwrap();
    peer_stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let mut local_peer = std::io::BufReader::new(peer_stream);
    service_tests::exchange::<Hello>(&mut local_peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();

    for (id, raw) in [
        (2, r#"{"id":2,"method":"office.list","params":{}}"#),
        (
            3,
            &format!(
                r#"{{"id":3,"method":"office.restart","params":{{"office_id":"{office_a}"}}}}"#
            ),
        ),
    ] {
        writeln!(local_peer.get_mut(), "{raw}").unwrap();
        let mut line = String::new();
        local_peer.read_line(&mut line).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(resp["id"], id);
        assert_eq!(resp["error"]["code"], "not_authorized");
    }

    drop(local_peer);
    local_offices.shutdown();
    offices.shutdown();
}

#[test]
fn satellite_admin_sees_and_restarts_unlinked_satellite_only() {
    let home = tempfile::tempdir().unwrap();
    let (_root, authentication, _) = crate::authentication::tests::fixture();
    let (db, _) = authentication.provisioning_storage();
    let now = super::browser_now_ms().unwrap();
    let verifier =
        hash_password(&Password::new("synthetic test password".into()).unwrap()).unwrap();

    let org_sat = crate::ids::uuid_v7().unwrap();
    let org_hq = crate::ids::uuid_v7().unwrap();
    let sat_office_id = crate::ids::uuid_v7().unwrap();
    let hq_office_id = crate::ids::uuid_v7().unwrap();

    let admin_acct = crate::ids::generate().unwrap();
    let regular_acct = crate::ids::generate().unwrap();
    db.seed_account(&admin_acct, "HostAdmin", &verifier, true, now)
        .unwrap();
    db.seed_account(&regular_acct, "Regular", &verifier, false, now)
        .unwrap();

    let mut service =
        service_tests::service(home.path()).with_authentication(Some(Arc::new(authentication)));
    service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    service.offices.insert_catalog_for_test(OfficeFile::new(
        &sat_office_id,
        "Berlin",
        OfficeRole::Satellite,
        &org_sat,
        "Acme",
        "2026-11-25T00:00:00.000Z",
    ));
    service
        .offices
        .insert_catalog_for_test(office_file(&org_hq, &hq_office_id, "ForeignHQ"));
    let offices = service.offices.clone();

    let (host_stream, peer_stream) = std::os::unix::net::UnixStream::pair().unwrap();
    let _local = control_listener::LocalConnection::start(host_stream, Arc::new(service)).unwrap();
    peer_stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let mut peer = std::io::BufReader::new(peer_stream);
    service_tests::exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();

    // Instance admin with zero org links sees the local satellite office ONLY (never unlinked HQ).
    let admin_session = sign_in_native(&mut peer, 2, "HostAdmin");
    let listed = service_tests::exchange::<OfficeList>(
        &mut peer,
        3,
        &OfficeListParams {
            session: SessionHandle::new(admin_session.expose().into()).unwrap(),
            org_id: None,
        },
    )
    .unwrap()
    .into_result::<OfficeList>()
    .unwrap();
    assert_eq!(listed.offices.len(), 1);
    assert_eq!(listed.offices[0].id, sat_office_id);
    assert_eq!(listed.offices[0].role, OfficeRole::Satellite);

    // Filtering by the satellite's org succeeds; filtering by an unlinked HQ org is NotFound.
    let filtered = service_tests::exchange::<OfficeList>(
        &mut peer,
        4,
        &OfficeListParams {
            session: SessionHandle::new(admin_session.expose().into()).unwrap(),
            org_id: Some(org_sat.clone()),
        },
    )
    .unwrap()
    .into_result::<OfficeList>()
    .unwrap();
    assert_eq!(filtered.offices.len(), 1);
    assert_eq!(filtered.offices[0].id, sat_office_id);

    let foreign_hq_list = service_tests::exchange::<OfficeList>(
        &mut peer,
        5,
        &OfficeListParams {
            session: SessionHandle::new(admin_session.expose().into()).unwrap(),
            org_id: Some(org_hq.clone()),
        },
    )
    .unwrap()
    .into_result::<OfficeList>()
    .unwrap_err();
    assert_api_code(foreign_hq_list, ErrorCode::NotFound);

    // Admin can restart the unlinked satellite, but gets NotFound for the unlinked HQ.
    let restarted = service_tests::exchange::<OfficeRestart>(
        &mut peer,
        6,
        &OfficeRestartParams {
            session: SessionHandle::new(admin_session.expose().into()).unwrap(),
            operation_id: crate::ids::uuid_v7().unwrap(),
            office_id: sat_office_id.clone(),
        },
    )
    .unwrap()
    .into_result::<OfficeRestart>()
    .unwrap();
    assert!(matches!(restarted, HostOpResult::Completed { .. }));

    let foreign_hq_restart = service_tests::exchange::<OfficeRestart>(
        &mut peer,
        7,
        &OfficeRestartParams {
            session: SessionHandle::new(admin_session.expose().into()).unwrap(),
            operation_id: crate::ids::uuid_v7().unwrap(),
            office_id: hq_office_id,
        },
    )
    .unwrap()
    .into_result::<OfficeRestart>()
    .unwrap_err();
    assert_api_code(foreign_hq_restart, ErrorCode::NotFound);

    // Unlinked non-admin sees zero offices and gets NotFound on both org_sat and sat_office_id.
    let regular_session = sign_in_native(&mut peer, 8, "Regular");
    let reg_listed = service_tests::exchange::<OfficeList>(
        &mut peer,
        9,
        &OfficeListParams {
            session: SessionHandle::new(regular_session.expose().into()).unwrap(),
            org_id: None,
        },
    )
    .unwrap()
    .into_result::<OfficeList>()
    .unwrap();
    assert!(reg_listed.offices.is_empty());

    let reg_restart = service_tests::exchange::<OfficeRestart>(
        &mut peer,
        10,
        &OfficeRestartParams {
            session: SessionHandle::new(regular_session.expose().into()).unwrap(),
            operation_id: crate::ids::uuid_v7().unwrap(),
            office_id: sat_office_id,
        },
    )
    .unwrap()
    .into_result::<OfficeRestart>()
    .unwrap_err();
    assert_api_code(reg_restart, ErrorCode::NotFound);

    drop(peer);
    offices.shutdown();
}

#[test]
fn browser_office_restart_refuses_unlinked_satellite_for_admin() {
    use org_host_control::{
        browser::{
            BrowserOfficeRestart, BrowserOfficeRestartParams, BrowserOrigin, Digest32,
            SessionSignIn, SessionSignInParams,
        },
        organization::BrowserAuth,
    };

    let home = tempfile::tempdir().unwrap();
    let (_root, authentication, _) = crate::authentication::tests::fixture();
    let (db, _) = authentication.provisioning_storage();
    let now = super::browser_now_ms().unwrap();
    let verifier =
        hash_password(&Password::new("synthetic test password".into()).unwrap()).unwrap();

    let org_sat = crate::ids::uuid_v7().unwrap();
    let sat_office_id = crate::ids::uuid_v7().unwrap();
    let admin_acct = crate::ids::generate().unwrap();
    db.seed_account(&admin_acct, "HostAdmin", &verifier, true, now)
        .unwrap();

    let service =
        service_tests::service(home.path()).with_authentication(Some(Arc::new(authentication)));
    service.offices.insert_catalog_for_test(OfficeFile::new(
        &sat_office_id,
        "Berlin",
        OfficeRole::Satellite,
        &org_sat,
        "Acme",
        "2026-11-25T00:00:00.000Z",
    ));

    let (web, mut peer, offices) = service_tests::web_peer_for(
        service,
        r#"{"address":"127.0.0.1","port":48125,"scheme":"http_loopback","origin":"http://127.0.0.1:48125","hsts":"off"}"#,
    );
    service_tests::exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    let origin = BrowserOrigin {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port: 48125,
    };
    let cookie_digest = Digest32([33; 32]);
    let proof_digest = Digest32([34; 32]);
    service_tests::exchange::<SessionSignIn>(
        &mut peer,
        2,
        &SessionSignInParams {
            login: "HostAdmin".into(),
            password: Password::new("synthetic test password".into()).unwrap(),
            cookie_digest,
            proof_digest,
            origin: origin.clone(),
            client_address: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        },
    )
    .unwrap()
    .into_result::<SessionSignIn>()
    .unwrap();
    let auth = BrowserAuth {
        cookie_digest,
        proof_digest,
        origin,
        local_authless: false,
    };
    let err = service_tests::exchange::<BrowserOfficeRestart>(
        &mut peer,
        3,
        &BrowserOfficeRestartParams {
            auth,
            operation_id: crate::ids::uuid_v7().unwrap(),
            org_id: org_sat,
            office_id: sat_office_id,
        },
    )
    .unwrap()
    .into_result::<BrowserOfficeRestart>()
    .unwrap_err();
    assert_api_code(err, ErrorCode::NotFound);

    drop(web);
    drop(peer);
    offices.shutdown();
}
