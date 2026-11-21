//! Live office listing and authorized office restart handlers for browser and native
//! sessions (implementation plan O1, FC-019, FC-021).

use org_host_control::{
    browser::{BrowserOfficeRestart, SessionValidateParams},
    ops::{OfficeList, OfficeRestart},
    HostOpResult,
};
use org_proto::{ApiError, Empty, ErrorCode, ListOfficesResult, OfficeInfo, Request, Response};

use super::{
    browser_now_ms, finish_ended_browser_sessions, listener::Connection, ok, Authentication,
    Service,
};
use crate::hostdb::{
    accounts::Account,
    journal::{Attempt, Effect, Lookup, Stored, Write},
    Error as DbError, HostDb,
};

fn journal_error(error: DbError) -> ApiError {
    match error {
        DbError::OperationReused => ApiError::new(
            ErrorCode::OperationReused,
            "this operation ID was already used for another request",
        ),
        DbError::Expired => ApiError::new(ErrorCode::Expired, "this operation ID has expired"),
        DbError::ClockSkew => ApiError::new(
            ErrorCode::ClockSkew,
            "operation ID timestamp is too far ahead of host clock",
        ),
        DbError::InvalidId => ApiError::new(ErrorCode::InvalidRequest, "invalid operation_id"),
        _ => ApiError::new(ErrorCode::Unavailable, "host registry unavailable"),
    }
}

/// Resolve an office visible to `account`. Reserved offices whose creation has not yet
/// linked the organization are intentionally absent from `offices` and answer `NotFound`,
/// preventing pre-link existence oracles; host startup waits still surface `NotReady`.
fn find_linked_office(
    offices: &[OfficeInfo],
    account: &Account,
    org_filter: Option<&str>,
    office_id: &str,
) -> Result<(OfficeInfo, String), ApiError> {
    let office = offices
        .iter()
        .find(|o| o.id == office_id && org_filter.is_none_or(|org| o.organization_id == org))
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "office not found"))?;
    let link = account
        .links()
        .iter()
        .find(|l| l.org_id() == office.organization_id)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "office not found"))?;
    Ok((office.clone(), link.member_id().to_owned()))
}

fn check_restart_authority(
    db: &HostDb,
    account: &Account,
    org_id: &str,
    member_id: &str,
) -> Result<(), ApiError> {
    if account.admin() {
        return Ok(());
    }
    db.verify_selected_owner(org_id, account.account_id(), member_id)
        .map_err(|error| match error {
            DbError::Unsafe => ApiError::new(
                ErrorCode::NotAuthorized,
                "only the administrator or organization owner may restart an office",
            ),
            _ => ApiError::new(ErrorCode::Unavailable, "host registry unavailable"),
        })
}

fn run_journaled_restart(
    service: &Service,
    db: &HostDb,
    account: &Account,
    org_filter: Option<&str>,
    office_id: &str,
    attempt: &Attempt<'_>,
    now: i64,
) -> Result<HostOpResult<Empty>, ApiError> {
    let (office, member_id) =
        find_linked_office(&service.offices.offices(), account, org_filter, office_id)?;
    check_restart_authority(db, account, &office.organization_id, &member_id)?;
    // Serialize journal lookup -> restart -> journal write across concurrent worker threads
    // (mirroring `WebTls::set`) so simultaneous retries cannot race into false `Conflict` or
    // execute a duplicate restart before `OperationReused` is recorded.
    let _restart_guard = crate::sync::lock(&service.office_restarting);
    match db.lookup(attempt, now).map_err(journal_error)? {
        Lookup::Replay(stored) => return decode_stored(stored),
        Lookup::NeedsVerification { .. } => {
            return Err(ApiError::new(
                ErrorCode::OperationReused,
                "operation ID already used",
            ));
        }
        Lookup::Vacant => {}
    }
    service.offices.restart_office(&office.id)?;
    let write = db
        .write(attempt, now, |_| {
            Ok(Effect {
                stored: Stored::Completed {
                    result: serde_json::json!({}),
                },
                verifier: None,
            })
        })
        .map_err(journal_error)?;
    match write {
        Write::Committed(stored) | Write::Existing(Lookup::Replay(stored)) => decode_stored(stored),
        Write::Existing(_) => Err(ApiError::new(
            ErrorCode::OperationReused,
            "operation ID already used",
        )),
    }
}

