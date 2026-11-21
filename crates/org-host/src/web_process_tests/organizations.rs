//! A4 production-path proof: a signed-in browser creates organizations through the real
//! host and the real built `agentic-org-web` over loopback TCP, and the real built
//! `agentic-org-office` writes them (web-api-v1 §3; create-organization.md).
//!
//! It proves: a link registers the administrator, who creates an organization and polls the
//! lookup route to `completed`; the organization's files are on disk with UUIDv7 ids and the
//! creator as the top-level team's lead and the session's person; a lost response is
//! answered by the same operation id and a changed payload is `operation_reused`; the
//! organization, its files and the account's link survive a host restart; a host that stops
//! in the middle of a create finishes it at its next start with no client asking; one
//! listener serves two organizations and names a duplicate; the recovery code is released
//! once through the native path and appears in no HTTP response and no file; no credential
//! sits in plaintext outside the owner-only custody folder (ENC-2); and a refused request
//! reaches no host organization call.
//!
//! Every root is a disposable `/private/tmp` folder with debug development custody. Each
//! fixture binds its own free loopback port, so no fixed port base is shared with the
//! real-instance browser tests.
use super::{
    browser_sessions::{credentials, established, pair, wait_serving, Pair},
    create_body, fixture_config, host_at, http, launch, lookup, post_until_available, remote, send,
    wait_until, web_pids, Launched, Reply, Seed, PASSWORD,
};
use crate::{
    enc1_process_tests::{contains, encodings, files, unhex},
    recovery::TestPresence,
    service::listener::Connection,
    Host,
};
use org_host_control::{ops, recovery as dto};
use org_proto::{ErrorCode, OfficeStatus};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    fs,
    io::BufReader,
    net::SocketAddr,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    sync::Arc,
};

/// A signed-in browser. Every reply it reads is kept, so a test can prove what never
/// crossed HTTP.
struct Browser {
    address: SocketAddr,
    pair: Pair,
    instance: String,
    seen: RefCell<Vec<String>>,
}
impl Browser {
    fn new(host: &Host, address: SocketAddr, pair: Pair) -> Self {
        Self {
            address,
            pair,
            instance: host.instance.as_ref().unwrap().id().to_owned(),
            seen: RefCell::new(Vec::new()),
        }
    }
    /// Signs in as the seeded administrator.
    fn sign_in(host: &Host, address: SocketAddr) -> Self {
        let reply = post_until_available(address, "/api/v1/session/sign-in", &credentials());
        let browser = Self::new(host, address, pair(&reply, "sign-in"));
        browser
            .keep(reply)
            .body
            .get("result")
            .expect("a signed-in result");
        browser
    }
    fn keep(&self, reply: Reply) -> Reply {
        self.seen
            .borrow_mut()
            .push(format!("{}\n{}", reply.headers, reply.body));
        reply
    }
    fn get(&self, path: &str) -> Reply {
        self.keep(http(self.address, "GET", path, &self.pair.headers(), ""))
    }
    fn post(&self, path: &str, body: &str) -> Reply {
        self.keep(http(self.address, "POST", path, &self.pair.headers(), body))
    }
    fn body(&self, operation: &str, name: &str, slug: &str) -> String {
        create_body(&self.instance, operation, name, slug, remote(slug))
    }
    /// Posts a create and follows it on the lookup route to its organization view.
    fn create(&self, name: &str, slug: &str) -> (String, Value) {
        let operation = crate::ids::uuid_v7().unwrap();
        let posted = self.post("/api/v1/organizations", &self.body(&operation, name, slug));
        assert!(
            matches!(
                (posted.status, posted.body["outcome"].as_str()),
                (202, Some("accepted")) | (200, Some("completed"))
            ),
            "create {name}: {} {}",
            posted.status,
            posted.body
        );
        assert_eq!(posted.body["operation_id"], operation.as_str());
        let view = self.completed(&operation);
        (operation, view)
    }
    fn completed(&self, operation: &str) -> Value {
        // Every poll reply is kept, the ones before completion too.
        super::wait_completed_seen(
            self.address,
            &self.pair.headers(),
            &self.instance,
            operation,
            &mut |reply| {
                self.seen
                    .borrow_mut()
                    .push(format!("{}\n{}", reply.headers, reply.body));
            },
        )
    }
    /// `GET /api/v1/organizations`: the account's organizations and its creates in flight.
    fn list(&self) -> Value {
        let reply = self.get("/api/v1/organizations");
        assert_eq!(reply.status, 200, "{}", reply.body);
        reply.body["result"].clone()
    }
    fn session(&self) -> Value {
        let reply = self.get("/api/v1/session");
        assert_eq!(reply.status, 200, "{}", reply.body);
        reply.body["result"].clone()
    }
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_else(|| panic!("text: {value}"))
}

/// Canonical UUIDv7 text: lowercase hex in 8-4-4-4-12 with version 7 and the RFC variant.
fn is_uuid_v7(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(b),
        })
        && bytes[14] == b'7'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
}

