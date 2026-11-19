//! The host's org-proto surface: hello, unknown methods, unreadable lines, and the
//! owner-only files it serves from.

// Every host here keeps its offices' keys in a development key folder, which only debug builds
// have (a release host uses the Keychain, which tests never touch): debug builds only.
#![cfg(debug_assertions)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use org_host::Host;
use org_proto::{
    ErrorCode, Hello, HelloParams, HelloResult, OfficeInfo, OfficeList, OfficeStatus,
    OfficeStatusChanged, OrganizationList, Outcome, ServerMessage, Subscribe, SubscribeParams,
    API_VERSION, UNREADABLE_REQUEST_ID,
};
use support::{Client, TestHome};

fn error_code(outcome: &Outcome) -> Option<ErrorCode> {
    match outcome {
        Outcome::Error(e) => Some(e.code),
        Outcome::Result(_) => None,
    }
}

#[test]
fn hello_names_the_host_and_no_office() {
    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    let hello = Client::connect(host.socket_path())
        .call::<Hello>(&HelloParams::default())
        .unwrap();
    assert_eq!(hello.api_version, API_VERSION);
    assert_eq!(hello.server_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(hello.host, "This Mac");
    assert_eq!(hello.office, None);
    assert_eq!(hello.instance_id.as_deref(), Some(home.instance_id()));
    assert!(hello.capabilities.is_empty());
    host.shutdown();
}

/// X-02 §3: the host decodes `hello`'s params strictly. A malformed `api` or a non-string `client`
/// is `invalid_request`, never answered as v1, and the connection goes on; `{}`, a missing
/// `params` and `"api": null` are still a v1 `hello`.
#[test]
fn a_malformed_hello_is_invalid_request_and_an_empty_one_is_v1() {
    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());

    for (id, params) in [
        (2, r#"{"api":"2"}"#),
        (3, r#"{"api":2}"#),
        (4, r#"{"api":{"min":1}}"#),
        (5, r#"{"api":{"min":"1","max":2}}"#),
        (6, r#"{"api":{"min":2,"max":1}}"#),
        (7, r#"{"api":{"min":0,"max":1}}"#),
        (8, r#"{"client":7}"#),
        (9, r#"{"api":{"min":1,"max":2},"client":["agent-os"]}"#),
        (13, r#"{"api":[1,2]}"#),
        (14, r#"{"api":{"min":1,"max":2,"x":0}}"#),
        (16, r#"{"client":null}"#),
        (17, r#"{"api":{"min":-1,"max":2}}"#),
        (18, r#"{"api":{"min":1,"max":4294967296}}"#),
    ] {
        let reply = client.raw(&format!(
            r#"{{"id":{id},"method":"hello","params":{params}}}"#
        ));
        assert_eq!(reply.id, id, "{params}");
        assert_eq!(
            error_code(&reply.outcome),
            Some(ErrorCode::InvalidRequest),
            "{params}"
        );
    }

    for (id, line) in [
        (10, r#"{"id":10,"method":"hello","params":{}}"#),
        (11, r#"{"id":11,"method":"hello"}"#),
        (
            12,
            r#"{"id":12,"method":"hello","params":{"client":"agent-os 0.1.0"}}"#,
        ),
        (15, r#"{"id":15,"method":"hello","params":{"api":null}}"#),
    ] {
        let reply = client.raw(line);
        assert_eq!(reply.id, id, "{line}");
        let Outcome::Result(result) = reply.outcome else {
            panic!("{line}: {:?}", reply.outcome);
        };
        let hello: HelloResult = serde_json::from_value(result).unwrap();
        assert_eq!(hello.api_version, API_VERSION, "{line}");
        assert_eq!(hello.host, "This Mac", "{line}");
    }
    host.shutdown();
}

#[test]
fn unknown_methods_and_unreadable_lines_get_errors_and_the_connection_goes_on() {
    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    client
        .call::<org_host_control::Hello>(&Default::default())
        .unwrap();

    let reply = client.raw(r#"{"id":5,"method":"org.teleport","params":{}}"#);
    assert_eq!(reply.id, 5);
    assert_eq!(error_code(&reply.outcome), Some(ErrorCode::NotAuthorized));

    let reply = client.raw("this isn't json");
    assert_eq!(reply.id, UNREADABLE_REQUEST_ID);
    assert_eq!(error_code(&reply.outcome), Some(ErrorCode::InvalidRequest));

    let reply = client.raw(r#"{"id":6,"method":"office.list","params":[1]}"#);
    assert_eq!(reply.id, 6);
    assert_eq!(error_code(&reply.outcome), Some(ErrorCode::NotAuthorized));
    let reply = client
        .raw(r#"{"id":7,"method":"subscribe","params":{"from_position":{"epoch":1,"seq":2}}}"#);
    assert_eq!(error_code(&reply.outcome), Some(ErrorCode::NotAuthorized));
    let status =
        client
            .call::<org_host_control::InstanceStatus>(
                &org_host_control::InstanceStatusParams::default(),
            )
            .unwrap();
    assert_eq!(status.offices.total, 0);
    host.shutdown();
}

#[test]
fn the_home_and_socket_are_owner_only() {
    let home = TestHome::new();
    fs::create_dir_all(home.path()).unwrap();
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let host = Host::start(home.config()).unwrap();
    let mode = |p: &std::path::Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&home.path()), 0o700);
    for dir in ["run", "logs", "offices"] {
        assert_eq!(mode(&home.path().join(dir)), 0o700, "{dir}");
    }
    assert_eq!(mode(host.socket_path()), 0o600);
    assert_eq!(mode(&home.path().join("host.lock")), 0o600);
    host.shutdown();
}

#[test]
fn a_stale_socket_is_replaced_but_another_file_is_not() {
    // A socket file left behind by a host that was killed.
    let stale = TestHome::new();
    fs::create_dir_all(stale.path()).unwrap();
    drop(std::os::unix::net::UnixListener::bind(stale.path().join("host.sock")).unwrap());
    let host = Host::start(stale.config()).unwrap();
    assert_eq!(
        Client::connect(host.socket_path())
            .call::<Hello>(&HelloParams::default())
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    host.shutdown();

    let blocked = TestHome::new();
    fs::create_dir_all(blocked.path()).unwrap();
    fs::write(blocked.path().join("host.sock"), b"mine").unwrap();
    assert!(matches!(
        Host::start(blocked.config()),
        Err(org_host::HostError::SocketInTheWay { .. })
    ));
    assert_eq!(fs::read(blocked.path().join("host.sock")).unwrap(), b"mine");
}

/// Shutdown with clients connected (idle and subscribed) and an office whose start never
/// answers ends promptly. It would hang if waking the accept thread, ending a connection's writer
/// or stopping a worker mid-start broke, so a watchdog turns that into a failure instead of a
/// stuck test run.
#[test]
fn shutdown_with_clients_connected_finishes_promptly() {
    const A: &str = "0190a5d2-7c3e-7000-8000-00000000000a";
    let home = TestHome::registered();
    home.write_office(&support::hq(
        A,
        "0190a5d2-7c3e-7000-8000-00000000010a",
        "Acme",
        "2026-11-08T00:00:00.000Z",
    ));
    home.script(serde_json::json!({A: {"do": "hang"}}));
    let host = home.start(home.config()).unwrap();
    let _idle = std::os::unix::net::UnixStream::connect(host.socket_path()).unwrap();
    let mut subscribed = Client::for_host(&host);
    subscribed
        .call::<Subscribe>(&SubscribeParams::default())
        .unwrap();
    support::eventually(
        std::time::Duration::from_secs(10),
        "the office's start to begin",
        || home.runs(A) == 1,
    );

    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        host.shutdown();
        let _ = done.send(());
    });
    finished
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("shutdown hung");
    assert!(!home.runtime().join("host.sock").exists());
    for pid in home.pids(A) {
        assert!(!support::alive(pid), "the hanging worker outlived shutdown");
    }
}

/// An event published after a list snapshot never reaches a client before that snapshot: no
/// message on a subscribed connection shows an office in an earlier state than one before it.
/// List calls stream in on one subscribed connection (handled up to 16 at a time) while the
/// host's offices go from `starting` to `active`, each after its own delay.
#[test]
fn a_list_answer_never_arrives_after_an_event_it_doesnt_reflect() {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    const OFFICES: usize = 12;
    let home = TestHome::registered();
    let ids: Vec<String> = (0..OFFICES).map(|i| support::uuid7(i as u64 + 1)).collect();
    let mut script = serde_json::Map::new();
    for (i, id) in ids.iter().enumerate() {
        home.write_office(&support::hq(
            id,
            &support::uuid7(i as u64 + 101),
            &format!("Org {i}"),
            "2026-11-08T00:00:00.000Z",
        ));
        script.insert(
            id.clone(),
            serde_json::json!({"do": "ready", "delay_ms": 200 + 100 * i as u64}),
        );
    }
    home.script(serde_json::Value::Object(script));
    let host = home.start(home.config()).unwrap();
    let fixture = org_host::test_support::Fixture::new(&host);
    fixture.submit(org_proto::Request::new::<Subscribe>(0, &SubscribeParams::default()).unwrap());
    assert!(
        matches!(fixture.recv(std::time::Duration::from_secs(30)),Some(ServerMessage::Response(r)) if r.id==0)
    );

    // `last_id` is the id of the writer's last list call, 0 until the writer has decided
    // which call that is.
    let (starting, last_id) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicU64::new(0)));
    let answered = Arc::new(AtomicU64::new(0));
    let writer = {
        let out = Arc::clone(&fixture);
        let (starting, last_id, answered) = (
            Arc::clone(&starting),
            Arc::clone(&last_id),
            Arc::clone(&answered),
        );
        std::thread::spawn(move || {
            let mut id = 0u64;
            // Lists keep coming until every office is active, then one more. At most 64 are
            // unanswered, so the answers never fill the connection's queue (a client that lets
            // it fill is disconnected).
            loop {
                while id - answered.load(Ordering::SeqCst) >= 64 {
                    std::thread::yield_now();
                }
                let last = !starting.load(Ordering::SeqCst);
                id += 1;
                if last {
                    // Published before the call is made: its answer cannot reach the reader
                    // before the reader can know that it is the last one. (The reader must
                    // not learn it from this thread having ended: the answer is queued
                    // inside `submit`, so the reader can read it, find this thread still
                    // running and wait for a message that never comes.)
                    last_id.store(id, Ordering::SeqCst);
                }
                // Both lists, in turn.
                let method = if id % 2 == 1 {
                    "organization.list"
                } else {
                    "office.list"
                };
                let line = format!("{{\"id\":{id},\"method\":\"{method}\",\"params\":{{}}}}\n");
                out.submit(serde_json::from_str(&line).unwrap());
                if last {
                    return;
                }
            }
        })
    };

    // Each office goes from `starting` to `active`; no message may show an office in an
    // earlier state than one before it on this connection (an event older than a list answer,
    // or a list answer older than an event).
    let mut furthest: HashMap<String, OfficeStatus> = HashMap::new();
    let mut see = |office: &OfficeInfo, what: &str| {
        let rank = |status: OfficeStatus| match status {
            OfficeStatus::Starting => 0,
            OfficeStatus::Active => 1,
            other => panic!("{} became {other:?}", office.id),
        };
        let seen = furthest.entry(office.id.clone()).or_insert(office.status);
        assert!(
            rank(office.status) >= rank(*seen),
            "{what} showed {} {:?} after {:?}",
            office.id,
            office.status,
            seen
        );
        *seen = office.status;
        seen_active(&furthest)
    };
    fn seen_active(furthest: &HashMap<String, OfficeStatus>) -> usize {
        furthest
            .values()
            .filter(|s| **s == OfficeStatus::Active)
            .count()
    }
    let (mut events, mut answers) = (0usize, 0u64);
    // A writer that panicked ends the run instead of leaving it waiting.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "the run took over 60 s"
        );
        // Done when the answer to the writer's last call has been read: every call before
        // it was answered before it, on this one connection.
        let last = last_id.load(Ordering::SeqCst);
        if last != 0 && answers == last {
            break;
        }
        assert!(
            last != 0 || !writer.is_finished(),
            "the writer ended before its last list call"
        );
        let message = fixture
            .recv(std::time::Duration::from_secs(30))
            .expect("native event or response");
        let active = match message {
            ServerMessage::Event(event) => match event.event.as_str() {
                "office.status_changed" => {
                    events += 1;
                    let office = event.data::<OfficeStatusChanged>().unwrap().unwrap().office;
                    see(&office, "office.status_changed")
                }
                other => panic!("unexpected event {other}"),
            },
            ServerMessage::Response(response) => {
                let offices: Vec<OfficeInfo> = if response.id % 2 == 1 {
                    response
                        .into_result::<OrganizationList>()
                        .unwrap()
                        .organizations
                        .into_iter()
                        .flat_map(|o| o.offices)
                        .collect()
                } else {
                    response.into_result::<OfficeList>().unwrap().offices
                };
                assert_eq!(offices.len(), OFFICES, "a list missed an office");
                let mut active = 0;
                for office in &offices {
                    active = see(office, "a list answer");
                }
                answers += 1;
                answered.store(answers, Ordering::SeqCst);
                active
            }
            ServerMessage::Unknown(v) => panic!("unexpected {v}"),
        };
        if active == OFFICES {
            starting.store(false, Ordering::SeqCst);
        }
    }
    writer.join().unwrap();
    assert!(events > 0, "no status change was published while listing");
    host.shutdown();
}
