use super::super::listener::Connection;
use super::*;
use crate::accounts::identity::{AccountId, LoginKey};
use crate::host_secrets::HostSecrets;
use crate::host_secrets::{Name, Value};
use crate::hostdb::lockout::{Outcome, Socket};
use crate::hostdb::{HostDb, HostDbInit};
use org_host_control::ops::AccountVerifyResult;
use org_host_control::{Password, SessionHandle};
use org_proto::ErrorCode;
use sha2::Sha256;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// These behavioral tests use production Argon2 parameters in a parallel debug
// suite. This bounds a hung test; sign-in latency is not the assertion here.
const SIGN_IN_WATCHDOG: Duration = Duration::from_secs(30);

#[derive(Default)]
struct TestCustody(Mutex<BTreeMap<Name, Vec<u8>>>);
impl HostSecrets for TestCustody {
    fn put(&self, name: &Name, value: &Value) -> Result<(), crate::host_secrets::Error> {
        self.0
            .lock()
            .unwrap()
            .insert(name.clone(), value.expose().to_vec());
        Ok(())
    }
    fn load(&self, name: &Name) -> Result<Option<Value>, crate::host_secrets::Error> {
        self.0
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .map(Value::new)
            .transpose()
    }
    fn delete(&self, name: &Name) -> Result<(), crate::host_secrets::Error> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
    fn list(&self) -> Result<Vec<Name>, crate::host_secrets::Error> {
        Ok(self.0.lock().unwrap().keys().cloned().collect())
    }
}
pub(crate) fn fixture() -> (tempfile::TempDir, Authentication, Arc<Connection>) {
    fixture_for(&crate::ids::uuid_v7().unwrap())
}
pub(crate) fn fixture_for(instance: &str) -> (tempfile::TempDir, Authentication, Arc<Connection>) {
    fixture_seeded(instance, true)
}
/// A host database with no account at all: a setup link can still register the administrator.
pub(crate) fn fixture_without_accounts() -> (tempfile::TempDir, Authentication) {
    let (root, auth, _) = fixture_seeded(&crate::ids::uuid_v7().unwrap(), false);
    (root, auth)
}
fn fixture_seeded(
    instance: &str,
    seed: bool,
) -> (tempfile::TempDir, Authentication, Arc<Connection>) {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let custody: Arc<dyn HostSecrets> = Arc::new(TestCustody::default());
    let account = crate::ids::generate().unwrap();
    let init = HostDbInit {
        instance_id: instance.into(),
        clock_high_ms: wall_ms().unwrap() as i64,
    };
    let db = HostDb::create(root.path(), &*custody, &init).unwrap();
    let verifier =
        hash_password(&Password::new("synthetic test password".into()).unwrap()).unwrap();
    if seed {
        db.seed_account(&account, "Owner", &verifier, true, init.clock_high_ms)
            .unwrap();
    }
    drop(db);
    let auth = Authentication::open(root.path(), custody, instance).unwrap();
    (root, auth, Connection::for_test())
}
fn login(auth: &Authentication, connection: &Connection) -> AccountVerifyResult {
    auth.verify(
        connection,
        "OWNER",
        &Password::new("synthetic test password".into()).unwrap(),
    )
    .unwrap()
}