/// The view of a completed create: its name, UUIDv7 ids, one HQ office, the creator's
/// person and the recovery state, never the code. Returns (organization, office, creator).
fn checked(view: &Value, name: &str, slug: &str) -> (String, String, String) {
    assert_eq!(view["name"], name, "{view}");
    assert_eq!(view["slug"], slug, "{view}");
    let (id, creator) = (text(&view["id"]), text(&view["employee_id"]));
    assert!(is_uuid_v7(id), "organization id {id}");
    assert!(is_uuid_v7(creator), "creator id {creator}");
    let offices = view["offices"].as_array().expect("offices");
    assert_eq!(offices.len(), 1, "{view}");
    let office = text(&offices[0]["id"]);
    assert!(is_uuid_v7(office), "office id {office}");
    assert_eq!(offices[0]["role"], "hq", "{view}");
    assert_eq!(offices[0]["organization_id"], id, "{view}");
    assert!(offices[0].get("endpoint").is_none(), "{view}");
    assert_eq!(view["recovery"], "pending", "{view}");
    (id.to_owned(), office.to_owned(), creator.to_owned())
}

fn all_files(root: &Path) -> Vec<PathBuf> {
    let mut all = Vec::new();
    files(root, &mut all);
    all.sort();
    all
}

/// The organization's files as the office wrote them: path under its state folder → bytes.
/// The office on its store publishes them at `<data root>/<organization>/durable/state/`.
fn state_files(base: &Path, organization: &str) -> (PathBuf, Vec<(PathBuf, Vec<u8>)>) {
    let root = all_files(base)
        .into_iter()
        .find(|p| {
            p.file_name().unwrap() == "organization.yaml"
                && p.ends_with(format!("{organization}/durable/state/organization.yaml"))
        })
        .unwrap_or_else(|| panic!("no organization.yaml for {organization}"))
        .parent()
        .unwrap()
        .to_path_buf();
    let found = all_files(&root)
        .into_iter()
        .filter(|p| !p.strip_prefix(&root).unwrap().starts_with(".git"))
        .map(|p| {
            let bytes = fs::read(&p).unwrap();
            (p.strip_prefix(&root).unwrap().to_path_buf(), bytes)
        })
        .collect();
    (root, found)
}

