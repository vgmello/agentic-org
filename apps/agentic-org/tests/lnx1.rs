//! ADR-0017 LNX-1: the real host composition admits a real office worker under the host's
//! writer custody, the worker serves its local socket, and the host stops cleanly on `SIGTERM`
//! and takes its worker with it. The same test runs on every system that has an admission
//! adapter.
//!
//! Everything here is a real executable built by cargo: `agentic-org serve --name default`
//! on a registered instance (setup's steps through A2's fixture), the real web process
//! `agentic-org-web` beside the real worker `agentic-org-office`. The office is made the way
//! a browser makes it: the administrator the fixture seeded signs in over HTTP and posts
//! `POST /api/v1/organizations`; the host reserves the office's writer custody and has the
//! worker create the HQ. The host admits each process through the platform's admission
//! (`org_platform_host::admission`); nothing in this file names a system.
//!
//! The office is then reached on its own socket by a client that needs no agent: an unbound
//! `hello` through `org_client`, answered with the office's identity. `agent-os` comes back
//! to LNX-1 once an agent is placed in an office and `agent-os` addresses a selected office
//! (implementation plan, B2, A5 and B6).
//!
//! The worker and web programs belong to other packages, so cargo gives this test no path for
//! them: they are taken from the folder cargo built `agentic-org` into. `cargo test
//! --workspace` (what CI runs) builds them all; a narrower run needs `cargo build -p
//! agentic-org-office -p agentic-org-web` first, and a missing program fails the test saying
//! so.

// The host keeps its keys in a development key folder, which only debug builds have.
#![cfg(debug_assertions)]

mod support;

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use org_host::setup_flow::fixture::{
    registered_account, seed_administrator, RegisteredAccount, SeededAdministrator,
};
use serde_json::Value;
use support::{eventually, sibling_program};

const WAIT: Duration = Duration::from_secs(30);

/// A host process that is killed and reaped when dropped.
struct HostProcess(Child);

