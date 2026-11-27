//! Native `host.sock` local-authless session issuance (`session.local`) and organization
//! lifecycle calls (`org.create`, `org.lookup`, `org.list`) (`TASK-113`).
//!
//! These calls are strictly isolated from browser-authenticated `organization.*` /
//! `operation.lookup` ops:
//! - Every op here is refused with `not_authorized` if it arrives on the web control connection.
//! - `session.local` is additionally refused whenever `web_tls` is configured, the instance
//!   listener uses HTTPS, or `HostDb::local_authless_account()` is `None` (authentication enabled).
//! - `org.create`, `org.lookup`, and `org.list` require a live native `SessionHandle` verified
//!   before full parameter decoding (`authorize_session_first`) and delegate directly to
//!   [`crate::provisioning::Provisioning`].

use std::sync::Arc;

use org_host_control::journal;
use org_host_control::listen::ListenMode;
use org_host_control::ops::{OrgCreate, OrgList, OrgOperationLookup, SessionLocal};
use org_proto::{ApiError, ErrorCode, Request, Response};

use super::reply::{not_authorized, ok, secret_response};
use super::session_office::authorize_session_first;
use super::Service;
use crate::authentication::Authentication;
use crate::control_listener::Connection;

pub(super) fn is_native_org_or_local_session(request: &Request) -> bool {
    request.is::<SessionLocal>()
        || request.is::<OrgCreate>()
        || request.is::<OrgOperationLookup>()
        || request.is::<OrgList>()
}

pub(super) fn dispatch(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Arc<Connection>,
) -> bool {
    if !is_native_org_or_local_session(request) {
        return false;
    }
    if connection.web_listen().is_some() {
        connection.send_response(not_authorized(request.id));
        return true;
    }
    if request.is::<SessionLocal>() {
        handle_session_local(service, authentication, request, connection);
    } else if request.is::<OrgCreate>() {
        handle_org_create(service, authentication, request, connection);
    } else if request.is::<OrgOperationLookup>() {
        handle_org_lookup(service, authentication, request, connection);
    } else {
        handle_org_list(service, authentication, request, connection);
    }
    true
}

fn handle_session_local(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Arc<Connection>,
) {
    let denied = || {
        ApiError::new(
            ErrorCode::NotAuthorized,
            "local authless session is unavailable on this host",
        )
    };
    let result = (|| {
        if service.web_tls.is_some() {
            return Err(denied());
        }
        if let Some(instance) = &service.instance {
            let listen = instance.browser().ok().flatten().ok_or_else(denied)?;
            if !matches!(
                listen.scheme(),
                ListenMode::HttpLoopback | ListenMode::HttpPublished
            ) {
                return Err(denied());
            }
        }
        let (db, _) = authentication.provisioning_storage();
        let account_id = db
            .local_authless_account()
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "host registry unavailable"))?
            .ok_or_else(denied)?;
        request.params::<SessionLocal>().map_err(|_| {
            ApiError::new(ErrorCode::InvalidRequest, "invalid local session request")
        })?;
        let mut issued = authentication.issue_local_authless(connection, &account_id)?;
        let offices = service.offices.offices();
        for link in &mut issued.orgs {
            link.office_ids = offices
                .iter()
                .filter(|office| office.organization_id == link.org_id)
                .map(|office| office.id.clone())
                .collect();
        }
        Ok(issued)
    })();
    match result {
        Ok(issued) => secret_response(connection, request.id, &issued),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn provisioning_unavailable() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "organization creation is unavailable",
    )
}

