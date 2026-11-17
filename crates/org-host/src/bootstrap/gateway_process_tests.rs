//! F4 production-path proof (GW-01 falsification): the real host, composed through
//! the protected reverse bootstrap that creates the administrator and HQ, spawns
//! the real built `agentic-org-web`, which serves `/api/v1/version` over loopback
//! HTTP. Browser sign-in waits for stored sessions (FC-159): the session routes
//! are JSON 404 and sign in nobody.
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

/// A missing API route (web-api-v1 §2): JSON 404, no cookie.
fn assert_missing(reply: &Reply, what: &str) {
    assert_eq!(reply.status, 404, "{what}: {}", reply.headers);
    assert_eq!(reply.body["error"]["code"], "not_found", "{what}");
    assert!(
        !reply.headers.to_ascii_lowercase().contains("set-cookie"),
        "{what}: no cookie"
    );
}

/// Every browser session route is absent until FC-159.
fn assert_no_sign_in(address: SocketAddr) {
    let credentials = serde_json::json!({ "login": "Owner", "password": PASSWORD }).to_string();
    for (method, path, body) in [
        ("POST", "/api/v1/session/sign-in", credentials.as_str()),
        ("GET", "/api/v1/session", ""),
        ("POST", "/api/v1/session/refresh", "{}"),
        ("POST", "/api/v1/session/sign-out", "{}"),
    ] {
        assert_missing(&http(address, method, path, "", body), path);
    }
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
fn actual_host_spawns_web_gateway_serves_version_and_restarts_after_eof() {
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
    assert!(version
        .headers
        .to_ascii_lowercase()
        .contains("cache-control: no-store"));
    // 2. No browser sign-in yet: even the bootstrap administrator's correct
    //    credentials reach no host account.verify.
    assert_no_sign_in(address);

    // 3. Control EOF: kill the admitted gateway. Readiness turns false; the
    //    supervisor's bounded restart serves a new process.
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
    assert_eq!(http(address, "GET", "/api/v1/version", "", "").status, 200);
    assert_no_sign_in(address);

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
