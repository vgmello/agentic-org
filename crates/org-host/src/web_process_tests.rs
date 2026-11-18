//! F4 production-path proof (GW-01 falsification): the real host, started for a
//! registered instance whose registry holds a seeded administrator, spawns the
//! real built `agentic-org-web`, which serves `/api/v1/version` over loopback
//! HTTP. A protected session route without the browser pair is 401 before any
//! host call; routes not yet built are JSON 404.
//! The administrator is seeded directly (A2 D1: the reverse bootstrap is gone and
//! A3 creates administrators through a one-time link). The web process starts
//! once the instance is registered and authenticated, before any organization.
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
    Password, SessionHandle,
};
use org_proto::{frame, ApiError, Call, HelloParams, Request};
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

mod browser_sessions;
mod organizations;
mod recovery_custody;

const WAIT: Duration = Duration::from_secs(60);
const PASSWORD: &str = "synthetic first password";

struct Reply {
    status: u16,
    headers: String,
    body: serde_json::Value,
}
fn http(address: SocketAddr, method: &str, path: &str, headers: &str, body: &str) -> Reply {
    send(
        address,
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}
/// A sign-in or registration `POST`, tried again while the answer is 503. Under machine
/// load the product may answer `unavailable` (correctly) to a call that needs an Argon2
/// hash; the tests that use this prove what follows a successful call, not its first-try
/// timing. This is a bound on tries (20, 250 ms apart), not a wait for another event; the
/// last reply is returned whatever it is, so a persistent 503 still fails the caller.
fn post_until_available(address: SocketAddr, path: &str, body: &str) -> Reply {
    const TRIES: usize = 20;
    let mut reply = http(address, "POST", path, "", body);
    for _ in 1..TRIES {
        if reply.status != 503 {
            break;
        }
        thread::sleep(Duration::from_millis(250));
        reply = http(address, "POST", path, "", body);
    }
    reply
}
/// Sends `request` exactly as written and reads the whole reply.
fn send(address: SocketAddr, request: &str) -> Reply {
    let mut stream = TcpStream::connect(address).expect("web process listener");
    stream.set_read_timeout(Some(WAIT)).unwrap();
    // One write for head and body: a split write can leave late body bytes unread
    // when the web process answers early, and its close then resets the socket
    // (a known transport limit tracked for A3, not what this test proves).
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

/// What the fixture registry holds before the host first starts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Seed {
    /// One administrator with [`PASSWORD`], as after the browser registration.
    Administrator,
    /// No account; one pending registration link, as `admin.link.initial` leaves it.
    RegistrationLink,
}

/// Opens the fixture instance under `base`, runs `create` (first start only) with its
/// office home and custody, and starts the host the way the registered start does,
/// including the instance name `instance.view` reports. Returns the host and its
/// `host.sock` path. A restart passes a `create` that does nothing.
pub(super) fn host_at(
    mut config: Config,
    base: &Path,
    create: impl FnOnce(
        &crate::instance::registry::Instance,
        &Path,
        &dyn crate::host_secrets::HostSecrets,
    ),
) -> (Host, PathBuf) {
    config.admit_runtime().unwrap();
    let registry = Registry::open(&base.join("locator"), &base.join("temp")).unwrap();
    registry.restore_temp_folders().unwrap();
    let instance = registry.reserve(None, &base.join("instance")).unwrap();
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
    create(&instance, &paths.office_home, &*custody);
    let name = instance.name().to_owned();
    let socket = layout.host_socket.clone();
    let host =
        Host::start_prepared(config, Some(instance), Some((lock, Arc::clone(&custody)))).unwrap();
    assert!(host.service.install_instance_name(&name));
    (host, socket)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// A registered instance as the service host will find it after setup: the instance
/// reserved with its browser address and its registry created under the host and
/// instance locks, holding `seed`. Returns the started host, its `host.sock` path
/// and, for [`Seed::RegistrationLink`], the link token the browser presents.
fn start_registered_host(
    config: Config,
    base: &Path,
    listen: &org_host_control::listen::ListenConfig,
    seed: Seed,
) -> (Host, PathBuf, Option<String>) {
    let mut link = None;
    let (host, socket) = host_at(config, base, |instance, home, custody| {
        instance.set_browser(listen).unwrap();
        let now = now_ms();
        let init = HostDbInit {
            instance_id: instance.id().to_owned(),
            clock_high_ms: now,
        };
        let db = HostDb::create(home, custody, &init).unwrap();
        match seed {
            Seed::Administrator => {
                let verifier = hash_password(&Password::new(PASSWORD.into()).unwrap()).unwrap();
                db.seed_account(
                    &crate::ids::generate().unwrap(),
                    "Owner",
                    &verifier,
                    true,
                    now,
                )
                .unwrap();
            }
            Seed::RegistrationLink => {
                use crate::links::{LinkKind, OneTimeLinks, Subject};
                let issued = db
                    .issue_first(
                        LinkKind::Registration,
                        &Subject::new("administrator").unwrap(),
                        now,
                    )
                    .unwrap()
                    .expect("the first registration link");
                link = Some(issued.token.encode().to_string());
            }
        }
    });
    (host, socket, link)
}

/// A started registered host running copies of the real worker and web images under a
/// disposable `/private/tmp` root, before any native sign-in. `temp` is declared last
/// so the whole value drops it after the host.
pub(super) struct Launched {
    pub(super) address: SocketAddr,
    pub(super) program: PathBuf,
    pub(super) host: Host,
    pub(super) socket: PathBuf,
    pub(super) link: Option<String>,
    pub(super) base: PathBuf,
    pub(super) temp: tempfile::TempDir,
}

/// The host configuration of the fixture under `base` (copies placed by [`launch`]).
pub(super) fn fixture_config(base: &Path) -> Config {
    let mut config = crate::Config::new(
        base.join("unused-home"),
        base.join("bin/agentic-org-office"),
    );
    config.dev_key_dir = Some(base.join("keys"));
    config.dev_app_path = Some(std::env::current_exe().unwrap());
    config
}

/// Starts [`Launched`] from `ORG_TEST_OFFICE_BIN` and `ORG_TEST_WEB_BIN`.
pub(super) fn launch(seed: Seed) -> Launched {
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
    let program = base.join("bin/agentic-org-web");
    fs::copy(office, base.join("bin/agentic-org-office")).unwrap();
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
    let (host, socket, link) = start_registered_host(fixture_config(&base), &base, &listen, seed);
    Launched {
        address,
        program,
        host,
        socket,
        link,
        base,
        temp,
    }
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

/// Starts [`Started`] from [`launch`] with a seeded administrator. Uses request IDs 1 and 2.
pub(super) fn start_signed_in() -> Started {
    let Launched {
        temp,
        address,
        program,
        host,
        socket,
        ..
    } = launch(Seed::Administrator);
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

/// The body of `POST /api/v1/organizations` (web-api-v1 §3): one operation on `instance`.
fn create_body(
    instance: &str,
    operation: &str,
    name: &str,
    slug: &str,
    repository: serde_json::Value,
) -> String {
    serde_json::json!({
        "operation_id": operation,
        "scope": { "kind": "instance", "instance_id": instance },
        "params": {
            "name": name, "slug": slug,
            "organization_repository": repository, "state_branch": "org/state",
        },
    })
    .to_string()
}

/// A remote repository registration for `slug`. Nothing contacts it.
fn remote(slug: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "remote", "remote_url": format!("https://example.test/{slug}.git") })
}

/// `GET /api/v1/operations/host/{operation}` for this instance.
fn lookup(address: SocketAddr, headers: &str, instance: &str, operation: &str) -> Reply {
    http(
        address,
        "GET",
        &format!("/api/v1/operations/host/{operation}?instance_id={instance}"),
        headers,
        "",
    )
}

/// Polls the lookup route until the create completed and returns its organization view.
fn wait_completed(
    address: SocketAddr,
    headers: &str,
    instance: &str,
    operation: &str,
) -> serde_json::Value {
    wait_completed_seen(address, headers, instance, operation, &mut |_| {})
}

/// [`wait_completed`], showing `seen` every poll reply, the ones before completion too.
fn wait_completed_seen(
    address: SocketAddr,
    headers: &str,
    instance: &str,
    operation: &str,
    seen: &mut dyn FnMut(&Reply),
) -> serde_json::Value {
    let until = Instant::now() + WAIT;
    loop {
        let reply = lookup(address, headers, instance, operation);
        seen(&reply);
        assert_eq!(reply.status, 200, "lookup: {}", reply.body);
        let result = &reply.body["result"];
        assert_eq!(result["kind"], "host", "{result}");
        assert_eq!(result["operation_id"], operation, "{result}");
        match result["outcome"].as_str() {
            Some("completed") => {
                assert_eq!(result["state"], "linked", "{result}");
                return result["result"].clone();
            }
            Some("accepted") => assert_eq!(result["state"], "reserved", "{result}"),
            _ => panic!("unexpected create outcome: {result}"),
        }
        assert!(Instant::now() < until, "organization create deadline");
        thread::sleep(Duration::from_millis(50));
    }
}

/// An organization created through the browser API.
pub(super) struct Created {
    pub(super) operation_id: String,
    pub(super) organization_id: String,
    pub(super) office_id: String,
}

/// Creates the organization "Example" the way a browser does: signs in as the seeded
/// administrator over HTTP, posts the create and polls the lookup route until it completes.
pub(super) fn create_hq(host: &Host, address: SocketAddr) -> Created {
    browser_sessions::wait_serving(host, address);
    let signed_in = http(
        address,
        "POST",
        "/api/v1/session/sign-in",
        "",
        &browser_sessions::credentials(),
    );
    let headers = browser_sessions::pair(&signed_in, "sign-in").headers();
    let instance = host.instance.as_ref().unwrap().id().to_owned();
    let operation_id = crate::ids::generate().unwrap();
    let body = create_body(
        &instance,
        &operation_id,
        "Example",
        "example",
        remote("example"),
    );
    let posted = http(address, "POST", "/api/v1/organizations", &headers, &body);
    assert!(
        matches!(posted.status, 200 | 202),
        "create: {} {}",
        posted.status,
        posted.body
    );
    let view = wait_completed(address, &headers, &instance, &operation_id);
    Created {
        operation_id,
        organization_id: view["id"].as_str().expect("organization id").to_owned(),
        office_id: view["offices"][0]["id"].as_str().expect("HQ id").to_owned(),
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
        wire,
        session: _,
    } = start_signed_in();
    // 1. Registered with authentication, no organization or office at all: the
    //    supervisor spawns and admits the real web image (lead ruling, A3 B5).
    assert!(host.offices.offices().is_empty(), "no office");
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