#[test]
fn inspected_session_uses_current_protected_facts_and_monotonic_absolute_limit() {
    let (_root, auth, connection) = fixture();
    let signed_in = login(&auth, &connection);
    let view = auth.inspect(&connection, &signed_in.session).unwrap();
    assert_eq!(view.account, signed_in.account);
    assert_eq!(view.login, "Owner");
    assert!(view.admin);
    assert!(view.orgs.is_empty());
    assert!(view.idle_expires_at_ms - view.server_time_ms <= IDLE.as_millis() as u64);
    assert!(auth
        .inspect(&Connection::for_test(), &signed_in.session)
        .is_err());
    assert!(auth.inspect(&connection, &signed_in.session).is_ok());
    auth.sessions
        .lock()
        .unwrap()
        .get_mut(&digest(&signed_in.session))
        .unwrap()
        .created = Instant::now() - (ABSOLUTE - Duration::from_millis(200));
    let view = auth.inspect(&connection, &signed_in.session).unwrap();
    // The absolute limit still bounds the reported idle expiry; it is not on the wire.
    assert!(view.idle_expires_at_ms - view.server_time_ms <= 200);
    let account = auth.authorize(&connection, &signed_in.session).unwrap();
    auth.db
        .establish_organization_owner(&account, "org", "01ARZ3NDEKTSV4RRFFQ69G5FAV")
        .unwrap();
    assert_eq!(
        auth.inspect(&connection, &signed_in.session).unwrap().orgs[0].org_id,
        "org"
    );
    auth.sessions
        .lock()
        .unwrap()
        .get_mut(&digest(&signed_in.session))
        .unwrap()
        .created = Instant::now() - ABSOLUTE;
    assert!(auth.inspect(&connection, &signed_in.session).is_err());
    assert!(!auth
        .sessions
        .lock()
        .unwrap()
        .contains_key(&digest(&signed_in.session)));
}