/// The files the create contract names, with the creator as the top-level team's lead.
fn assert_organization_files(base: &Path, organization: &str, creator: &str, slug: &str) {
    let (root, found) = state_files(base, organization);
    assert!(
        root.ends_with(format!("{organization}/durable/state")),
        "{root:?}"
    );
    let read = |path: &Path| {
        let (_, bytes) = found
            .iter()
            .find(|(p, _)| p == path)
            .unwrap_or_else(|| panic!("{path:?} missing among {:?}", names(&found)));
        String::from_utf8(bytes.clone()).unwrap()
    };
    assert!(read(Path::new("organization.yaml")).contains("Example"));
    let repositories = read(Path::new("repositories.yaml"));
    assert!(
        repositories
            .lines()
            .any(|line| line.contains(r#""kind":"remote""#)
                || line.trim_start_matches(['-', ' ']) == "kind: remote"),
        "{repositories}"
    );
    assert!(
        repositories.contains(&format!("https://example.test/{slug}.git")),
        "{repositories}"
    );
    let teams: Vec<_> = found
        .iter()
        .filter(|(p, _)| p.starts_with("teams") && p.file_name().unwrap() == "team.yaml")
        .map(|(p, _)| p.parent().unwrap().to_path_buf())
        .collect();
    assert_eq!(teams.len(), 1, "one top-level team: {:?}", names(&found));
    let team_id = teams[0].file_name().unwrap().to_str().unwrap();
    assert!(is_uuid_v7(team_id), "team id {team_id}");
    let team = read(&teams[0].join("team.yaml"));
    assert!(
        team.contains("Example"),
        "named after the organization: {team}"
    );
    assert!(
        has_field(&team, "lead_employee_id", creator),
        "the creator leads it: {team}"
    );
    let person = read(&teams[0].join(format!("people/{creator}/profile.md")));
    assert!(person.contains(creator), "{person}");
    let agents: Vec<_> = found
        .iter()
        .filter(|(p, _)| p.starts_with(teams[0].join("agents")))
        .map(|(p, _)| p.clone())
        .collect();
    assert_eq!(agents.len(), 1, "one agent, HR: {agents:?}");
    assert_eq!(agents[0].file_name().unwrap(), "profile.md");
    let hr_id = agents[0].parent().unwrap().file_name().unwrap();
    assert!(is_uuid_v7(hr_id.to_str().unwrap()), "HR id {hr_id:?}");
    let hr = read(&agents[0]);
    assert!(has_field(&hr, "handle", "hr"), "{hr}");
    assert!(has_field(&hr, "status", "active"), "{hr}");
    assert!(!hr.contains("agent_status"), "{hr}");
}

/// A `key: value` line of a YAML document, with the value quoted or bare.
fn has_field(document: &str, key: &str, value: &str) -> bool {
    document
        .lines()
        .any(|line| line == format!("{key}: {value}") || line == format!("{key}: \"{value}\""))
}

fn names(found: &[(PathBuf, Vec<u8>)]) -> Vec<&PathBuf> {
    found.iter().map(|(p, _)| p).collect()
}

/// No web or worker process of the fixture under `base` survives its host.
fn assert_no_process_left(base: &Path) {
    wait_until("the fixture's processes to exit", || {
        web_pids(base).is_empty()
    });
}

/// Stops the host and waits until nothing of it creates any more: no web or worker
/// process, and no create thread (a create in flight runs on its own thread, which owns
/// the host's creation state and ends when the supervisor stops its server).
fn stop(host: Host, base: &Path) {
    let creating = Arc::downgrade(
        host.service
            .provisioning_for_test()
            .expect("a registered host creates organizations"),
    );
    drop(host);
    wait_until("the stopped host's creates to end", || {
        creating.strong_count() == 0
    });
    assert_no_process_left(base);
}

fn restart(base: &Path, address: SocketAddr) -> Host {
    let (host, _socket) = host_at(fixture_config(base), base, |_, _, _| {});
    wait_serving(&host, address);
    host
}

fn wait_offices_active(host: &Host, count: usize) {
    wait_until("every office active", || {
        let offices = host.offices.offices();
        offices.len() == count && offices.iter().all(|o| o.status == OfficeStatus::Active)
    });
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_browser_creates_an_organization_that_survives_a_lost_response_and_a_host_restart() {
    let Launched {
        temp: _temp,
        address,
        host,
        link,
        base,
        ..
    } = launch(Seed::RegistrationLink);
    wait_serving(&host, address);
    // The link registers the administrator and signs the browser in.
    let registration = json!({ "link": link.expect("a registration link"), "login": "Owner", "password": PASSWORD });
    let registered =
        post_until_available(address, "/api/v1/links/register", &registration.to_string());
    let browser = Browser::new(&host, address, established(&registered, "register"));
    assert_eq!(
        browser.list(),
        json!({ "organizations": [], "creating": [] })
    );

    // Create, then poll the lookup route until it completed.
    let (operation, view) = browser.create("Example", "example");
    let (organization, office, creator) = checked(&view, "Example", "example");
    assert_organization_files(&base, &organization, &creator, "example");
    wait_offices_active(&host, 1);

    // The account is linked: its session names the creator as its person there, and the
    // list shows the organization with HQ and nothing still being created.
    // (The browser session view does not fill `office_ids`; the list carries the offices.)
    let linked = |session: Value| {
        let links = session["organizations"].as_array().expect("links").clone();
        assert_eq!(links.len(), 1, "{session}");
        assert_eq!(links[0]["org_id"], organization.as_str(), "{session}");
        assert_eq!(links[0]["employee_id"], creator.as_str(), "{session}");
    };
    linked(browser.session());
    let listed = browser.list();
    assert_eq!(listed["creating"], json!([]), "{listed}");
    assert_eq!(listed["organizations"].as_array().unwrap().len(), 1);
    assert_eq!(
        checked(&listed["organizations"][0], "Example", "example"),
        (organization.clone(), office.clone(), creator.clone())
    );

    // A lost response: the same request with the same operation id answers the same
    // organization; the same id with another payload is refused.
    let replay = browser.post(
        "/api/v1/organizations",
        &browser.body(&operation, "Example", "example"),
    );
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.body["outcome"], "completed");
    assert_eq!(replay.body["operation_id"], operation.as_str());
    assert_eq!(
        checked(&replay.body["result"], "Example", "example"),
        (organization.clone(), office.clone(), creator.clone())
    );
    let reused = browser.post(
        "/api/v1/organizations",
        &browser.body(&operation, "Example", "another"),
    );
    assert_eq!(reused.status, 409, "{}", reused.body);
    assert_eq!(reused.body["outcome"], "rejected");
    assert_eq!(reused.body["error"]["code"], "operation_reused");
    assert_eq!(browser.list(), listed, "a refused replay changes nothing");

    // The host restarts on the same data: the same cookie and proof list the organization
    // with HQ, the link survives and the files are byte-for-byte what they were.
    let (_, before) = state_files(&base, &organization);
    stop(host, &base);
    let host = restart(&base, address);
    wait_offices_active(&host, 1);
    let after = browser.list();
    assert_eq!(after["creating"], json!([]), "{after}");
    assert_eq!(
        checked(&after["organizations"][0], "Example", "example"),
        (organization.clone(), office.clone(), creator.clone())
    );
    linked(browser.session());
    assert_organization_files(&base, &organization, &creator, "example");
    assert!(
        state_files(&base, &organization).1 == before,
        "files changed"
    );
    // The operation still answers after the restart, by lookup and by replay.
    assert_eq!(
        checked(&browser.completed(&operation), "Example", "example").0,
        organization
    );
    let replay = browser.post(
        "/api/v1/organizations",
        &browser.body(&operation, "Example", "example"),
    );
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.body["result"]["id"], organization.as_str());
    assert_eq!(host.offices.offices().len(), 1, "no second office");
    stop(host, &base);
}

/// `recovery.package` on the host's local socket: the HQ's public recovery record, which
/// the host reads from the office (its recovery-public call), signed in as the seeded
/// administrator with presence answered yes.
fn recovery_package(host: &Host, socket: &Path, organization: &str) -> dto::PackageResult {
    let stream = UnixStream::connect(socket).unwrap();
    stream.set_read_timeout(Some(super::WAIT)).unwrap();
    let mut wire = BufReader::new(stream);
    super::answer::<org_host_control::Hello>(&mut wire, 1, &Default::default()).unwrap();
    let native = super::answer::<ops::AccountVerify>(
        &mut wire,
        2,
        &ops::AccountVerifyParams {
            login: "Owner".into(),
            password: org_host_control::Password::new(PASSWORD.into()).unwrap(),
        },
    )
    .unwrap();
    let params = dto::PackageParams {
        session: org_host_control::SessionHandle::new(native.session.expose().into()).unwrap(),
        org_id: organization.to_owned(),
    };
    host.service
        .recovery_for_test()
        .with_test_presence(Arc::new(Confirmed), || {
            super::answer::<dto::Package>(&mut wire, 3, &params).unwrap()
        })
}

