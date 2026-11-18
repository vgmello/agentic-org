//! F4 production-path proof (GW-01 falsification): the real host, started for a
//! registered instance whose registry holds a seeded administrator, spawns the
//! real built `agentic-org-web`, which serves `/api/v1/version` over loopback
//! HTTP. A protected session route without the browser pair is 401 before any
//! host call; routes not yet built are JSON 404.
//! The administrator is seeded directly (A2 D1: the reverse bootstrap is gone and
//! A3 creates administrators through a one-time link); HQ is provisioned over
//! `host.sock` exactly as before.
//! Only disposable `/private/tmp` roots and debug development custody are used;
//! debug `ServerCheck::Skip` establishes no release-signing evidence.
use crate::{
    home::{HostLock, Layout},
    hostdb::{HostDb, HostDbInit},
    instance::registry::Registry,
    service::authentication::hash_password,
    Config, Host,
};
use org_host_control::{
    ops::{AccountVerify, AccountVerifyParams},
    provision::{OrganizationProvision, Params, Provisioned},
    HostOpResult, Password, SessionHandle,
};
use org_proto::{frame, selected_bootstrap::PlanV1, ApiError, Call, HelloParams, Request};
use std::{
    fs,
    io::{BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod recovery_custody;

const WAIT: Duration = Duration::from_secs(60);
const PASSWORD: &str = "synthetic first password";

struct Reply {
    status: u16,
    headers: String,
    body: serde_json::Value,
}
fn http(address: SocketAddr, method: &str, path: &str, headers: &str, body: &str) -> Reply {
    let mut stream = TcpStream::connect(address).expect("web process listener");
    stream.set_read_timeout(Some(WAIT)).unwrap();
    // One write for head and body: a split write can leave late body bytes unread
    // when the web process answers early, and its close then resets the socket
    // (a known transport limit tracked for A3, not what this test proves).
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
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

/// Without a cookie and proof the protected session routes are 401 and set no
/// session cookie; routes not yet built stay JSON 404.
fn assert_no_sign_in(address: SocketAddr) {
    for (method, path, body) in [
        ("GET", "/api/v1/session", ""),
        ("GET", "/api/v1/organizations", ""),
    ] {
        let reply = http(address, method, path, "", body);
        assert_eq!(reply.status, 401, "{path}: {}", reply.headers);
        assert_eq!(reply.body["error"]["code"], "not_authorized", "{path}");
        assert!(
            !reply.headers.to_ascii_lowercase().contains("set-cookie"),
            "{path}: no cookie"
        );
    }
    assert_missing(
        &http(address, "POST", "/api/v1/session/refresh", "", "{}"),
        "/api/v1/session/refresh",
    );
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

fn web_ready(host: &Host) -> bool {
    host.web_ready()
}

/// Every live process executing exactly `program` (a unique temp path).
fn web_pids(program: &Path) -> Vec<String> {
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
fn web_pid(program: &Path) -> String {
    let pids = web_pids(program);
    assert_eq!(pids.len(), 1, "exactly one web process: {pids:?}");
    pids.into_iter().next().unwrap()
}

/// A registered instance as the service host will find it after setup and the
/// administrator's registration: the instance reserved with its browser address,
/// its registry created under the host and instance locks, one administrator.
/// Returns the started host and its `host.sock` path.
fn start_registered_host(
    mut config: Config,
    base: &Path,
    listen: &org_host_control::listen::ListenConfig,
) -> (Host, PathBuf) {
    config.admit_runtime().unwrap();
    let registry = Registry::open(&base.join("locator"), &base.join("temp")).unwrap();
    let instance = registry.reserve(None, &base.join("instance")).unwrap();
    instance.set_browser(listen).unwrap();
    let paths = instance.prepare_layout().unwrap();
    let layout = Layout::with_runtime(&paths.office_home, instance.temp_folder());
    layout.check_socket_paths().unwrap();
    layout.prepare().unwrap();
    let lock = HostLock::acquire(&layout).unwrap();
    let custody = crate::host_secrets::store_for(
        config.dev_key_dir.as_deref(),
        instance.root(),
        instance.id(),
        crate::code_signing::own_signing,
    )
    .unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let init = HostDbInit {
        instance_id: instance.id().to_owned(),
        clock_high_ms: now,
    };
    let db = HostDb::create(&paths.office_home, &*custody, &init).unwrap();
    let verifier = hash_password(&Password::new(PASSWORD.into()).unwrap()).unwrap();
    db.seed_account(
        &crate::ids::generate().unwrap(),
        "Owner",
        &verifier,
        true,
        now,
    )
    .unwrap();
    drop(db);
    let socket = layout.host_socket.clone();
    let host =
        Host::start_prepared(config, Some(instance), Some((lock, Arc::clone(&custody)))).unwrap();
    (host, socket)
}

/// A started registered host running copies of the real worker and web images,
/// with `wire` signed in as the seeded administrator `session` over `host.sock`.
/// `temp` is declared last so a whole `Started` drops it after the host; a
/// destructuring caller binds `temp` first for the same reason (bindings drop
/// in reverse order).
pub(super) struct Started {
    pub(super) address: SocketAddr,
    pub(super) program: PathBuf,
    pub(super) host: Host,
    pub(super) socket: PathBuf,
    pub(super) wire: BufReader<UnixStream>,
    pub(super) session: SessionHandle,
    pub(super) temp: tempfile::TempDir,
}

/// Starts [`Started`] from `ORG_TEST_OFFICE_BIN` and `ORG_TEST_WEB_BIN` under a
/// disposable `/private/tmp` root. Uses request IDs 1 and 2.
pub(super) fn start_signed_in() -> Started {
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
    // The host runs the web process as the worker's sibling; the web image serves
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

    let mut config = crate::Config::new(base.join("unused-home"), worker);
    config.dev_key_dir = Some(base.join("keys"));
    config.dev_app_path = Some(std::env::current_exe().unwrap());
    let (host, socket) = start_registered_host(config, &base, &listen);
    let stream = UnixStream::connect(&socket).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    let mut wire = BufReader::new(stream);
    answer::<org_host_control::Hello>(&mut wire, 1, &HelloParams::default()).unwrap();
    let first = answer::<AccountVerify>(
        &mut wire,
        2,
        &AccountVerifyParams {
            login: "Owner".into(),
            password: Password::new(PASSWORD.into()).unwrap(),
        },
    )
    .unwrap();
    assert!(first.admin);
    Started {
        address,
        program,
        host,
        socket,
        wire,
        session: first.session,
        temp,
    }
}

/// Provisions HQ over `wire` as `session`, polling until it completes, and
/// returns the request and its result. `id` is the next request ID.
pub(super) fn provision_hq(
    wire: &mut BufReader<UnixStream>,
    id: &mut u64,
    session: SessionHandle,
) -> (Params, Provisioned) {
    let prepared = Params {
        session,
        operation_id: crate::ids::generate().unwrap(),
        plan: PlanV1 {
            version: 1,
            name: "Example".into(),
            slug: "example".into(),
            repository_remote_url: "https://example.test/organization.git".into(),
            state_branch: "org/state".into(),
            home_team_name: "Core".into(),
        },
    };
    let until = Instant::now() + WAIT;
    loop {
        let result = answer::<OrganizationProvision>(wire, *id, &prepared).unwrap();
        *id += 1;
        match result {
            HostOpResult::Completed { result } => return (prepared, result),
            HostOpResult::Accepted => {
                assert!(Instant::now() < until, "HQ provisioning deadline");
                thread::sleep(Duration::from_millis(50));
            }
            _ => panic!("unexpected provisioning result"),
        }
    }
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_host_spawns_web_process_serves_version_and_restarts_after_eof() {
    // `_temp` is bound first so it drops last, after the host and its children.
    let Started {
        temp: _temp,
        address,
        program,
        host,
        socket: _,
        mut wire,
        session,
    } = start_signed_in();
    // Authenticated but no HQ yet: the web process is not serving.
    assert!(!web_ready(&host));
    // The wired start-eligibility gate holds the supervisor before any spawn or bind:
    // across several supervisor polls no agentic-org-web child runs and the browser
    // port accepts nothing.
    let window = Instant::now() + Duration::from_secs(3);
    while Instant::now() < window {
        let pids = web_pids(&program);
        assert!(
            pids.is_empty(),
            "web process spawned before HQ is Active: {pids:?}"
        );
        assert!(
            TcpStream::connect(address).is_err(),
            "browser port accepted before HQ is Active"
        );
        thread::sleep(Duration::from_millis(50));
    }
    assert!(!web_ready(&host));
    let mut id = 3;
    provision_hq(&mut wire, &mut id, session);

    // 1. Admission after HQ: the supervisor spawns and admits the real web image.
    wait_until("web_ready", || web_ready(&host));
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
    // 2. No browser sign-in yet: even the administrator's correct
    //    credentials reach no host account.verify.
    assert_no_sign_in(address);

    // 3. Control EOF: kill the admitted web process. Readiness turns false; the
    //    supervisor's bounded restart serves a new process.
    let killed = web_pid(&program);
    let status = Command::new("/bin/kill")
        .args(["-KILL", &killed])
        .status()
        .unwrap();
    assert!(status.success());
    wait_until("web_ready false after EOF", || !web_ready(&host));
    wait_until("restarted web_ready", || web_ready(&host));
    wait_until("restarted HTTP readiness", || http_ready(address));
    assert_ne!(web_pid(&program), killed, "a new admitted process");
    assert_eq!(http(address, "GET", "/api/v1/version", "", "").status, 200);
    assert_no_sign_in(address);

    let _ = id;
    drop(wire);
    drop(host);
    // Host shutdown drops the web process first: no web process survives.
    let left = Command::new("/usr/bin/pgrep")
        .arg("-f")
        .arg(&program)
        .output()
        .unwrap();
    assert!(
        left.stdout.is_empty(),
        "web process reaped on host shutdown"
    );
}

/// Ported from the deleted bootstrap release gate: a registered-instance host with
/// neither a pinned runtime release nor explicit development custody refuses before
/// it creates a registry, a custody namespace or any credential.
#[test]
fn registered_host_without_release_or_development_custody_refuses_before_registry_or_credentials() {
    let temp = tempfile::tempdir_in("/private/tmp").unwrap();
    let base = temp.path().canonicalize().unwrap();
    fs::create_dir(base.join("temp")).unwrap();
    fs::set_permissions(base.join("temp"), fs::Permissions::from_mode(0o700)).unwrap();
    let registry = Registry::open(&base.join("locator"), &base.join("temp")).unwrap();
    let instance = registry
        .reserve(Some("fresh"), &base.join("instance"))
        .unwrap();
    let before = tree(&base);
    let config = Config::new(base.join("unused-home"), base.join("agentic-org-office"));
    assert!(Host::start_prepared(config, Some(instance), None).is_err());
    assert_eq!(tree(&base), before, "nothing written after the refusal");
    assert!(!before.iter().any(|p| p.ends_with("host-registry.db")));
}

fn tree(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                stack.push(path.clone());
            }
            out.push(path);
        }
    }
    out.sort();
    out
}
