//! F4 production-path proof (GW-01 falsification): the real host, composed through
//! the protected reverse bootstrap that creates the administrator and HQ, spawns
//! the real built `agentic-org-web` and serves browser sessions over loopback HTTP.
//! Only disposable `/private/tmp` roots and debug development custody are used;
//! debug `ServerCheck::Skip` establishes no release-signing evidence.
use crate::bootstrap::BootstrapHost;
use org_host_control::{
    ops::{Bootstrap, BootstrapOutcome, BootstrapParams, Exit},
    provision::{OrganizationProvision, Params},
    HostOpResult, Password,
};
use org_proto::{frame, selected_bootstrap::PlanV1, ApiError, Call, Empty, HelloParams, Request};
use std::{
    fs,
    io::{BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::{fs::PermissionsExt, net::UnixListener, net::UnixStream},
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(60);
const PASSWORD: &str = "synthetic first password";

struct Reply {
    status: u16,
    headers: String,
    body: serde_json::Value,
}
impl Reply {
    fn set_cookie(&self) -> Option<&str> {
        self.headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("set-cookie")
                .then(|| value.trim())
        })
    }
    /// The cookie name=value pair plus the proof, as request headers.
    fn pair(&self) -> String {
        let cookie = self.set_cookie().expect("session cookie");
        let proof = self.body["result"]["proof"].as_str().expect("proof");
        format!(
            "Cookie: {}\r\nX-Org-Session-Proof: {proof}\r\n",
            cookie.split(';').next().unwrap()
        )
    }
}

fn http(address: SocketAddr, method: &str, path: &str, headers: &str, body: &str) -> Reply {
    let mut stream = TcpStream::connect(address).expect("gateway listener");
    stream.set_read_timeout(Some(WAIT)).unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").expect("HTTP response");
    let status = headers
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("status line");
    Reply {
        status,
        headers: headers.into(),
        body: serde_json::from_str(body).unwrap_or(serde_json::Value::Null),
    }
}

fn sign_in(address: SocketAddr, login: &str, password: &str) -> Reply {
    let body = serde_json::json!({ "login": login, "password": password }).to_string();
    http(address, "POST", "/api/v1/session/sign-in", "", &body)
}