#[test]
fn issued_grant_absolute_expiry_and_idle_or_eof_revoke_without_another_signin() {
    use hmac::{Hmac, Mac};
    use org_office_launch::TransferRecord;
    for ending in ["absolute", "idle", "eof"] {
        let (_root, auth, connection) = fixture();
        let result = login(&auth, &connection);
        let account = auth.authorize(&connection, &result.session).unwrap();
        auth.db
            .establish_organization_owner(&account, "org", "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap();
        let (endpoint, slot, worker, _control) =
            crate::supervisor::channels::Endpoint::for_grant_test();
        worker
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        connection.record_received_channel_for_test("channel");
        let session_id = {
            let mut sessions = auth.sessions.lock().unwrap();
            let session = sessions.get_mut(&digest(&result.session)).unwrap();
            session.channels.insert(
                "channel".into(),
                Channel {
                    endpoint: Arc::downgrade(&endpoint),
                    organization: "org".into(),
                    satellite_operator: false,
                    _slot: slot,
                },
            );
            if ending == "absolute" {
                session.created = Instant::now() - (ABSOLUTE - Duration::from_millis(250));
            }
            session.id.clone()
        };
        // This is an actual grant issued by the production verifier/signer,
        // using a synthetic admitted channel, not only a registry-live assertion.
        let grant = auth
            .grant(&connection, &result.session, "office", "channel")
            .unwrap()
            .grant;
        let mut mac = Hmac::<Sha256>::new_from_slice(endpoint.key.expose_bytes()).unwrap();
        mac.update(&org_proto::grant_mac_input(&grant.claims).unwrap());
        mac.verify_slice(grant.mac.expose_bytes()).unwrap();
        if ending == "absolute" {
            assert!(grant.claims.exp.saturating_sub(wall_ms().unwrap()) <= 250);
        }
        if ending == "idle" {
            auth.sessions
                .lock()
                .unwrap()
                .get_mut(&digest(&result.session))
                .unwrap()
                .touched = Instant::now() - IDLE;
        }
        if ending == "eof" {
            connection.close();
        }
        let auth = Arc::new(auth);
        let _lifecycle = Lifecycle::start(&auth).unwrap();
        let mut bytes = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
        let n = worker.recv(&mut bytes).unwrap();
        assert!(
            matches!(org_office_launch::decode_transfer_record(&bytes[..n]).unwrap(),TransferRecord::Revoked {session_ids} if session_ids==[session_id])
        );
        assert!(auth.sessions.lock().unwrap().is_empty());
        assert!(auth
            .grant(&connection, &result.session, "office", "channel")
            .is_err());
        assert!(
            endpoint.is_live(),
            "expiry does not roll back accepted office work"
        );
    }
}

#[test]
fn protected_account_changes_revoke_both_independently_issued_session_grants() {
    use hmac::{Hmac, Mac};
    use org_office_launch::TransferRecord;
    for revoke in [false, true] {
        let (_root, auth, first_connection) = fixture();
        let second_connection = Connection::for_test();
        let first = login(&auth, &first_connection);
        let second = login(&auth, &second_connection);
        assert_eq!(first.account, second.account);
        assert_ne!(first.session.expose(), second.session.expose());
        let account = auth.authorize(&first_connection, &first.session).unwrap();
        auth.db
            .establish_organization_owner(&account, "org", "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap();
        let mut issued = Vec::new();
        for (connection, result) in [(&first_connection, &first), (&second_connection, &second)] {
            let (endpoint, slot, worker, control) =
                crate::supervisor::channels::Endpoint::for_grant_test();
            worker
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            connection.record_received_channel_for_test("channel");
            let session_id = {
                let mut sessions = auth.sessions.lock().unwrap();
                let session = sessions.get_mut(&digest(&result.session)).unwrap();
                session.channels.insert(
                    "channel".into(),
                    Channel {
                        endpoint: Arc::downgrade(&endpoint),
                        organization: "org".into(),
                        satellite_operator: false,
                        _slot: slot,
                    },
                );
                session.id.clone()
            };
            let grant = auth
                .grant(connection, &result.session, "office", "channel")
                .unwrap()
                .grant;
            assert_eq!(grant.claims.account, first.account);
            assert_eq!(grant.claims.session_id, session_id);
            let mut mac = Hmac::<Sha256>::new_from_slice(endpoint.key.expose_bytes()).unwrap();
            mac.update(&org_proto::grant_mac_input(&grant.claims).unwrap());
            mac.verify_slice(grant.mac.expose_bytes()).unwrap();
            issued.push((endpoint, worker, control, session_id));
        }
        assert_ne!(issued[0].3, issued[1].3);
        assert!(auth
            .grant(&second_connection, &first.session, "office", "channel")
            .is_err());
        assert_eq!(auth.sessions.lock().unwrap().len(), 2);
        // Change the actual protected registry fact after both grants exist.
        // No further sign-in/renewal/authorization request triggers the reaper.
        auth.db
            .change_authority_fixture(&AccountId::parse(&first.account).unwrap(), revoke)
            .unwrap();
        let auth = Arc::new(auth);
        let _lifecycle = Lifecycle::start(&auth).unwrap();
        for (endpoint, worker, _control, session_id) in issued {
            let mut bytes = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
            let n = worker.recv(&mut bytes).unwrap();
            assert!(matches!(
                org_office_launch::decode_transfer_record(&bytes[..n]).unwrap(),
                TransferRecord::Revoked { session_ids } if session_ids == [session_id]
            ));
            assert!(endpoint.is_live(), "revocation leaves accepted work intact");
        }
        assert!(auth.sessions.lock().unwrap().is_empty());
        for (connection, result) in [(&first_connection, &first), (&second_connection, &second)] {
            assert!(auth.inspect(connection, &result.session).is_err());
            assert!(auth
                .grant(connection, &result.session, "office", "channel")
                .is_err());
        }
    }
}

#[test]
fn actual_registry_signin_is_connection_bound_live_and_ends_without_persisting_handles() {
    let (root, auth, connection) = fixture();
    let result = login(&auth, &connection);
    assert!(result.admin);
    assert!(result.orgs.is_empty());
    let other = Connection::for_test();
    assert_eq!(
        auth.authorize(&other, &result.session).unwrap_err().code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        auth.authorize(&connection, &result.session)
            .unwrap()
            .account_id(),
        result.account
    );
    auth.end(&connection, &result.session).unwrap();
    assert_eq!(
        auth.authorize(&connection, &result.session)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    let again = login(&auth, &connection);
    connection.close();
    assert!(auth.authorize(&connection, &again.session).is_err());
    // Secret handles belong solely to volatile host memory, never SQL or files.
    for entry in std::fs::read_dir(root.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            assert!(!std::fs::read(path)
                .unwrap()
                .windows(again.session.expose().len())
                .any(|b| b == again.session.expose().as_bytes()));
        }
    }
}

#[test]
fn wrong_unknown_revoked_and_changed_passwords_are_uniform_and_pool_is_bounded() {
    let (_root, auth, connection) = fixture();
    let wrong = Password::new("wrong synthetic password".into()).unwrap();
    let a = auth.verify(&connection, "Owner", &wrong).unwrap_err();
    let b = auth.verify(&connection, "unknown", &wrong).unwrap_err();
    assert_eq!(a.code, b.code);
    assert_eq!(a.message, b.message);
    let result = login(&auth, &connection);
    let id = AccountId::parse(&result.account).unwrap();
    auth.db.change_authority_fixture(&id, false).unwrap();
    assert_eq!(
        auth.authorize(&connection, &result.session)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    let result = login(&auth, &connection);
    auth.db.change_authority_fixture(&id, true).unwrap();
    assert_eq!(
        auth.authorize(&connection, &result.session)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        auth.verify(
            &connection,
            "Owner",
            &Password::new("synthetic test password".into()).unwrap()
        )
        .unwrap_err()
        .code,
        ErrorCode::NotAuthorized
    );
    auth.hashing.store(MAX_HASHES, Ordering::SeqCst);
    assert_eq!(
        auth.verify(&connection, "Owner", &wrong).unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert_eq!(auth.hashing.load(Ordering::SeqCst), MAX_HASHES);
}

/// A locked native login is refused before it takes a hash permit: with every
/// permit held, a locked call answers `not_authorized`, not `unavailable`, and
/// leaves the permit count unchanged.
#[test]
fn a_locked_native_verify_takes_no_hash_permit() {
    let (_root, auth, connection) = fixture();
    let key = LoginKey::new("Owner").unwrap();
    // Failures recorded an hour ahead move the host clock there, so the lock
    // holds however long the test runs.
    let ahead = wall_ms().unwrap() + 3_600_000;
    for socket in [Socket::Local, Socket::Control] {
        for _ in 0..6 {
            auth.db
                .record_login(socket, key.as_str(), ahead, Outcome::Failure)
                .unwrap();
        }
        assert!(auth
            .db
            .login_lockout(socket, key.as_str(), ahead)
            .unwrap()
            .is_locked());
    }
    auth.hashing.store(MAX_HASHES, Ordering::SeqCst);
    let right = Password::new("synthetic test password".into()).unwrap();
    assert_eq!(
        auth.verify(&connection, "Owner", &right).unwrap_err().code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(auth.hashing.load(Ordering::SeqCst), MAX_HASHES);
    // An unlocked login with the pool full still answers `unavailable`.
    assert_eq!(
        auth.verify(&connection, "Other", &right).unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert_eq!(auth.hashing.load(Ordering::SeqCst), MAX_HASHES);
}

#[test]
fn session_absolute_and_idle_expiry_remain_independent_of_wall_clock() {
    let (_root, auth, connection) = fixture();
    for absolute in [false, true] {
        let result = login(&auth, &connection);
        {
            let mut sessions = auth.sessions.lock().unwrap();
            let session = sessions.get_mut(&digest(&result.session)).unwrap();
            if absolute {
                session.created = Instant::now() - ABSOLUTE;
            } else {
                session.touched = Instant::now() - IDLE;
            }
        }
        assert_eq!(
            auth.authorize(&connection, &result.session)
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
    }
}

#[test]
fn real_host_listener_negotiates_signin_renewal_and_logout() {
    use crate::service::listener::{self, Listener, ListenerConfig};
    use org_host_control::ops::{
        AccountVerify, AccountVerifyParams, SessionEnd, SessionParams, SessionRenew,
    };
    use org_proto::{frame, Call, Request, Response};
    use std::{io::BufReader, os::unix::net::UnixStream};
    let (_root, auth, _) = fixture();
    let sockets = tempfile::Builder::new()
        .prefix("o-auth")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = sockets.path().join("h.sock");
    let mut service = super::super::tests::service(_root.path());
    service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    service.authentication = Some(Arc::new(auth));
    let offices = service.offices.clone();
    let mut listener = Listener::start(
        listener::bind(&socket).unwrap(),
        ListenerConfig {
            max_connections: 3,
            max_in_flight: 2,
            expected_uid: org_platform::posix::effective_uid(),
        },
        Arc::new(service),
    )
    .unwrap();
    let stream = UnixStream::connect(socket).unwrap();
    stream.set_read_timeout(Some(SIGN_IN_WATCHDOG)).unwrap();
    let mut peer = BufReader::new(stream);
    fn exchange<C: Call>(
        peer: &mut BufReader<UnixStream>,
        id: u64,
        params: &C::Params,
    ) -> Response {
        frame::write_message(peer.get_mut(), &Request::new::<C>(id, params).unwrap()).unwrap();
        frame::read_message(peer).unwrap().unwrap()
    }
    let credentials = || AccountVerifyParams {
        login: "Owner".into(),
        password: Password::new("synthetic test password".into()).unwrap(),
    };
    assert!(matches!(
        exchange::<AccountVerify>(&mut peer, 1, &credentials()).outcome,
        org_proto::Outcome::Error(_)
    ));
    exchange::<org_host_control::Hello>(&mut peer, 2, &Default::default())
        .into_result::<org_host_control::Hello>()
        .unwrap();
    let signed_in = exchange::<AccountVerify>(&mut peer, 3, &credentials())
        .into_result::<AccountVerify>()
        .unwrap();
    let params = SessionParams {
        session: signed_in.session,
    };
    exchange::<SessionRenew>(&mut peer, 4, &params)
        .into_result::<SessionRenew>()
        .unwrap();
    exchange::<SessionEnd>(&mut peer, 5, &params)
        .into_result::<SessionEnd>()
        .unwrap();
    assert!(
        matches!(exchange::<SessionRenew>(&mut peer, 6, &params).outcome,
        org_proto::Outcome::Error(error) if error.code == ErrorCode::NotAuthorized)
    );
    drop(peer);
    listener.close();
    listener.join();
    offices.shutdown();
}

#[test]
fn actual_host_connection_pair_signs_in_and_eof_invalidates_registry_session() {
    use org_host_control::ops::{AccountVerify, AccountVerifyParams, SessionParams, SessionRenew};
    use org_proto::{frame, Request, Response};
    use std::{io::BufReader, os::unix::net::UnixStream};
    let (root, auth, _) = fixture();
    let auth = Arc::new(auth);
    let mut service = super::super::tests::service(root.path());
    service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    service.authentication = Some(auth.clone());
    let offices = service.offices.clone();
    let (host, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(SIGN_IN_WATCHDOG)).unwrap();
    let thread =
        std::thread::spawn(move || super::super::listener::serve_pair(host, Arc::new(service)));
    let mut peer = BufReader::new(peer);
    frame::write_message(
        peer.get_mut(),
        &Request::new::<org_host_control::Hello>(1, &Default::default()).unwrap(),
    )
    .unwrap();
    frame::read_message::<Response>(&mut peer)
        .unwrap()
        .unwrap()
        .into_result::<org_host_control::Hello>()
        .unwrap();
    frame::write_message(
        peer.get_mut(),
        &Request::new::<AccountVerify>(
            2,
            &AccountVerifyParams {
                login: "Owner".into(),
                password: Password::new("synthetic test password".into()).unwrap(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    let result = frame::read_message::<Response>(&mut peer)
        .unwrap()
        .unwrap()
        .into_result::<AccountVerify>()
        .unwrap();
    let params = SessionParams {
        session: result.session,
    };
    frame::write_message(
        peer.get_mut(),
        &Request::new::<SessionRenew>(3, &params).unwrap(),
    )
    .unwrap();
    frame::read_message::<Response>(&mut peer)
        .unwrap()
        .unwrap()
        .into_result::<SessionRenew>()
        .unwrap();
    drop(peer);
    thread.join().unwrap();
    assert!(auth
        .sessions
        .lock()
        .unwrap()
        .values()
        .all(|s| !s.live(Instant::now())));
    assert!(auth
        .authorize(&Connection::for_test(), &params.session)
        .is_err());
    offices.shutdown();
}

#[test]
fn recovery_private_permission_stamp_dies_on_logout_expiry_and_registry_revocation() {
    for cause in ["logout", "expiry", "revoke"] {
        let (_dir, auth, connection) = fixture();
        let result = login(&auth, &connection);
        let (digest, live) = auth
            .recovery_session_binding(&connection, &result.session)
            .unwrap();
        assert!(live.upgrade().unwrap().load(Ordering::SeqCst));
        assert!(auth
            .recovery_session_binding(&Connection::for_test(), &result.session)
            .is_err());
        assert!(live.upgrade().unwrap().load(Ordering::SeqCst));
        match cause {
            "logout" => {
                auth.end(&connection, &result.session).unwrap();
            }
            "expiry" => {
                auth.sessions
                    .lock()
                    .unwrap()
                    .get_mut(&digest)
                    .unwrap()
                    .created = Instant::now() - ABSOLUTE - Duration::from_secs(1);
                auth.reap();
            }
            _ => {
                auth.db
                    .change_authority_fixture(&AccountId::parse(&result.account).unwrap(), true)
                    .unwrap();
                auth.reap();
            }
        }
        assert!(live.upgrade().is_none_or(|v| !v.load(Ordering::SeqCst)));
        assert!(auth
            .recovery_session_binding(&connection, &result.session)
            .is_err());
    }
}

pub(crate) fn link_owner(auth: &Authentication, connection: &Connection, handle: &SessionHandle) {
    let account = auth.authorize(connection, handle).unwrap();
    auth.db
        .establish_organization_owner(&account, "org", "01ARZ3NDEKTSV4RRFFQ69G5FAV")
        .unwrap();
}
#[test]
fn session_64_cross_office_permissions_prune_physical_closure_before_limit_65() {
    use crate::supervisor::channels::tests::{await_closed, worker_fixture};
    let (_root, auth, connection) = fixture();
    let signed_in = login(&auth, &connection);
    link_owner(&auth, &connection, &signed_in.session);
    let workers: Vec<_> = (0..9)
        .map(|n| worker_fixture(&format!("office-{n}")))
        .collect();
    let mut held = Vec::new();
    for worker in &workers[..8] {
        for _ in 0..8 {
            held.push(
                auth.open_channel(
                    &connection,
                    &signed_in.session,
                    Arc::clone(&worker.endpoint),
                    "org",
                )
                .unwrap(),
            );
        }
    }
    assert_eq!(
        auth.sessions.lock().unwrap()[&digest(&signed_in.session)]
            .channels
            .len(),
        64
    );
    assert!(auth
        .open_channel(
            &connection,
            &signed_in.session,
            Arc::clone(&workers[8].endpoint),
            "org"
        )
        .is_err());
    assert!(workers[8].endpoint.is_live());
    let (id, stream, slot) = held.remove(0);
    drop(stream);
    await_closed(&slot);
    let replacement = auth
        .open_channel(
            &connection,
            &signed_in.session,
            Arc::clone(&workers[8].endpoint),
            "org",
        )
        .unwrap();
    let sessions = auth.sessions.lock().unwrap();
    let session = &sessions[&digest(&signed_in.session)];
    assert_eq!(session.channels.len(), 64);
    assert!(!session.channels.contains_key(&id));
    assert!(session.channels.contains_key(&replacement.0));
    drop(sessions);
    drop((held, replacement));
}
#[test]
fn actual_close_denies_grants_and_logout_revokes_without_freeing_a_held_descriptor() {
    use crate::supervisor::channels::tests::{await_closed, worker_fixture};
    use org_office_launch::{ChannelKind, MAX_CHANNELS, MAX_WEB_CHANNELS};
    let (_root, auth, connection) = fixture();
    let signed_in = login(&auth, &connection);
    link_owner(&auth, &connection, &signed_in.session);
    let worker = worker_fixture("office");
    let (id, stream, slot) = auth
        .open_channel(
            &connection,
            &signed_in.session,
            Arc::clone(&worker.endpoint),
            "org",
        )
        .unwrap();
    connection.record_received_slot_for_test(&id, slot.clone());
    assert!(auth
        .grant(&connection, &signed_in.session, "office", &id)
        .is_ok());
    drop(stream);
    await_closed(&slot);
    assert!(!connection.channel_received(&id));
    assert!(auth
        .grant(&connection, &signed_in.session, "office", &id)
        .is_err());
    let (id, stream, slot) = auth
        .open_channel(
            &connection,
            &signed_in.session,
            Arc::clone(&worker.endpoint),
            "org",
        )
        .unwrap();
    connection.record_received_slot_for_test(&id, slot.clone());
    assert!(auth
        .grant(&connection, &signed_in.session, "office", &id)
        .is_ok());
    auth.end(&connection, &signed_in.session).unwrap();
    assert!(auth
        .grant(&connection, &signed_in.session, "office", &id)
        .is_err());
    assert!(
        slot.is_live(),
        "logout ends permission, not physical peer ownership"
    );
    // Fill every other ordinary slot: the web allowance, then local callers up to the total,
    // beside the logged-out channel's still-held descriptor.
    let _held: Vec<_> = (0..MAX_WEB_CHANNELS)
        .map(|_| worker.endpoint.open(ChannelKind::Web).unwrap())
        .chain(
            (MAX_WEB_CHANNELS + 1..MAX_CHANNELS)
                .map(|_| worker.endpoint.open(ChannelKind::Local).unwrap()),
        )
        .collect();
    assert!(worker.endpoint.open(ChannelKind::Local).is_err());
    drop(stream);
    await_closed(&slot);
    assert!(worker.endpoint.open(ChannelKind::Local).is_ok());
    assert!(worker.endpoint.is_live());
}

#[test]
fn satellite_operator_channel_survives_reap_while_admin_and_revokes_on_demotion() {
    use crate::supervisor::channels::tests::worker_fixture;
    let (_root, auth, connection) = fixture();
    let signed_in = login(&auth, &connection);
    let admin_id = AccountId::parse(&signed_in.account).unwrap();

    // Seed a second non-admin account, link both to a shared org so we can perform a real
    // `handover_administrator` demotion later, while keeping `"sat-org"` completely unlinked.
    let verifier =
        hash_password(&Password::new("synthetic test password".into()).unwrap()).unwrap();
    let second_acct = crate::ids::generate().unwrap();
    let second_id = AccountId::parse(&second_acct).unwrap();
    auth.db
        .seed_account(&second_acct, "Successor", &verifier, false, 10_000)
        .unwrap();
    auth.db
        .seed_link_for_test(&signed_in.account, "shared-org", "mem-admin")
        .unwrap();
    auth.db
        .seed_link_for_test(&second_acct, "shared-org", "mem-successor")
        .unwrap();

    let worker = worker_fixture("sat-office");

    // Non-admin is refused `open_satellite_operator_channel` on an unlinked org.
    let second_conn = Connection::for_test();
    let second_signed = auth
        .verify(
            &second_conn,
            "Successor",
            &Password::new("synthetic test password".into()).unwrap(),
        )
        .unwrap();
    assert!(auth
        .open_satellite_operator_channel(
            &second_conn,
            &second_signed.session,
            Arc::clone(&worker.endpoint),
            "sat-org",
        )
        .is_err());

    // Unlinked admin opens a satellite operator channel, survives `reap()`, and gets a grant
    // whose `claims.member` equals the satellite `office_id`.
    let (ch_id, _stream, slot) = auth
        .open_satellite_operator_channel(
            &connection,
            &signed_in.session,
            Arc::clone(&worker.endpoint),
            "sat-org",
        )
        .unwrap();
    connection.record_received_slot_for_test(&ch_id, slot);
    auth.reap();
    let granted = auth
        .grant(&connection, &signed_in.session, "sat-office", &ch_id)
        .expect("admin satellite operator session must survive reap()");
    assert_eq!(granted.grant.claims.member, "sat-office");
    assert_eq!(granted.grant.claims.org_id, "sat-org");

    // Demoting the admin via `handover_administrator` causes `reap()` to revoke the session
    // and deny subsequent grants immediately.
    let handover = auth
        .db
        .handover_administrator(&admin_id, &second_id)
        .unwrap();
    assert!(matches!(
        handover,
        crate::hostdb::sqlite::accounts::multi_person::AdminHandoverOutcome::Transferred
    ));
    auth.reap();
    assert!(auth
        .grant(&connection, &signed_in.session, "sat-office", &ch_id)
        .is_err());
}
