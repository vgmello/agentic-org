#[test]
fn web_start_needs_authentication_and_a_registered_instance() {
    use super::web_start_eligible;
    assert!(web_start_eligible(true, true));
    assert!(!web_start_eligible(false, true), "no authentication");
    assert!(!web_start_eligible(true, false), "not registered");
    assert!(!web_start_eligible(false, false));
}

/// The service fixture, and the predicate the host wires into its web process
/// supervisor, refuse to start the web process without authentication or without a
/// registered instance, and allow it once both hold, before any organization or
/// office exists (lead ruling: the person registers and creates the organization
/// in the browser).
#[test]
fn the_wired_web_eligibility_needs_authentication_and_registration_but_no_office() {
    let home = tempfile::tempdir().unwrap();
    let bare = Arc::new(service(home.path()));
    assert!(!bare.web_eligible(), "no authentication");
    assert!(!(bare.web_eligibility())(), "wired: no authentication");
    let (_root, authentication, _connection) = crate::authentication::tests::fixture();
    let authentication = Arc::new(authentication);
    let unregistered =
        Arc::new(service(home.path()).with_authentication(Some(Arc::clone(&authentication))));
    assert!(unregistered.instance_id.is_none());
    assert!(!unregistered.web_eligible(), "not registered");
    assert!(!(unregistered.web_eligibility())(), "wired: not registered");
    let mut registered = service(home.path()).with_authentication(Some(authentication));
    registered.instance_id = Some(crate::ids::uuid_v7().unwrap());
    let registered = Arc::new(registered);
    assert!(registered.offices.offices().is_empty(), "no office at all");
    assert!(registered.web_eligible(), "registered, no organization yet");
    assert!((registered.web_eligibility())(), "wired: registered");
    assert!(!registered.web_ready(), "eligible is not serving");
    bare.offices.shutdown();
    unregistered.offices.shutdown();
    registered.offices.shutdown();
}

#[test]
fn web_ready_needs_authentication_registration_and_a_serving_web() {
    use super::web_serving;
    assert!(web_serving(true, true, WebState::Running));
    for state in [
        WebState::NotConfigured,
        WebState::Starting,
        WebState::Failed,
        WebState::Unknown,
    ] {
        assert!(!web_serving(true, true, state), "{state:?}");
    }
    assert!(!web_serving(false, true, WebState::Running));
    assert!(!web_serving(true, false, WebState::Running));
}

use super::dispatch::OpenOperation;
use super::*;
use crate::broadcast::Broadcast;
use crate::control_listener::{self, Connection, Handler};
use crate::offices::Offices;
use org_host_control::{Administrator, Hello, InstanceStatus, WebState};
use org_proto::{Call, ErrorCode, Request};
use std::path::Path;
use std::sync::Arc;

use org_proto::{Empty, HelloParams, OfficeList, OfficeRestart, OfficeRestartParams};

use crate::catalog::Catalog;
use crate::keys::memory::MemoryKeys;
use crate::supervisor::{Supervisor, SupervisorConfig};
use crate::timings::Timings;

#[test]
fn only_hello_and_instance_status_have_dispatch_variants() {
    assert_eq!(
        OpenOperation::from_method("hello"),
        Some(OpenOperation::Hello)
    );
    assert_eq!(
        OpenOperation::from_method("instance.status"),
        Some(OpenOperation::InstanceStatus)
    );
    for method in [
        "organization.create",
        "organization.list",
        "office.list",
        "subscribe",
        "office.app_token",
        "office.restart",
        "account.verify",
        "bootstrap",
        "exit",
        "session.grant",
        "recovery.release",
        "tls.fetch",
        "fds.received",
        "future.method",
        "",
        "Hello",
        "instance.status.extra",
    ] {
        assert_eq!(OpenOperation::from_method(method), None, "{method}");
    }
}

#[test]
fn status_reports_whether_an_administrator_exists() {
    let home = tempfile::tempdir().unwrap();
    let unregistered = service(home.path());
    assert_eq!(unregistered.administrator().unwrap(), Administrator::None);
    unregistered.offices.shutdown();
    let (_root, auth, _connection) = crate::authentication::tests::fixture();
    let registered = service(home.path()).with_authentication(Some(Arc::new(auth)));
    // The fixture seeds an administrator account.
    assert_eq!(registered.administrator().unwrap(), Administrator::Present);
    registered.offices.shutdown();
}

pub(crate) fn service(home: &Path) -> Service {
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
        "01JD5Z00000000000000000NST",
    );
    let broadcast = Arc::new(Broadcast::default());
    let offices = Offices::start(
        home,
        Catalog::default(),
        Vec::new(),
        supervisor,
        Arc::clone(&broadcast),
        &Timings::default(),
    );
    Service::new(offices, broadcast, None)
}

/// A service fixture with an instance id, as an admitted web process's handler, and
/// its offices for shutdown.
pub(crate) fn web_handler(home: &Path) -> (Arc<Service>, Arc<Offices>) {
    let mut service = service(home);
    service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    let offices = service.offices.clone();
    (Arc::new(service), offices)
}