/// The office's store keeps only a disposable projection under
/// `<data root>/<organization>/runtime/<office>/`: deleted while the host is stopped, the
/// office rebuilds it from the journal at the next start. The organization is listed as
/// before, its office is active, its files are byte-for-byte what they were, and the
/// recovery-public read still returns the same escrow package, the one in its files.
#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_office_runtime_deleted_while_stopped_is_rebuilt_and_the_organization_stays_listed() {
    let Launched {
        temp: _temp,
        address,
        host,
        socket,
        base,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let browser = Browser::sign_in(&host, address);
    let (_, view) = browser.create("Example", "example");
    let (organization, office, creator) = checked(&view, "Example", "example");
    wait_offices_active(&host, 1);
    let listed = browser.list();
    assert_eq!(listed["organizations"].as_array().unwrap().len(), 1);
    let (_, files_before) = state_files(&base, &organization);
    let package = recovery_package(&host, &socket, &organization);
    assert_eq!(package.organization_id, organization);
    assert_eq!(package.office_id, office);
    // The package is the escrow file in the organization's state, as hex.
    let escrow = files_before
        .iter()
        .find(|(path, _)| path == Path::new("security/ca-escrow.v1.bin"))
        .map(|(_, bytes)| bytes)
        .expect("the escrow package in the organization's files");
    let escrow_hex: String = escrow.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(package.package_hex, escrow_hex);
    stop(host, &base);

    // Delete the office's runtime folder: `<data root>/<organization>/runtime/<office>/`.
    let database = Path::new(&organization)
        .join("runtime")
        .join(&office)
        .join("office.db");
    let projection = all_files(&base)
        .into_iter()
        .find(|p| p.ends_with(&database))
        .unwrap_or_else(|| panic!("no {database:?} under {base:?}"))
        .parent()
        .unwrap()
        .to_path_buf();
    fs::remove_dir_all(&projection).unwrap();
    assert!(!projection.exists());

    // The host starts again on the same data and the office rebuilds its projection.
    let (host, socket) = host_at(fixture_config(&base), &base, |_, _, _| {});
    wait_serving(&host, address);
    wait_offices_active(&host, 1);
    assert!(
        projection.join("office.db").is_file(),
        "the projection was not rebuilt at {projection:?}"
    );
    let after = browser.list();
    assert_eq!(after["creating"], json!([]), "{after}");
    assert_eq!(after["organizations"].as_array().unwrap().len(), 1);
    assert_eq!(
        checked(&after["organizations"][0], "Example", "example"),
        (organization.clone(), office.clone(), creator.clone())
    );
    assert_eq!(after, listed, "the list is what it was");
    assert_organization_files(&base, &organization, &creator, "example");
    assert!(
        state_files(&base, &organization).1 == files_before,
        "files changed"
    );
    assert_eq!(recovery_package(&host, &socket, &organization), package);
    assert_eq!(host.offices.offices().len(), 1, "no second office");
    stop(host, &base);
}