fn handle_org_create(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Arc<Connection>,
) {
    let invalid = |_| ApiError::new(ErrorCode::InvalidRequest, "invalid organization request");
    let reply = authorize_session_first(authentication, request, connection).and_then(|account| {
        let params = request.params::<OrgCreate>().map_err(invalid)?;
        let fingerprint = journal::fingerprint(&params).map_err(invalid)?;
        let provisioning = service
            .provisioning
            .as_ref()
            .ok_or_else(provisioning_unavailable)?;
        provisioning.create(&account, &params.operation_id, &params.plan, fingerprint)
    });
    match reply {
        Ok(result) => connection.send_response(ok::<OrgCreate>(request.id, &result)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn handle_org_lookup(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Arc<Connection>,
) {
    let invalid = |_| ApiError::new(ErrorCode::InvalidRequest, "invalid organization lookup");
    let reply = authorize_session_first(authentication, request, connection).and_then(|account| {
        let params = request.params::<OrgOperationLookup>().map_err(invalid)?;
        let provisioning = service
            .provisioning
            .as_ref()
            .ok_or_else(provisioning_unavailable)?;
        provisioning.lookup(&account, &params.operation_id)
    });
    match reply {
        Ok(result) => connection.send_response(ok::<OrgOperationLookup>(request.id, &result)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn handle_org_list(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Arc<Connection>,
) {
    let invalid = |_| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid organization list request",
        )
    };
    let reply = authorize_session_first(authentication, request, connection).and_then(|account| {
        let _params = request.params::<OrgList>().map_err(invalid)?;
        let provisioning = service
            .provisioning
            .as_ref()
            .ok_or_else(provisioning_unavailable)?;
        provisioning.list(&account)
    });
    match reply {
        Ok(result) => connection.send_response(ok::<OrgList>(request.id, &result)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use org_host_control::ops::{
        AccountVerify, AccountVerifyParams, OrgCreate, OrgCreateParams, OrgList, OrgListParams,
        OrgOperationLookup, OrgOperationLookupParams, SessionLocal, SessionLocalParams,
    };
    use org_host_control::{HostOpResult, Password, SessionHandle};
    use org_proto::selected_bootstrap::PlanV1;
    use org_proto::{ApiError, CallError, ErrorCode, Hello, OfficeRole};

    use crate::accounts::rules::Username;
    use crate::provisioning::{Hq, Provisioning};
    use crate::writer::Writers;
    use crate::{authentication::hash_password, control_listener, service::tests as service_tests};

    struct InstantHq(Mutex<std::collections::BTreeMap<String, org_proto::Organization>>);

    impl Hq for InstantHq {
        fn provision_hq(
            &self,
            first: org_office_launch::CreateLine,
            _: org_proto::RecoveryCode,
        ) -> Result<(), ApiError> {
            let office = serde_json::from_value(serde_json::json!({
                "id": first.office.office_id,
                "organization_id": first.org_id,
                "name": "HQ",
                "role": "hq",
                "host": "This Computer",
                "endpoint": "/private/office.sock",
                "status": "active",
            }))
            .unwrap();
            self.0.lock().unwrap().insert(
                first.org_id.clone(),
                org_proto::Organization {
                    id: first.org_id,
                    name: first.org_name,
                    created_at: "2026-11-25T00:00:00.000Z".into(),
                    offices: vec![office],
                },
            );
            Ok(())
        }

        fn organization(&self, id: &str) -> Option<org_proto::Organization> {
            self.0.lock().unwrap().get(id).cloned()
        }
    }

    fn assert_api_code(error: CallError, expected: ErrorCode) {
        match error {
            CallError::Api(api) => assert_eq!(api.code, expected, "{api:?}"),
            other => panic!("expected Api({expected:?}), got {other:?}"),
        }
    }

    fn sample_plan(name: &str, slug: &str) -> PlanV1 {
        PlanV1 {
            version: 1,
            name: name.into(),
            slug: slug.into(),
            repository_remote_url: "https://example.test/org.git".into(),
            state_branch: "main".into(),
        }
    }

    fn authless_service(
        with_tls: bool,
    ) -> (
        tempfile::TempDir,
        tempfile::TempDir,
        tempfile::TempDir,
        Arc<crate::hostdb::HostDb>,
        Arc<crate::offices::Offices>,
        Arc<crate::service::Service>,
    ) {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let instance = crate::ids::uuid_v7().unwrap();
        let home = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let keys_tmp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(keys_tmp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let state_root = state.path().canonicalize().unwrap();
        let secrets_dir = keys_tmp.path().canonicalize().unwrap();
        let office_home = state_root.join("office-state");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&office_home)
            .unwrap();
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(office_home.join(org_office_launch::OFFICES_DIR_NAME))
            .unwrap();

        let custody: Arc<dyn crate::host_secrets::HostSecrets> = crate::host_secrets::store_for(
            Some(&secrets_dir),
            &state_root,
            &instance,
            crate::code_signing::own_signing,
            None,
        )
        .unwrap();
        let db = Arc::new(
            crate::hostdb::HostDb::create(
                &office_home,
                &*custody,
                &crate::hostdb::HostDbInit {
                    instance_id: instance.clone(),
                    clock_high_ms: 10_000,
                },
            )
            .unwrap(),
        );
        db.init_local_authless_if_empty(10_500).unwrap().unwrap();
        let auth = Arc::new(
            crate::authentication::Authentication::open(
                &office_home,
                Arc::clone(&custody),
                &instance,
            )
            .unwrap(),
        );
        let (db_for_prov, custody_for_prov) = auth.provisioning_storage();
        let store_for_writers = Arc::clone(&db_for_prov) as Arc<dyn crate::hostdb::HostStore>;
        let writers = Writers::open(
            Arc::clone(&custody_for_prov),
            store_for_writers,
            &instance,
            &state_root,
        )
        .unwrap();
        let provisioning = Arc::new(Provisioning::new(
            Arc::clone(&db_for_prov),
            Arc::clone(&custody_for_prov),
            Arc::new(crate::keys::memory::MemoryKeys::default()),
            writers,
            &instance,
            &office_home,
        ));
        provisioning.start(Arc::new(InstantHq(Mutex::default())));

        let mut service = service_tests::service(home.path())
            .with_authentication(Some(auth))
            .with_provisioning(Some(provisioning));
        service.instance_id = Some(instance);
        if with_tls {
            let listen: org_host_control::listen::ListenConfig = serde_json::from_str(
                r#"{"address":"127.0.0.1","port":8443,"scheme":"https","origin":"https://127.0.0.1:8443","hsts":"off"}"#,
            )
            .unwrap();
            let web_tls = crate::web_tls::WebTls::start(
                Arc::clone(&db_for_prov),
                Arc::clone(&custody_for_prov),
                listen.origin().clone(),
                None,
                crate::web_tls::now().unwrap(),
            )
            .unwrap();
            service = service.with_web_tls(Some(web_tls));
        }
        let offices = Arc::clone(&service.offices);
        (
            home,
            state,
            keys_tmp,
            db_for_prov,
            offices,
            Arc::new(service),
        )
    }

    #[test]
    fn session_local_and_native_org_ops_work_in_authless_http_and_fail_closed_when_auth_enabled() {
        let (_home, _state, _keys, db, offices, service) = authless_service(false);
        let (host_stream, peer_stream) = UnixStream::pair().unwrap();
        let handler = Arc::clone(&service) as Arc<dyn control_listener::Handler>;
        let _local = control_listener::LocalConnection::start(host_stream, handler).unwrap();
        peer_stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut peer = BufReader::new(peer_stream);
        service_tests::exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();

        // 1. session.local issues a live admin session in local authless HTTP mode.
        let local = service_tests::exchange::<SessionLocal>(&mut peer, 2, &SessionLocalParams {})
            .unwrap()
            .into_result::<SessionLocal>()
            .unwrap();
        assert!(local.admin);
        let handle = || SessionHandle::new(local.session.expose().into()).unwrap();

        // 2. org.create -> org.lookup -> org.list complete end-to-end on host.sock.
        let op_id = crate::ids::uuid_v7().unwrap();
        let created = service_tests::exchange::<OrgCreate>(
            &mut peer,
            3,
            &OrgCreateParams {
                session: handle(),
                operation_id: op_id.clone(),
                plan: sample_plan("Acme CLI", "acme-cli"),
            },
        )
        .unwrap()
        .into_result::<OrgCreate>()
        .unwrap();
        assert!(matches!(
            created,
            HostOpResult::Accepted | HostOpResult::Completed { .. }
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        let view = loop {
            match service_tests::exchange::<OrgOperationLookup>(
                &mut peer,
                4,
                &OrgOperationLookupParams {
                    session: handle(),
                    operation_id: op_id.clone(),
                },
            )
            .unwrap()
            .into_result::<OrgOperationLookup>()
            .unwrap()
            {
                HostOpResult::Completed { result } => break result,
                HostOpResult::Accepted => {
                    assert!(Instant::now() < deadline, "org.create never completed");
                    std::thread::sleep(Duration::from_millis(20));
                }
                other => panic!("unexpected lookup outcome: {other:?}"),
            }
        };
        assert_eq!(view.slug, "acme-cli");
        assert_eq!(view.offices.len(), 1);
        assert_eq!(view.offices[0].role, OfficeRole::Hq);
        assert_eq!(view.offices[0].endpoint, None);

        let listed =
            service_tests::exchange::<OrgList>(&mut peer, 5, &OrgListParams { session: handle() })
                .unwrap()
                .into_result::<OrgList>()
                .unwrap();
        assert_eq!(listed.organizations.len(), 1);
        assert_eq!(listed.organizations[0].id, view.id);

        // 3. Browser-only organization.create remains strictly refused on host.sock.
        writeln!(
            peer.get_mut(),
            r#"{{"id":6,"method":"organization.create","params":{{}}}}"#
        )
        .unwrap();
        let mut raw_line = String::new();
        std::io::BufRead::read_line(&mut peer, &mut raw_line).unwrap();
        let raw_resp: serde_json::Value = serde_json::from_str(&raw_line).unwrap();
        assert_eq!(raw_resp["error"]["code"], "not_authorized");

        // 4. Sessionless org.list fails closed with NotAuthorized.
        writeln!(
            peer.get_mut(),
            r#"{{"id":7,"method":"org.list","params":{{}}}}"#
        )
        .unwrap();
        raw_line.clear();
        std::io::BufRead::read_line(&mut peer, &mut raw_line).unwrap();
        let raw_resp: serde_json::Value = serde_json::from_str(&raw_line).unwrap();
        assert_eq!(raw_resp["error"]["code"], "not_authorized");

        // 5. Enabling authentication immediately closes BOTH new `session.local` requests AND
        //    already-issued `session.local` handles—even at the exact same timestamp (`10_000`)
        //    where `password_changed_at_ms` does not advance.
        let verifier =
            hash_password(&Password::new("synthetic test password".into()).unwrap()).unwrap();
        db.enable_local_auth(&Username::new("Owner").unwrap(), &verifier, 10_000)
            .unwrap();
        let existing_handle_refused =
            service_tests::exchange::<OrgList>(&mut peer, 8, &OrgListParams { session: handle() })
                .unwrap()
                .into_result::<OrgList>()
                .unwrap_err();
        assert_api_code(existing_handle_refused, ErrorCode::NotAuthorized);

        let refused = service_tests::exchange::<SessionLocal>(&mut peer, 9, &SessionLocalParams {})
            .unwrap()
            .into_result::<SessionLocal>()
            .unwrap_err();
        assert_api_code(refused, ErrorCode::NotAuthorized);

        // Malformed `session.local` params while auth is enabled must still answer NotAuthorized
        // (not InvalidRequest) so auth state is checked before param decoding.
        writeln!(
            peer.get_mut(),
            r#"{{"id":10,"method":"session.local","params":{{"unexpected":true}}}}"#
        )
        .unwrap();
        raw_line.clear();
        std::io::BufRead::read_line(&mut peer, &mut raw_line).unwrap();
        let raw_resp: serde_json::Value = serde_json::from_str(&raw_line).unwrap();
        assert_eq!(raw_resp["error"]["code"], "not_authorized");

        // 6. Password-authenticated admin session continues to list the created org.
        let signed = service_tests::exchange::<AccountVerify>(
            &mut peer,
            11,
            &AccountVerifyParams {
                login: "Owner".into(),
                password: Password::new("synthetic test password".into()).unwrap(),
            },
        )
        .unwrap()
        .into_result::<AccountVerify>()
        .unwrap();
        let listed_after = service_tests::exchange::<OrgList>(
            &mut peer,
            12,
            &OrgListParams {
                session: signed.session,
            },
        )
        .unwrap()
        .into_result::<OrgList>()
        .unwrap();
        assert_eq!(listed_after.organizations.len(), 1);
        assert_eq!(listed_after.organizations[0].id, view.id);

        drop(peer);
        offices.shutdown();
    }

    #[test]
    fn session_local_and_native_org_ops_refuse_https_and_web_connections() {
        // HTTPS instance refuses session.local even if local_authless_account exists.
        let (_home, _state, _keys, _db, offices_tls, tls_service) = authless_service(true);
        let (host_stream, peer_stream) = UnixStream::pair().unwrap();
        let _local = control_listener::LocalConnection::start(host_stream, tls_service).unwrap();
        peer_stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut peer = BufReader::new(peer_stream);
        service_tests::exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        let refused = service_tests::exchange::<SessionLocal>(&mut peer, 2, &SessionLocalParams {})
            .unwrap()
            .into_result::<SessionLocal>()
            .unwrap_err();
        assert_api_code(refused, ErrorCode::NotAuthorized);
        drop(peer);
        offices_tls.shutdown();

        // Web control connection refuses session.local and org.* with NotAuthorized.
        let (_home2, _state2, _keys2, _db2, _offices2, service_arc) = authless_service(false);
        let service_val = Arc::try_unwrap(service_arc).ok().expect("sole owner");
        let (web, mut web_peer, web_offices) = service_tests::web_peer_for(
            service_val,
            r#"{"address":"127.0.0.1","port":48129,"scheme":"http_loopback","origin":"http://127.0.0.1:48129","hsts":"off"}"#,
        );
        service_tests::exchange::<Hello>(&mut web_peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        let web_local =
            service_tests::exchange::<SessionLocal>(&mut web_peer, 2, &SessionLocalParams {})
                .unwrap()
                .into_result::<SessionLocal>()
                .unwrap_err();
        assert_api_code(web_local, ErrorCode::NotAuthorized);
        let web_list = service_tests::exchange::<OrgList>(
            &mut web_peer,
            3,
            &OrgListParams {
                session: SessionHandle::new("unused".into()).unwrap(),
            },
        )
        .unwrap()
        .into_result::<OrgList>()
        .unwrap_err();
        assert_api_code(web_list, ErrorCode::NotAuthorized);
        drop(web);
        drop(web_peer);
        web_offices.shutdown();

        // Instance with HTTPS listener in `instance.json` (even when `web_tls` is None) OR
        // missing/unreadable `instance.json` fails closed on `session.local`.
        for browser_json in [
            Some(
                r#"{"address":"127.0.0.1","port":8443,"scheme":"https","origin":"https://office.example:8443","hsts":"off"}"#,
            ),
            None,
        ] {
            use std::os::unix::fs::PermissionsExt;
            let (_home_i, _state_i, _keys_i, _db_i, offices_i, service_arc_i) =
                authless_service(false);
            let mut service_i = Arc::try_unwrap(service_arc_i).ok().expect("sole owner");
            let reg_tmp =
                tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap())
                    .unwrap();
            let base = reg_tmp.path().canonicalize().unwrap();
            let loc = base.join("loc");
            let user_tmp = base.join("tmp");
            let inst_root = base.join("inst");
            for dir in [&loc, &user_tmp, &inst_root] {
                std::fs::create_dir(dir).unwrap();
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            let locator = crate::instance::registry::Registry::open(&loc, &user_tmp).unwrap();
            let handle = locator.reserve(None, &inst_root).unwrap();
            if let Some(json) = browser_json {
                let listen: org_host_control::listen::ListenConfig =
                    serde_json::from_str(json).unwrap();
                handle.set_browser(&listen).unwrap();
            }
            service_i.instance = Some(Arc::new(handle));
            let (host_stream, peer_stream) = UnixStream::pair().unwrap();
            let _local =
                control_listener::LocalConnection::start(host_stream, Arc::new(service_i)).unwrap();
            peer_stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut peer = BufReader::new(peer_stream);
            service_tests::exchange::<Hello>(&mut peer, 1, &Default::default())
                .unwrap()
                .into_result::<Hello>()
                .unwrap();
            let refused =
                service_tests::exchange::<SessionLocal>(&mut peer, 2, &SessionLocalParams {})
                    .unwrap()
                    .into_result::<SessionLocal>()
                    .unwrap_err();
            assert_api_code(refused, ErrorCode::NotAuthorized);
            drop(peer);
            offices_i.shutdown();
        }
    }
}
