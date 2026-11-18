//! F4 production-path proof (GW-01 falsification): the real host, started for a
//! registered instance whose registry holds a seeded administrator, spawns the
//! real built `agentic-org-web`, which serves `/api/v1/version` over loopback
//! HTTP. A protected session route without the browser pair is 401 before any
//! host call; routes not yet built are JSON 404.
//! The administrator is seeded directly (A2 D1: the reverse bootstrap is gone and
//! A3 creates administrators through a one-time link). The web process starts
//! once the instance is registered and authenticated, before any organization.
//! Only disposable roots in the system's disk-backed temporary directory and debug
//! development custody are used;
//! debug `ServerCheck::Skip` establishes no release-signing evidence.
//!
//! ADR-0017 LNX-3 on the same real host (the `lnx_3_*` tests): the web process is
//! confined by its seccomp filter, with Landlock under it where the kernel and the
//! container's profile offer it; where the filter itself is refused the host still serves
//! `host.sock` and starts no web process. Each of those tests runs its host in a process
//! of its own, started from this test executable, under a profile as a container runtime's
//! where the scenario has one, and reads that process's standard error as the host log.
//! They need the built `agentic-org-web` and `agentic-org-office` beside this crate's
//! tests, where `cargo test --workspace` puts them before any test runs.
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
use org_host_control::{GatewayState, InstanceStatus, InstanceStatusParams};
use org_platform::confinement::testing::{refuse_call, RefusedCall};
use org_proto::{frame, selected_bootstrap::PlanV1, ApiError, Call, HelloParams, Request};
use std::{
    fs,
    io::{self, BufReader, Read, Write},
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
/// How long the real-host tests keep asking for the HQ whose worker must start within the
/// host's launch deadline ([`create_hq_beside`]).
const HQ_WAIT: Duration = Duration::from_secs(300);
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
/// A sign-in or registration `POST`, tried again while the answer is 503. Cause, as far as it
/// was diagnosed: the hash-permit pool is per host, so tests do not contend for it; a debug
/// build's unoptimized Argon2 took seconds per hash on a loaded machine (one sign-in-bearing
/// unit test ran 9.8 s at load 9), past the web process's 5 s host-call deadline, which
/// answers `unavailable`. The workspace now optimizes the hash crates in debug builds
/// (root `Cargo.toml`), so this should no longer trigger; the bound stays as a guard. The
/// tests that use this prove what follows a successful call, not its first-try timing. This is a bound on tries (20, 250 ms apart), not a wait for another event; the
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

/// Kills the process `pid`, a web process this test's own host started.
fn kill(pid: &str) {
    let pid: libc::pid_t = pid.parse().unwrap();
    // SAFETY: a signal to one process, named by the id `web_pid` just found for the
    // unique program path of this test's own web process.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
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
        None,
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
/// disposable root on disk, before any native sign-in. `temp` is declared last
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

/// The built program `name` of another package: the one `variable` names when it is set,
/// or else the one cargo built beside this crate's tests (`cargo test --workspace` builds
/// them all before any test runs).
fn built_program(variable: &str, name: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::test_programs::sibling_program(name))
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

/// Starts [`Launched`] from the built `agentic-org-office` and `agentic-org-web`
/// (`ORG_TEST_OFFICE_BIN` and `ORG_TEST_WEB_BIN` name others) under a disposable root on
/// disk.
pub(super) fn launch(seed: Seed) -> Launched {
    let office = built_program("ORG_TEST_OFFICE_BIN", "agentic-org-office");
    let web = built_program("ORG_TEST_WEB_BIN", "agentic-org-web");
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
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
    kill(&killed);
    wait_until("web_ready false after EOF", || !web_ready(&host));
    wait_until("restarted web_ready", || web_ready(&host));
    wait_until("restarted HTTP readiness", || http_ready(address));
    assert_ne!(web_pid(&program), killed, "a new admitted process");
    assert_eq!(http(address, "GET", "/api/v1/version", "", "").status, 200);
    assert_no_sign_in(address);

    drop(wire);
    drop(host);
    // Host shutdown drops the web process first: no web process survives.
    assert!(
        web_pids(&program).is_empty(),
        "web process reaped on host shutdown"
    );
}

/// The environment variable that makes a `lnx_3_*` test the host of its own scenario: its
/// value names what that host must come to.
const AS_HOST: &str = "ORG_HOST_WEB_PROCESS_TEST_AS_HOST";
/// How the host log names a start of the web process, and a failed one.
const STARTED: &str = "authenticated browser web process started, ";
const FILTER_ALONE: &str = "confined by the seccomp filter alone: Landlock was not applied: ";
const NOT_PERMITTED: &str = "EPERM (Operation not permitted (os error 1))";

/// What the host process of a `lnx_3_*` test printed and logged.
struct HostRun {
    /// Its observations, one `name: value` line each.
    facts: String,
    /// Its host log: the lines of its standard error that the host wrote.
    log: Vec<String>,
}

impl HostRun {
    fn fact(&self, line: &str) -> bool {
        self.facts.lines().any(|fact| fact == line)
    }

    fn logged(&self, part: &str) -> Vec<&str> {
        self.log
            .iter()
            .map(String::as_str)
            .filter(|line| line.contains(part))
            .collect()
    }
}

/// Runs the test `name` again in a process of its own, as the host that must come to
/// `outcome`, under a profile that refuses `refused` where one is named. `None` where this
/// system's confinement adapter does not confine with calls a profile can refuse: LNX-3 is
/// a requirement of the GNU/Linux host, and there is nothing of it to test elsewhere.
fn as_host(name: &str, outcome: &str, refused: Option<RefusedCall>) -> Option<HostRun> {
    let mut probe = Command::new(std::env::current_exe().unwrap());
    if let Err(error) = refuse_call(&mut probe, RefusedCall::Seccomp) {
        assert_eq!(error.kind(), io::ErrorKind::Unsupported, "{error}");
        return None;
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(AS_HOST, outcome);
    if let Some(call) = refused {
        refuse_call(&mut command, call).unwrap();
    }
    let output = {
        let _one = ONE_HOST
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        command.output().unwrap()
    };
    let facts = String::from_utf8_lossy(&output.stdout).into_owned();
    let errors = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "{refused:?}: {}\n{facts}\n{errors}",
        output.status
    );
    let log: Vec<String> = errors
        .lines()
        .filter_map(|line| line.strip_prefix("agentic-org: "))
        .map(str::to_owned)
        .collect();
    // For whoever reads this test's output: what that host observed, and what it logged
    // of its web process.
    for fact in facts.lines().filter(|line| line.contains(": ")) {
        println!("{refused:?}: {fact}");
    }
    for line in log.iter().filter(|line| line.contains("browser")) {
        println!("{refused:?}: host log: {line}");
    }
    Some(HostRun { facts, log })
}

/// The outcome this process must come to as the host of a `lnx_3_*` test, when it is one.
fn host_outcome() -> Option<String> {
    std::env::var(AS_HOST).ok()
}

/// One line of `/proc/<pid>/status`, as the kernel reports the web process.
fn kernel_status(pid: &str, name: &str) -> String {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    status
        .lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|| panic!("no {name} in the status of {pid}"))
}

/// Creates HQ for a `lnx_3_*` host: through the browser API, as [`create_hq`] does, when
/// the web process serves; otherwise in the host's process with the seeded administrator's
/// account, the call the web connection would make, since no browser can reach a host
/// whose web process never starts. These tests are about the web process, which needs no
/// HQ to start: HQ puts a real worker beside it. The in-process create asks again with the
/// same operation while the host answers that the creation is unresolved: a worker that
/// misses the launch deadline on a busy machine gives that answer, and it says to retry.
fn create_hq_beside(host: &Host, address: SocketAddr, serving: bool) {
    if serving {
        create_hq(host, address);
        return;
    }
    let creating = host
        .creating
        .as_ref()
        .expect("a registered host creates organizations");
    let account = creating.account_for_test("Owner");
    let operation_id = crate::ids::generate().unwrap();
    let plan = PlanV1 {
        version: 1,
        name: "Example".into(),
        slug: "example".into(),
        repository_remote_url: "https://example.test/organization.git".into(),
        state_branch: "org/state".into(),
    };
    let fingerprint: [u8; 32] =
        <sha2::Sha256 as sha2::Digest>::digest(serde_json::to_vec(&plan).unwrap()).into();
    // Its own, long bound: each retry needs only one launch that fits the host's deadline,
    // and on a machine busy with other test binaries several in a row can miss it.
    let until = Instant::now() + HQ_WAIT;
    loop {
        match creating.create(&account, &operation_id, &plan, fingerprint) {
            Ok(org_host_control::HostOpResult::Completed { .. }) => return,
            Ok(org_host_control::HostOpResult::Accepted) => {}
            Err(error) if error.code == org_proto::ErrorCode::Unavailable => {
                println!("HQ: {}", error.message);
            }
            Err(error) => panic!("unexpected create error: {error:?}"),
            Ok(org_host_control::HostOpResult::Rejected { refusal }) => {
                panic!("the create was refused: {refusal:?}")
            }
            Ok(_) => panic!("an unknown create result"),
        }
        assert!(Instant::now() < until, "HQ creation deadline");
        thread::sleep(Duration::from_millis(50));
    }
}

/// One real host at a time: each is a whole host with a real worker and a real web
/// process, and a worker that competes with others for the processor can miss the host's
/// launch deadline.
static ONE_HOST: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The host process of a `lnx_3_*` test: a real registered host with the real worker and
/// web images, signed in over `host.sock`. Its web process is eligible to start from the
/// host's start, before any organization; HQ is provisioned once the web supervisor has
/// come to its first settled state. It prints what it observes and must come to `outcome`:
///
/// - `serves`: the web process serves and the kernel shows it confined;
/// - `serves again`: the same, then the web process is killed and a new one serves;
/// - `starts nothing`: the web state is failed from the host's start, and stays so;
/// - `gives up`: the web state is failed for good after the supervisor's failed starts.
fn lnx_3_host(outcome: &str) {
    // `_temp` is bound first so it drops last, after the host and its children.
    let Started {
        temp: _temp,
        address,
        program,
        host,
        socket: _,
        mut wire,
        session: _,
    } = start_signed_in();
    println!("\nhost.sock: hello and account.verify answered");
    let mut id = 3;
    let state = |wire: &mut BufReader<UnixStream>, id: &mut u64| {
        *id += 1;
        answer::<InstanceStatus>(wire, *id - 1, &InstanceStatusParams::default())
            .expect("instance.status is answered")
            .gateway
            .state
    };
    // The supervisor's own first settled state, before any organization exists: the web
    // process serves, or its start was refused or failed.
    let mut before = GatewayState::NotConfigured;
    wait_until("the web supervisor's first settled state", || {
        before = state(&mut wire, &mut id);
        !matches!(before, GatewayState::NotConfigured | GatewayState::Starting)
    });
    println!("state before HQ: {before:?}");
    create_hq_beside(&host, address, matches!(before, GatewayState::Running));
    println!("HQ: provisioned");
    let serving = |host: &Host| {
        wait_until("web_ready", || web_ready(host));
        wait_until("HTTP readiness", || http_ready(address));
        let version = http(address, "GET", "/api/v1/version", "", "");
        let pid = web_pid(&program);
        println!(
            "web process: version {} NoNewPrivs {} Seccomp {}",
            version.status,
            kernel_status(&pid, "NoNewPrivs"),
            kernel_status(&pid, "Seccomp")
        );
        pid
    };
    // No web process and nothing on the browser port, watched over several supervisor polls.
    let nothing_runs = || {
        let window = Instant::now() + Duration::from_secs(2);
        while Instant::now() < window {
            let pids = web_pids(&program);
            assert!(pids.is_empty(), "a web process runs: {pids:?}");
            assert!(
                TcpStream::connect(address).is_err(),
                "the browser port accepts"
            );
            thread::sleep(Duration::from_millis(50));
        }
        println!("web processes: none; browser port: unbound");
    };
    match outcome {
        "serves" => {
            serving(&host);
        }
        "serves again" => {
            let killed = serving(&host);
            kill(&killed);
            wait_until("web_ready false after the kill", || !web_ready(&host));
            assert_ne!(serving(&host), killed, "a new web process");
        }
        "starts nothing" => {
            assert!(!web_ready(&host));
            nothing_runs();
        }
        "gives up" => {
            // The longest wait between two starts is 8 seconds, and the state is `failed`
            // through it. Failed for longer than that and a start: the supervisor gave up.
            let mut failed_since = None;
            wait_until("the supervisor gives up", || {
                let now = Instant::now();
                match state(&mut wire, &mut id) {
                    GatewayState::Failed => {
                        now.duration_since(*failed_since.get_or_insert(now))
                            > Duration::from_secs(11)
                    }
                    _ => {
                        failed_since = None;
                        false
                    }
                }
            });
            assert!(!web_ready(&host));
            nothing_runs();
        }
        other => panic!("an unknown outcome: {other}"),
    }
    // `host.sock` still answers, whatever became of the web process.
    println!("state: {:?}", state(&mut wire, &mut id));
    drop(wire);
    drop(host);
    assert!(
        web_pids(&program).is_empty(),
        "a web process outlived the host"
    );
}

/// LNX-3, the ordinary case: a real host starts the real web process, which serves, the
/// kernel shows it under its seccomp filter, and the host log says what it is confined by.
/// A restart goes through the admission the host built at its start: the image is read
/// and its digest logged once in the host's life.
#[test]
fn lnx_3_a_real_host_starts_the_real_web_process_confined_and_logs_by_what() {
    const NAME: &str = "web_process_tests::lnx_3_a_real_host_starts_the_real_web_process_confined_and_logs_by_what";
    if let Some(outcome) = host_outcome() {
        return lnx_3_host(&outcome);
    }
    let Some(run) = as_host(NAME, "serves again", None) else {
        return;
    };
    let confined = "web process: version 200 NoNewPrivs 1 Seccomp 2";
    assert_eq!(
        run.facts.lines().filter(|line| *line == confined).count(),
        2,
        "{}",
        run.facts
    );
    assert!(run.fact("state before HQ: Running"), "{}", run.facts);
    assert!(run.fact("state: Running"), "{}", run.facts);
    // Each of the two starts is logged once, with the layers the host verified: the filter,
    // and Landlock where this kernel has it (or why it has none).
    let started = run.logged(STARTED);
    assert_eq!(started.len(), 2, "{:?}", run.log);
    for line in &started {
        let by = line.strip_prefix(STARTED).unwrap();
        assert!(
            by.starts_with("confined by the seccomp filter and Landlock ABI ")
                || by.starts_with(FILTER_ALONE),
            "{line}"
        );
    }
    assert_eq!(started[0], started[1]);
    assert_eq!(
        run.logged("because the running web process stopped: the child exited (signal 9)")
            .len(),
        1,
        "{:?}",
        run.log
    );
    // One admission for both starts: the web image was read, and its digest logged, once.
    let read = run.logged("agentic-org-web; every server this host starts runs the bytes");
    assert_eq!(read.len(), 1, "{:?}", run.log);
}

/// LNX-3 where a container's profile refuses Landlock, at its version query, at the
/// creation of the ruleset or at the restriction: the real host's web process serves all
/// the same, the kernel shows it under its filter, and the host log says that it is
/// confined by the filter alone and why.
#[test]
fn lnx_3_under_a_profile_that_refuses_landlock_the_real_web_process_serves_by_the_filter_alone() {
    const NAME: &str = "web_process_tests::lnx_3_under_a_profile_that_refuses_landlock_the_real_web_process_serves_by_the_filter_alone";
    if let Some(outcome) = host_outcome() {
        return lnx_3_host(&outcome);
    }
    let profiles = [
        (
            RefusedCall::LandlockCreateRuleset,
            "landlock_create_ruleset failed with EPERM: the Landlock version query was refused"
                .to_owned(),
        ),
        (
            RefusedCall::LandlockRuleset,
            format!("landlock_create_ruleset failed with {NOT_PERMITTED}"),
        ),
        (
            RefusedCall::LandlockRestrictSelf,
            format!("landlock_restrict_self failed with {NOT_PERMITTED}"),
        ),
    ];
    for (call, reason) in &profiles {
        let Some(run) = as_host(NAME, "serves", Some(*call)) else {
            return;
        };
        assert!(
            run.fact("web process: version 200 NoNewPrivs 1 Seccomp 2"),
            "{call:?}: {}",
            run.facts
        );
        assert!(run.fact("state: Running"), "{call:?}: {}", run.facts);
        let started = run.logged(STARTED);
        let [line] = started[..] else {
            panic!("{call:?}: one start is logged: {:?}", run.log)
        };
        let given = line
            .strip_prefix(STARTED)
            .and_then(|by| by.strip_prefix(FILTER_ALONE))
            .unwrap_or_else(|| panic!("{call:?}: {line}"));
        // A kernel without Landlock gives its own reason before a profile can.
        assert!(
            given == reason
                || given.contains("this kernel has no Landlock")
                || given.contains("is not an enabled security module"),
            "{call:?}: {line}"
        );
    }
}

/// LNX-3 where the profile refuses `seccomp`, the layer that must hold: the real host
/// starts and serves `host.sock`, its web state is `failed` from the start and stays so
/// once HQ is there, its log says which call failed with which error, no web process ever
/// exists and nothing listens on the browser port.
#[test]
fn lnx_3_under_a_profile_that_refuses_seccomp_the_real_host_serves_host_sock_and_no_web_process() {
    const NAME: &str = "web_process_tests::lnx_3_under_a_profile_that_refuses_seccomp_the_real_host_serves_host_sock_and_no_web_process";
    if let Some(outcome) = host_outcome() {
        return lnx_3_host(&outcome);
    }
    let Some(run) = as_host(NAME, "starts nothing", Some(RefusedCall::Seccomp)) else {
        return;
    };
    for fact in [
        "host.sock: hello and account.verify answered",
        "state before HQ: Failed",
        "HQ: provisioned",
        "web processes: none; browser port: unbound",
        "state: Failed",
    ] {
        assert!(run.fact(fact), "{fact}: {}", run.facts);
    }
    assert_eq!(
        run.logged("browser service not started: "),
        [
            "browser service not started: the web process requires process confinement, which \
          is unavailable here: gateway confinement unavailable: seccomp failed with EPERM: \
          this kernel or its profile has no seccomp filter that kills the process"
        ],
        "{:?}",
        run.log
    );
    assert!(run.logged(STARTED).is_empty(), "{:?}", run.log);
    assert!(
        run.logged("authenticated browser").is_empty(),
        "{:?}",
        run.log
    );
}

/// LNX-3 where the host's question passes and the installation of the filter then fails in
/// the web process: each of the supervisor's five starts is logged with the call and the
/// error the web process named, the web state ends as `failed`, no web process is left and
/// `host.sock` still answers.
#[test]
fn lnx_3_a_filter_that_cannot_be_installed_is_logged_five_times_and_the_real_host_keeps_serving() {
    const NAME: &str = "web_process_tests::lnx_3_a_filter_that_cannot_be_installed_is_logged_five_times_and_the_real_host_keeps_serving";
    const REASON: &str = "it reported that it cannot confine itself: gateway confinement \
                          unavailable: seccomp failed with ENOMEM (Cannot allocate memory (os \
                          error 12))";
    if let Some(outcome) = host_outcome() {
        return lnx_3_host(&outcome);
    }
    let Some(run) = as_host(NAME, "gives up", Some(RefusedCall::SeccompInstall)) else {
        return;
    };
    for fact in [
        "host.sock: hello and account.verify answered",
        "state before HQ: Failed",
        "web processes: none; browser port: unbound",
        "state: Failed",
    ] {
        assert!(run.fact(fact), "{fact}: {}", run.facts);
    }
    assert!(run.logged(STARTED).is_empty(), "{:?}", run.log);
    // Four restarts, each after a longer wait, then the limit: five failed starts, every
    // one with the web process's own reason.
    let restarts = run.logged("authenticated browser restarting in ");
    assert_eq!(restarts.len(), 4, "{:?}", run.log);
    for (line, wait) in restarts.iter().zip(["1s", "2s", "4s", "8s"]) {
        assert!(
            line.starts_with(&format!(
                "authenticated browser restarting in {wait} because web process start failed: "
            )),
            "{line}"
        );
        assert!(line.ends_with(REASON), "{line}");
    }
    let limit = run.logged("authenticated browser restart limit reached after 5 failed starts");
    let [last] = limit[..] else {
        panic!("the limit is logged once: {:?}", run.log)
    };
    assert!(last.ends_with(REASON), "{last}");
}

/// Ported from the deleted bootstrap release gate: a registered-instance host with
/// neither a pinned runtime release nor explicit development custody refuses before
/// it creates a registry, a custody namespace or any credential.
#[test]
fn registered_host_without_release_or_development_custody_refuses_before_registry_or_credentials() {
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
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
