use super::*;
use std::{
    io::BufReader,
    os::unix::net::{UnixDatagram, UnixStream},
    sync::atomic::{AtomicBool, Ordering},
};

const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
struct Fixture {
    sessions: Arc<BrowserSessions>,
    stop: UnixStream,
    _transfer: UnixDatagram,
    thread: Option<std::thread::JoinHandle<()>>,
    ended: Arc<AtomicBool>,
}
impl Fixture {
    fn new() -> Self {
        let (gateway, host) = UnixStream::pair().unwrap();
        let stop = host.try_clone().unwrap();
        let (transfer, host_transfer) = UnixDatagram::pair().unwrap();
        let ended = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&ended);
        let thread = std::thread::spawn(move || {
            let mut host = BufReader::new(host);
            while let Ok(Some(request)) =
                org_proto::frame::read_message::<serde_json::Value>(&mut host)
            {
                let id = request["id"].as_u64().unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "hello" => serde_json::to_value(
                        org_proto::HelloResult::host("test", "disposable")
                            .with_instance_id(INSTANCE),
                    )
                    .unwrap(),
                    "account.verify" => {
                        assert_eq!(request["params"]["login"], "OWNER");
                        assert_eq!(request["params"]["password"], "generated unit credential");
                        serde_json::json!({"session":"synthetic native handle","account":"account","admin":true,"orgs":[]})
                    }
                    "session.inspect" => {
                        serde_json::json!({"account":"account","login":"Owner","admin":true,"orgs":[],"server_time_ms":1794312000000u64,"absolute_expires_at_ms":1794355200000u64,"idle_expires_at_ms":1794313800000u64})
                    }
                    "session.renew" => serde_json::json!({}),
                    "office.open_channel" => {
                        panic!("unlinked scope must not open a worker channel")
                    }
                    "session.end" => {
                        seen.store(true, Ordering::SeqCst);
                        serde_json::json!({})
                    }
                    _ => panic!("unexpected native method"),
                };
                if org_proto::frame::write_message(
                    host.get_mut(),
                    &serde_json::json!({"id":id,"result":result}),
                )
                .is_err()
                {
                    break;
                }
            }
        });
        let native = HostControl::from_handoff(
            gateway.into(),
            transfer.into(),
            INSTANCE.into(),
            Arc::new(|| panic!("unexpected ancillary failure")),
        )
        .unwrap();
        let sessions = BrowserSessions::new(native, INSTANCE.into(), false)
            .unwrap_or_else(|_| panic!("session construction"));
        Self {
            sessions,
            stop,
            _transfer: host_transfer,
            thread: Some(thread),
            ended,
        }
    }
    fn sign_in(&self) -> Established {
        self.sessions
            .sign_in(
                "OWNER".into(),
                Password::new("generated unit credential".into()).unwrap(),
            )
            .unwrap_or_else(|_| panic!("sign in"))
    }
}
#[test]
fn live_unlinked_scope_is_hidden_without_ending_the_browser_or_native_session() {
    let f = Fixture::new();
    let established = f.sign_in();
    for scope in [
        OfficeScope {
            kind: org_proto::selected_tasks::OfficeKind::Office,
            instance_id: INSTANCE.into(),
            org_id: "unlinked-org".into(),
            office_id: "missing-office".into(),
        },
        OfficeScope {
            kind: org_proto::selected_tasks::OfficeKind::Office,
            instance_id: "other-instance".into(),
            org_id: "unlinked-org".into(),
            office_id: "missing-office".into(),
        },
    ] {
        let error = match f.sessions.office(
            ticket(&f.sessions, &established),
            &scope,
            Duration::from_secs(1),
        ) {
            Ok(_) => panic!("unlinked scope admitted"),
            Err(error) => error,
        };
        assert_eq!(error.inspected().0, StatusCode::NOT_FOUND);
        let body: serde_json::Value = serde_json::from_slice(error.inspected().1).unwrap();
        assert_eq!(body["error"]["code"], "not_found");
        assert!(f.sessions.get(ticket(&f.sessions, &established)).is_ok());
        assert!(f.sessions.host.live());
        assert!(!f.ended.load(Ordering::SeqCst));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.stop.shutdown(std::net::Shutdown::Both);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
fn parts(cookie: &str, proof: &str) -> Parts {
    hyper::Request::builder()
        .uri("/api/v1/session")
        .header(hyper::header::COOKIE, cookie.split(';').next().unwrap())
        .header("X-Org-Session-Proof", proof)
        .body(())
        .unwrap()
        .into_parts()
        .0
}
fn ticket(sessions: &Arc<BrowserSessions>, established: &Established) -> Ticket {
    sessions
        .authenticate(&parts(&established.cookie, established.proof.expose()))
        .unwrap_or_else(|_| panic!("authorized pair"))
}
#[test]
fn native_bound_browser_pair_rotates_atomically_and_host_eof_ends_authority() {
    let f = Fixture::new();
    let first = f.sign_in();
    assert_eq!(first.view.account.login, "Owner");
    assert_eq!(first.view.absolute_expires_at, "2026-11-11T00:00:00.000Z");
    assert!(first
        .cookie
        .contains("HttpOnly; SameSite=Strict; Path=/api/v1"));
    assert!(!first.cookie.contains("Secure"));
    assert!(!first.cookie.contains(first.proof.expose()));
    let queued = ticket(&f.sessions, &first);
    let second = f
        .sessions
        .refresh(ticket(&f.sessions, &first))
        .unwrap_or_else(|_| panic!("refresh"));
    assert_ne!(first.proof.expose(), second.proof.expose());
    assert!(f
        .sessions
        .authenticate(&parts(&first.cookie, first.proof.expose()))
        .is_err());
    assert!(f.sessions.current(&queued).is_err());
    assert!(f.sessions.get(ticket(&f.sessions, &second)).is_ok());
    f.stop.shutdown(std::net::Shutdown::Both).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while f.sessions.host.live() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!f.sessions.host.live());
    assert!(f
        .sessions
        .authenticate(&parts(&second.cookie, second.proof.expose()))
        .is_err());
}
#[test]
fn pair_requires_both_exact_single_values_and_logout_ends_the_host_session() {
    let f = Fixture::new();
    let established = f.sign_in();
    for (cookie, proof) in [
        (established.cookie.as_str(), ""),
        ("", established.proof.expose()),
        (established.cookie.as_str(), &"a".repeat(64)),
    ] {
        assert!(f.sessions.authenticate(&parts(cookie, proof)).is_err());
    }
    let mut duplicate = parts(&established.cookie, established.proof.expose());
    duplicate.headers.append(
        "X-Org-Session-Proof",
        hyper::header::HeaderValue::from_str(established.proof.expose()).unwrap(),
    );
    assert!(f.sessions.authenticate(&duplicate).is_err());
    let mut duplicate = parts(&established.cookie, established.proof.expose());
    duplicate.headers.append(
        hyper::header::COOKIE,
        duplicate
            .headers
            .get(hyper::header::COOKIE)
            .unwrap()
            .clone(),
    );
    assert!(f.sessions.authenticate(&duplicate).is_err());
    f.sessions
        .sign_out(ticket(&f.sessions, &established))
        .unwrap_or_else(|_| panic!("logout"));
    assert!(f.ended.load(Ordering::SeqCst));
    assert!(f
        .sessions
        .authenticate(&parts(&established.cookie, established.proof.expose()))
        .is_err());
}
#[test]
fn credential_routes_decode_directly_and_reject_duplicates_arrays_and_unknowns() {
    use crate::{path::RequestPath, service::ApiBackend};
    let f = Fixture::new();
    let api = SessionApi::new(Arc::clone(&f.sessions));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    for body in [
        r#"{"login":"OWNER","password":"a","password":"b"}"#,
        r#"{"login":"OWNER","pass\u0077ord":"a","password":"b"}"#,
        r#"["OWNER","a"]"#,
        r#"{"login":"OWNER","password":"a","owner":true}"#,
    ] {
        let request = hyper::Request::builder()
            .method("POST")
            .uri("/api/v1/session/sign-in")
            .body(())
            .unwrap()
            .into_parts()
            .0;
        let route = api
            .route(
                &request,
                &RequestPath::parse("/api/v1/session/sign-in").unwrap(),
            )
            .unwrap_or_else(|_| panic!("route"));
        assert!(api.credentials(&route));
        let body = runtime
            .block_on(read_secret_body(http_body_util::Full::new(
                hyper::body::Bytes::copy_from_slice(body.as_bytes()),
            )))
            .unwrap();
        assert!(api.prepare_credentials(route, body).is_err());
    }
    let too_large = runtime.block_on(read_secret_body(http_body_util::Full::new(
        hyper::body::Bytes::from(vec![b'a'; ingress::SECRET_BODY_BYTES + 1]),
    )));
    assert!(matches!(too_large, Err(crate::body::BodyError::TooLarge)));
    assert_eq!(time::format_ms(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(time::format_ms(951782400000), "2000-02-29T00:00:00.000Z");
}