/// A registered service fixture (an instance id) answering for `authentication`, as the
/// host.sock handler the authentication tests sign in through, and its offices for shutdown.
pub(crate) fn registered_handler(
    home: &Path,
    authentication: Arc<crate::authentication::Authentication>,
) -> (Arc<Service>, Arc<Offices>) {
    let mut service = service(home).with_authentication(Some(authentication));
    service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    let offices = service.offices.clone();
    (Arc::new(service), offices)
}

/// The real handler on an admitted web process control connection, plus its peer end.
fn web_peer(
    home: &Path,
) -> (
    control_listener::WebConnection,
    std::io::BufReader<std::os::unix::net::UnixStream>,
    Arc<Offices>,
) {
    web_peer_on(
        home,
        r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#,
    )
}

/// [`web_peer`] whose web process listens on `listen` (JSON).
fn web_peer_on(
    home: &Path,
    listen: &str,
) -> (
    control_listener::WebConnection,
    std::io::BufReader<std::os::unix::net::UnixStream>,
    Arc<Offices>,
) {
    web_peer_for(service(home), listen)
}

/// [`web_peer_on`] over a service the caller prepared.
pub(crate) fn web_peer_for(
    mut service: Service,
    listen: &str,
) -> (
    control_listener::WebConnection,
    std::io::BufReader<std::os::unix::net::UnixStream>,
    Arc<Offices>,
) {
    service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    let offices = service.offices.clone();
    let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let (transfer, _child) = std::os::unix::net::UnixDatagram::pair().unwrap();
    let listen = serde_json::from_str(listen).unwrap();
    let web =
        control_listener::WebConnection::start(host, transfer, listen, Arc::new(service)).unwrap();
    (web, std::io::BufReader::new(peer), offices)
}

pub(crate) fn exchange<C: Call>(
    peer: &mut std::io::BufReader<std::os::unix::net::UnixStream>,
    id: u64,
    params: &C::Params,
) -> Option<org_proto::Response> {
    org_proto::frame::write_message(peer.get_mut(), &Request::new::<C>(id, params).unwrap())
        .unwrap();
    org_proto::frame::read_message(peer).ok().flatten()
}

fn refused(response: Option<org_proto::Response>, code: ErrorCode) -> bool {
    matches!(response.map(|r| r.outcome),
        Some(org_proto::Outcome::Error(error)) if error.code == code)
}

fn wait_closed(web: &control_listener::WebConnection) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while web.live() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!web.live(), "protocol error closes the control connection");
}

#[test]
fn web_serving_is_accepted_once_after_hello_and_tls_fetch() {
    use org_host_control::ops::{TlsFetch, WebServing};
    let home = tempfile::tempdir().unwrap();
    let (web, mut peer, offices) = web_peer(home.path());
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    exchange::<TlsFetch>(&mut peer, 2, &Default::default())
        .unwrap()
        .into_result::<TlsFetch>()
        .unwrap();
    assert!(!web.reported_serving());
    exchange::<WebServing>(&mut peer, 3, &Default::default())
        .unwrap()
        .into_result::<WebServing>()
        .unwrap();
    assert!(web.reported_serving());
    assert!(web.live());
    assert!(!web.serving_refused());
    // A duplicate is refused and closes the control connection.
    assert!(refused(
        exchange::<WebServing>(&mut peer, 4, &Default::default()),
        ErrorCode::InvalidRequest
    ));
    wait_closed(&web);
    // The supervisor logs the refusal, not only the EOF that follows it.
    assert!(web.serving_refused());
    drop(web);
    offices.shutdown();
}

#[test]
fn web_serving_with_params_that_do_not_fit_is_a_protocol_error() {
    use org_host_control::ops::TlsFetch;
    let home = tempfile::tempdir().unwrap();
    let (web, mut peer, offices) = web_peer(home.path());
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    exchange::<TlsFetch>(&mut peer, 2, &Default::default())
        .unwrap()
        .into_result::<TlsFetch>()
        .unwrap();
    // In order, but its params don't fit `web.serving`.
    let request: Request = serde_json::from_value(serde_json::json!({
        "id": 3,
        "method": "web.serving",
        "params": {"ready": true},
    }))
    .unwrap();
    org_proto::frame::write_message(peer.get_mut(), &request).unwrap();
    let response = org_proto::frame::read_message(&mut peer).ok().flatten();
    assert!(refused(response, ErrorCode::InvalidRequest));
    wait_closed(&web);
    assert!(!web.reported_serving());
    assert!(web.serving_refused());
    drop(web);
    offices.shutdown();
}

