//! The host's org-proto surface: hello, unknown methods, unreadable lines, and the
//! owner-only files it serves from.

// Every host here keeps its offices' keys in a development key folder, which only debug builds
// have (a release host uses the Keychain, which tests never touch): debug builds only.
#![cfg(debug_assertions)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use org_office_host::Host;
use org_proto::{
    ErrorCode, Hello, HelloParams, HelloResult, OfficeInfo, OfficeList, OfficeStatus,
    OfficeStatusChanged, OrganizationCreate, OrganizationCreateParams, OrganizationCreated,
    OrganizationList, Outcome, ServerMessage, Subscribe, SubscribeParams, API_VERSION,
    UNREADABLE_REQUEST_ID,
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
        (9, r#"{"api":{"min":1,"max":2},"client":["org-agent"]}"#),
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
            r#"{"id":12,"method":"hello","params":{"client":"org-agent 0.1.0"}}"#,
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
        Err(org_office_host::HostError::SocketInTheWay { .. })
    ));
    assert_eq!(fs::read(blocked.path().join("host.sock")).unwrap(), b"mine");
}

/// Shutdown with clients connected (idle, subscribed, and one with a create in flight) ends
/// promptly. It would hang if waking the accept thread or ending a connection's writer broke,
/// so a watchdog turns that into a failure instead of a stuck test run.
#[test]
fn shutdown_with_clients_connected_finishes_promptly() {
    let home = TestHome::registered();
    home.script(serde_json::json!({"create": {"do": "hang"}}));
    let host = home.start(home.config()).unwrap();
    let _idle = std::os::unix::net::UnixStream::connect(host.socket_path()).unwrap();
    let mut subscribed = Client::for_host(&host);
    subscribed
        .call::<Subscribe>(&SubscribeParams::default())
        .unwrap();
    let mut creating = Client::for_host(&host);
    let creator = std::thread::spawn(move || {
        creating.call::<OrganizationCreate>(&OrganizationCreateParams {
            name: "Acme".into(),
        })
    });
    support::eventually(
        std::time::Duration::from_secs(10),
        "the create to start",
        || home.runs("create") == 1,
    );

    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        host.shutdown();
        let _ = done.send(());
    });
    finished
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("shutdown hung");
    // The create in flight got an error or its connection closed: never a success.
    assert!(creator.join().unwrap().is_err());
    assert!(!home.runtime().join("host.sock").exists());
    // Nothing was stored. The interrupted create's folder can't be discarded while stopping,
    // so the next start discards it.
    for folder in home.office_folders() {
        assert!(!home.office_dir(&folder).join("office.json").exists());
    }
    home.script(serde_json::json!({}));
    let host = support::restart_host(&home, &home.config());
    support::eventually(
        std::time::Duration::from_secs(10),
        "the interrupted create's folder to be discarded",
        || home.office_folders().is_empty(),
    );
    host.shutdown();
}

