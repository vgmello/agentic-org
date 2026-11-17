//! A scripted host on a real socket pair exercises the browser registry, the
//! control actor and the HTTP routes. It proves no production admission; the
//! real-process host/gateway test owns that.
use super::*;
use crate::{
    assets::BuildFiles,
    ingress::RequestAdmission,
    service::{ApiLimits, Gateway},
    Hsts, StaticUi,
};
use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, header, Method, Request};
use org_host_control::listen::ListenOrigin;
use std::{
    io::BufReader,
    os::unix::net::{UnixDatagram, UnixStream},
};

const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const LOGIN: &str = "OWNER";
const PASSWORD: &str = "generated unit credential";
const ORIGIN: &str = "http://127.0.0.1:8080";
const HOST: &str = "127.0.0.1:8080";
/// The only host methods the F4 gateway may call. Anything else, including
/// an office channel open, panics the scripted host and fails the test.
const ALLOWED: [&str; 6] = [
    "hello",
    "tls.fetch",
    "account.verify",
    "session.inspect",
    "session.renew",
    "session.end",
];

struct Fixture {
    sessions: Arc<BrowserSessions>,
    stop: UnixStream,
    _transfer: UnixDatagram,
    thread: Option<std::thread::JoinHandle<()>>,
    calls: Arc<Mutex<Vec<String>>>,
    errors: Arc<Mutex<BTreeMap<&'static str, &'static str>>>,
    ended: Arc<Mutex<std::collections::BTreeSet<String>>>,
    /// Stalls every scripted `session.inspect` answer while non-zero.
    inspect_delay: Arc<Mutex<Duration>>,
}
fn reply(
    request: &serde_json::Value,
    issued: &mut usize,
    end_delay: Duration,
) -> serde_json::Value {
    let id = request["id"].as_u64().unwrap();
    let method = request["method"].as_str().unwrap();
    assert!(ALLOWED.contains(&method), "unexpected host method {method}");
    if method == "session.end" {
        std::thread::sleep(end_delay);
    }
    let result = match method {
        "hello" => serde_json::to_value(
            org_proto::HelloResult::host("test", "disposable").with_instance_id(INSTANCE),
        )
        .unwrap(),
        "account.verify" => {
            let p = &request["params"];
            if p["login"] != LOGIN || p["password"] != PASSWORD {
                return serde_json::json!({"id":id,"error":{"code":"not_authorized","message":"Sign-in failed."}});
            }
            *issued += 1;
            serde_json::json!({"session":format!("synthetic native handle {issued}"),"account":"account","admin":true,"orgs":[]})
        }
        "session.inspect" => {
            serde_json::json!({"account":"account","login":"Owner","admin":true,"orgs":[],"server_time_ms":1794312000000u64,"absolute_expires_at_ms":1794355200000u64,"idle_expires_at_ms":1794313800000u64})
        }
        _ => serde_json::json!({}),
    };
    serde_json::json!({"id":id,"result":result})
}
impl Fixture {
    fn new() -> Self {
        Self::with_secure(false)
    }
    fn with_secure(secure: bool) -> Self {
        Self::scripted(secure, Duration::ZERO)
    }
    /// `end_delay` stalls every scripted `session.end` answer.
    fn scripted(secure: bool, end_delay: Duration) -> Self {
        let (gateway, host) = UnixStream::pair().unwrap();
        let stop = host.try_clone().unwrap();
        let (transfer, host_transfer) = UnixDatagram::pair().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let errors = Arc::new(Mutex::new(BTreeMap::new()));
        let scripted_errors = Arc::clone(&errors);
        let ended = Arc::new(Mutex::new(std::collections::BTreeSet::<String>::new()));
        let ended_sessions = Arc::clone(&ended);
        let inspect_delay = Arc::new(Mutex::new(Duration::ZERO));
        let stall = Arc::clone(&inspect_delay);
        let thread = std::thread::spawn(move || {
            let mut host = BufReader::new(host);
            let mut issued = 0;
            while let Ok(Some(request)) =
                org_proto::frame::read_message::<serde_json::Value>(&mut host)
            {
                seen.lock()
                    .unwrap()
                    .push(request["method"].as_str().unwrap().to_owned());
                if request["method"] == "session.inspect" {
                    let delay = *stall.lock().unwrap();
                    std::thread::sleep(delay);
                }
                let error = scripted_errors
                    .lock()
                    .unwrap()
                    .remove(request["method"].as_str().unwrap())
                    .or_else(|| {
                        request["params"]["session"]
                            .as_str()
                            .filter(|handle| ended_sessions.lock().unwrap().contains(*handle))
                            .map(|_| "not_authorized")
                    });
                let answer = match error {
                    Some(code) => {
                        serde_json::json!({"id":request["id"],"error":{"code":code,"message":"scripted host refusal"}})
                    }
                    None => {
                        let answer = reply(&request, &mut issued, end_delay);
                        if request["method"] == "session.end" {
                            ended_sessions
                                .lock()
                                .unwrap()
                                .insert(request["params"]["session"].as_str().unwrap().to_owned());
                        }
                        answer
                    }
                };
                if org_proto::frame::write_message(host.get_mut(), &answer).is_err() {
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
        let sessions = BrowserSessions::new(native, INSTANCE.into(), secure)
            .unwrap_or_else(|_| panic!("session construction"));
        Self {
            sessions,
            stop,
            _transfer: host_transfer,
            thread: Some(thread),
            calls,
            errors,
            ended,
            inspect_delay,
        }
    }
    fn sign_in(&self) -> Established {
        self.sessions
            .sign_in(LOGIN.into(), Password::new(PASSWORD.into()).unwrap())
            .unwrap_or_else(|_| panic!("sign in"))
    }
    fn count(&self, method: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|m| *m == method)
            .count()
    }
    fn refused(&self, cookie: &str, proof: &str) -> bool {
        match self.sessions.authenticate(&parts(cookie, proof)) {
            Ok(_) => false,
            Err(reply) => is_session_ended(&reply),
        }
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
fn is_session_ended(reply: &ApiReply) -> bool {
    let (status, body) = reply.inspected();
    let body: serde_json::Value = serde_json::from_slice(body).unwrap();
    status == StatusCode::UNAUTHORIZED
        && body["error"]["code"] == "not_authorized"
        && body["error"]["session_reason"] == "session_ended"
}
fn cookie_pair(set_cookie: &str) -> &str {
    set_cookie.split(';').next().unwrap()
}
fn parts(cookie: &str, proof: &str) -> Parts {
    let mut builder = Request::builder().uri("/api/v1/session");
    if !cookie.is_empty() {
        builder = builder.header(header::COOKIE, cookie_pair(cookie));
    }
    if !proof.is_empty() {
        builder = builder.header(SESSION_PROOF_HEADER, proof);
    }
    builder.body(()).unwrap().into_parts().0
}
fn ticket(sessions: &Arc<BrowserSessions>, established: &Established) -> Ticket {
    sessions
        .authenticate(&parts(&established.cookie, established.proof.expose()))
        .unwrap_or_else(|_| panic!("authorized pair"))
}
fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[test]
fn cookie_and_proof_are_independent_256_bit_values_and_both_are_required() {
    let f = Fixture::new();
    let established = f.sign_in();
    let cookie_value = cookie_pair(&established.cookie).split_once('=').unwrap().1;
    assert!(hex64(cookie_value));
    assert!(hex64(established.proof.expose()));
    assert_ne!(cookie_value, established.proof.expose());
    assert!(!established.cookie.contains(established.proof.expose()));
    // Cookie-only, proof-only, malformed and duplicated values are refused.
    assert!(f.refused(&established.cookie, ""));
    assert!(f.refused("", established.proof.expose()));
    assert!(f.refused(&established.cookie, &"a".repeat(64)));
    assert!(f.refused(
        &established.cookie,
        &established.proof.expose().to_uppercase()
    ));
    let mut duplicate = parts(&established.cookie, established.proof.expose());
    duplicate.headers.append(
        SESSION_PROOF_HEADER,
        header::HeaderValue::from_str(established.proof.expose()).unwrap(),
    );
    assert!(f.sessions.authenticate(&duplicate).is_err());
    let mut duplicate = parts(&established.cookie, established.proof.expose());
    let cookie = duplicate.headers.get(header::COOKIE).unwrap().clone();
    duplicate.headers.append(header::COOKIE, cookie);
    assert!(f.sessions.authenticate(&duplicate).is_err());
    assert!(f.sessions.get(ticket(&f.sessions, &established)).is_ok());
}

#[test]
fn a_valid_cookie_with_another_sessions_proof_is_refused() {
    let f = Fixture::new();
    let first = f.sign_in();
    let second = f.sign_in();
    assert!(f.refused(&first.cookie, second.proof.expose()));
    assert!(f.refused(&second.cookie, first.proof.expose()));
    assert!(f.sessions.get(ticket(&f.sessions, &first)).is_ok());
    assert!(f.sessions.get(ticket(&f.sessions, &second)).is_ok());
}

#[test]
fn unknown_login_and_wrong_password_have_the_identical_refusal() {
    let f = Fixture::new();
    let attempt = |login: &str, password: &str| match f
        .sessions
        .sign_in(login.into(), Password::new(password.into()).unwrap())
    {
        Ok(_) => panic!("refused credentials established a session"),
        Err(reply) => {
            let (status, body) = reply.inspected();
            (status, body.to_vec())
        }
    };
    let unknown = attempt("NOBODY", PASSWORD);
    let wrong = attempt(LOGIN, "not the credential");
    assert_eq!(unknown, wrong);
    assert_eq!(unknown.0, StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = serde_json::from_slice(&unknown.1).unwrap();
    assert_eq!(body["error"]["code"], "not_authorized");
    assert!(!String::from_utf8_lossy(&unknown.1).contains("NOBODY"));
    assert!(f.sessions.sessions.lock().unwrap().is_empty());
}

#[test]
fn passwords_and_secret_bodies_are_redacted() {
    let password = Password::new(PASSWORD.into()).unwrap();
    assert!(!format!("{password:?}").contains(PASSWORD));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let body = runtime
        .block_on(read_secret_body(Full::new(Bytes::from_static(
            b"{\"login\":\"OWNER\",\"password\":\"generated unit credential\"}",
        ))))
        .unwrap();
    assert_eq!(format!("{body:?}"), "SecretBody(<redacted>)");
}

#[test]
fn refresh_renews_on_the_host_and_rotates_both_values_atomically() {
    let f = Fixture::new();
    let first = f.sign_in();
    let queued = ticket(&f.sessions, &first);
    let renewals = f.count("session.renew");
    let second = f
        .sessions
        .refresh(ticket(&f.sessions, &first))
        .unwrap_or_else(|_| panic!("refresh"));
    assert_eq!(f.count("session.renew"), renewals + 1);
    assert_ne!(first.proof.expose(), second.proof.expose());
    assert_ne!(cookie_pair(&first.cookie), cookie_pair(&second.cookie));
    // The old pair and any half-mixed pair are refused; a ticket authenticated
    // before the swap can no longer act.
    assert!(f.refused(&first.cookie, first.proof.expose()));
    assert!(f.refused(&first.cookie, second.proof.expose()));
    assert!(f.refused(&second.cookie, first.proof.expose()));
    assert!(f.sessions.current(&queued).is_err());
    assert!(f.sessions.refresh(queued).is_err());
    assert_eq!(f.sessions.sessions.lock().unwrap().len(), 1);
    assert!(f.sessions.get(ticket(&f.sessions, &second)).is_ok());
    assert_eq!(f.count("session.end"), 0);
}

#[test]
fn sign_out_ends_the_host_session_and_clears_the_pair() {
    let f = Fixture::new();
    let established = f.sign_in();
    let other = f.sign_in();
    f.sessions
        .sign_out(ticket(&f.sessions, &established))
        .unwrap_or_else(|_| panic!("sign out"));
    assert_eq!(f.count("session.end"), 1);
    assert!(f.refused(&established.cookie, established.proof.expose()));
    let clear = f.sessions.clear_cookie();
    assert!(clear.starts_with(&format!("{}=;", f.sessions.cookie_name)));
    assert!(clear.contains("Max-Age=0"));
    assert!(f.sessions.get(ticket(&f.sessions, &other)).is_ok());
}

#[test]
fn failed_sign_in_preparation_ends_only_the_new_host_session() {
    let f = Fixture::new();
    let live = f.sign_in();
    for (n, code) in ["unavailable", "storage_error", "not_found"]
        .into_iter()
        .enumerate()
    {
        f.errors.lock().unwrap().insert("session.inspect", code);
        let error = f
            .sessions
            .sign_in(LOGIN.into(), Password::new(PASSWORD.into()).unwrap())
            .err()
            .expect("inspection failure must refuse sign-in");
        assert_eq!(error.inspected().0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(f.count("session.end"), n + 1);
        {
            let ended = f.ended.lock().unwrap();
            assert_eq!(ended.len(), n + 1);
            assert!(ended.contains(&format!("synthetic native handle {}", n + 2)));
        }
        assert_eq!(f.sessions.sessions.lock().unwrap().len(), 1);
        assert!(f.sessions.host.live());
        assert!(f.sessions.get(ticket(&f.sessions, &live)).is_ok());
    }
    let recovered = f.sign_in();
    assert!(f.sessions.get(ticket(&f.sessions, &recovered)).is_ok());
    assert_eq!(f.sessions.sessions.lock().unwrap().len(), 2);
}

#[test]
fn sign_in_reclaims_unpresented_host_revocations_at_capacity() {
    for keep_live in [false, true] {
        let f = Fixture::new();
        let live = f.sign_in();
        for _ in 1..MAX_SESSIONS {
            f.sign_in();
        }
        let live_key = ticket(&f.sessions, &live).key;
        {
            let bindings = f.sessions.sessions.lock().unwrap();
            let mut ended = f.ended.lock().unwrap();
            for (key, session) in bindings.iter() {
                if !keep_live || *key != live_key {
                    ended.insert(session.handle.expose().to_owned());
                }
            }
        }
        // No request presents any revoked pair before correct reauthentication.
        let replacement = f.sign_in();
        assert!(f.sessions.get(ticket(&f.sessions, &replacement)).is_ok());
        assert!(f.sessions.sessions.lock().unwrap().len() <= MAX_SESSIONS);
        if keep_live {
            assert!(f.sessions.get(ticket(&f.sessions, &live)).is_ok());
        }
    }
}

#[test]
fn capacity_recheck_preserves_bindings_on_transient_host_errors() {
    let f = Fixture::new();
    let live = f.sign_in();
    for _ in 1..MAX_SESSIONS {
        f.sign_in();
    }
    for code in ["not_found", "unavailable", "storage_error"] {
        f.errors.lock().unwrap().insert("session.inspect", code);
        let error = f.sessions.reclaim_capacity().unwrap_err();
        assert_eq!(error.inspected().0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(f.sessions.sessions.lock().unwrap().len(), MAX_SESSIONS);
        assert!(f.sessions.get(ticket(&f.sessions, &live)).is_ok());
        assert!(f.sessions.host.live());
    }
}

#[test]
fn an_exhausted_capacity_scan_keeps_the_control_connection_and_sessions() {
    let f = Fixture::new();
    let live = f.sign_in();
    for _ in 1..MAX_SESSIONS {
        f.sign_in();
    }
    // Each probe takes 60 ms and the scan may start probes for 100 ms, so the
    // window ends while the second probe is still in flight at the host.
    *f.inspect_delay.lock().unwrap() = Duration::from_millis(60);
    let started = Instant::now();
    assert!(f
        .sessions
        .reclaim_capacity_within(Duration::from_millis(100))
        .is_ok());
    assert!(started.elapsed() >= Duration::from_millis(100));
    *f.inspect_delay.lock().unwrap() = Duration::ZERO;
    assert!(f.sessions.host.live());
    assert!(f.sessions.get(ticket(&f.sessions, &live)).is_ok());
    assert_eq!(f.sessions.sessions.lock().unwrap().len(), MAX_SESSIONS);
}

#[test]
fn host_denial_releases_capacity_for_reauthentication() {
    let f = Fixture::new();
    let revoked = f.sign_in();
    let live = f.sign_in();
    for _ in 2..MAX_SESSIONS {
        f.sign_in();
    }
    assert_eq!(f.sessions.sessions.lock().unwrap().len(), MAX_SESSIONS);
    assert!(f
        .sessions
        .sign_in(LOGIN.into(), Password::new(PASSWORD.into()).unwrap())
        .is_err());

    // The host has ended this handle (for example after a password change),
    // but its control connection and unrelated sessions are still live.
    f.errors
        .lock()
        .unwrap()
        .insert("session.renew", "not_authorized");
    let result = f.sessions.get(ticket(&f.sessions, &revoked));
    assert!(matches!(result, Err(ref error) if is_session_ended(error)));
    assert_eq!(f.sessions.sessions.lock().unwrap().len(), MAX_SESSIONS - 1);
    assert!(f.refused(&revoked.cookie, revoked.proof.expose()));
    let replacement = f.sign_in();
    assert!(f.sessions.get(ticket(&f.sessions, &replacement)).is_ok());
    assert!(f.sessions.get(ticket(&f.sessions, &live)).is_ok());
    assert_eq!(f.sessions.sessions.lock().unwrap().len(), MAX_SESSIONS);
}

#[test]
fn host_denial_evicts_the_pair_but_transient_errors_preserve_it() {
    for (operation, method) in [
        ("get", "session.renew"),
        ("get", "session.inspect"),
        ("refresh", "session.renew"),
        ("refresh", "session.inspect"),
        ("sign-out", "session.end"),
    ] {
        for code in ["not_authorized", "unavailable"] {
            let f = Fixture::new();
            let pair = f.sign_in();
            let live = f.sign_in();
            let queued = ticket(&f.sessions, &pair);
            f.errors.lock().unwrap().insert(method, code);
            let result = match operation {
                "get" => f.sessions.get(queued).map(|_| ()),
                "refresh" => f.sessions.refresh(queued).map(|_| ()),
                "sign-out" => f.sessions.sign_out(queued),
                _ => unreachable!(),
            };
            let error = match result {
                Err(error) => error,
                Ok(()) => panic!("{operation}/{method} ignored host {code}"),
            };
            if code == "not_authorized" {
                assert!(is_session_ended(&error));
                assert!(f.refused(&pair.cookie, pair.proof.expose()));
                assert_eq!(f.sessions.sessions.lock().unwrap().len(), 1);
            } else {
                assert_eq!(error.inspected().0, StatusCode::SERVICE_UNAVAILABLE);
                assert!(f.sessions.get(ticket(&f.sessions, &pair)).is_ok());
                assert_eq!(f.sessions.sessions.lock().unwrap().len(), 2);
            }
            assert!(f.sessions.get(ticket(&f.sessions, &live)).is_ok());
            assert!(f.sessions.host.live());
        }
    }
}

#[test]
fn host_denial_of_an_inflight_read_evicts_a_concurrently_rotated_pair() {
    let f = Fixture::new();
    let first = f.sign_in();
    let queued = ticket(&f.sessions, &first);
    let rotated = f
        .sessions
        .refresh(ticket(&f.sessions, &first))
        .unwrap_or_else(|_| panic!("refresh"));
    // A read can pass current()/renew before refresh rotates, then inspect the
    // retained session afterwards. Both pairs refer to the same host handle.
    f.errors
        .lock()
        .unwrap()
        .insert("session.inspect", "not_authorized");
    let result = f.sessions.view(&queued.session);
    assert!(matches!(result, Err(ref error) if is_session_ended(error)));
    assert!(f.refused(&rotated.cookie, rotated.proof.expose()));
    assert!(f.sessions.sessions.lock().unwrap().is_empty());
}

#[test]
fn idle_and_absolute_expiry_end_the_pair() {
    let f = Fixture::new();
    let idle = f.sign_in();
    let absolute = f.sign_in();
    let past = |by: Duration| {
        Instant::now()
            .checked_sub(by + Duration::from_secs(1))
            .expect("monotonic clock far enough from boot")
    };
    let idle_key = ticket(&f.sessions, &idle).key;
    let absolute_key = ticket(&f.sessions, &absolute).key;
    {
        let mut map = f.sessions.sessions.lock().unwrap();
        *map[&idle_key].touched.lock().unwrap() = past(IDLE);
        let current = Arc::clone(&map[&absolute_key]);
        let aged = Session {
            handle: Arc::clone(&current.handle),
            proof_mac: current.proof_mac,
            account: current.account.clone(),
            admin: current.admin,
            organizations: current.organizations.clone(),
            created: past(ABSOLUTE),
            touched: Mutex::new(Instant::now()),
        };
        map.insert(absolute_key, Arc::new(aged));
    }
    assert!(f.refused(&idle.cookie, idle.proof.expose()));
    assert!(f.refused(&absolute.cookie, absolute.proof.expose()));
    assert!(f.sessions.sessions.lock().unwrap().is_empty());
    // Browser expiry also ends both host sessions instead of leaving their
    // host slots held until the host's own idle timeout (off the request path).
    let until = Instant::now() + Duration::from_secs(5);
    while f.count("session.end") < 2 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(f.count("session.end"), 2);
}

#[test]
fn reap_never_waits_for_a_stalled_host_end() {
    // Every host end stalls for 2 s; before the fix reap waited for each one.
    let f = Fixture::scripted(false, Duration::from_secs(2));
    let expired = f.sign_in();
    let live = f.sign_in();
    let key = ticket(&f.sessions, &expired).key;
    {
        let mut map = f.sessions.sessions.lock().unwrap();
        let stale = Instant::now()
            .checked_sub(IDLE + Duration::from_secs(1))
            .expect("monotonic clock far enough from boot");
        *map[&key].touched.lock().unwrap() = stale;
        // More expired pairs than the cleanup queue holds: overflow is dropped.
        let session = Arc::clone(&map[&key]);
        for n in 0..CLEANUP_QUEUE + 8 {
            let mut filler = [0xeeu8; 32];
            filler[..8].copy_from_slice(&(n as u64).to_be_bytes());
            map.insert(filler, Arc::clone(&session));
        }
    }
    let started = Instant::now();
    assert!(f.refused(&expired.cookie, expired.proof.expose()));
    // An unrelated live pair is also served without waiting for host cleanup.
    let parts = parts(&live.cookie, live.proof.expose());
    assert!(f.sessions.authenticate(&parts).is_ok());
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "reap blocked on host cleanup: {:?}",
        started.elapsed()
    );
    assert_eq!(f.sessions.sessions.lock().unwrap().len(), 1);
    // The cleanup thread does reach the host.
    let until = Instant::now() + Duration::from_secs(5);
    while f.count("session.end") == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(f.count("session.end") >= 1);
}

#[test]
fn control_eof_ends_every_session() {
    let f = Fixture::new();
    let first = f.sign_in();
    let second = f.sign_in();
    f.stop.shutdown(std::net::Shutdown::Both).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while f.sessions.host.live() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!f.sessions.host.live());
    assert!(f.refused(&first.cookie, first.proof.expose()));
    assert!(f.refused(&second.cookie, second.proof.expose()));
    assert!(f.sessions.sessions.lock().unwrap().is_empty());
    assert!(f
        .sessions
        .sign_in(LOGIN.into(), Password::new(PASSWORD.into()).unwrap())
        .is_err());
}

#[test]
fn session_cap_refuses_new_sign_ins_and_ends_the_unused_host_session() {
    let f = Fixture::new();
    let template = f.sign_in();
    let key = ticket(&f.sessions, &template).key;
    {
        let mut map = f.sessions.sessions.lock().unwrap();
        let session = Arc::clone(&map[&key]);
        for n in 0..MAX_SESSIONS {
            let mut filler = [0xffu8; 32];
            filler[..8].copy_from_slice(&(n as u64).to_be_bytes());
            map.insert(filler, Arc::clone(&session));
        }
        assert!(map.len() >= MAX_SESSIONS);
    }
    let ended = f.count("session.end");
    assert!(f
        .sessions
        .sign_in(LOGIN.into(), Password::new(PASSWORD.into()).unwrap())
        .is_err());
    assert_eq!(f.count("session.end"), ended + 1);
}

#[test]
fn cookie_attributes_are_scoped_and_secure_on_https() {
    let plain = set_cookie("org_session_0123456789abcdef", "v", false, false);
    assert_eq!(
        plain.as_str(),
        "org_session_0123456789abcdef=v; HttpOnly; SameSite=Strict; Path=/api/v1"
    );
    let secure = set_cookie("org_session_0123456789abcdef", "v", true, false);
    assert_eq!(
        secure.as_str(),
        "org_session_0123456789abcdef=v; HttpOnly; SameSite=Strict; Path=/api/v1; Secure"
    );
    let cleared = set_cookie("org_session_0123456789abcdef", "", true, true);
    assert_eq!(
        cleared.as_str(),
        "org_session_0123456789abcdef=; Max-Age=0; HttpOnly; SameSite=Strict; Path=/api/v1; Secure"
    );
    for cookie in [&plain, &secure, &cleared] {
        assert!(!cookie.to_ascii_lowercase().contains("domain"));
    }
    let f = Fixture::with_secure(true);
    let established = f.sign_in();
    assert!(established.cookie.ends_with("; Secure"));
    let name = cookie_pair(&established.cookie).split_once('=').unwrap().0;
    let suffix = name.strip_prefix("org_session_").unwrap();
    assert_eq!(suffix.len(), 16);
    assert!(suffix
        .bytes()
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
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
        r#"{"login":"OWNER","password":"a","password":"b"}"#,
        r#"["OWNER","a"]"#,
        r#"{"login":"OWNER","password":"a","owner":true}"#,
    ] {
        let request = Request::builder()
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
            .block_on(read_secret_body(Full::new(Bytes::copy_from_slice(
                body.as_bytes(),
            ))))
            .unwrap();
        assert!(api.prepare_credentials(route, body).is_err());
    }
    let too_large = runtime.block_on(read_secret_body(Full::new(Bytes::from(vec![
        b'a';
        ingress::SECRET_BODY_BYTES
            + 1
    ]))));
    assert!(matches!(too_large, Err(crate::body::BodyError::TooLarge)));
    assert_eq!(time::format_ms(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(time::format_ms(951782400000), "2000-02-29T00:00:00.000Z");
    assert_eq!(f.count("account.verify"), 0);
}

// HTTP surface through the real gateway service, its Host/Origin admission
// and the session routes.
fn gateway(f: &Fixture) -> Gateway<SessionApi> {
    let origin = ListenOrigin::parse(ORIGIN).unwrap();
    let files =
        BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())]).unwrap();
    Gateway::new(
        StaticUi::new(files, origin.clone(), Hsts::Off),
        SessionApi::new(Arc::clone(&f.sessions)),
        ApiLimits::default(),
        RequestAdmission::new(origin),
    )
    .unwrap()
}
struct Sent {
    status: StatusCode,
    headers: header::HeaderMap,
    body: serde_json::Value,
}
fn send(
    gateway: &Gateway<SessionApi>,
    method: Method,
    target: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Sent {
    let mut builder = Request::builder().method(method).uri(target);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder
        .body(Full::new(Bytes::copy_from_slice(body.as_bytes())))
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let response = runtime.block_on(gateway.respond(request));
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = runtime
        .block_on(response.into_body().collect())
        .unwrap()
        .to_bytes();
    Sent {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    }
}
const SIGN_IN: &str = r#"{"login":"OWNER","password":"generated unit credential"}"#;

#[test]
fn origin_or_host_mismatch_is_refused_before_any_host_call() {
    let f = Fixture::new();
    let gateway = gateway(&f);
    for headers in [
        vec![("host", "evil.test:8080"), ("origin", ORIGIN)],
        vec![("host", HOST), ("origin", "http://evil.test:8080")],
        vec![("host", HOST), ("origin", "https://127.0.0.1:8080")],
        vec![("host", HOST)],
    ] {
        let sent = send(
            &gateway,
            Method::POST,
            "/api/v1/session/sign-in",
            &headers,
            SIGN_IN,
        );
        assert!(
            matches!(sent.status, StatusCode::BAD_REQUEST | StatusCode::FORBIDDEN),
            "{headers:?}"
        );
        assert!(sent.headers.get(header::SET_COOKIE).is_none());
    }
    assert_eq!(f.count("account.verify"), 0);
}

#[test]
fn http_session_lifecycle_and_unknown_api_routes() {
    let f = Fixture::new();
    let gateway = gateway(&f);
    let same = [("host", HOST), ("origin", ORIGIN)];
    let version = send(
        &gateway,
        Method::GET,
        "/api/v1/version",
        &[("host", HOST)],
        "",
    );
    assert_eq!(version.status, StatusCode::OK);
    assert_eq!(
        version.body,
        serde_json::json!({"result":{"api_version":1,"contract_revision":org_proto::gateway::CONTRACT_REVISION}})
    );
    for target in [
        "/api/v1/tasks",
        "/api/v1/organizations",
        "/api/v1/session/other",
    ] {
        let missing = send(&gateway, Method::GET, target, &[("host", HOST)], "");
        assert_eq!(missing.status, StatusCode::NOT_FOUND, "{target}");
        assert_eq!(missing.body["error"]["code"], "not_found");
    }
    let signed = send(
        &gateway,
        Method::POST,
        "/api/v1/session/sign-in",
        &same,
        SIGN_IN,
    );
    assert_eq!(signed.status, StatusCode::OK);
    let set_cookie = signed.headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(set_cookie.ends_with("; HttpOnly; SameSite=Strict; Path=/api/v1"));
    let proof = signed.body["result"]["proof"].as_str().unwrap().to_owned();
    assert!(hex64(&proof));
    assert_eq!(signed.body["result"]["account"]["login"], "Owner");
    assert!(!signed.body.to_string().contains(PASSWORD));
    let cookie = cookie_pair(&set_cookie).to_owned();
    // Cookie-only GET is refused with the session_ended shape.
    let cookie_only = send(
        &gateway,
        Method::GET,
        "/api/v1/session",
        &[("host", HOST), ("cookie", &cookie)],
        "",
    );
    assert_eq!(cookie_only.status, StatusCode::UNAUTHORIZED);
    assert_eq!(cookie_only.body["error"]["session_reason"], "session_ended");
    let paired = [
        ("host", HOST),
        ("origin", ORIGIN),
        ("cookie", cookie.as_str()),
        (SESSION_PROOF_HEADER, proof.as_str()),
    ];
    let got = send(&gateway, Method::GET, "/api/v1/session", &paired, "");
    assert_eq!(got.status, StatusCode::OK);
    assert!(got.body["result"].get("proof").is_none());
    assert!(got.headers.get(header::SET_COOKIE).is_none());
    let refreshed = send(
        &gateway,
        Method::POST,
        "/api/v1/session/refresh",
        &paired,
        "{}",
    );
    assert_eq!(refreshed.status, StatusCode::OK);
    let next_cookie = refreshed.headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_owned();
    let next_proof = refreshed.body["result"]["proof"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(cookie_pair(&next_cookie), cookie);
    assert_ne!(next_proof, proof);
    let stale = send(&gateway, Method::GET, "/api/v1/session", &paired, "");
    assert_eq!(stale.status, StatusCode::UNAUTHORIZED);
    let next = [
        ("host", HOST),
        ("origin", ORIGIN),
        ("cookie", cookie_pair(&next_cookie)),
        (SESSION_PROOF_HEADER, next_proof.as_str()),
    ];
    let out = send(
        &gateway,
        Method::POST,
        "/api/v1/session/sign-out",
        &next,
        "{}",
    );
    assert_eq!(out.status, StatusCode::OK);
    assert_eq!(out.body, serde_json::json!({"result":{}}));
    assert!(out.headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    let after = send(&gateway, Method::GET, "/api/v1/session", &next, "");
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
    assert_eq!(f.count("session.end"), 1);
}

#[test]
fn the_f4_gateway_sends_no_office_channel_request() {
    // Source-level guard in addition to the scripted host's method allowlist.
    for (name, source) in [
        ("sessions.rs", include_str!("../sessions.rs")),
        ("control.rs", include_str!("control.rs")),
        ("http.rs", include_str!("http.rs")),
        ("process.rs", include_str!("../process.rs")),
    ] {
        for forbidden in [
            ["office", ".open_channel"].concat(),
            ["Office", "OpenChannel"].concat(),
            ["selected", "_tasks"].concat(),
            ["org", "_client"].concat(),
        ] {
            assert!(!source.contains(&forbidden), "{name} contains {forbidden}");
        }
    }
}