/// The reservation's `linked` flag, read from the registry file while no host runs.
fn linked_on_disk(base: &Path, operation: &str) -> bool {
    let registry = all_files(base)
        .into_iter()
        .find(|p| p.file_name().unwrap() == "host-registry.db")
        .expect("the host registry");
    crate::hostdb::sqlite::provisioning_linked_on_disk_for_test(&registry, operation)
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_host_stopped_in_the_middle_of_a_create_finishes_it_at_its_next_start_with_no_client() {
    let Launched {
        temp: _temp,
        address,
        host,
        base,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let browser = Browser::sign_in(&host, address);
    // Admitted, then the host stops before the create is linked: its creation thread is
    // held until the stop, so it cannot link first. The browser never asks about this
    // operation again.
    host.service
        .provisioning_for_test()
        .expect("a registered host creates organizations")
        .hold_drives();
    let operation = crate::ids::uuid_v7().unwrap();
    let posted = browser.post(
        "/api/v1/organizations",
        &browser.body(&operation, "Example", "example"),
    );
    assert_eq!(posted.status, 202, "{}", posted.body);
    assert_eq!(posted.body["outcome"], "accepted");
    stop(host, &base);
    assert!(!linked_on_disk(&base, &operation), "held until the stop");
    // The office on its store publishes at `<data root>/<organization>/durable/state/`.
    assert!(
        !all_files(&base.join("instance")).iter().any(|p| p
            .ancestors()
            .any(|folder| folder.ends_with("durable/state"))),
        "nothing of the organization was written before the stop"
    );

    // The starting host continues the reserved create by itself.
    let host = restart(&base, address);
    let (db, _) = host
        .service
        .authentication
        .as_ref()
        .expect("registered host authentication")
        .provisioning_storage();
    wait_until("the reserved create to be linked", || {
        db.provisioning_reserved().unwrap().is_empty()
    });
    assert_eq!(host.service.organization_ops(), 0, "no client asked");
    drop(db);
    wait_offices_active(&host, 1);
    // The first thing the browser asks is its list: the organization is there, created.
    let listed = browser.list();
    assert_eq!(listed["creating"], json!([]), "{listed}");
    assert_eq!(listed["organizations"].as_array().unwrap().len(), 1);
    let (organization, _, creator) = checked(&listed["organizations"][0], "Example", "example");
    assert_organization_files(&base, &organization, &creator, "example");
    assert_eq!(
        checked(&browser.completed(&operation), "Example", "example").0,
        organization
    );
    stop(host, &base);
    assert!(linked_on_disk(&base, &operation));
}

/// A create whose worker ignores its closed stdin: the stopping host kills it at the end
/// of the supervisor's grace period, and no process of it is left the moment the stop
/// returns (nothing waits for it afterwards). The create stays reserved, unlinked.
#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_host_stop_leaves_no_create_worker_that_ignores_its_closed_stdin() {
    let Launched {
        temp: _temp,
        address,
        host,
        base,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let browser = Browser::sign_in(&host, address);
    // No office runs yet, so the next worker the host starts is the create's. It is this
    // test binary's stand-in: it takes the real handoff, then never reports `created` and
    // does not exit when its stdin closes.
    let pid_file = base.join("create-worker.pid");
    let worker = base.join("bin/agentic-org-office");
    fs::remove_file(&worker).unwrap();
    crate::supervisor::tests::write_create_server(&worker, &pid_file, true);
    let operation = crate::ids::uuid_v7().unwrap();
    let posted = browser.post(
        "/api/v1/organizations",
        &browser.body(&operation, "Example", "example"),
    );
    assert_eq!(posted.status, 202, "{}", posted.body);
    let mut pid: Option<libc::pid_t> = None;
    wait_until("the create's worker to start", || {
        pid = fs::read_to_string(&pid_file)
            .ok()
            .and_then(|text| text.parse().ok());
        pid.is_some()
    });
    let pid = pid.unwrap();
    // SAFETY: signal 0 only checks the pid.
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "the worker is running");
    drop(host);
    // SAFETY: signal 0 only checks the pid.
    let left = unsafe { libc::kill(pid, 0) } == 0;
    assert!(!left, "the create's worker outlived the host's stop");
    assert!(!linked_on_disk(&base, &operation), "it never reported");
    assert_no_process_left(&base);
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_listener_serves_two_organizations_and_a_refused_create_reaches_no_host_call() {
    let Launched {
        temp: _temp,
        address,
        host,
        base,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let browser = Browser::sign_in(&host, address);
    let operation = crate::ids::uuid_v7().unwrap();
    let body = browser.body(&operation, "Example", "example");
    let before = host.service.organization_ops();

    // Ingress refusals: decided from the request's headers.
    let raw = |extra: &str, content_type: &str, origin: &str, host_header: &str| {
        format!(
            "POST /api/v1/organizations HTTP/1.1\r\nHost: {host_header}\r\n{origin}Content-Type: {content_type}\r\nContent-Length: {}\r\n{}{extra}Connection: close\r\n\r\n{body}",
            body.len(),
            browser.pair.headers()
        )
    };
    let here = address.to_string();
    let origin = format!("Origin: http://{address}\r\n");
    for (request, status, what) in [
        (
            raw("Sec-Fetch-Site: cross-site\r\n", "application/json", &origin, &here),
            403,
            "cross-site",
        ),
        (
            raw("", "application/json", "Origin: http://evil.test\r\n", &here),
            403,
            "foreign Origin",
        ),
        (raw("", "application/json", "", &here), 403, "no Origin"),
        (raw("", "text/plain", &origin, &here), 415, "text/plain"),
        (
            raw("", "application/json", &origin, "evil.test"),
            400,
            "foreign Host",
        ),
        (
            format!("OPTIONS /api/v1/organizations HTTP/1.1\r\nHost: {address}\r\n{origin}Connection: close\r\n\r\n"),
            405,
            "OPTIONS",
        ),
    ] {
        let reply = send(address, &request);
        assert_eq!(reply.status, status, "{what}: {}", reply.headers);
        assert!(
            !reply.headers.to_ascii_lowercase().contains("access-control-"),
            "{what}: no CORS"
        );
    }
    // No pair: 401 on each of the three routes.
    for (method, path, body) in [
        ("POST", "/api/v1/organizations".to_owned(), body.as_str()),
        ("GET", "/api/v1/organizations".to_owned(), ""),
        (
            "GET",
            format!(
                "/api/v1/operations/host/{operation}?instance_id={}",
                browser.instance
            ),
            "",
        ),
    ] {
        let reply = http(address, method, &path, "", body);
        assert_eq!(reply.status, 401, "{method} {path}: {}", reply.body);
    }
    // Refusals the web process decides from the body: 400 naming the input. The pair is
    // only parsed before the body is checked; the session is validated when the call
    // runs, so a refused body reaches no host call at all.
    let ops_before = host.service.browser_ops();
    let instance = browser.instance.as_str();
    let other_instance = "0193a5c0-0000-7000-8000-000000000009";
    let ulid_operation = "01M72N8MG00000000000000001";
    for (body, field, rule, what) in [
        (
            create_body(
                instance,
                "not-an-operation-id",
                "Example",
                "example",
                remote("example"),
            ),
            Some("operation_id"),
            Some("format"),
            "an operation id that is no UUIDv7",
        ),
        (
            create_body(
                instance,
                ulid_operation,
                "Example",
                "example",
                remote("example"),
            ),
            Some("operation_id"),
            Some("format"),
            "a ULID operation id",
        ),
        (
            create_body(
                other_instance,
                &operation,
                "Example",
                "example",
                remote("example"),
            ),
            None,
            None,
            "another instance's scope",
        ),
        (
            create_body(
                instance,
                &operation,
                "Example",
                "example",
                json!({ "kind": "local", "path": "/tmp/example" }),
            ),
            Some("organization_repository"),
            Some("remote_required"),
            "a local repository with a path",
        ),
        (
            create_body(
                instance,
                &operation,
                "Example",
                "example",
                json!({ "kind": "local" }),
            ),
            Some("organization_repository"),
            Some("remote_required"),
            "a local repository",
        ),
        (
            create_body(
                instance,
                &operation,
                "Example",
                "example",
                json!({ "kind": "remote" }),
            ),
            Some("organization_repository"),
            Some("remote_required"),
            "a remote with no URL",
        ),
        (
            create_body(
                instance,
                &operation,
                "Example",
                "example",
                json!({ "kind": "remote", "remote_url": "https://example.test/example.git",
                    "credential_ref": { "id": "x" } }),
            ),
            Some("organization_repository"),
            Some("unsupported"),
            "a credential reference",
        ),
    ] {
        let reply = browser.post("/api/v1/organizations", &body);
        assert_eq!(reply.status, 400, "{what}: {}", reply.body);
        assert_eq!(reply.body["error"]["code"], "invalid_request", "{what}");
        if let Some(field) = field {
            assert_eq!(
                reply.body["error"]["field"], field,
                "{what}: {}",
                reply.body
            );
            assert_eq!(reply.body["error"]["rule"], rule.unwrap(), "{what}");
        }
    }
    // A local path given as a URL is refused by name too.
    let file_url = browser.post(
        "/api/v1/organizations",
        &create_body(
            instance,
            &operation,
            "Example",
            "example",
            json!({ "kind": "remote", "remote_url": "file:///tmp/example.git" }),
        ),
    );
    assert_eq!(file_url.status, 400, "{}", file_url.body);
    assert_eq!(
        file_url.body["error"]["field"], "organization_repository",
        "{}",
        file_url.body
    );
    assert_eq!(file_url.body["error"]["rule"], "remote_required");
    // The lookup route decides its id and scope the same way.
    for target in [
        format!("/api/v1/operations/host/{ulid_operation}?instance_id={instance}"),
        format!("/api/v1/operations/host/{operation}?instance_id={other_instance}"),
        format!("/api/v1/operations/host/{operation}"),
    ] {
        assert_eq!(browser.get(&target).status, 400, "{target}");
    }
    assert_eq!(
        host.service.organization_ops(),
        before,
        "a refused request reached a host organization call"
    );
    assert_eq!(
        host.service.browser_ops(),
        ops_before,
        "a refused body or lookup target reached a host call"
    );
    assert_eq!(
        browser.list(),
        json!({ "organizations": [], "creating": [] }),
        "nothing was reserved"
    );
    assert_eq!(host.service.organization_ops(), before + 1, "the list");
    assert!(host.offices.offices().is_empty());

    // SRV-24: two organizations behind the one listener, each with its own HQ.
    let posted = browser.post("/api/v1/organizations", &body);
    assert!(matches!(posted.status, 200 | 202), "{}", posted.body);
    let first = checked(&browser.completed(&operation), "Example", "example");
    let (_, view) = browser.create("Second", "second");
    let second = checked(&view, "Second", "second");
    assert_ne!(first.0, second.0, "two organizations");
    assert_ne!(first.1, second.1, "two offices");
    wait_offices_active(&host, 2);
    assert_organization_files(&base, &first.0, &first.2, "example");
    let listed = browser.list();
    let organizations = listed["organizations"].as_array().unwrap();
    assert_eq!(organizations.len(), 2, "{listed}");
    let mut names: Vec<_> = organizations.iter().map(|o| text(&o["name"])).collect();
    names.sort();
    assert_eq!(names, ["Example", "Second"]);
    assert_eq!(listed["creating"], json!([]));

    // A duplicate name (any letter case) or slug is named, and stores nothing.
    for (name, slug, field) in [("EXAMPLE", "third", "name"), ("Third", "example", "slug")] {
        let duplicate = crate::ids::uuid_v7().unwrap();
        let reply = browser.post(
            "/api/v1/organizations",
            &browser.body(&duplicate, name, slug),
        );
        assert_eq!(reply.status, 409, "{name}/{slug}: {}", reply.body);
        assert_eq!(reply.body["outcome"], "rejected");
        assert_eq!(reply.body["error"]["code"], "already_exists");
        assert_eq!(reply.body["error"]["field"], field, "{}", reply.body);
        let missing = lookup(address, &browser.pair.headers(), instance, &duplicate);
        assert_eq!(missing.status, 404, "{}", missing.body);
    }
    assert_eq!(browser.list(), listed, "a refused create changes nothing");
    assert_eq!(host.offices.offices().len(), 2);
    stop(host, &base);
}

/// The OS presence prompt, answered yes: a local foreground person confirmed.
struct Confirmed;
impl TestPresence for Confirmed {
    fn local(&self, _: &Connection) -> bool {
        true
    }
    fn confirm(&self, _: &dyn Fn() -> bool) -> Result<(), ()> {
        Ok(())
    }
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_recovery_code_is_released_once_natively_and_no_credential_reaches_http_or_a_plain_file() {
    let Launched {
        temp: _temp,
        address,
        host,
        socket,
        base,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let browser = Browser::sign_in(&host, address);
    let (operation, view) = browser.create("Example", "example");
    let (organization, office, _) = checked(&view, "Example", "example");
    wait_offices_active(&host, 1);
    let recovery_state = || browser.list()["organizations"][0]["recovery"].clone();
    assert_eq!(recovery_state(), "pending");

    // The native path on the HQ machine: sign in on host.sock, confirm presence, release.
    let stream = UnixStream::connect(&socket).unwrap();
    stream.set_read_timeout(Some(super::WAIT)).unwrap();
    let mut wire = BufReader::new(stream);
    let mut next = 0;
    let mut id = || {
        next += 1;
        next
    };
    super::answer::<org_host_control::Hello>(&mut wire, id(), &Default::default()).unwrap();
    let native = super::answer::<ops::AccountVerify>(
        &mut wire,
        id(),
        &ops::AccountVerifyParams {
            login: "Owner".into(),
            password: org_host_control::Password::new(PASSWORD.into()).unwrap(),
        },
    )
    .unwrap();
    // The native sign-in names the organization: what `recovery --org <name>` resolves.
    assert!(native
        .orgs
        .iter()
        .any(|o| o.org_id == organization && o.name == "Example"));
    let session = || org_host_control::SessionHandle::new(native.session.expose().into()).unwrap();
    let release = |redeliver| ops::RecoveryReleaseParams {
        session: session(),
        org_id: organization.clone(),
        redeliver,
    };
    let code = host
        .service
        .recovery_for_test()
        .with_test_presence(Arc::new(Confirmed), || {
            let released =
                super::answer::<ops::RecoveryRelease>(&mut wire, id(), &release(false)).unwrap();
            let code = released.code.expose().to_owned();
            assert_eq!(recovery_state(), "delivered");
            let _: org_proto::Empty = super::answer::<ops::RecoveryAcknowledge>(
                &mut wire,
                id(),
                &ops::RecoveryAcknowledgeParams {
                    session: session(),
                    org_id: organization.clone(),
                },
            )
            .unwrap();
            assert_eq!(recovery_state(), "acknowledged");
            // Delivered once: neither a second release nor a redelivery shows it again.
            for redeliver in [false, true] {
                assert_eq!(
                    super::answer::<ops::RecoveryRelease>(&mut wire, id(), &release(redeliver))
                        .unwrap_err()
                        .code,
                    ErrorCode::Conflict,
                    "redeliver {redeliver}"
                );
            }
            code
        });
    assert!(code.len() >= 16, "a recovery code");
    drop(wire);

    // Everything else the browser can read about the organization, after the release.
    browser.session();
    browser.get("/api/v1/instance");
    browser.completed(&operation);
    let replay = browser.post(
        "/api/v1/organizations",
        &browser.body(&operation, "Example", "example"),
    );
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.body["result"]["recovery"], "acknowledged");
    let seen = browser.seen.borrow();
    assert!(seen.len() > 8, "{}", seen.len());
    let compact: String = code.chars().filter(char::is_ascii_alphanumeric).collect();
    for reply in seen.iter() {
        for form in [code.as_str(), compact.as_str()] {
            assert!(
                !reply
                    .to_ascii_lowercase()
                    .contains(&form.to_ascii_lowercase()),
                "the recovery code crossed HTTP: {reply}"
            );
        }
        assert!(!reply.contains(PASSWORD), "the password crossed HTTP");
    }
    drop(seen);
    stop(host, &base);
    // What the web process printed, which the host keeps in no file.
    let web_stderr = crate::web::captured_stderr(&browser.instance)
        .expect("this instance's web process stderr was drained into the capture");

    // ENC-2: scan the stopped instance. The office secrets key is in its custody file
    // (positive control) and custody is owner-only.
    let custody = base.join("keys");
    let key_file = all_files(&custody)
        .into_iter()
        .find(|p| p.file_name().unwrap() == format!("{office}.key").as_str())
        .expect("the office's custody key file");
    let key = unhex(&fs::read_to_string(&key_file).unwrap());
    assert_eq!(key.len(), 32);
    let mut owner_only = vec![custody.clone()];
    owner_only.extend(all_files(&custody));
    for path in &owner_only {
        let mode = fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{path:?} is not owner-only: {mode:o}");
    }
    // No credential in plaintext in any database, log, runtime file or Git content: not
    // the password, the browser's cookie or proof, the recovery code, or the office key.
    let cookie = browser.pair.cookie.split_once('=').unwrap().1.to_owned();
    let mut secrets: Vec<(&str, Vec<u8>)> = vec![
        ("password", PASSWORD.as_bytes().to_vec()),
        ("session cookie", cookie.into_bytes()),
        ("session proof", browser.pair.proof.clone().into_bytes()),
        ("recovery code", code.clone().into_bytes()),
        ("recovery code", compact.into_bytes()),
    ];
    secrets.extend(encodings(&key).map(|form| ("office secrets key", form)));
    for (what, secret) in &secrets {
        assert!(
            !contains(&web_stderr, secret),
            "{what} in the web process's stderr"
        );
    }
    let (mut scanned, mut databases) = (0, 0);
    for path in all_files(&base) {
        if path.starts_with(base.join("bin")) || path.starts_with(&custody) {
            continue;
        }
        let bytes = fs::read(&path).unwrap();
        scanned += 1;
        databases += usize::from(bytes.starts_with(b"SQLite format 3\0"));
        for (what, secret) in &secrets {
            assert!(!contains(&bytes, secret), "{what} in {path:?}");
        }
        // A private key is never in a database or in the organization's Git content, and
        // any file that holds one is owner-only.
        if contains(&bytes, b"PRIVATE KEY-----") {
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "private key in {path:?} ({mode:o})");
            assert!(!bytes.starts_with(b"SQLite format 3\0"), "{path:?}");
            assert!(
                !path
                    .ancestors()
                    .any(|folder| folder.ends_with("durable/state")),
                "private key in Git content: {path:?}"
            );
        }
    }
    // Positive controls: the scan read the registry and the office's projection database,
    // and the custody file does hold the key in a scanned form.
    assert!(
        databases >= 2,
        "{databases} databases among {scanned} files"
    );
    assert!(contains(&fs::read(&key_file).unwrap(), &encodings(&key)[1]));
}

/// B2 Q8 production-path proof: a signed-in browser reads a live organization snapshot and
/// creates a team through `agentic-org-web` -> `org-host` (`browser.open_channel` +
/// `browser.grant`) -> `agentic-org-office` (`org-store`), surviving office restart and
/// full deletion of `runtime/<office>/office.db`, with auth fail-closed on sign-out.
#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_browser_reads_live_org_snapshot_and_creates_team_surviving_projection_rebuild() {
    let Launched {
        temp: _temp,
        address,
        host,
        base,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let browser = Browser::sign_in(&host, address);
    let (_, view) = browser.create("Example", "example");
    let (organization, office, creator) = checked(&view, "Example", "example");
    wait_offices_active(&host, 1);

    // 1. Live snapshot via browser-to-office Web channel.
    let snap_path = format!("/api/v1/organizations/{organization}/snapshot");
    let teams_path = format!("/api/v1/organizations/{organization}/teams");
    let snap = browser.get(&snap_path);
    assert_eq!(snap.status, 200, "{}", snap.body);
    let snapshot = &snap.body["result"]["snapshot"];
    assert_eq!(snapshot["position"]["seq"], 1, "{snapshot}");
    let teams = snapshot["teams"].as_array().expect("teams");
    assert_eq!(teams.len(), 1, "{snapshot}");
    let root_team_id = text(&teams[0]["id"]).to_owned();
    assert!(is_uuid_v7(&root_team_id), "root team id {root_team_id}");
    assert_eq!(teams[0]["lead_member_id"], creator.as_str(), "{snapshot}");
    assert!(
        snap.body["result"]["resume_cursor"]
            .as_str()
            .is_some_and(|c| !c.is_empty()),
        "{}",
        snap.body
    );

    // 2. Fail-closed checks: unauthenticated -> 401; unlinked org -> 404; bad scope -> 400.
    let unauth = http(address, "GET", &snap_path, "", "");
    assert_eq!(unauth.status, 401, "{}", unauth.body);
    let unknown_org = "0193a5c0-0000-7000-8000-000000000099";
    let missing = browser.get(&format!("/api/v1/organizations/{unknown_org}/snapshot"));
    assert_eq!(missing.status, 404, "{}", missing.body);

    let child_team_id = "0193a5c0-0000-7000-8000-000000000042";
    let op_id = crate::ids::uuid_v7().unwrap();
    let bad_scope = json!({
        "operation_id": op_id,
        "scope": { "kind": "organization", "instance_id": browser.instance, "org_id": unknown_org },
        "params": { "team_id": child_team_id, "name": "Platform", "owner_team_id": root_team_id }
    });
    let refused = browser.post(&teams_path, &bad_scope.to_string());
    assert_eq!(refused.status, 400, "{}", refused.body);

    // 3. Create a child team end-to-end through the Web channel and verify idempotent replay.
    let create_team = json!({
        "operation_id": op_id,
        "scope": { "kind": "organization", "instance_id": browser.instance, "org_id": organization },
        "params": { "team_id": child_team_id, "name": "Platform", "owner_team_id": root_team_id }
    });
    let created = browser.post(&teams_path, &create_team.to_string());
    assert_eq!(created.status, 200, "{}", created.body);
    assert_eq!(created.body["outcome"], "completed", "{}", created.body);
    assert_eq!(created.body["operation_id"], op_id.as_str());

    let replay = browser.post(&teams_path, &create_team.to_string());
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.body["outcome"], "completed", "{}", replay.body);

    let snap2 = browser.get(&snap_path);
    assert_eq!(snap2.status, 200, "{}", snap2.body);
    assert_eq!(snap2.body["result"]["snapshot"]["position"]["seq"], 2);
    let teams2 = snap2.body["result"]["snapshot"]["teams"]
        .as_array()
        .expect("teams");
    assert_eq!(teams2.len(), 2, "{}", snap2.body);
    assert!(
        teams2
            .iter()
            .any(|t| t["id"] == child_team_id && t["name"] == "Platform"),
        "{}",
        snap2.body
    );

    // 4. Stop host, delete `runtime/<office>/office.db`, restart host: projection rebuilds
    // from durable files and the browser reads the committed child team over a fresh channel.
    stop(host, &base);
    let database = Path::new(&organization)
        .join("runtime")
        .join(&office)
        .join("office.db");
    let projection = all_files(&base)
        .into_iter()
        .find(|p| p.ends_with(&database))
        .unwrap_or_else(|| panic!("no {database:?} under {base:?}"))
        .parent()
        .unwrap()
        .to_path_buf();
    fs::remove_dir_all(&projection).unwrap();
    assert!(!projection.exists());

    let host = restart(&base, address);
    wait_offices_active(&host, 1);
    assert!(projection.join("office.db").is_file());
    let after = browser.get(&snap_path);
    assert_eq!(after.status, 200, "{}", after.body);
    assert_eq!(after.body["result"]["snapshot"]["position"]["seq"], 2);
    let teams_after = after.body["result"]["snapshot"]["teams"]
        .as_array()
        .expect("teams");
    assert!(
        teams_after
            .iter()
            .any(|t| t["id"] == child_team_id && t["name"] == "Platform"),
        "{}",
        after.body
    );

    // 5. Sign out revokes the browser session; subsequent snapshot calls fail closed with 401.
    let signed_out = browser.post("/api/v1/session/sign-out", "{}");
    assert_eq!(signed_out.status, 200, "{}", signed_out.body);
    let after_logout = browser.get(&snap_path);
    assert_eq!(after_logout.status, 401, "{}", after_logout.body);
    stop(host, &base);
}