/// An event published after a list snapshot never reaches a client before that snapshot: every
/// organization a subscriber has heard of is in every list answer that comes after the event.
/// List calls stream in on one subscribed connection (handled up to 16 at a time) while
/// organizations are created on another.
#[test]
fn a_list_answer_never_arrives_after_an_event_it_doesnt_reflect() {
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    const CREATES: usize = 40;
    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    let fixture = org_office_host::test_support::Fixture::new(&host);
    fixture.submit(org_proto::Request::new::<Subscribe>(0, &SubscribeParams::default()).unwrap());
    assert!(
        matches!(fixture.recv(std::time::Duration::from_secs(30)),Some(ServerMessage::Response(r)) if r.id==0)
    );

    let (creating, sent) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicU64::new(0)));
    let answered = Arc::new(AtomicU64::new(0));
    let writer = {
        let out = Arc::clone(&fixture);
        let (creating, sent, answered) = (
            Arc::clone(&creating),
            Arc::clone(&sent),
            Arc::clone(&answered),
        );
        std::thread::spawn(move || {
            let mut id = 0u64;
            // Lists keep coming until the creates are done, then one more. At most 64 are
            // unanswered, so the answers never fill the connection's queue (a client that lets
            // it fill is disconnected).
            loop {
                while id - answered.load(Ordering::SeqCst) >= 64 {
                    std::thread::yield_now();
                }
                let last = !creating.load(Ordering::SeqCst);
                id += 1;
                // Both lists, in turn.
                let method = if id % 2 == 1 {
                    "organization.list"
                } else {
                    "office.list"
                };
                let line = format!("{{\"id\":{id},\"method\":\"{method}\",\"params\":{{}}}}\n");
                out.submit(serde_json::from_str(&line).unwrap());
                sent.store(id, Ordering::SeqCst);
                if last {
                    return;
                }
            }
        })
    };
    let creator = {
        let creating = Arc::clone(&creating);
        let mut client = Client::for_host(&host);
        std::thread::spawn(move || {
            for i in 0..CREATES {
                client
                    .call::<OrganizationCreate>(&OrganizationCreateParams {
                        name: format!("Org {i}"),
                    })
                    .unwrap();
            }
            creating.store(false, Ordering::SeqCst);
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
    };
    let (mut heard, mut answers) = (HashSet::new(), 0u64);
    loop {
        let done = writer.is_finished() && answers == sent.load(Ordering::SeqCst);
        if done && heard.len() == CREATES {
            break;
        }
        match fixture
            .recv(std::time::Duration::from_secs(30))
            .expect("native event or response")
        {
            ServerMessage::Event(event) => match event.event.as_str() {
                "organization.created" => {
                    let org = event.data::<OrganizationCreated>().unwrap().unwrap();
                    for office in &org.offices {
                        see(office, "organization.created");
                    }
                    heard.insert(org.id);
                }
                "office.status_changed" => {
                    let office = event.data::<OfficeStatusChanged>().unwrap().unwrap().office;
                    // Never before its organization was announced.
                    assert!(
                        heard.contains(&office.organization_id),
                        "office.status_changed for {} came before its organization.created",
                        office.id
                    );
                    see(&office, "office.status_changed");
                }
                other => panic!("unexpected event {other}"),
            },
            ServerMessage::Response(response) => {
                // The organizations each list answer shows.
                let listed: HashSet<String> = if response.id % 2 == 1 {
                    let orgs = response
                        .into_result::<OrganizationList>()
                        .unwrap()
                        .organizations;
                    for office in orgs.iter().flat_map(|o| &o.offices) {
                        see(office, "organization.list");
                    }
                    orgs.into_iter().map(|o| o.id).collect()
                } else {
                    let offices = response.into_result::<OfficeList>().unwrap().offices;
                    for office in &offices {
                        see(office, "office.list");
                    }
                    offices.into_iter().map(|o| o.organization_id).collect()
                };
                let missing: Vec<_> = heard.difference(&listed).collect();
                assert!(
                    missing.is_empty(),
                    "a list answer came after the event for {missing:?} without it"
                );
                answers += 1;
                answered.store(answers, Ordering::SeqCst);
            }
            ServerMessage::Unknown(v) => panic!("unexpected {v}"),
        }
    }
    writer.join().unwrap();
    creator.join().unwrap();
    host.shutdown();
}

/// A subscriber that creates organizations on its own connection gets each answer before any
/// event about that organization: the answer, `organization.created` and the office's status
/// changes go out in that order.
#[test]
fn a_subscribed_creator_gets_its_answer_before_any_event_about_it() {
    use std::collections::HashSet;

    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    let fixture = org_office_host::test_support::Fixture::new(&host);
    let next = || {
        fixture
            .recv(std::time::Duration::from_secs(30))
            .expect("native response or event")
    };
    fixture.submit(org_proto::Request::new::<Subscribe>(0, &SubscribeParams::default()).unwrap());
    assert!(matches!(next(), ServerMessage::Response(r) if r.id == 0));

    let mut answered: HashSet<String> = HashSet::new();
    let mut announced: HashSet<String> = HashSet::new();
    for id in 1..=15u64 {
        let line = format!(
            "{{\"id\":{id},\"method\":\"organization.create\",\"params\":{{\"name\":\"Org {id}\"}}}}\n"
        );
        fixture.submit(serde_json::from_str(&line).unwrap());
        // Read until this create's answer; every event before it must be about organizations
        // already answered (and, for a status change, already announced).
        loop {
            match next() {
                ServerMessage::Response(response) => {
                    assert_eq!(response.id, id);
                    let org = response.into_result::<OrganizationCreate>().unwrap();
                    answered.insert(org.id);
                    break;
                }
                ServerMessage::Event(event) => match event.event.as_str() {
                    "organization.created" => {
                        let org = event.data::<OrganizationCreated>().unwrap().unwrap();
                        assert!(answered.contains(&org.id), "announced before its answer");
                        announced.insert(org.id);
                    }
                    "office.status_changed" => {
                        let office = event.data::<OfficeStatusChanged>().unwrap().unwrap().office;
                        assert!(
                            answered.contains(&office.organization_id)
                                && announced.contains(&office.organization_id),
                            "a status change came before its organization's answer or event"
                        );
                    }
                    other => panic!("unexpected event {other}"),
                },
                ServerMessage::Unknown(v) => panic!("unexpected {v}"),
            }
        }
    }
    host.shutdown();
}