/// An `https` listener with its stored pair: `tls.fetch` answers the chain and the key
/// for the snapshot's origin, and the web process may then report that it serves. A pair
/// that does not name the origin is `unavailable` with `tls_name_mismatch`, and one whose
/// key is gone `storage_error`.
#[test]
fn tls_fetch_on_an_https_listener_answers_the_stored_pair_and_enables_serving() {
    use org_host_control::listen::ListenFailure;
    use org_host_control::ops::{TlsFetch, TlsFetchResult, WebServing};
    let listen = r#"{"address":"127.0.0.1","port":8443,"scheme":"https","origin":"https://127.0.0.1:8443","hsts":"on"}"#;
    let home = tempfile::tempdir().unwrap();
    let (kept, tls) = crate::web_tls::tests::started("https://127.0.0.1:8443");
    let (web, mut peer, offices) = web_peer_for(
        service(home.path()).with_web_tls(Some(Arc::clone(&tls))),
        listen,
    );
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    let fetched = exchange::<TlsFetch>(&mut peer, 2, &Default::default())
        .unwrap()
        .into_result::<TlsFetch>()
        .unwrap();
    let TlsFetchResult::Https {
        cert,
        key,
        origin,
        hsts,
    } = fetched
    else {
        panic!("not the https mode: {fetched:?}");
    };
    assert_eq!(origin.as_str(), "https://127.0.0.1:8443");
    assert_eq!(hsts, org_host_control::listen::Hsts::On);
    let (chain, stored) = tls.fetch().unwrap();
    assert_eq!(cert, chain);
    assert_eq!(key.expose(), stored.expose());
    exchange::<WebServing>(&mut peer, 3, &Default::default())
        .unwrap()
        .into_result::<WebServing>()
        .unwrap();
    assert!(web.reported_serving());
    drop(web);
    offices.shutdown();
    // Another host's pair, then no key.
    crate::web_tls::tests::store_pair_of(&kept, &tls, "https://elsewhere.example");
    let home = tempfile::tempdir().unwrap();
    let (web, mut peer, offices) = web_peer_for(
        service(home.path()).with_web_tls(Some(Arc::clone(&tls))),
        listen,
    );
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    let fetched = exchange::<TlsFetch>(&mut peer, 2, &Default::default())
        .unwrap()
        .into_result::<TlsFetch>()
        .unwrap();
    let TlsFetchResult::Unavailable(ListenFailure::TlsNameMismatch { uncovered }) = fetched else {
        panic!("not a name mismatch: {fetched:?}");
    };
    assert_eq!(uncovered.origins()[0].as_str(), "https://127.0.0.1:8443");
    crate::web_tls::tests::lose_keys(&kept);
    assert!(refused(
        exchange::<TlsFetch>(&mut peer, 3, &Default::default()),
        ErrorCode::StorageError
    ));
    drop(web);
    offices.shutdown();
}

/// A published plain-HTTP listener is answered as that mode, with its origin and no
/// pair, and the web process may then report that it serves.
#[test]
fn tls_fetch_on_a_published_listener_answers_http_published_and_enables_serving() {
    use org_host_control::ops::{TlsFetch, TlsFetchResult, WebServing};
    let home = tempfile::tempdir().unwrap();
    let (web, mut peer, offices) = web_peer_on(
        home.path(),
        r#"{"address":"0.0.0.0","port":8443,"scheme":"http_published","origin":"http://127.0.0.1:8443","hsts":"off"}"#,
    );
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    let fetched = exchange::<TlsFetch>(&mut peer, 2, &Default::default())
        .unwrap()
        .into_result::<TlsFetch>()
        .unwrap();
    let TlsFetchResult::HttpPublished { origin, hsts } = fetched else {
        panic!("not the published mode: {fetched:?}");
    };
    assert_eq!(origin.as_str(), "http://127.0.0.1:8443");
    assert_eq!(hsts, org_host_control::listen::Hsts::Off);
    exchange::<WebServing>(&mut peer, 3, &Default::default())
        .unwrap()
        .into_result::<WebServing>()
        .unwrap();
    assert!(web.reported_serving());
    drop(web);
    offices.shutdown();
}

/// An `https` listener on a host without its pair (no registry): `tls.fetch` is the plain
/// error `storage_error`, and the web process may not report that it serves.
#[test]
fn tls_fetch_on_an_https_listener_without_a_pair_is_refused_and_never_enables_serving() {
    use org_host_control::ops::{TlsFetch, WebServing};
    let home = tempfile::tempdir().unwrap();
    let (web, mut peer, offices) = web_peer_on(
        home.path(),
        r#"{"address":"127.0.0.1","port":8443,"scheme":"https","origin":"https://127.0.0.1:8443","hsts":"off"}"#,
    );
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    assert!(refused(
        exchange::<TlsFetch>(&mut peer, 2, &Default::default()),
        ErrorCode::StorageError
    ));
    assert!(refused(
        exchange::<WebServing>(&mut peer, 3, &Default::default()),
        ErrorCode::InvalidRequest
    ));
    wait_closed(&web);
    assert!(!web.reported_serving());
    drop(web);
    offices.shutdown();
}

#[test]
fn web_serving_before_tls_fetch_is_a_protocol_error() {
    use org_host_control::ops::WebServing;
    let home = tempfile::tempdir().unwrap();
    let (web, mut peer, offices) = web_peer(home.path());
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    assert!(refused(
        exchange::<WebServing>(&mut peer, 2, &Default::default()),
        ErrorCode::InvalidRequest
    ));
    wait_closed(&web);
    assert!(!web.reported_serving());
    assert!(web.serving_refused());
    drop(web);
    offices.shutdown();
}

#[test]
fn web_serving_from_a_local_connection_is_not_authorized() {
    use org_host_control::ops::WebServing;
    let home = tempfile::tempdir().unwrap();
    let mut service = service(home.path());
    service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    let offices = service.offices.clone();
    let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    // Every host.sock connection is `Local`.
    let local = control_listener::LocalConnection::start(host, Arc::new(service)).unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let mut peer = std::io::BufReader::new(peer);
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    assert!(refused(
        exchange::<WebServing>(&mut peer, 2, &Default::default()),
        ErrorCode::NotAuthorized
    ));
    assert!(local.live());
    drop(local);
    offices.shutdown();
}