fn decode_stored(stored: Stored) -> Result<HostOpResult<Empty>, ApiError> {
    match stored {
        Stored::Completed { .. } => Ok(HostOpResult::Completed { result: Empty {} }),
        Stored::Rejected { refusal } => Ok(HostOpResult::Rejected { refusal }),
        _ => Err(ApiError::new(
            ErrorCode::Unavailable,
            "unrecognized stored restart state",
        )),
    }
}

/// `browser.office_restart` on the admitted web connection only.
pub(super) fn restart_browser_office(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let p = request
            .params::<BrowserOfficeRestart>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid restart request"))?;
        let account = authentication.browser_account(
            &SessionValidateParams {
                cookie_digest: p.auth.cookie_digest,
                proof_digest: p.auth.proof_digest,
                origin: p.auth.origin.clone(),
            },
            now,
            &mut ended,
        )?;
        let (db, _) = authentication.provisioning_storage();
        let attempt = Attempt::new(&p, account.account_id()).map_err(journal_error)?;
        run_journaled_restart(
            service,
            &db,
            &account,
            Some(&p.org_id),
            &p.office_id,
            &attempt,
            now,
        )
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(res) => connection.send_response(ok::<BrowserOfficeRestart>(request.id, &res)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

/// Authenticate a native `host.sock` session before full parameter decoding so retired
/// session-less `office.list` / `office.restart` calls always fail closed with `NotAuthorized`.
fn authorize_session_first(
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) -> Result<Account, ApiError> {
    let raw = request
        .params
        .get("session")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ApiError::new(ErrorCode::NotAuthorized, "session required"))?;
    let handle = org_host_control::SessionHandle::new(raw.to_owned())
        .map_err(|_| ApiError::new(ErrorCode::NotAuthorized, "not authorized"))?;
    authentication.authorize(connection, &handle)
}

/// `office.list` on `host.sock` for a native authenticated session.
pub(super) fn list_session_offices(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let reply = authorize_session_first(authentication, request, connection).and_then(|account| {
        let p = request
            .params::<OfficeList>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid office list request"))?;
        if let Some(org_id) = &p.org_id {
            if !account.links().iter().any(|l| l.org_id() == org_id) {
                return Err(ApiError::new(ErrorCode::NotFound, "organization not found"));
            }
        }
        let offices = service
            .offices
            .offices()
            .into_iter()
            .filter(|o| {
                account
                    .links()
                    .iter()
                    .any(|l| l.org_id() == o.organization_id)
                    && p.org_id.as_deref().is_none_or(|id| o.organization_id == id)
            })
            .map(|mut o| {
                o.endpoint = None;
                o
            })
            .collect();
        Ok(ListOfficesResult { offices })
    });
    match reply {
        Ok(res) => connection.send_response(ok::<OfficeList>(request.id, &res)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

/// `office.restart` on `host.sock` for a native authenticated session.
pub(super) fn restart_session_office(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let reply = authorize_session_first(authentication, request, connection).and_then(|account| {
        let now = browser_now_ms()?;
        let p = request
            .params::<OfficeRestart>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid restart request"))?;
        let (db, _) = authentication.provisioning_storage();
        let attempt = Attempt::new(&p, account.account_id()).map_err(journal_error)?;
        run_journaled_restart(service, &db, &account, None, &p.office_id, &attempt, now)
    });
    match reply {
        Ok(res) => connection.send_response(ok::<OfficeRestart>(request.id, &res)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

#[cfg(test)]
mod tests {
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

    use crate::service::{authentication::hash_password, listener, tests as service_tests};

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
        let (_root, authentication, _) = crate::service::authentication::tests::fixture();
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
        let _local = listener::LocalConnection::start(host_stream, Arc::new(service)).unwrap();
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
        let (_root, authentication, _) = crate::service::authentication::tests::fixture();
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
            listener::LocalConnection::start(host_stream, Arc::new(local_service)).unwrap();
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
}