impl Drop for HostProcess {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// A registered instance under a short temporary base with one administrator, and this
/// user's own copies of the worker and web programs side by side (the host runs the web
/// process as the worker's sibling, which serves the UI from `ui/` beside it).
struct World {
    dir: tempfile::TempDir,
    account: RegisteredAccount,
    admin: SeededAdministrator,
    port: u16,
    worker: PathBuf,
}

impl World {
    fn new() -> Self {
        let dir = support::short_tempdir("ol");
        let root = dir.path().canonicalize().unwrap();
        for name in ["a", "k", "u", "b"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(name))
                .unwrap();
        }
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let account = registered_account(&root.join("a"), None, port).unwrap();
        let admin = seed_administrator(&root.join("a"), None, &root.join("k")).unwrap();
        let bin = root.join("b");
        for program in ["agentic-org-office", "agentic-org-web"] {
            // A copy only this user can write: the admission starts no image anyone else
            // could have written.
            fs::copy(sibling_program(program), bin.join(program)).unwrap();
            fs::set_permissions(bin.join(program), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::DirBuilder::new()
            .mode(0o700)
            .create(bin.join("ui"))
            .unwrap();
        // The web process refuses an entry without its inline bootstrap, as for a real build.
        fs::write(
            bin.join("ui/index.html"),
            b"<html><body>LNX-1<script>/* bootstrap */</script></body></html>",
        )
        .unwrap();
        Self {
            dir,
            account,
            admin,
            port,
            worker: bin.join("agentic-org-office"),
        }
    }

    fn base(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap()
    }

    /// Starts `agentic-org serve --name default` with the worker `program`, with only the
    /// environment the account's service gives it plus the development base and key folder.
    fn spawn_host(&self, program: &Path) -> HostProcess {
        let stderr = File::create(self.base().join("host.err")).unwrap();
        HostProcess(
            Command::new(env!("CARGO_BIN_EXE_agentic-org"))
                .args(["serve", "--name", "default", "--server-bin"])
                .arg(program)
                .env_clear()
                .env("HOME", self.base().join("u"))
                .env("ORG_DEV_ACCOUNT_BASE", self.base().join("a"))
                .env("ORG_OFFICE_DEV_KEY_DIR", self.base().join("k"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(stderr)
                .spawn()
                .unwrap(),
        )
    }

    fn host_log(&self) -> String {
        fs::read_to_string(self.base().join("host.err")).unwrap_or_default()
    }

    fn office_socket(&self, office: &str) -> PathBuf {
        org_proto::location::selected_office_socket_in(&self.account.runtime, office).unwrap()
    }
}

/// One HTTP reply.
struct Reply {
    status: u16,
    headers: String,
    body: Value,
}

/// One request to the web process at the instance's origin, as the UI makes it.
fn http(port: u16, method: &str, path: &str, headers: &str, body: &str) -> Option<Reply> {
    let address = format!("127.0.0.1:{port}");
    let mut stream = TcpStream::connect(&address).ok()?;
    stream.set_read_timeout(Some(WAIT)).ok()?;
    // One write for head and body (the web process may answer before reading a late body).
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\n\
         Sec-Fetch-Site: same-origin\r\nSec-Fetch-Mode: cors\r\nSec-Fetch-Dest: empty\r\n\
         Accept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         {headers}Connection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let (head, body) = response.split_once("\r\n\r\n")?;
    let status = head.split(' ').nth(1)?.parse().ok()?;
    Some(Reply {
        status,
        headers: head.into(),
        body: serde_json::from_str(body).unwrap_or(Value::Null),
    })
}

/// A canonical lowercase UUIDv7 made now (the operation IDs the host accepts).
fn uuid_v7_now() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let hi = (ms << 16) | (0x7 << 12) | 0x0123;
    let lo = (0b10u64 << 62) | 0x1234_5678;
    let hex = format!("{hi:016x}{lo:016x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// The session a sign-in reply carries: its cookie and the proof, as request headers.
fn session_headers(reply: &Reply) -> String {
    let cookie = reply
        .headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("set-cookie")
                .then(|| value.trim().split(';').next().unwrap().to_owned())
        })
        .expect("a session cookie");
    let proof = reply.body["result"]["proof"].as_str().expect("a proof");
    format!("Cookie: {cookie}\r\nX-Org-Session-Proof: {proof}\r\n")
}

/// LNX-1, as far as B1b leaves it. The real host admits the real web process and the real
/// worker; the administrator creates the organization through the browser API; the host
/// starts its HQ under the writer custody it reserved; the worker serves its local socket as
/// this office (an unbound `hello` answers its identity). Then the host stops cleanly on
/// `SIGTERM` and takes its worker with it.
#[test]
fn the_real_host_starts_the_real_worker_under_custody_and_its_socket_answers() {
    if !support::may_run_the_service(
        "the_real_host_starts_the_real_worker_under_custody_and_its_socket_answers",
    ) {
        return;
    }
    let world = World::new();
    let mut host = world.spawn_host(&world.worker);
    let alive = |host: &mut HostProcess| {
        assert!(
            host.0.try_wait().unwrap().is_none(),
            "serve exited early: {}",
            world.host_log()
        );
    };
    eventually(WAIT, "the web process to serve", || {
        alive(&mut host);
        http(world.port, "GET", "/api/v1/version", "", "").is_some_and(|r| r.status == 200)
    });

    // The administrator signs in, as the UI does (a 503 while the host hashes is asked again).
    let credentials =
        serde_json::json!({ "login": world.admin.login, "password": world.admin.password })
            .to_string();
    let mut signed_in = None;
    for _ in 0..40 {
        let reply = http(
            world.port,
            "POST",
            "/api/v1/session/sign-in",
            "",
            &credentials,
        );
        if reply.as_ref().is_some_and(|r| r.status != 503) {
            signed_in = reply;
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let signed_in = signed_in.expect("the sign-in was answered");
    assert_eq!(signed_in.status, 200, "sign-in: {}", signed_in.body);
    assert_eq!(signed_in.body["result"]["admin"], true);
    let session = session_headers(&signed_in);

    // The organization, created through the browser API.
    let instance = &world.account.instance_id;
    let operation = uuid_v7_now();
    let create = serde_json::json!({
        "operation_id": operation,
        "scope": { "kind": "instance", "instance_id": instance },
        "params": {
            "name": "Acme", "slug": "acme", "state_branch": "org/state",
            "organization_repository": {
                "kind": "remote", "remote_url": "https://example.test/acme.git"
            },
        },
    })
    .to_string();
    let posted = http(
        world.port,
        "POST",
        "/api/v1/organizations",
        &session,
        &create,
    )
    .unwrap();
    assert!(
        matches!(posted.status, 200 | 202),
        "create: {} {}",
        posted.status,
        posted.body
    );
    let lookup = format!("/api/v1/operations/host/{operation}?instance_id={instance}");
    let deadline = Instant::now() + Duration::from_secs(60);
    let created = loop {
        alive(&mut host);
        let reply = http(world.port, "GET", &lookup, &session, "").unwrap();
        assert_eq!(reply.status, 200, "lookup: {}", reply.body);
        let result = &reply.body["result"];
        match result["outcome"].as_str() {
            Some("completed") => break result["result"].clone(),
            Some("accepted") => {}
            _ => panic!("the create failed: {result}\n{}", world.host_log()),
        }
        assert!(Instant::now() < deadline, "the create did not complete");
        std::thread::sleep(Duration::from_millis(100));
    };
    let organization = created["id"].as_str().expect("the organization").to_owned();
    let office = created["offices"][0]["id"]
        .as_str()
        .expect("its HQ")
        .to_owned();

    // The create completed with its HQ active: the host started it under the writer custody
    // it reserved for it.
    assert_eq!(created["offices"][0]["status"], "active", "{created}");
    let log = world.host_log();
    assert!(!log.contains("couldn't start the server"), "{log}");
    assert!(!log.contains("refused office server"), "{log}");

    // The worker serves its local socket as this office: an unbound `hello`, as a client
    // makes it, answers the office's identity.
    let socket = world.office_socket(&office);
    let mut identity = None;
    eventually(WAIT, "the office to answer hello on its socket", || {
        alive(&mut host);
        identity = org_client::Client::connect(&socket)
            .and_then(|client| client.hello())
            .ok()
            .and_then(|hello| hello.office);
        identity.is_some()
    });
    let identity = identity.unwrap();
    assert_eq!(identity.office.id, office);
    assert_eq!(identity.office.organization_id, organization);
    assert_eq!(identity.organization_name, "Acme");

    // B6 / LNX-1: exercise real messaging through the browser API over the admitted host
    // channel and verify `agent-os` enforces its run-binding contract against the live socket
    // with no file-queue fallback.
    let channels_reply = http(
        world.port,
        "GET",
        &format!("/api/v1/organizations/{organization}/chat/channels"),
        &session,
        "",
    )
    .unwrap();
    assert_eq!(
        channels_reply.status, 200,
        "channels: {}",
        channels_reply.body
    );
    assert!(
        channels_reply.body["result"]["channels"]
            .as_array()
            .is_some_and(|list| list.iter().any(|c| c["name"] == "general")),
        "channels: {}",
        channels_reply.body
    );

    let msg_id = uuid_v7_now();
    let send_body = serde_json::json!({
        "id": msg_id,
        "to": "#general",
        "kind": "msg",
        "subject": "LNX-1",
        "body": "Hello from LNX-1 real office"
    })
    .to_string();
    let send_reply = http(
        world.port,
        "POST",
        &format!("/api/v1/organizations/{organization}/chat/messages"),
        &session,
        &send_body,
    )
    .unwrap();
    assert_eq!(send_reply.status, 200, "send: {}", send_reply.body);
    assert_eq!(send_reply.body["result"]["id"], msg_id);

    let list_body = serde_json::json!({ "conversation": "#general" }).to_string();
    let list_reply = http(
        world.port,
        "POST",
        &format!("/api/v1/organizations/{organization}/chat/messages/list"),
        &session,
        &list_body,
    )
    .unwrap();
    assert_eq!(list_reply.status, 200, "list: {}", list_reply.body);
    assert!(
        list_reply.body["result"]["messages"]
            .as_array()
            .is_some_and(|list| list
                .iter()
                .any(|m| m["id"] == msg_id && m["body"] == "Hello from LNX-1 real office")),
        "list: {}",
        list_reply.body
    );

    let inbox_reply = http(
        world.port,
        "GET",
        &format!("/api/v1/organizations/{organization}/inbox"),
        &session,
        "",
    )
    .unwrap();
    assert_eq!(inbox_reply.status, 200, "inbox: {}", inbox_reply.body);
    assert_eq!(
        inbox_reply.body["result"]["degraded"],
        serde_json::json!([])
    );

    // Real `agent-os` binary refuses unbound runs against the live office (no file queue).
    let agent_os = sibling_program("agent-os");
    let unbound = Command::new(&agent_os)
        .args(["msg", "inbox"])
        .env_clear()
        .env("AGENT_OS_OFFICE", &office)
        .env("AGENT_OS_RUNTIME_DIR", &world.account.runtime)
        .output()
        .unwrap();
    assert!(!unbound.status.success(), "unbound agent-os must refuse");
    let unbound_json: Value = serde_json::from_slice(&unbound.stdout).unwrap();
    assert_eq!(unbound_json["ok"], false);
    assert_eq!(unbound_json["error"]["code"], "not_authorized");

    // The service stops on SIGTERM, exits 0 and leaves no worker serving the socket.
    // SAFETY: sends a signal to this test's own child.
    assert_eq!(
        unsafe { libc::kill(host.0.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    let stopped = Instant::now();
    let status = loop {
        if let Some(status) = host.0.try_wait().unwrap() {
            break status;
        }
        assert!(stopped.elapsed() < WAIT, "the host didn't stop");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{status}: {}", world.host_log());
    eventually(WAIT, "the worker's socket to stop answering", || {
        UnixStream::connect(&socket).is_err()
    });
}