fn browser_sign_in(seed: u8) -> org_host_control::browser::SessionSignInParams {
    use org_host_control::browser::{BrowserOrigin, Digest32, SessionSignInParams};
    SessionSignInParams {
        login: "owner".into(),
        password: org_host_control::Password::new("synthetic test password".into()).unwrap(),
        client_address: "192.0.2.1".parse().unwrap(),
        cookie_digest: Digest32([seed; 32]),
        proof_digest: Digest32([seed ^ 0xff; 32]),
        origin: BrowserOrigin {
            scheme: "https".into(),
            host: "org.example".into(),
            port: 443,
        },
    }
}

/// `session.end` deletes the record, then writes `session.ended` before its
/// reply on the web connection's one ordered writer; a later validate fails.
#[test]
fn browser_session_end_pushes_session_ended_before_its_reply_on_the_web_connection() {
    use org_host_control::browser::{
        BrowserSessionEnd, BrowserSessionEndParams, SessionEnded, SessionSignIn, SessionValidate,
        SessionValidateParams, SESSION_ENDED,
    };
    let home = tempfile::tempdir().unwrap();
    let (_root, authentication, _c) = crate::authentication::tests::fixture();
    let service = service(home.path()).with_authentication(Some(Arc::new(authentication)));
    let (web, mut peer, offices) = web_peer_for(
        service,
        r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#,
    );
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    let signed_in = browser_sign_in(1);
    let session = exchange::<SessionSignIn>(&mut peer, 2, &signed_in)
        .unwrap()
        .into_result::<SessionSignIn>()
        .unwrap();
    org_proto::frame::write_message(
        peer.get_mut(),
        &Request::new::<BrowserSessionEnd>(
            3,
            &BrowserSessionEndParams {
                session_id: session.session_id.clone(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    let event: org_proto::Event = org_proto::frame::read_message(&mut peer).unwrap().unwrap();
    assert_eq!(event.event, SESSION_ENDED);
    let ended: SessionEnded = serde_json::from_value(event.data).unwrap();
    assert_eq!(ended.session_id, session.session_id);
    let reply: org_proto::Response = org_proto::frame::read_message(&mut peer).unwrap().unwrap();
    assert_eq!(reply.id, 3);
    reply.into_result::<BrowserSessionEnd>().unwrap();
    let validate = SessionValidateParams {
        cookie_digest: signed_in.cookie_digest,
        proof_digest: signed_in.proof_digest,
        origin: signed_in.origin,
    };
    assert!(refused(
        exchange::<SessionValidate>(&mut peer, 4, &validate),
        ErrorCode::NotAuthorized
    ));
    drop(web);
    offices.shutdown();
}

/// `instance.view` on the web connection: `unavailable` until the registered start
/// installs the name, then the name, mode, web state and administrator presence.
/// On host.sock it is `not_authorized`. Every accepted op counts once.
#[test]
fn instance_view_is_served_only_on_the_web_connection_after_the_name_is_installed() {
    use org_host_control::browser::{InstanceView, InstanceViewResult};
    use org_host_control::{Administrator, InstanceMode, WebState};
    let home = tempfile::tempdir().unwrap();
    let (_root, authentication, _c) = crate::authentication::tests::fixture();
    let authentication = Arc::new(authentication);
    let mut service = service(home.path()).with_authentication(Some(authentication.clone()));
    service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    let service = Arc::new(service);
    let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let (transfer, _child) = std::os::unix::net::UnixDatagram::pair().unwrap();
    let listen = serde_json::from_str(
        r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#,
    )
    .unwrap();
    let web = control_listener::WebConnection::start(
        host,
        transfer,
        listen,
        service.clone() as Arc<dyn Handler>,
    )
    .unwrap();
    let mut peer = std::io::BufReader::new(peer);
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    assert!(refused(
        exchange::<InstanceView>(&mut peer, 2, &Default::default()),
        ErrorCode::Unavailable
    ));
    assert!(service.install_instance_name("studio"));
    assert!(!service.install_instance_name("other"), "installed once");
    let view = exchange::<InstanceView>(&mut peer, 3, &Default::default())
        .unwrap()
        .into_result::<InstanceView>()
        .unwrap();
    assert_eq!(
        view,
        InstanceViewResult {
            name: "studio".into(),
            status: InstanceMode::Service,
            web_status: WebState::NotConfigured,
            administrator: Administrator::Present,
            auth_enabled: true,
            tls: None,
        }
    );
    assert_eq!(service.browser_ops(), 2);
    // Unknown params are refused, not ignored.
    org_proto::frame::write_message(
        peer.get_mut(),
        &serde_json::json!({"id": 4, "method": InstanceView::METHOD, "params": {"path": "/"}}),
    )
    .unwrap();
    let response: org_proto::Response = org_proto::frame::read_message(&mut peer).unwrap().unwrap();
    assert!(matches!(
        response.outcome,
        org_proto::Outcome::Error(ref e) if e.code == ErrorCode::InvalidRequest
    ));
    // Host-only CLI approval creation and polling are refused on a web connection.
    assert!(refused(
        exchange::<org_host_control::ops::CliApprovalCreate>(
            &mut peer,
            5,
            &org_host_control::ops::CliApprovalCreateParams {},
        ),
        ErrorCode::NotAuthorized
    ));
    assert!(refused(
        exchange::<org_host_control::ops::CliApprovalPoll>(
            &mut peer,
            6,
            &org_host_control::ops::CliApprovalPollParams {
                approval_id: "0193a5c0-0000-7000-8000-000000000001".into(),
            },
        ),
        ErrorCode::NotAuthorized
    ));
    drop(web);

    let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let local = control_listener::LocalConnection::start(host, service.clone() as Arc<dyn Handler>)
        .unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let mut peer = std::io::BufReader::new(peer);
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    assert!(refused(
        exchange::<InstanceView>(&mut peer, 2, &Default::default()),
        ErrorCode::NotAuthorized
    ));
    assert_eq!(service.browser_ops(), 3, "host.sock reaches no browser op");
    drop(local);
    service.offices.shutdown();
}

/// A local (host.sock) connection cannot create or validate browser sessions:
/// every browser op is `not_authorized` there and writes no session or throttle state.
#[test]
fn browser_session_ops_off_the_web_connection_are_refused() {
    use org_host_control::browser::{
        LinkRegister, SessionSignIn, SessionValidate, SessionValidateParams,
    };
    let home = tempfile::tempdir().unwrap();
    let (_root, authentication, _c) = crate::authentication::tests::fixture();
    let authentication = Arc::new(authentication);
    let mut service = service(home.path()).with_authentication(Some(authentication.clone()));
    service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    let offices = service.offices.clone();
    let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let local = control_listener::LocalConnection::start(host, Arc::new(service)).unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let mut peer = std::io::BufReader::new(peer);
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    // The right password: had it been served, a session would exist.
    let signed_in = browser_sign_in(2);
    assert!(refused(
        exchange::<SessionSignIn>(&mut peer, 2, &signed_in),
        ErrorCode::NotAuthorized
    ));
    // Six wrong passwords: had they counted, the login would now be locked.
    let mut wrong = browser_sign_in(3);
    wrong.password = org_host_control::Password::new("not the password".into()).unwrap();
    for id in 3..9 {
        assert!(refused(
            exchange::<SessionSignIn>(&mut peer, id, &wrong),
            ErrorCode::NotAuthorized
        ));
    }
    let validate = SessionValidateParams {
        cookie_digest: signed_in.cookie_digest,
        proof_digest: signed_in.proof_digest,
        origin: signed_in.origin.clone(),
    };
    assert!(refused(
        exchange::<SessionValidate>(&mut peer, 9, &validate),
        ErrorCode::NotAuthorized
    ));
    let digest = "ab".repeat(32);
    org_proto::frame::write_message(
        peer.get_mut(),
        &serde_json::json!({"id": 10, "method": LinkRegister::METHOD, "params": {
            "link": "A".repeat(43), "login": "ada", "password": "long enough password",
            "client_address": "192.0.2.1", "cookie_digest": digest, "proof_digest": digest,
            "origin": {"scheme": "https", "host": "org.example", "port": 443}}}),
    )
    .unwrap();
    let response: org_proto::Response = org_proto::frame::read_message(&mut peer).unwrap().unwrap();
    assert!(matches!(
        response.outcome,
        org_proto::Outcome::Error(ref e) if e.code == ErrorCode::NotAuthorized
    ));
    org_proto::frame::write_message(
        peer.get_mut(),
        &serde_json::json!({
            "id": 11,
            "method": org_host_control::browser::BrowserCliApprovalDecide::METHOD,
            "params": {
                "auth": {
                    "cookie_digest": digest,
                    "proof_digest": digest,
                    "origin": {"scheme": "https", "host": "org.example", "port": 443}
                },
                "link": "A".repeat(43),
                "approve": true
            }
        }),
    )
    .unwrap();
    let response: org_proto::Response = org_proto::frame::read_message(&mut peer).unwrap().unwrap();
    assert!(matches!(
        response.outcome,
        org_proto::Outcome::Error(ref e) if e.code == ErrorCode::NotAuthorized
    ));
    drop(local);
    offices.shutdown();
    // Nothing was stored: no session for the served digests, and the login
    // and address still have their whole budget.
    let now = i64::try_from(crate::authentication::wall_ms().unwrap()).unwrap();
    let mut ended = Vec::new();
    assert_eq!(
        authentication
            .validate_browser(&validate, now, &mut ended)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    authentication
        .sign_in_browser(&browser_sign_in(4), now, &mut ended)
        .unwrap();
    assert!(ended.is_empty());
}

/// `tls.set` is the administrator's, on `host.sock` only: a bad session, an account that
/// is not the administrator and the web connection are `not_authorized`; an instance
/// without an `https` pair is `unavailable`; the administrator's pair is installed.
#[test]
fn tls_set_needs_the_administrators_session_on_host_sock() {
    use org_host_control::ops::{AccountVerify, AccountVerifyParams, TlsSet, TlsSetParams};
    use org_host_control::{HostOpResult, Password, SessionHandle, TlsPrivateKey};
    let (_root, authentication, _c) = crate::authentication::tests::fixture();
    let authentication = Arc::new(authentication);
    let verifier = crate::authentication::hash_password(
        &Password::new("synthetic test password".into()).unwrap(),
    )
    .unwrap();
    authentication
        .sqlite_for_test()
        .seed_account(
            &crate::ids::generate().unwrap(),
            "Member",
            &verifier,
            false,
            1,
        )
        .unwrap();
    let (_kept, tls) = crate::web_tls::tests::started("https://127.0.0.1:8443");
    let now = crate::web_tls::now().unwrap();
    let (cert, key) = crate::web_tls::tests::pair_for(&["127.0.0.1"], now - 60, now + 30 * 86_400);
    let params = |session: SessionHandle| TlsSetParams {
        session,
        operation_id: crate::ids::uuid_v7().unwrap(),
        cert: cert.clone(),
        key: TlsPrivateKey::new(key.clone()).unwrap(),
    };
    // The administrator calls' admission: one that admits this test's own executable as
    // the development `agentic-org` peer, or one that admits no unsigned peer.
    let admission = |admits: bool| {
        let policy = crate::app_auth::AppPolicy::new(crate::HOST_SIGNING_ID, None).unwrap();
        let policy = if admits {
            policy
                .with_dev_app(&std::env::current_exe().unwrap())
                .unwrap()
        } else {
            policy
        };
        Arc::new(crate::admin::Admin::new(
            authentication.accounts_store(),
            authentication.links_store(),
            &crate::ids::uuid_v7().unwrap(),
            org_host_control::listen::ListenOrigin::parse("https://127.0.0.1:8443").unwrap(),
            policy,
            Arc::new(crate::admin::NativePresence),
        ))
    };
    let local = |web_tls: Option<Arc<crate::web_tls::WebTls>>, admits: Option<bool>| {
        let home = tempfile::tempdir().unwrap();
        let mut service = service(home.path())
            .with_authentication(Some(Arc::clone(&authentication)))
            .with_web_tls(web_tls);
        if let Some(admits) = admits {
            assert!(service.install_admin(admission(admits)));
        }
        service.instance_id = Some(crate::ids::uuid_v7().unwrap());
        let offices = service.offices.clone();
        let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let local = control_listener::LocalConnection::start(host, Arc::new(service)).unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut peer = std::io::BufReader::new(peer);
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        (home, local, peer, offices)
    };
    let sign_in = |peer: &mut std::io::BufReader<std::os::unix::net::UnixStream>, login: &str| {
        exchange::<AccountVerify>(
            peer,
            2,
            &AccountVerifyParams {
                login: login.into(),
                password: Password::new("synthetic test password".into()).unwrap(),
            },
        )
        .unwrap()
        .into_result::<AccountVerify>()
        .unwrap()
        .session
    };
    // Before the administrator service is installed, and for a peer it does not admit
    // (an unsigned process of the same user, where the system names a peer's code), even
    // with the administrator's session. Where it names a peer by its user alone (Linux),
    // the user is the admission, as for every administrator call.
    let by_code = matches!(
        crate::test_programs::image_identity(),
        org_platform_host::admission::ImageIdentity::CodeSignature
    );
    let before = tls.fetch().unwrap().0;
    for admits in [None, Some(false)] {
        let (_home, connection, mut peer, offices) = local(Some(Arc::clone(&tls)), admits);
        let owner = sign_in(&mut peer, "Owner");
        let answer = exchange::<TlsSet>(&mut peer, 3, &params(owner));
        match admits {
            None => assert!(refused(answer, ErrorCode::Unavailable)),
            Some(_) if by_code => assert!(refused(answer, ErrorCode::NotAuthorized)),
            Some(_) => assert!(answer
                .unwrap()
                .into_result::<TlsSet>()
                .is_ok_and(|r| matches!(r, HostOpResult::Completed { .. }))),
        }
        drop(connection);
        offices.shutdown();
    }
    let (_home, connection, mut peer, offices) = local(Some(Arc::clone(&tls)), Some(true));
    let bogus = SessionHandle::new("A".repeat(43)).unwrap();
    assert!(refused(
        exchange::<TlsSet>(&mut peer, 3, &params(bogus)),
        ErrorCode::NotAuthorized
    ));
    let member = sign_in(&mut peer, "Member");
    assert!(refused(
        exchange::<TlsSet>(&mut peer, 4, &params(member)),
        ErrorCode::NotAuthorized
    ));
    let owner = sign_in(&mut peer, "Owner");
    let answered = exchange::<TlsSet>(&mut peer, 5, &params(owner))
        .unwrap()
        .into_result::<TlsSet>()
        .unwrap();
    assert_eq!(
        answered,
        HostOpResult::Completed {
            result: org_proto::Empty {}
        }
    );
    let after = tls.fetch().unwrap().0;
    assert_ne!(after, before);
    drop(connection);
    offices.shutdown();
    // Without an https pair.
    let (_home, connection, mut peer, offices) = local(None, Some(true));
    let owner = sign_in(&mut peer, "Owner");
    assert!(refused(
        exchange::<TlsSet>(&mut peer, 3, &params(owner)),
        ErrorCode::Unavailable
    ));
    drop(connection);
    offices.shutdown();
    // On the web connection: refused before its params are read.
    let home = tempfile::tempdir().unwrap();
    let (web, mut peer, offices) = web_peer_for(
        service(home.path())
            .with_authentication(Some(Arc::clone(&authentication)))
            .with_web_tls(Some(Arc::clone(&tls))),
        r#"{"address":"127.0.0.1","port":8443,"scheme":"https","origin":"https://127.0.0.1:8443","hsts":"off"}"#,
    );
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    assert!(refused(
        exchange::<TlsSet>(
            &mut peer,
            2,
            &params(SessionHandle::new("A".repeat(43)).unwrap())
        ),
        ErrorCode::NotAuthorized
    ));
    assert_eq!(tls.fetch().unwrap().0, after);
    drop(web);
    offices.shutdown();
}

/// Before the registered start installs the administrator service, both admin calls
/// answer `unavailable` on the local channel rather than reaching any registry.
#[test]
fn admin_calls_are_unavailable_before_install_admin() {
    use org_host_control::admin::{AdminLinkInitial, AdminLinkInitialParams, AdminReset};
    let home = tempfile::tempdir().unwrap();
    let mut service = service(home.path());
    service.instance_id = Some(crate::ids::uuid_v7().unwrap());
    assert!(service.admin().is_none());
    let offices = service.offices.clone();
    let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let local = control_listener::LocalConnection::start(host, Arc::new(service)).unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let mut peer = std::io::BufReader::new(peer);
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    let params = AdminLinkInitialParams {
        setup_operation: crate::admin::tests::OPERATION.into(),
    };
    assert!(refused(
        exchange::<AdminLinkInitial>(&mut peer, 2, &params),
        ErrorCode::Unavailable
    ));
    assert!(refused(
        exchange::<AdminReset>(&mut peer, 3, &Default::default()),
        ErrorCode::Unavailable
    ));
    assert!(local.live());
    drop(local);
    offices.shutdown();
}

/// An installed administrator service refuses both calls off a non-local channel (here
/// the web process's) before any registry read, and issues no link.
#[test]
fn admin_calls_off_a_non_local_channel_are_refused_and_issue_nothing() {
    use crate::links::{LinkKind, Subject};
    use org_host_control::admin::{AdminLinkInitial, AdminLinkInitialParams, AdminReset};
    let home = tempfile::tempdir().unwrap();
    let (_registry, admin) = crate::admin::tests::admin();
    let admin = Arc::new(admin);
    let service = service(home.path());
    assert!(service.install_admin(admin.clone()));
    assert!(!service.install_admin(admin.clone()), "installed once");
    let (web, mut peer, offices) = web_peer_for(
        service,
        r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#,
    );
    exchange::<Hello>(&mut peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    let params = AdminLinkInitialParams {
        setup_operation: crate::admin::tests::OPERATION.into(),
    };
    assert!(refused(
        exchange::<AdminLinkInitial>(&mut peer, 2, &params),
        ErrorCode::NotAuthorized
    ));
    assert!(refused(
        exchange::<AdminReset>(&mut peer, 3, &Default::default()),
        ErrorCode::NotAuthorized
    ));
    // Nothing was issued: the first link for the registration subject is still available.
    assert!(crate::admin::tests::registry(&admin)
        .pending(
            LinkKind::Registration,
            &Subject::new("administrator").unwrap(),
            1
        )
        .unwrap()
        .is_none());
    drop(web);
    offices.shutdown();
}

/// Hello and pre-hello requests run inline (see the listener's
/// `an_inline_call_runs_before_anything_more_is_read`).
#[test]
fn hello_and_pre_hello_requests_run_inline() {
    let dir = tempfile::tempdir().unwrap();
    let service = service(dir.path());
    let id = "01AAAAAAAAAAAAAAAAAAAAAAAA".to_owned();
    let inline = [
        Request::new::<Hello>(1, &HelloParams::default()).unwrap(),
        // The retired `office.app_token`: refused inline, like any call the host doesn't serve.
        Request {
            id: 1,
            method: "office.app_token".to_owned(),
            params: serde_json::json!({"office_id": id.clone()}),
            principal: None,
        },
        Request::new::<OfficeRestart>(1, &OfficeRestartParams { office_id: id }).unwrap(),
    ];
    let threaded = [
        Request::new::<InstanceStatus>(1, &Default::default()).unwrap(),
        Request::new::<OfficeList>(1, &Empty {}).unwrap(),
        // The retired name-only create: no typed call exists for it any more.
        Request {
            id: 1,
            method: "organization.create".to_owned(),
            params: serde_json::json!({"name": "Acme"}),
            principal: None,
        },
    ];
    let connection = Connection::for_test();
    for request in inline.iter().chain(&threaded) {
        assert!(service.inline(request, &connection), "{}", request.method);
    }
    connection.complete_hello();
    for request in inline.iter().chain(&threaded) {
        assert_eq!(
            service.inline(request, &connection),
            request.is::<Hello>(),
            "{}",
            request.method
        );
    }
    service.offices.shutdown();
}

#[test]
fn local_access_and_tls_enable_refused_off_web_and_on_https() {
    use org_host_control::browser::{
        BrowserAuthEnable, BrowserAuthEnableParams, BrowserLocalSession, BrowserLocalSessionParams,
        BrowserOrigin, BrowserTlsEnable, BrowserTlsEnableParams, Digest32, TlsEnableMode,
    };
    use org_host_control::organization::BrowserAuth;
    use org_host_control::Password;

    let dir = tempfile::tempdir().unwrap();
    let (_auth_root, authentication) = crate::authentication::tests::fixture_without_accounts();
    let auth = Arc::new(authentication);
    let (db, _) = auth.provisioning_storage();
    assert!(db.init_local_authless_if_empty(1000).unwrap().is_some());
    assert!(!db.auth_enabled().unwrap());

    let origin = BrowserOrigin {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port: 48123,
    };

    // 1. Off the web connection (ordinary host.sock peer): all three browser ops are refused.
    let mut local_svc = service(dir.path()).with_authentication(Some(Arc::clone(&auth)));
    local_svc.instance_id = Some(crate::ids::uuid_v7().unwrap());
    let local_offices = local_svc.offices.clone();
    let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let local = control_listener::LocalConnection::start(host, Arc::new(local_svc)).unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let mut local_peer = std::io::BufReader::new(peer);
    exchange::<Hello>(&mut local_peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    assert!(refused(
        exchange::<BrowserLocalSession>(
            &mut local_peer,
            2,
            &BrowserLocalSessionParams {
                origin: origin.clone(),
            },
        ),
        ErrorCode::NotAuthorized
    ));
    assert!(refused(
        exchange::<BrowserAuthEnable>(
            &mut local_peer,
            3,
            &BrowserAuthEnableParams {
                login: "Owner".into(),
                password: Password::new("long-valid-password-123".into()).unwrap(),
                client_address: "127.0.0.1".parse().unwrap(),
                cookie_digest: Digest32([0x11; 32]),
                proof_digest: Digest32([0x22; 32]),
                origin: origin.clone(),
            },
        ),
        ErrorCode::NotAuthorized
    ));
    assert!(refused(
        exchange::<BrowserTlsEnable>(
            &mut local_peer,
            4,
            &BrowserTlsEnableParams {
                auth: BrowserAuth::local_authless(origin.clone()),
                mode: TlsEnableMode::SelfSigned,
                hsts: None,
            },
        ),
        ErrorCode::NotAuthorized
    ));
    drop(local);
    local_offices.shutdown();

    // 2. On an HTTPS web connection: local_authless session is refused (fail closed).
    let https_svc = service(dir.path()).with_authentication(Some(Arc::clone(&auth)));
    let (https_web, mut https_peer, offices) = web_peer_for(
        https_svc,
        r#"{"address":"127.0.0.1","port":48123,"scheme":"https","origin":"https://127.0.0.1:48123","hsts":"off"}"#,
    );
    exchange::<Hello>(&mut https_peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    assert!(refused(
        exchange::<BrowserLocalSession>(
            &mut https_peer,
            2,
            &BrowserLocalSessionParams {
                origin: BrowserOrigin {
                    scheme: "https".into(),
                    host: "127.0.0.1".into(),
                    port: 48123,
                },
            },
        ),
        ErrorCode::NotAuthorized
    ));
    drop(https_web);
    offices.shutdown();

    // 3. On a plain HTTP web connection: local_session succeeds, auth_enable upgrades & disables authless.
    let http_svc = service(dir.path()).with_authentication(Some(Arc::clone(&auth)));
    let (http_web, mut http_peer, offices2) = web_peer_for(
        http_svc,
        r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#,
    );
    exchange::<Hello>(&mut http_peer, 1, &Default::default())
        .unwrap()
        .into_result::<Hello>()
        .unwrap();
    let local_sess = exchange::<BrowserLocalSession>(
        &mut http_peer,
        2,
        &BrowserLocalSessionParams {
            origin: origin.clone(),
        },
    )
    .unwrap()
    .into_result::<BrowserLocalSession>()
    .unwrap();
    assert!(local_sess.admin);
    org_proto::frame::write_message(
        http_peer.get_mut(),
        &Request::new::<BrowserAuthEnable>(
            3,
            &BrowserAuthEnableParams {
                login: "Owner".into(),
                password: Password::new("long-valid-password-123".into()).unwrap(),
                client_address: "127.0.0.1".parse().unwrap(),
                cookie_digest: Digest32([0x33; 32]),
                proof_digest: Digest32([0x44; 32]),
                origin: origin.clone(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    let ended_event: serde_json::Value = org_proto::frame::read_message(&mut http_peer)
        .unwrap()
        .unwrap();
    assert_eq!(
        ended_event["event"],
        org_host_control::browser::SESSION_ENDED
    );
    assert_eq!(
        ended_event["data"]["session_id"],
        "01000000000000000000000000"
    );
    let enabled: org_proto::Response = org_proto::frame::read_message(&mut http_peer)
        .unwrap()
        .unwrap();
    let enabled = enabled.into_result::<BrowserAuthEnable>().unwrap();
    assert_eq!(enabled.account.login, "Owner");
    assert!(db.auth_enabled().unwrap());
    assert!(refused(
        exchange::<BrowserLocalSession>(&mut http_peer, 4, &BrowserLocalSessionParams { origin },),
        ErrorCode::NotAuthorized
    ));
    drop(http_web);
    offices2.shutdown();
}