/// The single protected-auth refusal (web-api-v1 §4), with no cookie issued.
fn assert_refused(reply: &Reply, what: &str) {
    assert_eq!(reply.status, 401, "{what}: {}", reply.headers);
    assert_eq!(reply.body["error"]["code"], "not_authorized", "{what}");
    assert_eq!(
        reply.body["error"]["session_reason"], "session_ended",
        "{what}"
    );
    assert!(reply.set_cookie().is_none(), "{what}: no cookie on refusal");
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Actual HTTP readiness; connection creation alone is not treated as success.
fn http_ready(address: SocketAddr) -> bool {
    let Ok(mut stream) = TcpStream::connect(address) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    if write!(
        stream,
        "GET /api/v1/version HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .is_err()
    {
        return false;
    }
    let mut first = [0; 12];
    stream.read_exact(&mut first).is_ok() && &first == b"HTTP/1.1 200"
}

fn answer<C: Call>(
    wire: &mut BufReader<UnixStream>,
    id: u64,
    p: &C::Params,
) -> Result<C::Result, ApiError> {
    frame::write_message(wire.get_mut(), &Request::new::<C>(id, p).unwrap()).unwrap();
    let response: org_proto::Response = frame::read_message(wire).unwrap().unwrap();
    assert_eq!(response.id, id);
    response.into_result::<C>().map_err(|error| match error {
        org_proto::CallError::Api(error) => error,
        _ => panic!("malformed typed response"),
    })
}

fn gateway_ready(host: &BootstrapHost) -> bool {
    let state = host.state.state.lock().unwrap();
    state.host.as_ref().is_some_and(crate::Host::gateway_ready)
}

/// Every live process executing exactly `program` (a unique temp path).
fn gateway_pids(program: &Path) -> Vec<String> {
    let out = Command::new("/usr/bin/pgrep")
        .arg("-f")
        .arg(program)
        .output()
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// The single live process executing exactly `program` (a unique temp path).
fn gateway_pid(program: &Path) -> String {
    let pids = gateway_pids(program);
    assert_eq!(pids.len(), 1, "exactly one gateway process: {pids:?}");
    pids.into_iter().next().unwrap()
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual gateway/worker listeners require permitted local bind"]
fn actual_host_spawns_web_gateway_with_sessions_refresh_sign_out_eof_and_lockout() {
    let office = std::env::var_os("ORG_TEST_OFFICE_BIN")
        .expect("ORG_TEST_OFFICE_BIN must name the built agentic-org-office");
    let web = std::env::var_os("ORG_TEST_WEB_BIN")
        .expect("ORG_TEST_WEB_BIN must name the built agentic-org-web");
    let temp = tempfile::tempdir_in("/private/tmp").unwrap();
    let base = temp.path().canonicalize().unwrap();
    for name in ["temp", "keys", "bin"] {
        fs::create_dir(base.join(name)).unwrap();
        fs::set_permissions(base.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    // The host runs the gateway as the worker's sibling; the web image serves
    // `<exe dir>/ui`. Copies keep the build outputs untouched.
    let worker = base.join("bin/agentic-org-office");
    let program = base.join("bin/agentic-org-web");
    fs::copy(office, &worker).unwrap();
    fs::copy(web, &program).unwrap();
    fs::create_dir(base.join("bin/ui")).unwrap();
    // Startup validation hashes the entry's inline bootstrap into the CSP and
    // refuses an entry without one (MissingInlineScript), as for a real build.
    fs::write(
        base.join("bin/ui/index.html"),
        b"<html><body>public component fixture<script>/* fixture bootstrap */</script></body></html>",
    )
    .unwrap();
    let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reserved.local_addr().unwrap();
    drop(reserved);
    let listen = serde_json::from_value(serde_json::json!({
        "address":"127.0.0.1","port":address.port(),"scheme":"http_loopback",
        "origin":format!("http://{address}"),"hsts":"off"
    }))
    .unwrap();

    let socket = base.join("setup.sock");
    let listening = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let mut config = crate::Config::new(base.join("unused-home"), worker);
    config.dev_key_dir = Some(base.join("keys"));
    config.dev_app_path = Some(std::env::current_exe().unwrap());
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
    stream.set_read_timeout(Some(WAIT)).unwrap();
    let mut wire = BufReader::new(stream);
    answer::<org_host_control::Hello>(&mut wire, 1, &HelloParams::default()).unwrap();
    let p = BootstrapParams {
        operation_id: crate::ids::generate().unwrap(),
        hq_operation_id: crate::ids::generate().unwrap(),
        login: "Owner".into(),
        password: Password::new(PASSWORD.into()).unwrap(),
        tls: None,
    };
    let BootstrapOutcome::Completed { result: first } =
        answer::<Bootstrap>(&mut wire, 2, &p).unwrap()
    else {
        panic!("bootstrap refused")
    };
    // Authenticated but HQ is not Active yet: the gateway is not serving.
    assert!(!gateway_ready(&host));
    // The wired start-eligibility gate holds the supervisor before any spawn or bind:
    // across several supervisor polls no agentic-org-web child runs and the browser
    // port accepts nothing.
    let window = Instant::now() + Duration::from_secs(3);
    while Instant::now() < window {
        let pids = gateway_pids(&program);
        assert!(
            pids.is_empty(),
            "gateway spawned before HQ is Active: {pids:?}"
        );
        assert!(
            TcpStream::connect(address).is_err(),
            "browser port accepted before HQ is Active"
        );
        thread::sleep(Duration::from_millis(50));
    }
    assert!(!gateway_ready(&host));
    let prepared = Params {
        session: first.session,
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
    let mut id = 3;
    let until = Instant::now() + WAIT;
    loop {
        let result = answer::<OrganizationProvision>(&mut wire, id, &prepared).unwrap();
        id += 1;
        match result {
            HostOpResult::Completed { .. } => break,
            HostOpResult::Accepted => {
                assert!(Instant::now() < until, "HQ provisioning deadline");
                thread::sleep(Duration::from_millis(50));
            }
            _ => panic!("unexpected provisioning result"),
        }
    }

    // 1. Admission after HQ: the supervisor spawns and admits the real web image.
    wait_until("gateway_ready", || gateway_ready(&host));
    wait_until("HTTP readiness", || http_ready(address));
    let version = http(address, "GET", "/api/v1/version", "", "");
    assert_eq!(version.status, 200);
    assert_eq!(version.body["result"]["api_version"], 1);
    assert_eq!(
        version.body["result"]["contract_revision"],
        org_proto::gateway::CONTRACT_REVISION
    );
    assert_refused(&http(address, "GET", "/api/v1/session", "", ""), "no pair");

    // 2. Sign-in with the bootstrap administrator through host account.verify.
    let signed = sign_in(address, "Owner", PASSWORD);
    assert_eq!(signed.status, 200, "{}", signed.headers);
    assert!(signed
        .headers
        .to_ascii_lowercase()
        .contains("cache-control: no-store"));
    let cookie = signed.set_cookie().unwrap();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
    assert!(!cookie.contains("Secure"), "loopback HTTP cookie");
    assert_eq!(signed.body["result"]["account"]["login"], "Owner");
    let pair = signed.pair();

    // 3. GET session needs the cookie+proof pair; neither half alone works.
    let view = http(address, "GET", "/api/v1/session", &pair, "");
    assert_eq!(view.status, 200, "{}", view.headers);
    assert_eq!(view.body["result"]["admin"], true);
    assert!(view.body["result"].get("proof").is_none());
    let cookie_only = pair.lines().next().unwrap().to_owned() + "\r\n";
    assert_refused(
        &http(address, "GET", "/api/v1/session", &cookie_only, ""),
        "cookie without proof",
    );

    // 4. Refresh rotates both halves: the old pair is refused, the new one works.
    let refreshed = http(address, "POST", "/api/v1/session/refresh", &pair, "{}");
    assert_eq!(refreshed.status, 200, "{}", refreshed.headers);
    let next = refreshed.pair();
    assert_ne!(next, pair);
    assert_refused(
        &http(address, "GET", "/api/v1/session", &pair, ""),
        "pre-refresh pair",
    );
    assert_eq!(
        http(address, "GET", "/api/v1/session", &next, "").status,
        200
    );

    // 5. Sign-out ends the session.
    let out = http(address, "POST", "/api/v1/session/sign-out", &next, "{}");
    assert_eq!(out.status, 200, "{}", out.headers);
    assert_refused(
        &http(address, "GET", "/api/v1/session", &next, ""),
        "signed-out pair",
    );

    // 6. Control EOF: kill the admitted gateway. Readiness turns false and its
    //    sessions end; the supervisor's bounded restart serves a new process.
    let live = sign_in(address, "Owner", PASSWORD);
    assert_eq!(live.status, 200);
    let live = live.pair();
    assert_eq!(
        http(address, "GET", "/api/v1/session", &live, "").status,
        200
    );
    let killed = gateway_pid(&program);
    let status = Command::new("/bin/kill")
        .args(["-KILL", &killed])
        .status()
        .unwrap();
    assert!(status.success());
    wait_until("gateway_ready false after EOF", || !gateway_ready(&host));
    wait_until("restarted gateway_ready", || gateway_ready(&host));
    wait_until("restarted HTTP readiness", || http_ready(address));
    assert_ne!(gateway_pid(&program), killed, "a new admitted process");
    assert_refused(
        &http(address, "GET", "/api/v1/session", &live, ""),
        "session from the ended control connection",
    );

    // 7. One sign-in lockout through the gateway (HEAD per-socket login_lockout,
    //    Socket::Control). Wrong password, unknown login and a locked correct
    //    password are indistinguishable (C-r3-2).
    let unknown = sign_in(address, "nobody-here", "wrong synthetic password");
    assert_refused(&unknown, "unknown login");
    for attempt in 0..6 {
        let wrong = sign_in(address, "Owner", "wrong synthetic password");
        assert_refused(&wrong, &format!("wrong password {attempt}"));
        assert_eq!(wrong.body, unknown.body, "no account existence revealed");
    }
    let locked = sign_in(address, "Owner", PASSWORD);
    assert_refused(&locked, "correct password while locked");
    assert_eq!(locked.body, unknown.body, "lockout is not distinguishable");
    // The refusal was the timed lockout: once it lapses the same credential works.
    let until = Instant::now() + WAIT;
    loop {
        thread::sleep(Duration::from_millis(1_100));
        let retry = sign_in(address, "Owner", PASSWORD);
        if retry.status == 200 {
            break;
        }
        assert_refused(&retry, "locked retry");
        assert!(Instant::now() < until, "lockout never lapsed");
    }

    answer::<Exit>(&mut wire, id, &Empty {}).unwrap();
    host.shutdown();
    drop(wire);
    // Host shutdown drops the gateway first: no web process survives.
    let left = Command::new("/usr/bin/pgrep")
        .arg("-f")
        .arg(&program)
        .output()
        .unwrap();
    assert!(left.stdout.is_empty(), "gateway reaped on host shutdown");
}
